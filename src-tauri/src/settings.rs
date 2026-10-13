//! Settings live here and nowhere else.
//!
//! The UI has no store of its own: it reads through `get_settings` and writes
//! through `set_settings`, and every write is persisted before it is announced.

use std::sync::{Mutex, MutexGuard};

use talkie_shared::Settings;
use tauri::{AppHandle, Runtime};
use tauri_plugin_store::StoreExt;

const STORE_FILE: &str = "settings.json";
const KEY: &str = "settings";

/// Which shape of blob is in the store, kept beside it under its own key.
///
/// `#[serde(default)]` covers a field that is *absent*; it cannot tell a value
/// an old version wrote out from one the user chose. When a default changes,
/// only a version number can say which stored values predate it.
const SCHEMA_KEY: &str = "schema";

/// Bump when a stored blob needs rewriting rather than merely filling in, and
/// add the step to `migrate`.
///
/// 1 — push-to-talk became the default (M2.6). Earlier versions wrote
///     `push_to_talk: false` into every store, which serde faithfully read
///     back, so a flipped default never reached an existing install.
const SCHEMA: u64 = 1;

/// The live settings, managed as Tauri state.
pub struct SettingsState(pub Mutex<Settings>);

impl SettingsState {
    /// The settings, for code that can report a failure rather than panic.
    ///
    /// Only a panic while the lock is held poisons it, and a release build
    /// aborts on the panic first, so this fails only in a debug build after
    /// some other bug.
    pub fn lock(&self) -> Result<MutexGuard<'_, Settings>, String> {
        self.0
            .lock()
            .map_err(|e| format!("the settings are unavailable: {e}"))
    }
}

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

/// Read settings from disk, filling in anything the stored blob predates and
/// rewriting anything an older version wrote differently.
pub fn load<R: Runtime>(app: &AppHandle<R>) -> Settings {
    let store = app.store(STORE_FILE).ok();
    let stored = store.as_ref().and_then(|store| store.get(KEY));
    let schema = store
        .as_ref()
        .and_then(|store| store.get(SCHEMA_KEY))
        .and_then(|value| value.as_u64())
        .unwrap_or(0);

    let had_blob = stored.is_some();
    let mut settings = stored
        .and_then(|value| serde_json::from_value::<Settings>(value).ok())
        .unwrap_or_default();

    if settings.note_path.trim().is_empty() {
        settings.note_path = default_note_path();
    }
    settings.launch_at_login = crate::login_item::enabled();

    // A fresh install has nothing to migrate; its first save stamps the schema.
    if had_blob && schema < SCHEMA {
        migrate(&mut settings, schema);
        if let Err(e) = save(app, &settings) {
            log::warn!("talkie: could not persist migrated settings: {e}");
        }
    }
    settings
}

/// Bring a blob written under schema `from` up to `SCHEMA`.
fn migrate(settings: &mut Settings, from: u64) {
    if from < 1 {
        // Every store before schema 1 has `push_to_talk: false` written out,
        // whether or not anyone chose it — it was the default. Apply the
        // current default the way a fresh install gets it.
        settings.push_to_talk = Settings::default().push_to_talk;
    }
}

pub fn save<R: Runtime>(app: &AppHandle<R>, settings: &Settings) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    let value = serde_json::to_value(settings).map_err(|e| e.to_string())?;
    store.set(KEY, value);
    store.set(SCHEMA_KEY, SCHEMA);
    store.save().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_zero_takes_the_push_to_talk_default() {
        let mut settings = Settings {
            push_to_talk: false,
            ..Settings::default()
        };
        migrate(&mut settings, 0);
        assert!(settings.push_to_talk);
    }

    #[test]
    fn a_current_blob_is_left_alone() {
        let mut settings = Settings {
            push_to_talk: false,
            ..Settings::default()
        };
        migrate(&mut settings, SCHEMA);
        assert!(!settings.push_to_talk);
    }

    /// Commands that can fail report a poisoned settings lock as an error
    /// rather than panicking a second time.
    #[test]
    fn a_poisoned_lock_is_an_error_not_a_panic() {
        let state = SettingsState(Mutex::new(Settings::default()));
        assert!(state.lock().is_ok());

        let poisoner = std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let _guard = state.0.lock();
                    panic!("poison the settings lock");
                })
                .join()
        });
        assert!(
            poisoner.is_err(),
            "the poisoning thread should have panicked"
        );

        let error = state.lock().expect_err("a poisoned lock must be an error");
        assert!(error.starts_with("the settings are unavailable"), "{error}");
    }
}
