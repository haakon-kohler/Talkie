//! The global shortcut — the only way into a capture that doesn't involve a
//! window — and the recorder that lets the user set it by pressing it.
//!
//! ## Why not `tauri-plugin-global-shortcut`
//!
//! The plugin sits on Carbon's `RegisterEventHotKey`, which has one `Command`
//! bit with no side to it and refuses a hotkey that is nothing but modifiers.
//! Neither "hold the right ⌘" nor "tell ⌘-left from ⌘-right" can be expressed
//! through it, however good the recorder in front of it is. `handy-keys` reads a
//! `CGEventTap` instead, so both are ordinary cases — at the price of macOS
//! **Accessibility permission**, which Talkie now asks for during onboarding.
//!
//! ## Shape
//!
//! `HotkeyManager` owns a `Receiver`, so it is not `Sync` and cannot live in
//! Tauri's managed state. One thread owns it instead and everything else talks
//! to that thread over a channel:
//!
//! ```text
//!   commands / settings ──Cmd──▶ engine thread ──▶ Recorder      (bound hotkey)
//!                                             └──▶ SHORTCUT_CAPTURE events
//!                                                               (recording mode)
//! ```
//!
//! The two modes are exclusive by construction: while the recorder is listening
//! the bound hotkey is unregistered, so pressing your own shortcut to re-record
//! it doesn't also start a capture.

use std::str::FromStr;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Result};
use handy_keys::{Hotkey, HotkeyId, HotkeyManager, HotkeyState, Key, KeyboardListener};
use talkie_shared::{events, ShortcutCapture};
use tauri::{AppHandle, Emitter, Manager};

use crate::recorder::Recorder;
use crate::settings::SettingsState;

/// How long the engine thread waits for a command before looking at the
/// keyboard again. Also the worst-case latency from a keypress to a capture
/// starting, which at 10 ms is far below anything a hand can notice.
const TICK: Duration = Duration::from_millis(10);

/// What the rest of the app can ask the engine thread to do.
enum Cmd {
    /// Bind this accelerator, replacing any previous one. Empty unbinds.
    Bind {
        accelerator: String,
        reply: Sender<Result<(), String>>,
    },
    StartRecording {
        reply: Sender<Result<(), String>>,
    },
    StopRecording,
}

/// Handle on the engine thread, managed as Tauri state.
pub struct ShortcutState {
    tx: Mutex<Sender<Cmd>>,
}

impl ShortcutState {
    fn send(&self, cmd: Cmd) -> Result<(), String> {
        self.tx
            .lock()
            .map_err(|_| "the shortcut engine is wedged".to_string())?
            .send(cmd)
            .map_err(|_| "the shortcut engine has stopped".to_string())
    }

    /// Send a command and wait for the thread's verdict.
    fn request(&self, make: impl FnOnce(Sender<Result<(), String>>) -> Cmd) -> Result<(), String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.send(make(reply_tx))?;
        reply_rx
            .recv()
            .map_err(|_| "the shortcut engine did not answer".to_string())?
    }

    pub fn start_recording(&self) -> Result<(), String> {
        self.request(|reply| Cmd::StartRecording { reply })
    }

    pub fn stop_recording(&self) -> Result<(), String> {
        self.send(Cmd::StopRecording)
    }
}

/// Start the engine thread and register it as managed state. Called once, from
/// `lib.rs`, before the first `apply`.
pub fn init(app: &AppHandle) {
    let (tx, rx) = mpsc::channel();
    let handle = app.clone();
    std::thread::Builder::new()
        .name("talkie-shortcut".into())
        .spawn(move || run(handle, rx))
        .expect("could not start the shortcut engine thread");
    app.manage(ShortcutState { tx: Mutex::new(tx) });
}

/// Bind the shortcut from settings, replacing whatever was bound before.
///
/// Called at startup and again whenever the accelerator changes, so it has to be
/// idempotent.
pub fn apply(app: &AppHandle) -> Result<()> {
    let accelerator = {
        let state = app.state::<SettingsState>();
        let settings = state.0.lock().expect("settings mutex poisoned");
        settings.shortcut.clone()
    };

    let state = app.state::<ShortcutState>();
    state
        .request(|reply| Cmd::Bind { accelerator, reply })
        .map_err(|e| anyhow!("{e}"))
}

