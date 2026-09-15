//! The last ten things that went wrong.
//!
//! A menu-bar app launched from Finder has no stderr anyone can see, so every
//! `log::warn!` in the host used to vanish. This keeps the most recent ten in
//! `<app-data>/debug.log` — the file *is* the buffer, rewritten on every entry,
//! so a crash leaves its own last line behind.
//!
//! Two severities, no more: `warn` is something Talkie handled and carried on
//! from; `fatal` is a panic, recorded from the panic hook on the way down.
//! Read it with `Talkie --debug-log`, which prints the file and exits before
//! the single-instance guard runs.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use log::{Level, LevelFilter, Log, Metadata, Record};

/// How many lines the buffer holds. Ten is enough to see what led up to the
/// last failure and few enough to read at a glance.
const LINES: usize = 10;

const FILE: &str = "debug.log";
const FLAG: &str = "--debug-log";

static JOURNAL: OnceLock<Journal> = OnceLock::new();

struct Journal {
    path: PathBuf,
    ring: Mutex<VecDeque<String>>,
    /// Everything still goes to stderr, so `cargo tauri dev` reads as before.
    stderr: env_logger::Logger,
}

/// The two things the buffer distinguishes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Severity {
    Warn,
    Fatal,
}

impl Severity {
    const fn as_str(self) -> &'static str {
        match self {
            Severity::Warn => "warn ",
            Severity::Fatal => "fatal",
        }
    }
}

/// Install the journal as the global logger and hook panics into it.
///
/// `data_dir` is the app-data directory Tauri would report; the journal takes
/// the path rather than an `AppHandle` because `--debug-log` needs it before
/// any app exists.
pub fn init(data_dir: &Path) {
    let path = data_dir.join(FILE);
    let stderr =
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).build();
    let level = stderr.filter();

    let ring = load(&path);
    assert!(ring.len() <= LINES, "loaded more than the buffer holds");

    let journal = JOURNAL.get_or_init(|| Journal {
        path,
        ring: Mutex::new(ring),
        stderr,
    });
    // Both fail only if a logger is already installed, which is a programming
    // error: `init` runs once, first thing in `run`.
    log::set_logger(journal).expect("journal installed twice");
    log::set_max_level(level.max(LevelFilter::Warn));

    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| info.payload().downcast_ref::<String>().map(String::as_str))
            .unwrap_or("panic without a message");
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "unknown location".into());
        record(Severity::Fatal, &format!("{payload} at {location}"));
        default_hook(info);
    }));
}

/// `Talkie --debug-log`: print the buffer and exit. Returns `false` when the
/// flag is absent so `run` carries on to the app.
///
/// This runs before the Tauri builder, so the second process never reaches
/// the single-instance socket — it reads the file the running one wrote.
pub fn handle_flag(data_dir: &Path) -> bool {
    if !std::env::args().skip(1).any(|arg| arg == FLAG) {
        return false;
    }
    let lines = load(&data_dir.join(FILE));
    if lines.is_empty() {
        eprintln!("talkie: nothing in the debug log");
    }
    for line in lines {
        println!("{line}");
    }
    true
}

/// Whether a set of launch arguments carries the flag. The guard asserts the
/// negative: by the time argv reaches the running instance, `handle_flag`
/// has already consumed it.
pub fn flagged(argv: &[String]) -> bool {
    argv.iter().skip(1).any(|arg| arg == FLAG)
}

fn load(path: &Path) -> VecDeque<String> {
    let text = fs::read_to_string(path).unwrap_or_default();
    let mut ring: VecDeque<String> = text.lines().map(str::to_owned).collect();
    while ring.len() > LINES {
        ring.pop_front();
    }
    ring
}

