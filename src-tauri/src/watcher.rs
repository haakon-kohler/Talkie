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
//! timing, this module tracks a hash of the content Talkie last *knew about* —
//! set by `read_note` and `write_note` — and emits only when what lands on disk
//! differs from it. Self-inflicted events therefore cost one hash and stop
//! there, and the capture pipeline's `append` deliberately does not update the
//! hash, so a silent capture reaches the editor by exactly the same path an
//! external edit does.
//!
//! The directory is watched, not the file: editors save by writing a temporary
//! file and renaming it over the target — Talkie's own `note::write` included —
//! which replaces the inode and would silently detach a file watch.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Mutex;
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

/// The live watch, managed as Tauri state.
pub struct NoteWatcher {
    /// Dropping the previous watcher is what stops it, so re-arming is just a
    /// replace. The thread behind it ends on its own when the channel closes.
    watcher: Mutex<Option<RecommendedWatcher>>,
    /// The content Talkie last read or wrote. `None` before the first read,
    /// which makes the first external event unconditionally interesting.
    ///
    /// The text itself, not a hash of it: it is the *base* a save merges
    /// against when the file moved underneath the editor, and a hash cannot be
    /// diffed.
    seen: Mutex<Option<String>>,
}

impl NoteWatcher {
    fn new() -> Self {
        Self {
            watcher: Mutex::new(None),
            seen: Mutex::new(None),
        }
    }

    /// Remember content as Talkie's own, so the watcher stays quiet about it.
    pub fn remember(&self, text: &str) {
        *self.seen.lock().expect("watcher mutex poisoned") = Some(text.to_string());
    }

    /// What Talkie last saw on disk — the base for a merge.
    pub fn last_seen(&self) -> Option<String> {
        self.seen.lock().expect("watcher mutex poisoned").clone()
    }

    /// Whether `text` is news, and if so remember it. One call, because every
    /// caller does both and doing them separately invites a race.
    fn take_if_new(&self, text: &str) -> bool {
        let mut seen = self.seen.lock().expect("watcher mutex poisoned");
        if seen.as_deref() == Some(text) {
            return false;
        }
        *seen = Some(text.to_string());
        true
    }
}

/// Register the watcher state. Called once, from `lib.rs`, before `arm`.
pub fn init(app: &AppHandle) {
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

    let directory = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    // The folder need not exist yet — the first capture creates it — but there
    // is nothing to watch until it does.
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

    let handle = app.clone();
    let watched = path.clone();
    std::thread::Builder::new()
        .name("talkie-note-watcher".into())
        .spawn(move || {
            // Ends when the watcher is dropped and the sender goes with it,
            // which is how re-arming retires the previous thread.
            while let Ok(first) = rx.recv() {
                if !concerns(&first, &watched) {
                    continue;
                }
                // Drain the rest of the flurry before reading: keep swallowing
                // events until SETTLE passes with nothing new.
                while rx.recv_timeout(SETTLE).is_ok() {}
                report(&handle, &watched);
            }
        })
        .context("could not start the note watcher thread")?;

    *app.state::<NoteWatcher>()
        .watcher
        .lock()
        .expect("watcher mutex poisoned") = Some(watcher);

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

/// Read the file and tell the editor, if what is there is not already ours.
fn report(app: &AppHandle, path: &Path) {
    let text = match note::read(path) {
        Ok(text) => text,
        Err(e) => {
            log::warn!("talkie: could not read {}: {e:#}", path.display());
            return;
        }
    };

    if !app.state::<NoteWatcher>().take_if_new(&text) {
        return;
    }

    if let Err(e) = app.emit(events::NOTE_CHANGED_EXTERNALLY, ()) {
        log::error!("talkie: could not announce a note change: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_content_is_only_news_once() {
        let watcher = NoteWatcher::new();
        assert!(watcher.take_if_new("hello"));
        assert!(!watcher.take_if_new("hello"));
        assert!(watcher.take_if_new("hello there"));
    }

    #[test]
    fn remembering_our_own_write_silences_it() {
        let watcher = NoteWatcher::new();
        watcher.remember("what the editor just saved");
        assert!(!watcher.take_if_new("what the editor just saved"));
    }
}