/// Reject an accelerator that cannot serve as a global shortcut, before it is
/// persisted. Empty is allowed and means "captures from the tray only".
///
/// A lone key with no modifier parses perfectly well and would be a disaster:
/// binding `K` swallows the letter everywhere, so the settings field refuses it
/// rather than leaving the user to work out why typing broke.
pub fn validate(accelerator: &str) -> Result<(), String> {
    let accelerator = accelerator.trim();
    if accelerator.is_empty() {
        return Ok(());
    }

    let hotkey = Hotkey::from_str(accelerator)
        .map_err(|e| format!("`{accelerator}` is not a valid shortcut: {e}"))?;

    if hotkey.modifiers.is_empty() {
        // COPY: settings.shortcut.invalid — placeholder
        return Err("A shortcut needs at least one modifier — ⌘, ⌥, ⌃ or ⇧.".to_string());
    }
    Ok(())
}

/// Everything the engine thread owns. None of it is shared, which is the point.
struct Engine {
    app: AppHandle,
    manager: Option<HotkeyManager>,
    /// The accelerator settings asked for, remembered so recording mode can put
    /// it back and so a failed `HotkeyManager` can be retried later.
    wanted: String,
    bound: Option<HotkeyId>,
    listener: Option<KeyboardListener>,
}

fn run(app: AppHandle, rx: Receiver<Cmd>) {
    let mut engine = Engine {
        app,
        manager: None,
        wanted: String::new(),
        bound: None,
        listener: None,
    };

    loop {
        match rx.recv_timeout(TICK) {
            Ok(cmd) => engine.handle(cmd),
            Err(RecvTimeoutError::Timeout) => {}
            // Every sender is gone, which only happens as the app exits.
            Err(RecvTimeoutError::Disconnected) => return,
        }

        engine.poll();
    }
}

impl Engine {
    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Bind { accelerator, reply } => {
                self.wanted = accelerator;
                let result = self.rebind();
                let _ = reply.send(result);
            }
            Cmd::StartRecording { reply } => {
                let result = self.start_recording();
                let _ = reply.send(result);
            }
            Cmd::StopRecording => self.stop_recording(),
        }
    }

    /// Get a manager, building it on first use.
    ///
    /// Lazily, and retried on every bind, because the tap cannot be created
    /// before Accessibility is granted — and on first run it is granted *after*
    /// the app has already started.
    fn manager(&mut self) -> Result<&HotkeyManager, String> {
        if self.manager.is_none() {
            // Blocking mode: a bound shortcut is Talkie's and does not also
            // reach whatever is in front. That is what makes a held modifier
            // usable as a push-to-talk key rather than a stray ⌘.
            let manager = HotkeyManager::new_with_blocking().map_err(|e| e.to_string())?;
            self.manager = Some(manager);
        }
        Ok(self.manager.as_ref().expect("just built"))
    }

    /// Bind `wanted`, dropping any previous binding first.
    fn rebind(&mut self) -> Result<(), String> {
        self.unbind();

        let accelerator = self.wanted.trim().to_string();
        if accelerator.is_empty() {
            log::warn!("talkie: no shortcut configured; captures can only start from the tray");
            return Ok(());
        }

        // A value that predates validation must not leave the app hotkey-less:
        // fall back to the default rather than give up.
        let hotkey = match Hotkey::from_str(&accelerator) {
            Ok(hotkey) => hotkey,
            Err(e) => {
                log::warn!(
                    "talkie: `{accelerator}` is not a valid shortcut ({e}); using {} instead",
                    talkie_shared::DEFAULT_SHORTCUT
                );
                self.wanted = talkie_shared::DEFAULT_SHORTCUT.to_string();
                Hotkey::from_str(&self.wanted).map_err(|e| {
                    format!("the default shortcut `{}` is not valid: {e}", self.wanted)
                })?
            }
        };

        // Recording mode deliberately leaves nothing bound; the accelerator is
        // remembered and takes effect when recording stops.
        if self.listener.is_some() {
            return Ok(());
        }

        let id = self
            .manager()?
            .register(hotkey)
            .map_err(|e| format!("could not register the shortcut `{}`: {e}", self.wanted))?;
        self.bound = Some(id);

        log::info!("talkie: shortcut bound to {}", self.wanted);
        Ok(())
    }

    fn unbind(&mut self) {
        if let (Some(id), Some(manager)) = (self.bound.take(), self.manager.as_ref()) {
            let _ = manager.unregister(id);
        }
    }

    fn start_recording(&mut self) -> Result<(), String> {
        if self.listener.is_some() {
            return Ok(());
        }
        // Release the live binding first: without this, pressing the current
        // shortcut in order to re-record it would also start a capture, and
        // blocking mode would eat the very keys being recorded.
        self.unbind();

        let listener = KeyboardListener::new().map_err(|e| e.to_string())?;
        self.listener = Some(listener);
        Ok(())
    }

    fn stop_recording(&mut self) {
        if self.listener.take().is_none() {
            return;
        }
        // `wanted` is whatever the recorder just saved, because `set_settings`
        // calls `apply` before the UI stops recording.
        if let Err(e) = self.rebind() {
            log::error!("talkie: could not restore the shortcut after recording: {e}");
        }
    }

    /// One pass over whichever source is live.
    fn poll(&mut self) {
        if self.listener.is_some() {
            self.poll_recording();
        } else {
            self.poll_hotkey();
        }
    }

    fn poll_hotkey(&mut self) {
        let Some(manager) = self.manager.as_ref() else {
            return;
        };
        while let Some(event) = manager.try_recv() {
            if Some(event.id) == self.bound {
                dispatch(&self.app, event.state);
            }
        }
    }

    fn poll_recording(&mut self) {
        let Some(listener) = self.listener.as_ref() else {
            return;
        };
        while let Some(event) = listener.try_recv() {
            let hotkey = event.as_hotkey().map(|h| h.to_string()).unwrap_or_default();
            let payload = ShortcutCapture {
                display: talkie_shared::format_shortcut(&hotkey),
                hotkey,
                has_key: event.key.is_some(),
                is_key_down: event.is_key_down,
                is_escape: event.key == Some(Key::Escape),
            };
            if let Err(e) = self.app.emit(events::SHORTCUT_CAPTURE, &payload) {
                log::error!("talkie: could not report a recorded key: {e}");
            }
        }
    }
}

