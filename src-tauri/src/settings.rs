//! Settings live here and nowhere else.
//!
//! The UI has no store of its own: it reads through `get_settings` and writes
//! through `set_settings`, and every write is persisted before it is announced.

use std::sync::Mutex;

use talkie_shared::Settings;
use tauri::{AppHandle, Runtime};
use tauri_plugin_store::StoreExt;

const STORE_FILE: &str = "settings.json";
const KEY: &str = "settings";

/// The live settings, managed as Tauri state.
pub struct SettingsState(pub Mutex<Settings>);

/// `~/Documents/Talkie/talkie.md` — the one long file, until the user points
/// Talkie somewhere else (an Obsidian vault, say).
pub fn default_note_path() -> String {
    let base = dirs::document_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    base.join("Talkie")
        .join("talkie.md")
        .to_string_lossy()
        .into_owned()
}

/// Read settings from disk, filling in anything the stored blob predates.
pub fn load<R: Runtime>(app: &AppHandle<R>) -> Settings {
    let mut settings = app
        .store(STORE_FILE)
        .ok()
        .and_then(|store| store.get(KEY))
        .and_then(|value| serde_json::from_value::<Settings>(value).ok())
        .unwrap_or_default();

    if settings.note_path.trim().is_empty() {
        settings.note_path = default_note_path();
    }
    settings
}

pub fn save<R: Runtime>(app: &AppHandle<R>, settings: &Settings) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    let value = serde_json::to_value(settings).map_err(|e| e.to_string())?;
    store.set(KEY, value);
    store.save().map_err(|e| e.to_string())
}

/// Make the OS agree with the Start at Login setting.
///
/// Called at startup as well as on change, because the two can drift: the
/// LaunchAgent survives the app moving or the user clearing it by hand, and a
/// checkbox that no longer describes reality is worse than no checkbox.
/// Best-effort — a login item Talkie cannot write must not stop it starting.
pub fn sync_autostart<R: Runtime>(app: &AppHandle<R>, wanted: bool) {
    use tauri_plugin_autostart::ManagerExt;

    let autolaunch = app.autolaunch();
    let result = if wanted {
        autolaunch.enable()
    } else if autolaunch.is_enabled().unwrap_or(false) {
        autolaunch.disable()
    } else {
        Ok(())
    };
    if let Err(e) = result {
        log::warn!("talkie: could not update Start at Login: {e}");
    }
}
