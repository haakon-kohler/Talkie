//! Every command the webview can call. Names come from `talkie_shared::commands`
//! so the two sides can never drift apart silently.

use std::sync::Mutex;

use talkie_shared::{events, Settings, WindowLabel};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::settings::{self, SettingsState};
use crate::windows;

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
    settings::save(&app, &settings)?;
    *state.0.lock().expect("settings mutex poisoned") = settings.clone();
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

/// Convenience for `lib.rs`: seed the managed state at startup.
pub fn manage_settings(app: &AppHandle, settings: Settings) {
    app.manage(SettingsState(Mutex::new(settings)));
}
