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
//!
//! ## Double-tap
//!
//! Two quick taps of the shortcut show or hide the notepad. Waiting to see
//! whether a second tap is coming would put that wait in front of every
//! capture, so the first tap is never held back: it starts recording at once,
//! and a second tap inside `DOUBLE_TAP_WINDOW` throws that capture away before
//! anything is transcribed. `Taps` decides; `Engine::perform` acts.

use std::str::FromStr;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use handy_keys::{Hotkey, HotkeyId, HotkeyManager, HotkeyState, Key, KeyboardListener};
use talkie_shared::{events, RecorderState, ShortcutCapture, WindowLabel};
use tauri::{AppHandle, Emitter, Manager};

use crate::recorder::Recorder;
use crate::settings::SettingsState;
use crate::{sounds, windows};

/// How long the engine thread waits for a command before looking at the
/// keyboard again. Also the worst-case latency from a keypress to a capture
/// starting, which at 10 ms is far below anything a hand can notice.
const TICK: Duration = Duration::from_millis(10);

/// How long a caller waits for the engine thread's verdict before giving up.
///
/// The callers are Tauri commands, which run on the main thread. The engine
/// thread, in turn, drives the recorder, and the recorder updates the tray —
/// which marshals onto the main thread and waits. A hotkey press landing in
/// the same instant as a settings save would otherwise hold both threads
/// waiting on each other forever. A bounded wait turns that into one failed
/// command.
const REPLY_TIMEOUT: Duration = Duration::from_secs(5);

/// How often the engine asks macOS whether it still holds Accessibility while
/// a tap exists.
///
/// The grant can vanish under a running app: `tccutil reset`, the switch in
/// System Settings, or a rebuild that changes the ad-hoc code hash. A tap
/// whose process is no longer trusted does not simply go quiet — every event
/// it returns is refused by WindowServer as a synthesized event, the tap is
/// disabled, the callback re-enables it, and the two ping-pong at over 100 Hz.
/// That storm hung this machine on 2026-09-12. Tearing the tap down within
/// one interval bounds it. The check is one XPC round trip to tccd, so it
/// stays rare.
const TRUST_CHECK: Duration = Duration::from_secs(5);

/// Press to press: how soon a second press must follow the first to count as
/// a double-tap. A toggle-mode capture shorter than this is impossible — it
/// becomes the double-tap instead — but one that short is below the
/// recorder's 250 ms floor and would have been dropped anyway.
const DOUBLE_TAP_WINDOW: Duration = Duration::from_millis(400);

/// Push-to-talk only: a hold shorter than this is a tap, and its release is
/// held back until the double-tap window closes. Matches the recorder's
/// minimum capture, so a tap that gets no second press is discarded rather
/// than transcribed — it would have been too short to keep anyway, and
/// recording on through the wait must not turn it into a capture.
const TAP_HOLD: Duration = Duration::from_millis(250);
const _: () = assert!(
    TAP_HOLD.as_millis() < DOUBLE_TAP_WINDOW.as_millis(),
    "a tap has to end before the window for the next one closes"
);

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
            .map_err(|e| format!("the shortcut engine is wedged: {e}"))?
            .send(cmd)
            .map_err(|e| format!("the shortcut engine has stopped: {e}"))
    }

    /// Send a command and wait — a bounded time — for the thread's verdict.
    fn request(&self, make: impl FnOnce(Sender<Result<(), String>>) -> Cmd) -> Result<(), String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.send(make(reply_tx))?;
        reply_rx.recv_timeout(REPLY_TIMEOUT).map_err(|e| match e {
            RecvTimeoutError::Timeout => "the shortcut engine did not answer in time".to_string(),
            RecvTimeoutError::Disconnected => "the shortcut engine did not answer".to_string(),
        })?
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
    assert!(
        app.try_state::<ShortcutState>().is_none(),
        "the shortcut engine was started twice"
    );
    let (tx, rx) = mpsc::channel();
    let handle = app.clone();
    std::thread::Builder::new()
        .name("talkie-shortcut".into())
        .spawn(move || run(handle, &rx))
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
        let settings = state.lock().map_err(anyhow::Error::msg)?;
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
        // COPY: settings.shortcut.invalid
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
    /// When the Accessibility grant was last confirmed. See `TRUST_CHECK`.
    trust_checked: Instant,
    taps: Taps,
}

fn run(app: AppHandle, rx: &Receiver<Cmd>) {
    let mut engine = Engine {
        app,
        manager: None,
        wanted: String::new(),
        bound: None,
        listener: None,
        trust_checked: Instant::now(),
        taps: Taps::default(),
    };

    loop {
        match rx.recv_timeout(TICK) {
            Ok(cmd) => engine.handle(cmd),
            Err(RecvTimeoutError::Timeout) => {}
            // Every sender is gone, which only happens as the app exits.
            Err(RecvTimeoutError::Disconnected) => return,
        }

        engine.check_trust();
        engine.expire_tap();
        engine.poll();
        engine.check_invariants();
    }
}

