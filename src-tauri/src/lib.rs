//! Talkie's Tauri host.
//!
//! Speak, and it lands in your notes: a global shortcut records, a local model
//! transcribes, and the text is appended to one long markdown file. This module
//! is the assembly point — the pipeline itself arrives in M1.

mod audio_toolkit;
mod commands;
mod hooks;
mod journal;
mod login_item;
mod models;
mod note;
mod recorder;
mod settings;
mod shortcut;
mod sounds;
mod transcriber;
mod tray;
mod watcher;
mod windows;

use std::sync::Arc;

use settings::SettingsState;
use talkie_shared::WindowLabel;
use tauri::{AppHandle, Manager, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let context = tauri::generate_context!();
    // The journal needs app-data before any app exists, so derive it the way
    // Tauri's `app_data_dir` does: the platform data dir plus the identifier.
    let data_dir = dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(&context.config().identifier);

    // `Talkie --debug-log` prints the last ten problems and leaves, before
    // the single-instance guard could hand this launch to a running Talkie.
    if journal::handle_flag(&data_dir) {
        return;
    }

    // Every failure path reports through `log`; the journal keeps the last
    // ten and tees the rest to stderr, so a silent app is not silent about
    // its own bugs.
    journal::init(&data_dir);

    tauri::Builder::default()
        // Registered first on purpose: a second instance exits inside this
        // plugin's init, before the store, tray, hotkey tap, or watcher exist.
        // Two Talkies would otherwise both hold the shortcut and capture every
        // utterance twice. macOS only dedupes launches of the *same bundle
        // path*, so a debug bundle beside an installed copy slips through.
        .plugin(tauri_plugin_single_instance::init(second_launch))
        .plugin(tauri_plugin_store::Builder::default().build())
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::set_settings,
            commands::complete_onboarding,
            commands::show_window,
            commands::hide_window,
            commands::get_model_status,
            commands::download_model,
            commands::request_microphone,
            commands::toggle_recording,
            commands::get_recorder_state,
            commands::read_note,
            commands::write_note,
            commands::start_shortcut_recording,
            commands::stop_shortcut_recording,
            commands::get_accessibility,
            commands::open_accessibility_settings,
            commands::retry_shortcut,
        ])
        .setup(|app| {
            let handle = app.handle().clone();

            let settings = settings::load(&handle);
            let first_run = !settings.onboarding_complete;
            commands::manage_settings(&handle, settings);

            handle.manage(Arc::new(recorder::Recorder::new(handle.clone())));

            tray::build(&handle)?;

            watcher::init(&handle);
            // A watch that cannot start is not fatal: the editor still opens
            // the file, it just will not notice edits made elsewhere.
            if let Err(e) = watcher::arm(&handle) {
                log::warn!("talkie: {e:#}");
            }

            shortcut::init(&handle);
            // Neither a bad accelerator nor a missing Accessibility grant may
            // stop the app from starting: log it and let the tray still drive
            // captures. On first run the grant arrives during onboarding, and
            // `apply` runs again once it does.
            if let Err(e) = shortcut::apply(&handle) {
                log::warn!("talkie: {e:#}");
            }

            // Menu-bar app: no Dock icon until a window is actually shown.
            #[cfg(target_os = "macos")]
            let _ = handle.set_activation_policy(tauri::ActivationPolicy::Accessory);

            if first_run {
                if let Err(e) = windows::show(&handle, WindowLabel::Onboarding) {
                    log::warn!("talkie: could not open onboarding: {e}");
                }
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            // ⌘W (and the traffic-light close button) hide the window; the app
            // keeps living in the menu bar.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
                windows::sync_activation_policy(window.app_handle());
            }
        })
        .run(context)
        .expect("error while running Talkie");
}

/// The running instance's side of a second launch.
///
/// The loser has already exited; the user clicked Talkie and wants it to
/// appear, so show whichever window they should be looking at. No dialog:
/// an "already running" alert would only explain something they never saw.
fn second_launch(app: &AppHandle, argv: Vec<String>, _cwd: String) {
    assert!(
        !argv.is_empty(),
        "a launch always carries its own executable path"
    );
    assert!(
        !journal::flagged(&argv),
        "`--debug-log` is answered before the guard and never reaches the primary"
    );
    // The listener is armed during plugin init, but a relaunch cannot
    // round-trip before `setup` has finished building the tray.
    debug_assert!(
        app.tray_by_id(tray::TRAY_ID).is_some(),
        "second launch reached the primary before setup finished"
    );

    let onboarding_complete = app
        .try_state::<SettingsState>()
        .and_then(|state| state.0.lock().ok().map(|s| s.onboarding_complete))
        .unwrap_or(true);
    let label = if onboarding_complete {
        WindowLabel::Editor
    } else {
        WindowLabel::Onboarding
    };
    if let Err(e) = windows::show(app, label) {
        log::warn!(
            "talkie: second launch could not show `{}`: {e}",
            label.as_str()
        );
    }
}
