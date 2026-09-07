//! Talkie's Tauri host.
//!
//! Speak, and it lands in your notes: a global shortcut records, a local model
//! transcribes, and the text is appended to one long markdown file. This module
//! is the assembly point — the pipeline itself arrives in M1.

mod audio_toolkit;
mod commands;
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

use talkie_shared::WindowLabel;
use tauri::{Manager, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Every failure path reports through `log`; without a backend those lines
    // vanish and a silent app stays silent about its own bugs.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    tauri::Builder::default()
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
                    eprintln!("talkie: could not open onboarding: {e}");
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
        .run(tauri::generate_context!())
        .expect("error while running Talkie");
}
