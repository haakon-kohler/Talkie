//! Hooks: executables Talkie runs at the edges of a capture.
//!
//! This is the whole plugin system, and it is deliberately small. A hook is any
//! executable file in `<app-data>/hooks/` — on macOS,
//! `~/Library/Application Support/com.haakonkohler.talkie/hooks/` — named for
//! the moment it runs. No manifest, no registry, no language: a shebang line
//! is the whole format, so a hook can be a shell script, a Python file, or a
//! compiled binary. Nothing loads at launch; a process starts only when its
//! moment comes, and there is no Talkie-side cost to a hook that is not there.
//!
//! Two moments exist:
//!
//! - `on-capture` runs after transcription and before the write. It gets the
//!   transcript on stdin, and whatever it prints to stdout replaces it. Exit
//!   zero with nothing printed drops the capture — the hook has said there is
//!   nothing to keep. A non-zero exit, a crash, output that is not UTF-8, or
//!   running past [`FILTER_TIMEOUT`] keeps the original transcript, so a broken
//!   filter can never lose what was said.
//! - `after-capture` runs once the entry is in the file. It gets the text that
//!   was written on stdin and its exit status is only logged. Talkie waits on
//!   it in the background, not in the capture path, so it may take as long as
//!   it likes; a hook that hands off to something long-running should still
//!   detach that work rather than sit on it.
//!
//! Both get the resolved note path in `TALKIE_NOTE`. Their stderr is captured
//! and logged, because a bundled app's own stderr goes nowhere.
//!
//! Hooks run as the user, with the user's permissions, at the user's own risk.
//! That is the point: it is a program they put in a folder.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use tauri::{AppHandle, Manager};

/// The hook that filters a transcript before it is written.
const ON_CAPTURE: &str = "on-capture";
/// The hook that is told once an entry has landed.
const AFTER_CAPTURE: &str = "after-capture";

/// How long `on-capture` may take before Talkie gives up on it and writes
/// the transcript as heard. It sits inside the capture path, holding the
/// recorder in `Transcribing`, so it cannot be allowed to hang.
pub const FILTER_TIMEOUT: Duration = Duration::from_secs(30);

/// `<app-data>/hooks`, created on demand so it is there to be found.
pub fn dir(app: &AppHandle) -> Result<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .context("no app data directory")?
        .join("hooks");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Run the transcript through `on-capture`, if there is one.
///
/// Returns the text to write, or `None` when the hook chose to drop the
/// capture. Without a hook the text comes back untouched. An empty transcript
/// is not a capture and is not offered to the hook.
pub fn on_capture(app: &AppHandle, note: &Path, text: &str) -> Option<String> {
    if text.trim().is_empty() {
        return Some(text.to_owned());
    }
    let Some(hook) = find(app, ON_CAPTURE) else {
        return Some(text.to_owned());
    };

    match run(&hook, note, text, Some(FILTER_TIMEOUT)) {
        Ok(Outcome { status, stdout }) if status.success() => {
            let filtered = stdout.trim_end();
            if filtered.trim().is_empty() {
                None
            } else {
                Some(filtered.to_owned())
            }
        }
        Ok(Outcome { status, .. }) => {
            log::warn!(
                "talkie: {ON_CAPTURE} exited with {status}; keeping the transcript as heard"
            );
            Some(text.to_owned())
        }
        Err(e) => {
            log::warn!("talkie: {ON_CAPTURE} failed: {e:#}; keeping the transcript as heard");
            Some(text.to_owned())
        }
    }
}

/// Tell `after-capture`, if there is one, what was just written. Returns at
/// once; the hook is waited on and reported from a background thread.
pub fn after_capture(app: &AppHandle, note: &Path, text: &str) {
    let Some(hook) = find(app, AFTER_CAPTURE) else {
        return;
    };
    let note = note.to_owned();
    let text = text.to_owned();
    thread::spawn(move || match run(&hook, &note, &text, None) {
        Ok(Outcome { status, .. }) if status.success() => {}
        Ok(Outcome { status, .. }) => {
            log::warn!("talkie: {AFTER_CAPTURE} exited with {status}");
        }
        Err(e) => log::warn!("talkie: {AFTER_CAPTURE} failed: {e:#}"),
    });
}

/// The hook file for `name`, if one is installed and runnable.
///
/// A file that exists but lacks the executable bit is the one mistake every
/// first-time hook author makes, so it is called out by name rather than
/// silently skipped.
fn find(app: &AppHandle, name: &str) -> Option<PathBuf> {
    let dir = match dir(app) {
        Ok(dir) => dir,
        Err(e) => {
            log::warn!("talkie: no hooks directory: {e:#}");
            return None;
        }
    };
    let path = dir.join(name);
    let meta = fs::metadata(&path).ok()?;
    if !meta.is_file() {
        return None;
    }
    if !is_executable(&meta) {
        log::warn!(
            "talkie: {} exists but is not executable; `chmod +x` it to enable the hook",
            path.display()
        );
        return None;
    }
    Some(path)
}

#[cfg(unix)]
fn is_executable(meta: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_meta: &fs::Metadata) -> bool {
    true
}

