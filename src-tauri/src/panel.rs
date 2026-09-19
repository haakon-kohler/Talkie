//! The native file panel behind the notes-file field.
//!
//! An `NSOpenPanel` that takes a folder *or* a file: pick a folder and the
//! notes file is `talkie.md` inside it, pick an existing markdown file and
//! that file is the notes file. A save panel was the obvious alternative and
//! was rejected because clicking an existing file in one asks "replace it?" —
//! Talkie never replaces anything, it appends, and the wrong question at that
//! moment is worse than no panel.
//!
//! Nothing here persists: the panel answers with a path and the settings form
//! decides what to do with it, through the same `set_settings` validation a
//! typed path gets.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use tauri::AppHandle;

/// The file a folder choice resolves to.
pub const DEFAULT_FILE_NAME: &str = "talkie.md";

/// Open the panel as a sheet on the settings window and wait for an answer.
///
/// `current` seeds the panel's starting folder. Resolves to `None` when the
/// panel is cancelled.
#[cfg(target_os = "macos")]
pub async fn choose_note_path(app: &AppHandle, current: &Path) -> Result<Option<PathBuf>> {
    use std::cell::RefCell;

    use block2::RcBlock;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSModalResponse, NSOpenPanel, NSWindow};
    use objc2_foundation::{NSString, NSURL};
    use talkie_shared::WindowLabel;
    use tauri::Manager;

    /// `NSModalResponseOK` is a preprocessor constant in AppKit's headers,
    /// which is why the bindings have no name for it.
    const NS_MODAL_RESPONSE_OK: NSModalResponse = 1;

    let window = app
        .get_webview_window(WindowLabel::Settings.as_str())
        .ok_or_else(|| anyhow!("no settings window to attach the file panel to"))?;
    let start_dir = current
        .parent()
        .filter(|dir| dir.is_dir())
        .map(Path::to_path_buf);

    let (tx, rx) = tokio::sync::oneshot::channel::<Option<PathBuf>>();
    app.run_on_main_thread(move || {
        let mtm = MainThreadMarker::new().expect("run_on_main_thread runs on the main thread");
        let ns_window = match window.ns_window() {
            Ok(ptr) => ptr,
            Err(e) => {
                log::warn!("talkie: could not reach the settings window for the file panel: {e}");
                return;
            }
        };
        // SAFETY: `ns_window` returns an autoreleased `NSWindow*` for a
        // window that is alive for as long as the app is, and this is the
        // main thread, where AppKit objects may be touched.
        let ns_window: &NSWindow = unsafe { &*ns_window.cast::<NSWindow>() };

        let panel = NSOpenPanel::openPanel(mtm);
        panel.setCanChooseFiles(true);
        panel.setCanChooseDirectories(true);
        panel.setCanCreateDirectories(true);
        panel.setAllowsMultipleSelection(false);
        // COPY: settings.note_path.panel_prompt
        panel.setPrompt(Some(&NSString::from_str("Choose")));
        if let Some(dir) = start_dir {
            let url = NSURL::fileURLWithPath_isDirectory(
                &NSString::from_str(&dir.to_string_lossy()),
                true,
            );
            panel.setDirectoryURL(Some(&url));
        }

        // The completion block is `Fn`, so the one-shot sender sits behind a
        // cell it can be taken out of exactly once.
        let tx = RefCell::new(Some(tx));
        let chosen = panel.clone();
        let handler = RcBlock::new(move |response: NSModalResponse| {
            let Some(tx) = tx.borrow_mut().take() else {
                return;
            };
            let path = (response == NS_MODAL_RESPONSE_OK)
                .then(|| chosen.URL())
                .flatten()
                .and_then(|url| url.path())
                .map(|path| resolve_choice(PathBuf::from(path.to_string())));
            let _ = tx.send(path);
        });
        panel.beginSheetModalForWindow_completionHandler(ns_window, &handler);
    })
    .map_err(|e| anyhow!("could not open the file panel: {e}"))?;

    rx.await
        .map_err(|_| anyhow!("the file panel closed without answering"))
}

#[cfg(not(target_os = "macos"))]
pub async fn choose_note_path(_app: &AppHandle, _current: &Path) -> Result<Option<PathBuf>> {
    Err(anyhow!("the file panel is only built for macOS"))
}

/// A folder means "the notes file goes in here"; a file means itself.
fn resolve_choice(chosen: PathBuf) -> PathBuf {
    if chosen.is_dir() {
        chosen.join(DEFAULT_FILE_NAME)
    } else {
        chosen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_choice_gets_the_default_file_name() {
        let dir = std::env::temp_dir();
        assert_eq!(resolve_choice(dir.clone()), dir.join(DEFAULT_FILE_NAME));
    }

    #[test]
    fn a_file_choice_is_kept_as_is() {
        let file = std::env::temp_dir().join("talkie-panel-test-notes.md");
        std::fs::write(&file, b"").expect("temp file");
        assert_eq!(resolve_choice(file.clone()), file);
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn a_path_that_does_not_exist_yet_is_kept_as_is() {
        let missing = std::env::temp_dir()
            .join("talkie-panel-test-missing")
            .join("notes.md");
        assert_eq!(resolve_choice(missing.clone()), missing);
    }
}
