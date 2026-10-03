//! Every command the webview can call. Names come from `talkie_shared::commands`
//! so the two sides can never drift apart silently.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use talkie_shared::{
    document, events, MicrophoneInfo, ModelStatus, Note, RecorderState, Settings, WindowLabel,
};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::recorder::Recorder;
use crate::settings::{self, SettingsState};
use crate::shortcut::ShortcutState;
use crate::watcher::NoteWatcher;
use crate::{login_item, models, note, panel, shortcut, watcher, windows};

#[tauri::command]
pub fn get_settings(state: State<'_, SettingsState>) -> Settings {
    let mut guard = state.0.lock().expect("settings mutex poisoned");
    // The login item can be switched off in System Settings behind Talkie's
    // back; macOS, not the store, knows whether it is on.
    guard.launch_at_login = login_item::enabled();
    guard.clone()
}

#[tauri::command]
pub fn set_settings(
    app: AppHandle,
    state: State<'_, SettingsState>,
    mut settings: Settings,
) -> Result<(), String> {
    // Refuse anything unusable before it is persisted: this command is the
    // store's only writer, so a saved-but-invalid value would come back on
    // every later launch. A shortcut that will not bind leaves the app without
    // a hotkey; a notes path that cannot be written fails on the first capture,
    // silently, long after the typo was made.
    shortcut::validate(&settings.shortcut)?;
    settings.note_path = settings.note_path.trim().to_string();
    note::validate(&settings.note_path)?;
    // Same reasoning for the login item: register it first, and only persist
    // a switch macOS actually honoured.
    login_item::apply(settings.launch_at_login).map_err(|e| format!("{e:#}"))?;

    let (shortcut_changed, note_path_changed) = {
        let mut guard = state.0.lock().expect("settings mutex poisoned");
        let changed = (
            guard.shortcut != settings.shortcut,
            guard.note_path != settings.note_path,
        );
        settings::save(&app, &settings)?;
        *guard = settings.clone();
        changed
    };

    // Re-bind before announcing: by the time the UI hears about the new
    // accelerator, it is the one the OS will actually deliver.
    if shortcut_changed {
        if let Err(e) = shortcut::apply(&app) {
            return Err(format!("{e:#}"));
        }
    }

    // Point the watcher at the new file before announcing the change, so the
    // editor's reload lands on a file that is actually being watched. The
    // editor saves what it still holds of the old file to the old file first:
    // every save names the file its text came from.
    if note_path_changed {
        if let Err(e) = watcher::arm(&app) {
            log::warn!("talkie: {e:#}");
        }
    }

    app.emit(events::SETTINGS_CHANGED, &settings)
        .map_err(|e| e.to_string())
}

/// Finish first run: remember it, close the onboarding window, open the editor.
#[tauri::command]
pub fn complete_onboarding(app: AppHandle, state: State<'_, SettingsState>) -> Result<(), String> {
    let settings = {
        let mut guard = state.0.lock().expect("settings mutex poisoned");
        guard.onboarding_complete = true;
        guard.clone()
    };
    settings::save(&app, &settings)?;
    let _ = app.emit(events::SETTINGS_CHANGED, &settings);

    windows::hide(&app, WindowLabel::Onboarding)?;
    windows::show(&app, WindowLabel::Editor)
}

#[tauri::command]
pub fn show_window(app: AppHandle, label: WindowLabel) -> Result<(), String> {
    windows::show(&app, label)
}

#[tauri::command]
pub fn hide_window(app: AppHandle, label: WindowLabel) -> Result<(), String> {
    windows::hide(&app, label)
}

#[tauri::command]
pub fn get_model_status(app: AppHandle) -> ModelStatus {
    models::status(&app)
}

/// Fetch the speech model. Progress arrives as `MODEL_PROGRESS` events rather
/// than through this call's return value, which resolves only at the end.
#[tauri::command]
pub async fn download_model(app: AppHandle) -> Result<(), String> {
    models::download(app).await.map_err(|e| format!("{e:#}"))
}

