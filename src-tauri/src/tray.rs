//! The menu-bar item. Talkie has no Dock presence, so this is the app's home.

use talkie_shared::WindowLabel;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Runtime};

use crate::windows;

const ID_OPEN_NOTES: &str = "open_notes";
const ID_RECORD: &str = "record";
const ID_SETTINGS: &str = "settings";
const ID_QUIT: &str = "quit";

pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let open_notes = MenuItem::with_id(app, ID_OPEN_NOTES, "Open Notes", true, None::<&str>)?;
    // Disabled until the capture pipeline exists (M1). The item is here now so
    // the menu's shape stops changing under the user later.
    let record = MenuItem::with_id(app, ID_RECORD, "Record", false, None::<&str>)?;
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

    TrayIconBuilder::with_id("talkie")
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
            ID_QUIT => app.exit(0),
            _ => {}
        })
        .build(app)?;

    Ok(())
}