impl Engine {
    /// What must hold between commands, whichever mode the engine is in.
    fn check_invariants(&self) {
        // A binding is an id handed out by a manager; without one it is stale.
        debug_assert!(
            self.bound.is_none() || self.manager.is_some(),
            "a hotkey is bound but there is no manager to have bound it"
        );
        // The two modes are exclusive by construction (see the module docs):
        // recording mode always releases the live binding first.
        debug_assert!(
            !(self.listener.is_some() && self.bound.is_some()),
            "recording mode and a live binding at the same time"
        );
    }

    /// Drop every tap the moment macOS stops trusting this process.
    ///
    /// Without the grant a tap cannot deliver anything, and leaving it
    /// installed is actively harmful — see `TRUST_CHECK`. The next `apply`
    /// (settings, or the Accessibility section's retry) builds a fresh one.
    fn check_trust(&mut self) {
        if self.manager.is_none() && self.listener.is_none() {
            return;
        }
        if self.trust_checked.elapsed() < TRUST_CHECK {
            return;
        }
        self.trust_checked = Instant::now();
        if accessibility_granted() {
            return;
        }

        log::warn!(
            "talkie: Accessibility was revoked while the shortcut was bound; releasing the keyboard"
        );
        self.unbind();
        self.listener = None;
        self.manager = None;
        debug_assert!(self.bound.is_none(), "unbind left a binding behind");
    }
    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Bind { accelerator, reply } => {
                self.wanted = accelerator;
                let result = self.rebind();
                #[expect(
                    clippy::let_underscore_must_use,
                    reason = "a caller that gave up waiting has already reported REPLY_TIMEOUT"
                )]
                let _ = reply.send(result);
            }
            Cmd::StartRecording { reply } => {
                let result = self.start_recording();
                #[expect(
                    clippy::let_underscore_must_use,
                    reason = "a caller that gave up waiting has already reported REPLY_TIMEOUT"
                )]
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
        let manager = match self.manager.take() {
            Some(manager) => manager,
            // Blocking mode: a bound shortcut is Talkie's and does not also
            // reach whatever is in front. That is what makes a held modifier
            // usable as a push-to-talk key rather than a stray ⌘.
            None => HotkeyManager::new_with_blocking().map_err(|e| e.to_string())?,
        };
        Ok(self.manager.insert(manager))
    }

    /// Bind `wanted`, dropping any previous binding first.
    fn rebind(&mut self) -> Result<(), String> {
        self.unbind();
        debug_assert!(self.bound.is_none(), "unbind left a binding behind");

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

        // A fresh manager means a fresh tap, whose trust starts from now.
        self.trust_checked = Instant::now();
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
            // Forgotten either way: the manager hands out a fresh id on the
            // next bind. Still worth knowing, as the old one may stay live.
            if let Err(e) = manager.unregister(id) {
                log::warn!("talkie: could not release the previous shortcut: {e}");
            }
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
        debug_assert!(self.bound.is_none(), "unbind left a binding behind");

        let listener = KeyboardListener::new().map_err(|e| e.to_string())?;
        self.trust_checked = Instant::now();
        self.listener = Some(listener);
        Ok(())
    }

    fn stop_recording(&mut self) {
        if self.listener.take().is_none() {
            return;
        }
        debug_assert!(self.listener.is_none(), "take left the listener in place");
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
        let mut events = Vec::new();
        while let Some(event) = manager.try_recv() {
            if Some(event.id) == self.bound {
                events.push(event.state);
            }
        }
        for state in events {
            self.dispatch(state);
        }
    }

    /// What a press or release of the bound shortcut means. `Taps` decides;
    /// this hands off.
    fn dispatch(&mut self, state: HotkeyState) {
        let settings = self.settings();
        let mode = Mode {
            push_to_talk: settings.push_to_talk,
            double_tap: settings.double_tap_notepad,
        };
        // A window that closed between ticks still has to close before the
        // event is read, or a held-back release would be lost behind it.
        self.expire_tap();
        let recording = self.app.state::<Arc<Recorder>>().state() == RecorderState::Recording;
        let action = self.taps.on_event(mode, state, recording, Instant::now());
        self.perform(action);
    }

    /// Close the double-tap window once it has run out.
    fn expire_tap(&mut self) {
        if let Some(action) = self.taps.expire(Instant::now()) {
            self.perform(action);
        }
    }

    fn perform(&self, action: Action) {
        let recorder = self.app.state::<Arc<Recorder>>();
        match action {
            Action::Nothing => {}
            Action::Start => recorder.start(),
            Action::Stop => recorder.stop(),
            Action::Toggle => recorder.toggle(),
            // The tap's stop chime, late by the length of the window. The
            // capture itself was too short to keep.
            Action::Abandon => {
                if recorder.cancel() && self.settings().play_sounds {
                    sounds::play_stop();
                }
            }
            Action::Notepad => {
                recorder.cancel();
                if self.settings().play_sounds {
                    sounds::play_notepad();
                }
                if let Err(e) = windows::toggle(&self.app, WindowLabel::Editor) {
                    log::warn!("talkie: the double-tap could not open the notepad: {e}");
                }
            }
        }
    }

    fn settings(&self) -> talkie_shared::Settings {
        self.app
            .state::<SettingsState>()
            .0
            .lock()
            .expect("settings mutex poisoned")
            .clone()
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

/// The two settings that decide what a key edge means.
#[derive(Debug, Clone, Copy)]
struct Mode {
    push_to_talk: bool,
    double_tap: bool,
}

/// What the engine does with a key edge, once `Taps` has decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Nothing,
    Start,
    Stop,
    Toggle,
    /// A push-to-talk tap that no second tap followed: discard it, stop chime.
    Abandon,
    /// The second tap: discard the capture the first one started, notepad chime,
    /// show or hide the notepad.
    Notepad,
}