/// Push one line, dropping the oldest, and rewrite the file.
///
/// A repeat of the last line bumps a counter on it instead of taking a slot:
/// a fault that fires at 100 Hz should cost one line, not the whole buffer.
fn record(severity: Severity, message: &str) {
    let Some(journal) = JOURNAL.get() else {
        return;
    };
    let message = message.replace('\n', " ");
    debug_assert!(!message.contains('\n'), "a line is one line");

    let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    let line = format!("{stamp} {} {message}", severity.as_str());
    assert!(!line.is_empty(), "the stamp alone is never empty");

    // The panic hook records with whatever state the mutex is in; a poisoned
    // lock still holds a readable ring.
    let mut ring = journal
        .ring
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let repeated = ring.back().is_some_and(|last| strip(last) == strip(&line));
    if repeated {
        let last = ring.pop_back().unwrap_or_default();
        let (body, times) = split_count(&last);
        ring.push_back(format!("{body} ×{}", times + 1));
    } else {
        ring.push_back(line);
    }
    while ring.len() > LINES {
        ring.pop_front();
    }
    assert!(ring.len() <= LINES, "buffer overran its capacity");
    assert!(!ring.is_empty(), "a push always leaves one line");

    write(&journal.path, &ring);
}

/// The line without its timestamp or repeat counter, for comparing repeats.
fn strip(line: &str) -> &str {
    let body = split_count(line).0;
    // "YYYY-MM-DD HH:MM:SS " is 20 bytes of ASCII.
    body.get(20..).unwrap_or(body)
}

/// `("...", n)` for a line ending in ` ×n`, `("...", 1)` otherwise.
fn split_count(line: &str) -> (&str, u32) {
    match line.rsplit_once(" ×") {
        Some((body, n)) => match n.parse() {
            Ok(n) => (body, n),
            Err(_) => (line, 1),
        },
        None => (line, 1),
    }
}

/// Write-then-rename: a crash mid-write leaves the previous file, not a torn one.
fn write(path: &Path, ring: &VecDeque<String>) {
    let mut text = String::new();
    for line in ring {
        text.push_str(line);
        text.push('\n');
    }
    let tmp = path.with_extension("log.tmp");
    // Failing to write the journal must never itself become a warning: that
    // would recurse. Silence is the only option left here.
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if fs::write(&tmp, text).is_ok() {
        let _ = fs::rename(&tmp, path);
    }
}

impl Log for Journal {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Warn || self.stderr.enabled(metadata)
    }

    fn log(&self, record_: &Record) {
        self.stderr.log(record_);
        // `error!` and `warn!` both mean "handled, carried on"; the journal
        // does not tell them apart. Only a panic is fatal.
        if record_.level() <= Level::Warn {
            record(Severity::Warn, &record_.args().to_string());
        }
    }

    fn flush(&self) {
        self.stderr.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_repeat_counter_parses_and_the_rest_does_not() {
        assert_eq!(split_count("a b ×3"), ("a b", 3));
        assert_eq!(split_count("a b"), ("a b", 1));
        assert_eq!(split_count("a ×b"), ("a ×b", 1));
    }

    #[test]
    fn strip_drops_the_stamp_and_the_counter() {
        let line = "2026-09-15 14:02:11 warn  talkie: x ×2";
        assert_eq!(strip(line), "warn  talkie: x");
        assert_eq!(strip("short"), "short");
    }

    #[test]
    fn load_keeps_only_the_tail() {
        let dir = std::env::temp_dir().join(format!("talkie-journal-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join(FILE);
        let text: String = (0..LINES + 5).map(|i| format!("line {i}\n")).collect();
        fs::write(&path, text).expect("seed");

        let ring = load(&path);
        assert_eq!(ring.len(), LINES);
        assert_eq!(ring.front().map(String::as_str), Some("line 5"));
        assert_eq!(load(&dir.join("missing.log")).len(), 0);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn the_flag_is_recognised_after_the_executable() {
        let argv = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(flagged(&argv(&["talkie", "--debug-log"])));
        assert!(!flagged(&argv(&["talkie"])));
        assert!(
            !flagged(&argv(&["--debug-log"])),
            "argv[0] is the binary, not a flag"
        );
    }
}
