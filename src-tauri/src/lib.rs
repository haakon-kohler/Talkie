//! Talkie's Tauri host.
//!
//! Speak, and it lands in your notes: a global shortcut records, a local model
//! transcribes, and the text is appended to one long markdown file. This module
//! is the assembly point — the pipeline itself arrives in M1.

mod commands;
mod settings;
mod tray;
mod windows;

use talkie_shared::WindowLabel;
use tauri::{Manager, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::default().build())
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::set_settings,
            commands::complete_onboarding,
            commands::show_window,
            commands::hide_window,
        ])
        .setup(|app| {
            let handle = app.handle().clone();

            let settings = settings::load(&handle);
            let first_run = !settings.onboarding_complete;
            commands::manage_settings(&handle, settings);

            tray::build(&handle)?;

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
