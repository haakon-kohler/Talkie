//! The menu-bar item. Talkie has no Dock presence, so this is the app's home.

use std::sync::Arc;

use talkie_shared::{RecorderState, WindowLabel};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Manager, Runtime};

use crate::recorder::Recorder;
use crate::windows;

/// The tray item's id, so `set_state` can find it again.
const TRAY_ID: &str = "talkie";

const ID_OPEN_NOTES: &str = "open_notes";
const ID_RECORD: &str = "record";
const ID_SETTINGS: &str = "settings";
const ID_QUIT: &str = "quit";

pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let open_notes = MenuItem::with_id(app, ID_OPEN_NOTES, "Notepad", true, None::<&str>)?;
    let record = MenuItem::with_id(app, ID_RECORD, "Record", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, ID_SETTINGS, "Settings…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, ID_QUIT, "Quit Talkie", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &open_notes,
            &record,
            &PredefinedMenuItem::separator(app)?,
            &settings,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| tauri::Error::AssetNotFound("default window icon".into()))?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("Talkie")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            ID_OPEN_NOTES => {
                if let Err(e) = windows::show(app, WindowLabel::Editor) {
                    eprintln!("talkie: could not open the editor window: {e}");
                }
            }
            ID_SETTINGS => {
                if let Err(e) = windows::show(app, WindowLabel::Settings) {
                    eprintln!("talkie: could not open the settings window: {e}");
                }
            }
            ID_RECORD => {
                app.state::<Arc<Recorder>>().toggle();
            }
            ID_QUIT => app.exit(0),
            _ => {}
        })
        .build(app)?;

    Ok(())
}

/// Reflect the capture state in the menu bar.
///
/// v1 has no recording overlay, so the tooltip and the menu item's wording are
/// the only visible sign that Talkie is listening. A real set of icon variants
/// is an M3 task, alongside the app icon.
pub fn set_state<R: Runtime>(app: &AppHandle<R>, state: RecorderState) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };

    let tooltip = match state {
        RecorderState::Idle => "Talkie",
        RecorderState::Recording => "Talkie — recording",
        RecorderState::Transcribing => "Talkie — transcribing",
    };
    let _ = TrayIcon::set_tooltip(&tray, Some(tooltip));
}