/// The shortcut's recent history, as much of it as a double-tap needs.
///
/// Pure: it is handed the time and the recorder's state rather than reading
/// them, so the tests below can drive it press by press.
#[derive(Debug, Default)]
struct Taps {
    /// When the press that could be a double-tap's first half landed.
    armed: Option<Instant>,
    /// When the current press went down, to tell a tap from a hold.
    pressed: Option<Instant>,
    /// Push-to-talk: a tap's release, held back until the window closes.
    held_release: bool,
    /// The second tap's release belongs to the double-tap and stops nothing.
    swallow_release: bool,
}

impl Taps {
    fn on_event(
        &mut self,
        mode: Mode,
        state: HotkeyState,
        recording: bool,
        now: Instant,
    ) -> Action {
        // `armed` is only ever set with double-tap on, so a setting switched
        // off mid-window still lets that one double-tap finish.
        if state == HotkeyState::Pressed && self.armed_at(now) {
            self.armed = None;
            self.held_release = false;
            // In toggle mode the release does nothing anyway.
            self.swallow_release = mode.push_to_talk;
            return Action::Notepad;
        }

        match (mode.push_to_talk, state) {
            // Hold to talk: the key's own edges are the start and the stop.
            (true, HotkeyState::Pressed) => {
                debug_assert!(
                    !self.held_release,
                    "a held-back release outlived its window"
                );
                self.pressed = Some(now);
                self.armed = mode.double_tap.then_some(now);
                Action::Start
            }
            (true, HotkeyState::Released) => {
                if std::mem::take(&mut self.swallow_release) {
                    return Action::Nothing;
                }
                let tapped = self
                    .pressed
                    .take()
                    .is_some_and(|at| now.duration_since(at) < TAP_HOLD);
                if tapped && self.armed.is_some() {
                    self.held_release = true;
                    return Action::Nothing;
                }
                // A hold is a capture, never the first half of a double-tap.
                self.armed = None;
                Action::Stop
            }
            // Press to start, press again to stop. The release does nothing,
            // or the capture would end the instant it began. A press that
            // stops a capture cannot begin a double-tap.
            (false, HotkeyState::Pressed) => {
                // Switched out of push-to-talk mid-window: the tap's capture
                // is now a toggle capture, and this press is what stops it.
                self.held_release = false;
                self.armed = (mode.double_tap && !recording).then_some(now);
                Action::Toggle
            }
            (false, HotkeyState::Released) => Action::Nothing,
        }
    }

    /// Close the window if it has run out. A tap still waiting on it was a
    /// tap and nothing more.
    fn expire(&mut self, now: Instant) -> Option<Action> {
        if self.armed.is_none() || self.armed_at(now) {
            return None;
        }
        self.armed = None;
        std::mem::take(&mut self.held_release).then_some(Action::Abandon)
    }

    fn armed_at(&self, now: Instant) -> bool {
        self.armed
            .is_some_and(|at| now.duration_since(at) < DOUBLE_TAP_WINDOW)
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
#[cfg_attr(
    not(target_os = "macos"),
    expect(
        clippy::unnecessary_wraps,
        reason = "only the macOS half can fail; both halves share one signature"
    )
)]
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

#[cfg(test)]
mod tap_tests {
    use std::time::{Duration, Instant};

    use handy_keys::HotkeyState::{Pressed, Released};

    use super::{Action, Mode, Taps, DOUBLE_TAP_WINDOW, TAP_HOLD};

