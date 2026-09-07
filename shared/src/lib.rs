//! The contract between Talkie's two halves.
//!
//! Both the Tauri host and the WASM UI depend on this crate, so a command name,
//! an event name, or a payload shape is written down exactly once. There is no
//! codegen step and no generated bindings file to keep in sync — if it compiles
//! on both sides, the wire format agrees.

pub mod document;

use serde::{Deserialize, Serialize};

pub const APP_NAME: &str = "Talkie";

/// Default global shortcut: ⌃⌥Space, in `handy-keys` syntax.
///
/// Side-agnostic on purpose — a default should fire from either hand. A
/// shortcut *recorded* in settings keeps whichever side was actually pressed
/// (`CmdRight`, `OptLeft`…), which is the whole reason Talkie left Tauri's
/// global-shortcut plugin behind: Carbon hotkeys have no side bits.
pub const DEFAULT_SHORTCUT: &str = "Control+Alt+Space";

/// Tauri command names. Referenced by `#[tauri::command]` wiring on the host and
/// by `ipc::invoke` on the UI side.
pub mod commands {
    pub const GET_SETTINGS: &str = "get_settings";
    pub const SET_SETTINGS: &str = "set_settings";
    pub const COMPLETE_ONBOARDING: &str = "complete_onboarding";
    pub const SHOW_WINDOW: &str = "show_window";
    pub const HIDE_WINDOW: &str = "hide_window";
    /// Where the speech model is: missing, downloading, or ready.
    pub const GET_MODEL_STATUS: &str = "get_model_status";
    /// Fetch and unpack the speech model, reporting progress as events.
    pub const DOWNLOAD_MODEL: &str = "download_model";
    /// Ask the OS for microphone access (first run).
    pub const REQUEST_MICROPHONE: &str = "request_microphone";
    /// Start / stop a capture from the UI or the tray, same as the shortcut.
    pub const TOGGLE_RECORDING: &str = "toggle_recording";
    pub const GET_RECORDER_STATE: &str = "get_recorder_state";
    /// Put the hotkey engine into recording mode: the live binding is released
    /// and raw key events start arriving as `SHORTCUT_CAPTURE`.
    pub const START_SHORTCUT_RECORDING: &str = "start_shortcut_recording";
    /// Leave recording mode and re-bind whatever is in settings.
    pub const STOP_SHORTCUT_RECORDING: &str = "stop_shortcut_recording";
    /// Has macOS granted Accessibility? Without it there are no global keys.
    pub const GET_ACCESSIBILITY: &str = "get_accessibility";
    /// Open System Settings at the Accessibility pane.
    pub const OPEN_ACCESSIBILITY_SETTINGS: &str = "open_accessibility_settings";
    /// Read the note file. Returns its text, or an empty string when it does
    /// not exist yet.
    pub const READ_NOTE: &str = "read_note";
    /// Write the note file. The editor's autosave, and the only writer other
    /// than the capture pipeline's append.
    pub const WRITE_NOTE: &str = "write_note";
    /// Bind the shortcut again — the way back from a grant that arrived after
    /// the app had already given up on the keyboard.
    pub const RETRY_SHORTCUT: &str = "retry_shortcut";
    /// Open a native save dialog to choose (or create) the notes file. Returns
    /// the picked path, or `None` when cancelled — nothing is persisted here.
    pub const PICK_NOTE_PATH: &str = "pick_note_path";
    /// The names of every input device, for the settings dropdown.
    pub const LIST_MICROPHONES: &str = "list_microphones";
}

/// Event names for host → UI pushes. Namespaced so they can never collide with
/// Tauri's own `tauri://` events.
pub mod events {
    pub const SETTINGS_CHANGED: &str = "talkie://settings-changed";
    pub const RECORDER_STATE: &str = "talkie://recorder-state";
    /// The note file changed on disk underneath us (Obsidian, an agent, git…).
    pub const NOTE_CHANGED_EXTERNALLY: &str = "talkie://note-changed-externally";
    /// Model download progress, payload `ModelProgress`.
    pub const MODEL_PROGRESS: &str = "talkie://model-progress";
    /// One raw key event while the shortcut recorder is running. Payload is
    /// `ShortcutCapture`. Only emitted between `start_shortcut_recording` and
    /// `stop_shortcut_recording`.
    pub const SHORTCUT_CAPTURE: &str = "talkie://shortcut-capture";
    /// A capture failed. Payload is a human-readable sentence; the only way an
    /// otherwise silent pipeline can say something went wrong.
    pub const CAPTURE_FAILED: &str = "talkie://capture-failed";
}

/// Every window loads the same WASM bundle and routes on its own label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WindowLabel {
    Editor,
    Settings,
    Onboarding,
}

impl WindowLabel {
    pub const fn as_str(self) -> &'static str {
        match self {
            WindowLabel::Editor => "editor",
            WindowLabel::Settings => "settings",
            WindowLabel::Onboarding => "onboarding",
        }
    }

    pub fn parse(label: &str) -> Option<Self> {
        match label {
            "editor" => Some(WindowLabel::Editor),
            "settings" => Some(WindowLabel::Settings),
            "onboarding" => Some(WindowLabel::Onboarding),
            _ => None,
        }
    }
}

/// Where the capture pipeline is right now. Drives the tray icon and, later, the
/// chimes. Nothing records yet in M0 — this exists so the UI and tray can be
/// written against the final shape.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecorderState {
    #[default]
    Idle,
    Recording,
    Transcribing,
}