/// What a finished hook left behind.
#[derive(Debug)]
struct Outcome {
    status: ExitStatus,
    stdout: String,
}

/// Run one hook to completion: feed it `input`, collect its output, and log
/// anything it says on stderr. With a `timeout`, a hook still running at the
/// deadline is killed and reported as an error.
///
/// The deadline also covers collecting the output. A hook can exit while
/// something it spawned still holds its stdout open — a backgrounded job
/// without a redirect is the classic shape — and Talkie must not sit on that
/// pipe until the grandchild goes away.
fn run(hook: &Path, note: &Path, input: &str, timeout: Option<Duration>) -> Result<Outcome> {
    let deadline = timeout.map(|t| Instant::now() + t);
    let mut child = Command::new(hook)
        .env("TALKIE_NOTE", note)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("could not start {}", hook.display()))?;

    // Each pipe gets its own thread so a hook that fills one before reading
    // another cannot deadlock against us. The stdin thread closes the pipe
    // when it finishes, which is how the hook learns the transcript is over.
    // A hook that exits without reading leaves that thread a broken pipe,
    // which is the hook's business; nothing here waits on it.
    let mut stdin = child.stdin.take().expect("stdin was requested as a pipe");
    let mut stdout = child.stdout.take().expect("stdout was requested as a pipe");
    let mut stderr = child.stderr.take().expect("stderr was requested as a pipe");
    let input = input.to_owned();
    thread::spawn(move || stdin.write_all(input.as_bytes()));
    let (out_tx, out_rx) = mpsc::channel();
    thread::spawn(move || {
        let mut s = String::new();
        let _ = out_tx.send(stdout.read_to_string(&mut s).map(|_| s));
    });
    let (err_tx, err_rx) = mpsc::channel();
    thread::spawn(move || {
        let mut s = String::new();
        let _ = err_tx.send(stderr.read_to_string(&mut s).map(|_| s));
    });

    let status = wait(&mut child, deadline)?;

    let stdout = recv_by(&out_rx, deadline)
        .context("exited but something still holds its stdout open")?
        .context("the hook's output was not UTF-8")?;
    if let Some(Ok(text)) = recv_by(&err_rx, deadline) {
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            log::warn!("talkie: {}: {line}", hook.display());
        }
    }
    Ok(Outcome { status, stdout })
}

/// Wait for `child`, killing it at the deadline if there is one.
fn wait(child: &mut Child, deadline: Option<Instant>) -> Result<ExitStatus> {
    let Some(deadline) = deadline else {
        return child.wait().context("could not wait for the hook");
    };
    loop {
        if let Some(status) = child.try_wait().context("could not poll the hook")? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(anyhow!("still running at the deadline; killed"));
        }
        thread::sleep(Duration::from_millis(20));
    }
}

/// One value from `rx`, or `None` if the deadline passes first. Without a
/// deadline this waits as long as it takes.
fn recv_by<T>(rx: &mpsc::Receiver<T>, deadline: Option<Instant>) -> Option<T> {
    match deadline {
        None => rx.recv().ok(),
        Some(deadline) => rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway executable script, removed when dropped.
    struct Script(PathBuf);

    impl Script {
        fn new(name: &str, body: &str) -> Self {
            use std::os::unix::fs::PermissionsExt;
            let path =
                std::env::temp_dir().join(format!("talkie-hook-{name}-{}", std::process::id()));
            fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write script");
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod");
            Script(path)
        }
    }

    impl Drop for Script {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    #[test]
    fn stdout_replaces_the_transcript() {
        let hook = Script::new("upper", "tr a-z A-Z");
        let out = run(&hook.0, Path::new("/tmp/talkie.md"), "hello\n", None).expect("run");
        assert!(out.status.success());
        assert_eq!(out.stdout, "HELLO\n");
    }

    #[test]
    fn the_note_path_is_in_the_environment() {
        let hook = Script::new("env", "printf '%s' \"$TALKIE_NOTE\"");
        let out = run(&hook.0, Path::new("/somewhere/talkie.md"), "", None).expect("run");
        assert_eq!(out.stdout, "/somewhere/talkie.md");
    }

    #[test]
    fn a_failing_hook_reports_its_status() {
        let hook = Script::new("fail", "echo oops >&2; exit 3");
        let out = run(&hook.0, Path::new("/tmp/talkie.md"), "x", None).expect("run");
        assert_eq!(out.status.code(), Some(3));
    }

    #[test]
    fn a_hung_hook_is_killed_at_the_deadline() {
        let hook = Script::new("hang", "sleep 30");
        let started = Instant::now();
        let err = run(
            &hook.0,
            Path::new("/tmp/talkie.md"),
            "",
            Some(Duration::from_millis(200)),
        )
        .expect_err("should time out");
        assert!(err.to_string().contains("killed"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_hook_that_ignores_stdin_still_finishes() {
        let hook = Script::new("deaf", "echo done");
        let big = "x".repeat(1 << 20);
        let out = run(&hook.0, Path::new("/tmp/talkie.md"), &big, None).expect("run");
        assert_eq!(out.stdout, "done\n");
    }
}
