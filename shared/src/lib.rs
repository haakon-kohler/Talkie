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
    /// Where the speech model is: missing, downloading, or ready.
    pub const GET_MODEL_STATUS: &str = "get_model_status";
    /// Fetch and unpack the speech model, reporting progress as events.
    pub const DOWNLOAD_MODEL: &str = "download_model";
    /// Ask the OS for microphone access (first run).
    pub const REQUEST_MICROPHONE: &str = "request_microphone";
    /// Start / stop a capture from the UI or the tray, same as the shortcut.
    pub const TOGGLE_RECORDING: &str = "toggle_recording";
    pub const GET_RECORDER_STATE: &str = "get_recorder_state";
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
