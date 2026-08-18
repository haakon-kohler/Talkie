//! The contract between Talkie's two halves.
//!
//! Both the Tauri host and the WASM UI depend on this crate, so a command name,
//! an event name, or a payload shape is written down exactly once. There is no
//! codegen step and no generated bindings file to keep in sync — if it compiles
//! on both sides, the wire format agrees.

use serde::{Deserialize, Serialize};

pub const APP_NAME: &str = "Talkie";

/// Default global shortcut: ⌃⌥Space. Deliberately clear of Handy's ⌥Space and
/// ⌥⇧Space so both apps can stay installed and bound at the same time.
pub const DEFAULT_SHORTCUT: &str = "Control+Alt+Space";

/// Tauri command names. Referenced by `#[tauri::command]` wiring on the host and
/// by `ipc::invoke` on the UI side.
pub mod commands {
    pub const GET_SETTINGS: &str = "get_settings";
    pub const SET_SETTINGS: &str = "set_settings";
    pub const COMPLETE_ONBOARDING: &str = "complete_onboarding";
    pub const SHOW_WINDOW: &str = "show_window";
    pub const HIDE_WINDOW: &str = "hide_window";
}

/// Event names for host → UI pushes. Namespaced so they can never collide with
/// Tauri's own `tauri://` events.
pub mod events {
    pub const SETTINGS_CHANGED: &str = "talkie://settings-changed";
    pub const RECORDER_STATE: &str = "talkie://recorder-state";
    /// The note file changed on disk underneath us (Obsidian, an agent, git…).
    pub const NOTE_CHANGED_EXTERNALLY: &str = "talkie://note-changed-externally";
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
    /// Hold-to-talk instead of press-to-start / press-to-stop.
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
            push_to_talk: false,
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

/// Argument payload for `set_settings`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetSettingsArgs {
    pub settings: Settings,
}
