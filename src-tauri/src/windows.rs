//! Window visibility, and the macOS Dock-icon dance that goes with it.
//!
//! Talkie is a menu-bar app: with no window on screen it runs as an Accessory
//! (no Dock icon, no menu bar of its own). Showing a window flips it to Regular
//! so the window can actually take focus and accept typing; hiding the last one
//! flips it back.

use talkie_shared::WindowLabel;
use tauri::{AppHandle, Manager, Runtime};

pub fn show<R: Runtime>(app: &AppHandle<R>, label: WindowLabel) -> Result<(), String> {
    let window = app
        .get_webview_window(label.as_str())
        .ok_or_else(|| format!("no window labelled `{}`", label.as_str()))?;

    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);

    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())
}

pub fn hide<R: Runtime>(app: &AppHandle<R>, label: WindowLabel) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(label.as_str()) {
        window.hide().map_err(|e| e.to_string())?;
    }
    sync_activation_policy(app);
    Ok(())
}

/// Drop back to Accessory once nothing is on screen. Safe to call anywhere.
pub fn sync_activation_policy<R: Runtime>(app: &AppHandle<R>) {
    #[cfg(target_os = "macos")]
    {
        let any_visible = app
            .webview_windows()
            .values()
            .any(|w| w.is_visible().unwrap_or(false));
        if !any_visible {
            let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}