/// User settings. Lives on the host (tauri-plugin-store); the UI never keeps its
/// own copy of the truth, it reads via `get_settings` and writes via
/// `set_settings`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Absolute path to the one long markdown file. Empty means "not resolved
    /// yet" — the host fills in `~/Documents/Talkie/talkie.md` on first load.
    pub note_path: String,
    /// Global shortcut accelerator, in Tauri's syntax.
    pub shortcut: String,
    /// Hold-to-talk instead of press-to-start / press-to-stop. The default:
    /// holding the key is what a capture *is* — you know it is recording
    /// because your finger is on the key, and letting go cannot leave a
    /// recording running by accident.
    pub push_to_talk: bool,
    /// Start/stop chimes — the only feedback in an otherwise silent flow.
    pub play_sounds: bool,
    /// Input device name; `None` follows the system default.
    pub microphone: Option<String>,
    pub launch_at_login: bool,
    pub onboarding_complete: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            note_path: String::new(),
            shortcut: DEFAULT_SHORTCUT.to_string(),
            push_to_talk: true,
            play_sounds: true,
            microphone: None,
            launch_at_login: false,
            onboarding_complete: false,
        }
    }
}

/// Argument payload for `show_window` / `hide_window`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowArgs {
    pub label: WindowLabel,
}

/// Argument payload for `write_note`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteNoteArgs {
    pub text: String,
}

/// Argument payload for `set_settings`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetSettingsArgs {
    pub settings: Settings,
}

/// Whether the local speech model is on disk yet. Drives the onboarding page and
/// the recorder's refusal to start without a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelStatus {
    Missing,
    Downloading,
    Ready,
}

/// Download progress for the speech model. `total_bytes` is `None` until the
/// server's `Content-Length` is known.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelProgress {
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    /// Set once the bytes are down and the archive is being unpacked — a step
    /// slow enough (456 MB) that the UI would otherwise look stalled.
    pub extracting: bool,
    pub done: bool,
    pub error: Option<String>,
}

impl ModelProgress {
    pub fn fraction(&self) -> Option<f32> {
        let total = self.total_bytes?;
        if total == 0 {
            return None;
        }
        Some(self.downloaded_bytes as f32 / total as f32)
    }
}

/// One key event from the shortcut recorder.
///
/// The host reports what is held *right now*; the recorder in the settings
/// window decides from the stream when a combination is finished. Keeping the
/// decision on the UI side is what lets the field commit on release without the
/// host having to guess whether the user is done.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortcutCapture {
    /// Everything held, in `handy-keys` syntax (`"CmdRight"`,
    /// `"Ctrl+Opt+Space"`). Empty once the last key is let go.
    pub hotkey: String,
    /// The same thing as glyphs, ready to paint.
    pub display: String,
    /// Whether a non-modifier key is part of it. A combination with a key wins
    /// over the modifier-only prefix that necessarily preceded it.
    pub has_key: bool,
    /// Down or up. The recorder commits on the up that empties `hotkey`.
    pub is_key_down: bool,
    /// Escape cancels; it can never be part of a shortcut.
    pub is_escape: bool,
}

/// Render a `handy-keys` accelerator as macOS glyphs: `"CmdRight+Shift+K"` →
/// `"R⌘⇧K"`.
///
/// Pure string work — no `handy-keys` dependency — so the WASM side can paint a
/// shortcut without pulling a CoreGraphics event tap into the browser.
pub fn format_shortcut(accelerator: &str) -> String {
    accelerator
        .split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(format_shortcut_part)
        .collect()
}

fn format_shortcut_part(part: &str) -> String {
    let lower = part.to_lowercase();
    let lower = lower.replace('_', "");

    // Split a trailing side off the modifier name, so each modifier needs one
    // arm below instead of three.
    let (base, side) = match lower.strip_suffix("left") {
        Some(base) => (base, "L"),
        None => match lower.strip_suffix("right") {
            Some(base) => (base, "R"),
            None => (lower.as_str(), ""),
        },
    };

    let glyph = match base {
        "cmd" | "command" | "meta" | "super" | "win" | "windows" => Some("⌘"),
        "shift" => Some("⇧"),
        "ctrl" | "control" => Some("⌃"),
        "opt" | "option" | "alt" => Some("⌥"),
        "fn" | "function" => Some("fn"),
        _ => None,
    };

    match glyph {
        Some(glyph) => format!("{side}{glyph}"),
        // Not a modifier, so `left`/`right` was part of the key's own name
        // (the arrow keys) and must not have been split off.
        None => format_key_name(&lower),
    }
}

fn format_key_name(name: &str) -> String {
    match name {
        "left" => "←".to_string(),
        "right" => "→".to_string(),
        "up" => "↑".to_string(),
        "down" => "↓".to_string(),
        "space" => "Space".to_string(),
        "escape" => "Esc".to_string(),
        "return" | "enter" => "↩".to_string(),
        "tab" => "⇥".to_string(),
        "backspace" | "delete" => "⌫".to_string(),
        _ => {
            let mut chars = name.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_the_default_shortcut() {
        assert_eq!(format_shortcut(DEFAULT_SHORTCUT), "⌃⌥Space");
    }

    #[test]
    fn keeps_the_side_of_a_sided_modifier() {
        assert_eq!(format_shortcut("CmdRight"), "R⌘");
        assert_eq!(format_shortcut("CmdLeft+ShiftRight+K"), "L⌘R⇧K");
    }

    #[test]
    fn does_not_mistake_an_arrow_key_for_a_side() {
        assert_eq!(format_shortcut("Cmd+Left"), "⌘←");
    }
}
