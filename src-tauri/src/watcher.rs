//! Watching `talkie.md` for changes Talkie's editor did not make.
//!
//! The file is the integration surface: Obsidian edits it, agents append to it,
//! git checks it out underneath us, and Talkie's own capture pipeline appends to
//! it while the editor may be open on screen. All of those have to reach the
//! editor, and none of them may clobber what the user is in the middle of
//! typing.
//!
//! ## Distinguishing our own writes
//!
//! A naive watcher fires on the editor's own autosave and hands the file back to
//! the editor that just wrote it. Rather than trying to suppress events by
//! timing, this module tracks the content Talkie last *knew about* — set by
//! `read_note` and `write_note`, and by each report — and emits only when what
//! lands on disk differs from it. Self-inflicted events therefore cost one
//! comparison and stop there, and the capture pipeline's `prepend` deliberately
//! does not update it, so a silent capture reaches the editor by exactly the
//! same path an external edit does.
//!
//! ## The merge base is not the watcher's
//!
//! What the watcher last saw and what the editor last loaded are two values,
//! kept apart on purpose. The watcher moves its own as soon as the file
//! settles, whether or not the editor took the change in — a dirty editor
//! ignores it. If saves merged against that, a capture that landed under
//! unsaved edits would look like nothing at all, and the next save would write
//! over it; so would a file the editor never showed, after the notes path
//! moved (#5). The base a save merges against is therefore moved only by
//! `read_note` and `write_note`, and it remembers which file it came from.
//!
//! The directory is watched, not the file: editors save by writing a temporary
//! file and renaming it over the target — Talkie's own `note::write` included —
//! which replaces the inode and would silently detach a file watch.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use talkie_shared::events;
use tauri::{AppHandle, Emitter, Manager};

use crate::note;
use crate::settings::SettingsState;

/// How long to keep collecting events after the first one before reading the
/// file. One save produces a flurry — create, modify, rename — and reading once
/// at the end of it beats reading four times and emitting three no-ops.
const SETTLE: Duration = Duration::from_millis(150);

/// The most settle periods one flurry may extend itself by before the file is
/// read anyway. A folder that never goes quiet — a sync client churning next
/// to the note — must not starve the editor of updates forever.
const MAX_SETTLE_ROUNDS: usize = 20;

/// The live watch, managed as Tauri state, and the editor's merge base beside
/// it.
pub struct NoteWatcher {
    /// Dropping the previous watcher is what stops it, so re-arming is just a
    /// replace. The thread behind it ends on its own when the channel closes.
    watcher: Mutex<Option<RecommendedWatcher>>,
    /// What Talkie last knew to be on disk. Shared with the watch thread,
    /// which records what it finds there.
    seen: Arc<Mutex<Seen>>,
    /// What the editor last read or saved. Never moved by the watch thread.
    base: Mutex<Option<Base>>,
}

/// The watcher's half: what is on disk, as far as Talkie knows.
#[derive(Default)]
struct Seen {
    /// `None` before the first read, which makes the first external event
    /// unconditionally interesting.
    text: Option<String>,
    /// Which watch may still record into `text`. Re-arming bumps it, so a
    /// thread retired mid-flurry — its channel closed under it — cannot read
    /// the old file into `text` or report it after the switch.
    generation: u64,
}

/// The editor's half: the text its document was last read from or saved as.
///
/// The text itself, not a hash of it: it is the *base* a save merges against
/// when the file moved underneath the editor, and a hash cannot be diffed.
struct Base {
    /// The file it came from. A save to any other file has no base at all.
    path: PathBuf,
    text: String,
}

impl NoteWatcher {
    pub(crate) fn new() -> Self {
        Self {
            watcher: Mutex::new(None),
            seen: Arc::new(Mutex::new(Seen::default())),
            base: Mutex::new(None),
        }
    }

    /// Remember content as Talkie's own, so the watcher stays quiet about it.
    pub fn remember(&self, text: &str) {
        self.seen.lock().expect("watcher mutex poisoned").text = Some(text.to_string());
    }

    /// The editor now holds `text` as the contents of `path`: it becomes the
    /// base the next save merges against, and the watcher stays quiet about it.
    pub fn adopt(&self, path: &Path, text: &str) {
        *self.base.lock().expect("watcher mutex poisoned") = Some(Base {
            path: path.to_path_buf(),
            text: text.to_string(),
        });
        self.remember(text);
    }

