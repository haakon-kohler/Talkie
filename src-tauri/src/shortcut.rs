//! The global shortcut — the only way into a capture that doesn't involve a
//! window.
//!
//! One accelerator serves both styles. In toggle mode a press starts and the
//! next press stops. In push-to-talk the press starts and the *release* stops,
//! which is why both edges are wired even when only one is in use.
//!
//! No accessibility permission is involved: Talkie never types into another app,
//! so the shortcut plugin and the microphone are the whole permission story.

use std::str::FromStr;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{Shortcut, ShortcutState};

use crate::recorder::Recorder;
use crate::settings::SettingsState;

/// Register the shortcut from settings, replacing whatever was bound before.
///
/// Called at startup and again whenever the accelerator changes in settings, so
/// it has to be idempotent.
pub fn apply(app: &AppHandle) -> Result<()> {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;

    let mut accelerator = {
        let state = app.state::<SettingsState>();
        let settings = state.0.lock().expect("settings mutex poisoned");
        settings.shortcut.clone()
    };

    let manager = app.global_shortcut();
    let _ = manager.unregister_all();

    if accelerator.trim().is_empty() {
        log::warn!("talkie: no shortcut configured; captures can only start from the tray");
        return Ok(());
    }

    // A value that predates validation (typed by hand, old store) must not
    // leave the app hotkey-less: fall back to the default rather than give up.
    let shortcut = match Shortcut::from_str(&accelerator) {
        Ok(shortcut) => shortcut,
        Err(e) => {
            log::warn!(
                "talkie: `{accelerator}` is not a valid shortcut ({e}); using {} instead",
                talkie_shared::DEFAULT_SHORTCUT
            );
            accelerator = talkie_shared::DEFAULT_SHORTCUT.to_string();
            Shortcut::from_str(&accelerator)
                .map_err(|e| anyhow!("the default shortcut `{accelerator}` is not valid: {e}"))?
        }
    };

    manager
        .register(shortcut)
        .with_context(|| format!("could not register the shortcut `{accelerator}`"))?;

    log::info!("talkie: shortcut bound to {accelerator}");
    Ok(())
}

/// Handle both edges of the accelerator. Registered once, in `lib.rs`, and kept
/// deliberately tiny: it decides what the press means and hands off.
pub fn on_event(app: &AppHandle, state: ShortcutState) {
    let push_to_talk = {
        let settings = app.state::<SettingsState>();
        let guard = settings.0.lock().expect("settings mutex poisoned");
        guard.push_to_talk
    };

    let recorder = app.state::<Arc<Recorder>>();

    match (push_to_talk, state) {
        // Hold to talk: the key's own edges are the start and the stop.
        (true, ShortcutState::Pressed) => recorder.start(),
        (true, ShortcutState::Released) => recorder.stop(),
        // Press to start, press again to stop. The release does nothing, or the
        // capture would end the instant it began.
        (false, ShortcutState::Pressed) => recorder.toggle(),
        (false, ShortcutState::Released) => {}
    }
}