/// Trip the macOS microphone prompt during onboarding, instead of letting it
/// appear mid-capture the first time the shortcut is pressed.
///
/// There is no "ask" API in cpal: opening an input stream *is* the request, so
/// this opens one and immediately drops it.
#[tauri::command]
pub async fn request_microphone() -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let mut recorder = crate::audio_toolkit::AudioRecorder::new().map_err(|e| e.to_string())?;
        match recorder.open(None) {
            Ok(()) => {
                let _ = recorder.close();
                Ok(true)
            }
            Err(e) => {
                let message = e.to_string();
                if crate::audio_toolkit::is_microphone_access_denied(&message) {
                    Ok(false)
                } else {
                    Err(message)
                }
            }
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Start or stop a capture — the same entry point the shortcut uses, so the UI
/// and the tray can never drift into a different state machine.
#[tauri::command]
pub fn toggle_recording(recorder: State<'_, Arc<Recorder>>) {
    recorder.toggle();
}

#[tauri::command]
pub fn get_recorder_state(recorder: State<'_, Arc<Recorder>>) -> RecorderState {
    recorder.state()
}

/// The note file, as the editor should show it, and which file that is — the
/// editor names it again on every save.
///
/// Reading makes this text the base the editor's next save merges against, and
/// tells the watcher what Talkie now believes is on disk, so the events this
/// read may have raced with do not come back as a phantom external change.
#[tauri::command]
pub fn read_note(app: AppHandle, state: State<'_, SettingsState>) -> Result<Note, String> {
    let path = {
        let guard = state.0.lock().expect("settings mutex poisoned");
        note::resolve(&guard.note_path)
    };
    let text = load_note(&path, &app.state::<NoteWatcher>())?;
    Ok(Note {
        path: path.to_string_lossy().into_owned(),
        text,
    })
}

/// `read_note` without the app around it.
fn load_note(path: &Path, watcher: &NoteWatcher) -> Result<String, String> {
    let text = note::read(path).map_err(|e| format!("{e:#}"))?;
    watcher.adopt(path, &text);
    Ok(text)
}

/// The editor's autosave.
///
/// Returns the text that is now on disk, which is usually exactly what came in —
/// but not always, so the editor applies the return value rather than assuming.
///
/// ## Why this is not a plain write
///
/// The editor can hold unsaved edits while a capture lands, and a capture is a
/// silent append to the same file. A blind write would then throw away whatever
/// was spoken, which is the one thing Talkie must never do. So the save compares
/// what is on disk against the text the editor last saw:
///
/// - unchanged → write the editor's text, the ordinary case;
/// - **appended to** (a capture, or an agent adding at the end) → keep both, the
///   editor's text followed by what was appended;
/// - **changed some other way** → refuse. Nothing is lost on either side, the
///   editor keeps its text and says so, and the next external change reloads it.
///
/// "The text the editor last saw" is what it last read or saved, never what
/// the watcher has seen since: a capture the editor has not taken in yet is
/// exactly what the merge has to find (#5).
///
/// `path` is the file the editor's text came from, as `read_note` named it —
/// not necessarily the configured one, which may have moved on while edits to
/// the old file were waiting to be saved. It is only ever the file the editor
/// last read or saved; a save to any other file has no base and is refused.
#[tauri::command]
pub fn write_note(app: AppHandle, path: String, text: String) -> Result<String, String> {
    save_note(&PathBuf::from(path), &text, &app.state::<NoteWatcher>())
}

/// `write_note` without the app around it.
fn save_note(path: &Path, text: &str, watcher: &NoteWatcher) -> Result<String, String> {
    let Some(base) = watcher.base(path) else {
        log::warn!(
            "talkie: refused a save to {}, which the editor did not read last",
            path.display()
        );
        // COPY: editor.trouble.moved
        return Err(
            "This text is from a notes file Talkie has stopped using, so it was not saved."
                .to_string(),
        );
    };
    let on_disk = note::read(path).map_err(|e| format!("{e:#}"))?;

    let to_write = match document::reconcile(text, &on_disk, Some(&base)) {
        document::Save::Write(text) => text,
        document::Save::Conflict => {
            log::warn!(
                "talkie: refused a save to {}, which changed in a way it cannot merge",
                path.display()
            );
            // COPY: editor.trouble.conflict
            return Err(
                "The notes file changed outside Talkie, so this text was not saved.".to_string(),
            );
        }
    };

    // What goes to disk is what the editor gets back, byte for byte, or the
    // next save would mistake the difference for someone else's capture.
    debug_assert!(
        document::normalized(&to_write) == to_write,
        "reconcile returned text that is not in the on-disk shape"
    );

    // Adopted before the write so the change notification it causes is
    // recognised as Talkie's own and never bounces back into the editor.
    watcher.adopt(path, &to_write);
    note::write(path, &to_write).map_err(|e| format!("{e:#}"))?;

    Ok(to_write)
}

/// Put the hotkey engine into recording mode.
///
/// The live binding is released for the duration, so the user can press the
/// shortcut they already have without starting a capture, and raw key events
/// arrive in the UI as `SHORTCUT_CAPTURE`.
#[tauri::command]
pub fn start_shortcut_recording(state: State<'_, ShortcutState>) -> Result<(), String> {
    state.start_recording()
}

/// Leave recording mode and re-bind whatever settings now hold.
#[tauri::command]
pub fn stop_shortcut_recording(state: State<'_, ShortcutState>) -> Result<(), String> {
    state.stop_recording()
}

/// Whether macOS has granted Accessibility, which the event tap behind every
/// global shortcut needs.
#[tauri::command]
pub fn get_accessibility() -> bool {
    shortcut::accessibility_granted()
}

#[tauri::command]
pub fn open_accessibility_settings() -> Result<(), String> {
    shortcut::open_accessibility_settings()
}

/// Bind the shortcut again.
///
/// The hotkey engine builds its event tap lazily and retries on every bind, so
/// this is all it takes to come back from a start-up where Accessibility had not
/// been granted yet — no restart, no reinstall.
#[tauri::command]
pub fn retry_shortcut(app: AppHandle) -> Result<(), String> {
    shortcut::apply(&app).map_err(|e| format!("{e:#}"))
}

/// Let the user point at the notes file with the native panel instead of
/// typing a path.
///
/// Returns the path the way the settings form should show it, or `None` for a
/// cancelled panel. Nothing is persisted here: the form puts the answer in the
/// field, and Save runs it through the same validation as a typed path.
#[tauri::command]
pub async fn pick_note_path(
    app: AppHandle,
    state: State<'_, SettingsState>,
) -> Result<Option<String>, String> {
    let current = {
        let guard = state.0.lock().expect("settings mutex poisoned");
        note::resolve(&guard.note_path)
    };
    let chosen = panel::choose_note_path(&app, &current)
        .await
        .map_err(|e| format!("{e:#}"))?;
    Ok(chosen.map(|path| path.to_string_lossy().into_owned()))
}

/// The input devices the OS reports right now.
///
/// Enumerated on every call rather than cached: there is no device-change
/// notification to hang a cache on, and a headset plugged in while settings is
/// open should show up the next time the list is opened.
#[tauri::command]
pub async fn list_microphones() -> Result<Vec<MicrophoneInfo>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let devices = crate::audio_toolkit::list_input_devices().map_err(|e| e.to_string())?;
        Ok(devices
            .into_iter()
            .map(|d| MicrophoneInfo {
                name: d.name,
                is_default: d.is_default,
            })
            .collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Convenience for `lib.rs`: seed the managed state at startup.
pub fn manage_settings(app: &AppHandle, settings: Settings) {
    assert!(
        app.try_state::<SettingsState>().is_none(),
        "settings state was registered twice"
    );
    debug_assert!(
        !settings.note_path.trim().is_empty(),
        "settings::load must resolve a note path before it is managed"
    );
    app.manage(SettingsState(Mutex::new(settings)));
}

#[cfg(test)]
mod tests {
    //! Issue #5 — the notes path changes under an open editor — driven the way
    //! the app drives it: `load_note` is the editor's `read_note`, `save_note`
    //! its autosave, `NoteWatcher::watch` what `set_settings` does when the
    //! path changes, and `note::prepend` a capture.
    //!
    //! The editor that never hears about the change is simulated by saving
    //! the text it last loaded to the configured path, which the real editor
    //! no longer does — it names the file its text came from — but which must
    //! stay harmless for any way into that state.

    use std::fs;
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    const A: &str = "## 2026-10-01 09:00\nOld file\n";
    const B: &str = "## 2026-10-02 10:00\nNew file\n";

    /// How long to wait for the watcher to report. Only ever spent in full
    /// when a test is about to fail.
    const PATIENCE: Duration = Duration::from_secs(5);

    /// Two note paths in two folders, the way switching vaults looks. Neither
    /// file exists yet.
    fn two_notes(name: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("talkie-commands-test-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("a")).unwrap();
        fs::create_dir_all(dir.join("b")).unwrap();
        // Canonical, because FSEvents reports the real path and the macOS
        // temporary folder sits behind a symlink.
        let dir = dir.canonicalize().unwrap();
        (dir.join("a/talkie.md"), dir.join("b/talkie.md"))
    }

    /// Point the watcher at `path`, as `arm` does at startup and on a path
    /// change. The reports the editor would hear arrive on the channel.
    fn arm(watcher: &NoteWatcher, path: &Path) -> mpsc::Receiver<()> {
        let (tx, rx) = mpsc::channel();
        watcher
            .watch(path, move || {
                let _ = tx.send(());
            })
            .expect("watch");
        rx
    }

    fn contents(path: &Path) -> String {
        note::read(path).expect("read")
    }

    /// The first save after the switch carries A's text. It has to be
    /// refused; a fix that forgets the merge base on a path change turns this
    /// into a write over B.
    #[test]
    fn a_stale_save_after_a_path_change_is_refused() {
        let (a, b) = two_notes("stale-refused");
        fs::write(&a, A).unwrap();
        fs::write(&b, B).unwrap();
        let watcher = NoteWatcher::new();
        let _reports = arm(&watcher, &a);
        let shown = load_note(&a, &watcher).expect("load");

        let _reports = arm(&watcher, &b);
        let saved = save_note(&b, &format!("{shown}typed\n"), &watcher);

        assert!(saved.is_err(), "A's text was saved into B: {saved:?}");
        assert_eq!(contents(&b), B);
        assert_eq!(contents(&a), A);
    }

    /// Switching to a path that does not exist yet: the stale save must not
    /// create B with A's text in it.
    #[test]
    fn a_stale_save_to_a_new_path_creates_nothing() {
        let (a, b) = two_notes("stale-new-path");
        fs::write(&a, A).unwrap();
        let watcher = NoteWatcher::new();
        let _reports = arm(&watcher, &a);
        let shown = load_note(&a, &watcher).expect("load");

        let _reports = arm(&watcher, &b);
        let saved = save_note(&b, &format!("{shown}typed\n"), &watcher);

        assert!(saved.is_err(), "A's text was saved into B: {saved:?}");
        assert!(!b.exists(), "B was created with {:?}", contents(&b));
    }

    /// The issue's step 4: a capture lands in B, the watcher reports it, the
    /// dirty editor ignores the report, and the next keystroke used to save
    /// A's text over B — capture included.
    #[test]
    fn a_stale_save_never_overwrites_the_new_file() {
        let (a, b) = two_notes("stale-overwrite");
        fs::write(&a, A).unwrap();
        fs::write(&b, B).unwrap();
        let watcher = NoteWatcher::new();
        let _reports = arm(&watcher, &a);
        let stale = format!("{}typed\n", load_note(&a, &watcher).expect("load"));

        let reports = arm(&watcher, &b);
        assert!(save_note(&b, &stale, &watcher).is_err());

        note::prepend(&b, "Spoken into B").expect("capture");
        let after_capture = contents(&b);
        reports
            .recv_timeout(PATIENCE)
            .expect("the watcher never reported the capture");
        let _ = save_note(&b, &stale, &watcher);

        assert_eq!(contents(&b), after_capture, "B was overwritten");
        assert_eq!(contents(&a), A);
    }

    /// The way out the UI fix will take: once the editor has read B, saves go
    /// to B as usual and A is left alone.
    #[test]
    fn the_editor_saves_to_the_new_file_once_it_has_read_it() {
        let (a, b) = two_notes("reloaded");
        fs::write(&a, A).unwrap();
        fs::write(&b, B).unwrap();
        let watcher = NoteWatcher::new();
        let _reports = arm(&watcher, &a);
        load_note(&a, &watcher).expect("load");

        let _reports = arm(&watcher, &b);
        let shown = load_note(&b, &watcher).expect("load");
        let edited = format!("{shown}typed\n");

        assert_eq!(save_note(&b, &edited, &watcher), Ok(edited.clone()));
        assert_eq!(contents(&b), edited);
        assert_eq!(contents(&a), A);
    }

    /// The same for a path that does not exist yet: the first save creates B
    /// with only what was typed.
    #[test]
    fn the_editor_creates_the_new_file_once_it_has_read_it() {
        let (a, b) = two_notes("reloaded-new-path");
        fs::write(&a, A).unwrap();
        let watcher = NoteWatcher::new();
        let _reports = arm(&watcher, &a);
        load_note(&a, &watcher).expect("load");

        let _reports = arm(&watcher, &b);
        assert_eq!(load_note(&b, &watcher), Ok(String::new()));

        assert_eq!(save_note(&b, "typed", &watcher), Ok("typed\n".to_string()));
        assert_eq!(contents(&b), "typed\n");
        assert_eq!(contents(&a), A);
    }

    /// A → B → A, reading each time: the editor ends up saving A, and B is
    /// never touched.
    #[test]
    fn switching_back_saves_to_the_old_file_and_leaves_the_new_one() {
        let (a, b) = two_notes("switch-back");
        fs::write(&a, A).unwrap();
        fs::write(&b, B).unwrap();
        let watcher = NoteWatcher::new();
        let _reports = arm(&watcher, &a);
        load_note(&a, &watcher).expect("load");
        let _reports = arm(&watcher, &b);
        load_note(&b, &watcher).expect("load");

        let _reports = arm(&watcher, &a);
        let edited = format!("{}typed\n", load_note(&a, &watcher).expect("load"));

        assert_eq!(save_note(&a, &edited, &watcher), Ok(edited.clone()));
        assert_eq!(contents(&a), edited);
        assert_eq!(contents(&b), B);
    }

    /// Edits still waiting to be saved when the path changes name the file
    /// they came from, and land there rather than in the new one.
    #[test]
    fn edits_waiting_at_a_switch_land_in_the_old_file() {
        let (a, b) = two_notes("waiting-edits");
        fs::write(&a, A).unwrap();
        let watcher = NoteWatcher::new();
        let _reports = arm(&watcher, &a);
        let edited = format!("{}typed\n", load_note(&a, &watcher).expect("load"));

        let _reports = arm(&watcher, &b);
        assert_eq!(save_note(&a, &edited, &watcher), Ok(edited.clone()));

        assert_eq!(contents(&a), edited);
        assert!(!b.exists(), "B was created with {:?}", contents(&b));
    }

    /// Once the editor has read B, a save of A that was still in flight is
    /// refused: the base belongs to B now, and A's text is not merged against
    /// it in either file.
    #[test]
    fn a_save_of_the_old_file_after_reading_the_new_one_is_refused() {
        let (a, b) = two_notes("in-flight-edits");
        fs::write(&a, A).unwrap();
        fs::write(&b, B).unwrap();
        let watcher = NoteWatcher::new();
        let _reports = arm(&watcher, &a);
        let edited = format!("{}typed\n", load_note(&a, &watcher).expect("load"));

        let _reports = arm(&watcher, &b);
        load_note(&b, &watcher).expect("load");

        assert!(save_note(&a, &edited, &watcher).is_err());
        assert_eq!(contents(&a), A);
        assert_eq!(contents(&b), B);
    }

    /// The issue's "switch back" case without the UI reload, and the worst
    /// version of step 4: the editor follows a capture into B, the path goes
    /// back to A, and once a capture into A was reported the editor's copy of
    /// B used to replace A's entire history.
    #[test]
    fn switching_back_without_a_reload_never_overwrites_the_old_file() {
        let (a, b) = two_notes("switch-back-stale");
        fs::write(&a, A).unwrap();
        fs::write(&b, B).unwrap();
        let watcher = NoteWatcher::new();
        let _reports = arm(&watcher, &a);
        load_note(&a, &watcher).expect("load");

        // On B, a capture is reported and the clean editor reloads.
        let reports = arm(&watcher, &b);
        note::prepend(&b, "Spoken into B").expect("capture");
        reports.recv_timeout(PATIENCE).expect("no report from B");
        let stale = format!("{}typed\n", load_note(&b, &watcher).expect("load"));

        // Back on A, the editor still shows B.
        let reports = arm(&watcher, &a);
        assert!(save_note(&a, &stale, &watcher).is_err());
        note::prepend(&a, "Spoken into A").expect("capture");
        let after_capture = contents(&a);
        reports.recv_timeout(PATIENCE).expect("no report from A");
        let _ = save_note(&a, &stale, &watcher);

        assert_eq!(contents(&a), after_capture, "A was overwritten");
    }

    // The same mechanism as step 4, with no path change at all. The merge in
    // `save_note` carries a capture over only while the merge base still
    // predates it, and the watcher used to advance the base as soon as the
    // file settled — 150 ms after the capture, well inside the editor's
    // 600 ms debounce.

    /// The merge works when the save gets there before the watcher does.
    #[test]
    fn a_capture_mid_edit_is_carried_over_when_the_save_beats_the_watcher() {
        let (a, _) = two_notes("capture-before-report");
        fs::write(&a, A).unwrap();
        let watcher = NoteWatcher::new();
        let edited = load_note(&a, &watcher)
            .expect("load")
            .replace("Old", "Edited");

        note::prepend(&a, "Spoken mid-edit").expect("capture");
        let saved = save_note(&a, &edited, &watcher).expect("save");

        assert!(saved.contains("Spoken mid-edit"), "lost: {saved:?}");
        assert!(saved.contains("Edited file"), "lost: {saved:?}");
        assert_eq!(contents(&a), saved);
    }

    /// And when the watcher gets there first: the editor is dirty, so it
    /// ignores the report, and the save used to find base and disk equal and
    /// write over the capture.
    #[test]
    fn a_capture_mid_edit_is_carried_over_after_the_watcher_reports_it() {
        let (a, _) = two_notes("capture-after-report");
        fs::write(&a, A).unwrap();
        let watcher = NoteWatcher::new();
        let reports = arm(&watcher, &a);
        let edited = load_note(&a, &watcher)
            .expect("load")
            .replace("Old", "Edited");

        note::prepend(&a, "Spoken mid-edit").expect("capture");
        reports
            .recv_timeout(PATIENCE)
            .expect("the watcher never reported the capture");
        let saved = save_note(&a, &edited, &watcher);

        let now = contents(&a);
        assert!(
            now.contains("Spoken mid-edit"),
            "the capture was lost: {now:?}"
        );
        assert!(saved.is_err() || now.contains("Edited file"), "{now:?}");
    }
}