    /// The base for a save to `path`: what the editor last read from or saved
    /// to that file. `None` when the editor's last read or save was another
    /// file, or there was none.
    pub fn base(&self, path: &Path) -> Option<String> {
        let base = self.base.lock().expect("watcher mutex poisoned");
        base.as_ref()
            .filter(|base| base.path == path)
            .map(|base| base.text.clone())
    }

    /// What Talkie last saw on disk.
    #[cfg(test)]
    pub fn last_seen(&self) -> Option<String> {
        self.seen
            .lock()
            .expect("watcher mutex poisoned")
            .text
            .clone()
    }

    /// Start watching `path`, replacing any previous watch.
    ///
    /// `on_change` runs on the watch thread each time the file settles on
    /// content Talkie has not seen. Kept apart from `arm` so the watch can be
    /// driven without an app around it.
    pub(crate) fn watch(&self, path: &Path, on_change: impl Fn() + Send + 'static) -> Result<()> {
        let directory = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        // The folder need not exist yet — the first capture creates it — but
        // there is nothing to watch until it does.
        std::fs::create_dir_all(&directory)
            .with_context(|| format!("could not create the notes folder {directory:?}"))?;

        let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
        let mut watcher = notify::recommended_watcher(move |event| {
            let _ = tx.send(event);
        })
        .context("could not create a file watcher")?;
        watcher
            .watch(&directory, RecursiveMode::NonRecursive)
            .with_context(|| format!("could not watch {directory:?}"))?;

        // Retire the previous thread before this one can report anything: from
        // here on, only this generation records what it finds.
        let generation = {
            let mut seen = self.seen.lock().expect("watcher mutex poisoned");
            seen.generation += 1;
            seen.generation
        };
        let seen = Arc::clone(&self.seen);
        let watched = path.to_path_buf();
        std::thread::Builder::new()
            .name("talkie-note-watcher".into())
            .spawn(move || {
                // Ends when the watcher is dropped and the sender goes with it,
                // which is how re-arming retires the previous thread.
                while let Ok(first) = rx.recv() {
                    if !concerns(&first, &watched) {
                        continue;
                    }
                    // Drain the rest of the flurry before reading: keep
                    // swallowing events until SETTLE passes with nothing new,
                    // or the bound is hit.
                    let mut rounds = 0;
                    while rounds < MAX_SETTLE_ROUNDS && rx.recv_timeout(SETTLE).is_ok() {
                        rounds += 1;
                    }
                    if noticed(&seen, generation, &watched) {
                        on_change();
                    }
                }
            })
            .context("could not start the note watcher thread")?;

        *self.watcher.lock().expect("watcher mutex poisoned") = Some(watcher);
        Ok(())
    }
}

#[cfg(test)]
fn take_if_new(seen: &Mutex<Seen>, text: &str) -> bool {
    seen.lock()
        .expect("watcher mutex poisoned")
        .take_if_new(text)
}

impl Seen {
    /// Whether `text` is news, and if so remember it. One call, because every
    /// caller does both and doing them separately invites a race.
    fn take_if_new(&mut self, text: &str) -> bool {
        if self.text.as_deref() == Some(text) {
            return false;
        }
        self.text = Some(text.to_string());
        true
    }
}

/// Register the watcher state. Called once, from `lib.rs`, before `arm`.
pub fn init(app: &AppHandle) {
    assert!(
        app.try_state::<NoteWatcher>().is_none(),
        "the note watcher was registered twice"
    );
    app.manage(NoteWatcher::new());
}

/// Start watching the note file named in settings, replacing any previous watch.
///
/// Called at startup and again whenever the note path changes, so it has to be
/// idempotent.
pub fn arm(app: &AppHandle) -> Result<()> {
    let path = {
        let settings = app.state::<SettingsState>();
        let guard = settings.0.lock().expect("settings mutex poisoned");
        note::resolve(&guard.note_path)
    };

    let handle = app.clone();
    app.state::<NoteWatcher>().watch(&path, move || {
        if let Err(e) = handle.emit(events::NOTE_CHANGED_EXTERNALLY, ()) {
            log::error!("talkie: could not announce a note change: {e}");
        }
    })?;

    log::info!("talkie: watching {}", path.display());
    Ok(())
}