    const HOLD: Mode = Mode {
        push_to_talk: true,
        double_tap: true,
    };
    const TOGGLE: Mode = Mode {
        push_to_talk: false,
        double_tap: true,
    };

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn the_first_press_starts_at_once() {
        let mut taps = Taps::default();
        assert_eq!(
            taps.on_event(HOLD, Pressed, false, Instant::now()),
            Action::Start
        );
    }

    #[test]
    fn a_hold_is_an_ordinary_capture() {
        let t = Instant::now();
        let mut taps = Taps::default();
        taps.on_event(HOLD, Pressed, false, t);
        assert_eq!(
            taps.on_event(HOLD, Released, true, t + ms(1500)),
            Action::Stop
        );
        assert_eq!(taps.expire(t + ms(2000)), None);
    }

    #[test]
    fn a_double_tap_opens_the_notepad_and_its_release_stops_nothing() {
        let t = Instant::now();
        let mut taps = Taps::default();
        taps.on_event(HOLD, Pressed, false, t);
        assert_eq!(
            taps.on_event(HOLD, Released, true, t + ms(80)),
            Action::Nothing
        );
        assert_eq!(
            taps.on_event(HOLD, Pressed, true, t + ms(200)),
            Action::Notepad
        );
        assert_eq!(
            taps.on_event(HOLD, Released, false, t + ms(280)),
            Action::Nothing
        );
        assert_eq!(taps.expire(t + ms(1000)), None);
    }

    #[test]
    fn a_lone_tap_is_abandoned_when_the_window_closes() {
        let t = Instant::now();
        let mut taps = Taps::default();
        taps.on_event(HOLD, Pressed, false, t);
        taps.on_event(HOLD, Released, true, t + ms(80));
        assert_eq!(
            taps.expire(
                t + DOUBLE_TAP_WINDOW
                    .checked_sub(ms(1))
                    .expect("the window is longer than a millisecond")
            ),
            None
        );
        assert_eq!(taps.expire(t + DOUBLE_TAP_WINDOW), Some(Action::Abandon));
        assert_eq!(taps.expire(t + DOUBLE_TAP_WINDOW + ms(10)), None);
    }

    #[test]
    fn a_short_hold_followed_by_a_press_is_not_a_double_tap() {
        let t = Instant::now();
        let mut taps = Taps::default();
        taps.on_event(HOLD, Pressed, false, t);
        assert_eq!(
            taps.on_event(HOLD, Released, true, t + TAP_HOLD),
            Action::Stop
        );
        assert_eq!(
            taps.on_event(HOLD, Pressed, false, t + ms(350)),
            Action::Start
        );
    }

    #[test]
    fn a_slow_second_press_starts_a_new_capture() {
        let t = Instant::now();
        let mut taps = Taps::default();
        taps.on_event(HOLD, Pressed, false, t);
        taps.on_event(HOLD, Released, true, t + ms(80));
        assert_eq!(taps.expire(t + DOUBLE_TAP_WINDOW), Some(Action::Abandon));
        assert_eq!(
            taps.on_event(HOLD, Pressed, false, t + DOUBLE_TAP_WINDOW + ms(50)),
            Action::Start
        );
    }

    #[test]
    fn switched_off_it_is_plain_push_to_talk() {
        let off = Mode {
            push_to_talk: true,
            double_tap: false,
        };
        let t = Instant::now();
        let mut taps = Taps::default();
        assert_eq!(taps.on_event(off, Pressed, false, t), Action::Start);
        assert_eq!(taps.on_event(off, Released, true, t + ms(80)), Action::Stop);
        assert_eq!(
            taps.on_event(off, Pressed, false, t + ms(200)),
            Action::Start
        );
    }

    #[test]
    fn toggle_mode_double_tap() {
        let t = Instant::now();
        let mut taps = Taps::default();
        assert_eq!(taps.on_event(TOGGLE, Pressed, false, t), Action::Toggle);
        assert_eq!(
            taps.on_event(TOGGLE, Released, true, t + ms(80)),
            Action::Nothing
        );
        assert_eq!(
            taps.on_event(TOGGLE, Pressed, true, t + ms(200)),
            Action::Notepad
        );
        assert_eq!(taps.expire(t + ms(1000)), None);
    }

    #[test]
    fn toggle_mode_stop_press_does_not_arm() {
        let t = Instant::now();
        let mut taps = Taps::default();
        taps.on_event(TOGGLE, Pressed, false, t);
        // Recording for a while, then stopped, then pressed again quickly:
        // that is a stop and a new start, not a double-tap.
        assert_eq!(
            taps.on_event(TOGGLE, Pressed, true, t + ms(5000)),
            Action::Toggle
        );
        assert_eq!(
            taps.on_event(TOGGLE, Pressed, false, t + ms(5200)),
            Action::Toggle
        );
    }
}