/// What a press of the bound shortcut means. Kept tiny on purpose: it decides
/// and hands off.
fn dispatch(app: &AppHandle, state: HotkeyState) {
    let push_to_talk = {
        let settings = app.state::<SettingsState>();
        let guard = settings.0.lock().expect("settings mutex poisoned");
        guard.push_to_talk
    };

    let recorder = app.state::<Arc<Recorder>>();

    match (push_to_talk, state) {
        // Hold to talk: the key's own edges are the start and the stop.
        (true, HotkeyState::Pressed) => recorder.start(),
        (true, HotkeyState::Released) => recorder.stop(),
        // Press to start, press again to stop. The release does nothing, or the
        // capture would end the instant it began.
        (false, HotkeyState::Pressed) => recorder.toggle(),
        (false, HotkeyState::Released) => {}
    }
}

/// Whether macOS has granted Accessibility. Without it there is no event tap and
/// so no global shortcut at all — the tray still works.
pub fn accessibility_granted() -> bool {
    #[cfg(target_os = "macos")]
    {
        handy_keys::check_accessibility()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

/// Open System Settings at the pane where the switch lives.
pub fn open_accessibility_settings() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        handy_keys::open_accessibility_settings().map_err(|e| e.to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::validate;

    #[test]
    fn accepts_the_default_and_a_sided_modifier() {
        assert!(validate(talkie_shared::DEFAULT_SHORTCUT).is_ok());
        assert!(validate("CmdRight").is_ok());
        assert!(validate("CmdLeft+Shift+K").is_ok());
    }

    #[test]
    fn accepts_nothing_at_all() {
        assert!(validate("  ").is_ok());
    }

    #[test]
    fn refuses_a_bare_key() {
        assert!(validate("K").is_err());
    }

    #[test]
    fn refuses_a_name_that_is_not_a_key() {
        assert!(validate("Wobble").is_err());
    }

    /// The M1 bug — a bare `"Command"` typed into the old free-text field —
    /// is a legitimate modifier-only hotkey now, and the recorder is how you
    /// would set it. Worth pinning so nobody "fixes" it back to an error.
    #[test]
    fn a_lone_modifier_is_bindable_now() {
        assert!(validate("Command").is_ok());
    }
}