/// Whether an event is about the file we care about. Renames report both names,
/// so any path in the event counts.
fn concerns(event: &notify::Result<Event>, path: &Path) -> bool {
    match event {
        Ok(event) => event.paths.iter().any(|p| p == path),
        Err(e) => {
            log::warn!("talkie: note watcher error: {e}");
            false
        }
    }
}

/// Read the file and decide whether it is news — if what is there is not
/// already ours, and the watch that asks is still the live one.
fn noticed(seen: &Mutex<Seen>, generation: u64, path: &Path) -> bool {
    match note::read(path) {
        // Checked under the lock that records it, after the read: the path
        // can change while the file is being read.
        Ok(text) => {
            let mut seen = seen.lock().expect("watcher mutex poisoned");
            seen.generation == generation && seen.take_if_new(&text)
        }
        Err(e) => {
            log::warn!("talkie: could not read {}: {e:#}", path.display());
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_content_is_only_news_once() {
        let watcher = NoteWatcher::new();
        assert!(take_if_new(&watcher.seen, "hello"));
        assert!(!take_if_new(&watcher.seen, "hello"));
        assert!(take_if_new(&watcher.seen, "hello there"));
    }

    #[test]
    fn remembering_our_own_write_silences_it() {
        let watcher = NoteWatcher::new();
        watcher.remember("what the editor just saved");
        assert!(!take_if_new(&watcher.seen, "what the editor just saved"));
    }

    /// How long to wait for a report that should come. Only ever spent in
    /// full when a test is about to fail.
    const PATIENCE: Duration = Duration::from_secs(5);

    /// Two note paths in two folders. Canonical, because FSEvents reports the
    /// real path and the macOS temporary folder sits behind a symlink.
    fn two_notes(name: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("talkie-watcher-test-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("a")).unwrap();
        std::fs::create_dir_all(dir.join("b")).unwrap();
        let dir = dir.canonicalize().unwrap();
        (dir.join("a/talkie.md"), dir.join("b/talkie.md"))
    }

    fn watching(watcher: &NoteWatcher, path: &Path) -> mpsc::Receiver<()> {
        let (tx, rx) = mpsc::channel();
        watcher
            .watch(path, move || {
                let _ = tx.send(());
            })
            .expect("watch");
        rx
    }

    #[test]
    fn a_change_to_the_file_is_reported_and_remembered() {
        let (a, _) = two_notes("reported");
        let watcher = NoteWatcher::new();
        let reports = watching(&watcher, &a);

        note::write(&a, "## 2026-10-02 10:00\nFrom Obsidian\n").unwrap();

        reports.recv_timeout(PATIENCE).expect("no report");
        assert_eq!(
            watcher.last_seen().as_deref(),
            Some("## 2026-10-02 10:00\nFrom Obsidian\n")
        );
    }

    /// Re-arming is how a path change reaches the watcher: the new file is
    /// reported, the old one no longer is.
    #[test]
    fn rearming_moves_the_watch_to_the_new_file() {
        let (a, b) = two_notes("rearmed");
        let watcher = NoteWatcher::new();
        let old = watching(&watcher, &a);
        let new = watching(&watcher, &b);

        note::write(&a, "A changed\n").unwrap();
        note::write(&b, "B changed\n").unwrap();

        new.recv_timeout(PATIENCE).expect("B was not reported");
        assert!(
            old.recv_timeout(SETTLE * 3).is_err(),
            "A was reported after the watch moved"
        );
        assert_eq!(watcher.last_seen().as_deref(), Some("B changed\n"));
    }

    /// The thread behind the old watch can be mid-flurry when the path
    /// changes. Dropping the watcher ends the flurry early, and the thread
    /// used to read the *old* file and report it after the switch.
    #[test]
    fn a_rearm_mid_flurry_does_not_report_the_old_file() {
        let (a, b) = two_notes("mid-flurry");
        let watcher = NoteWatcher::new();
        let old = watching(&watcher, &a);

        note::write(&a, "A changed\n").unwrap();
        // Inside SETTLE: the old thread has the event and is draining.
        std::thread::sleep(SETTLE / 3);
        let _new = watching(&watcher, &b);

        assert!(
            old.recv_timeout(SETTLE * 3).is_err(),
            "A was reported after the watch moved"
        );
        assert_ne!(watcher.last_seen().as_deref(), Some("A changed\n"));
    }
}
