//! Every command the webview can call. Names come from `talkie_shared::commands`
//! so the two sides can never drift apart silently.

use std::sync::{Arc, Mutex};

use talkie_shared::{events, ModelStatus, RecorderState, Settings, WindowLabel};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::recorder::Recorder;
use crate::settings::{self, SettingsState};
use crate::shortcut::ShortcutState;
use crate::{models, shortcut, windows};

#[tauri::command]
pub fn get_settings(state: State<'_, SettingsState>) -> Settings {
    state.0.lock().expect("settings mutex poisoned").clone()
}

#[tauri::command]
pub fn set_settings(
    app: AppHandle,
    state: State<'_, SettingsState>,
    settings: Settings,
) -> Result<(), String> {
    // Refuse an unbindable accelerator before anything is persisted: this
    // command is the store's only writer, and a saved-but-invalid shortcut would
    // leave every later launch without a hotkey.
    shortcut::validate(&settings.shortcut)?;

    let shortcut_changed = {
        let mut guard = state.0.lock().expect("settings mutex poisoned");
        let changed = guard.shortcut != settings.shortcut;
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

/// Convenience for `lib.rs`: seed the managed state at startup.
pub fn manage_settings(app: &AppHandle, settings: Settings) {
    app.manage(SettingsState(Mutex::new(settings)));
}
