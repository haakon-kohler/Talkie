//! The capture state machine: Idle → Recording → Transcribing → Idle.
//!
//! This is the module the shortcut, the tray, and the UI all drive. It owns the
//! microphone, decides what a press means, and — when a capture finishes — hands
//! the samples to the transcriber and the text to the append engine.
//!
//! Everything slow happens off the calling thread. A shortcut press returns
//! immediately; the microphone opens, the model runs, and the file is written on
//! a worker, with `RECORDER_STATE` events narrating the transitions.

use std::path::{Path, PathBuf};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use talkie_shared::{events, ModelStatus, RecorderState};
use tauri::{AppHandle, Emitter, Manager};

use crate::audio_toolkit::constants::{MAX_CAPTURE_SAMPLES, SAMPLE_RATE};
use crate::audio_toolkit::vad::{
    SileroVad, SmoothedVad, VoiceActivityDetector, VAD_HANGOVER_FRAMES, VAD_ONSET_FRAMES,
    VAD_PREFILL_FRAMES, VAD_THRESHOLD,
};
use crate::audio_toolkit::AudioRecorder;
use crate::settings::SettingsState;
use crate::transcriber::Transcriber;
use crate::{hooks, models, note, sounds, tray};

/// Captures shorter than this are treated as a slip of the finger — a
/// double-tap on the shortcut, a key held for a moment — and dropped without
/// running the model.
pub const MIN_CAPTURE_SAMPLES: usize = SAMPLE_RATE as usize / 4; // 250 ms
const _: () = assert!(
    MIN_CAPTURE_SAMPLES < MAX_CAPTURE_SAMPLES,
    "the shortest capture worth keeping must fit inside the longest allowed"
);

/// How long a capture waits for the first microphone samples before giving
/// up. Bluetooth headsets can take a second or two; a device that delivers
/// nothing in this long is not going to, and a thread parked on it forever
/// would hold the capture in `Recording` with no way out.
const MIC_READY_TIMEOUT: Duration = Duration::from_secs(5);

pub struct Recorder {
    app: AppHandle,
    /// `<app-data>`: where the model and the hooks live.
    data_dir: PathBuf,
    /// The bundled Silero weights.
    vad_path: PathBuf,
    state: Mutex<RecorderState>,
    audio: Mutex<Option<AudioRecorder>>,
    transcriber: Arc<Transcriber>,
}

impl Recorder {
    pub fn new(app: AppHandle, data_dir: PathBuf, vad_path: PathBuf) -> Self {
        Self {
            app,
            data_dir,
            vad_path,
            state: Mutex::new(RecorderState::Idle),
            audio: Mutex::new(None),
            transcriber: Arc::new(Transcriber::new()),
        }
    }

    pub fn state(&self) -> RecorderState {
        *self.state.lock().expect("recorder state mutex poisoned")
    }

    /// Move from `from` to `next`, but only if the machine is still at `from`.
    ///
    /// Every transition goes through here so two presses in the same instant
    /// cannot both win: the check and the change happen under one lock.
    fn transition(&self, from: RecorderState, next: RecorderState) -> bool {
        debug_assert!(
            matches!(
                (from, next),
                (
                    RecorderState::Idle,
                    RecorderState::Recording | RecorderState::Idle
                ) | (
                    RecorderState::Recording,
                    RecorderState::Transcribing | RecorderState::Idle
                ) | (RecorderState::Transcribing, RecorderState::Idle)
            ),
            "no such transition: {from:?} -> {next:?}"
        );
        {
            let mut guard = self.state.lock().expect("recorder state mutex poisoned");
            if *guard != from {
                return false;
            }
            *guard = next;
        }
        if let Err(e) = self.app.emit(events::RECORDER_STATE, next) {
            log::warn!("talkie: could not announce {next:?}: {e}");
        }
        tray::set_state(&self.app, next);
        true
    }

    /// Report a failure the ways a silent app can: the log, an event the UI may
    /// be listening to, and a system notification — the windows are usually all
    /// closed when a capture fails. Never panics the capture path.
    ///
    /// `from` is the state the failing path believes it is in. If the machine
    /// has moved on — a stop arrived while the start was still failing — the
    /// newer path owns the state and this one only logs.
    fn fail(&self, from: RecorderState, error: &anyhow::Error) {
        let message = format!("{error:#}");
        log::error!("talkie: {message}");
        notify(&self.app, &message);
        if let Err(e) = self.app.emit(events::CAPTURE_FAILED, message) {
            log::warn!("talkie: could not tell the editor the capture failed: {e}");
        }
        if !self.transition(from, RecorderState::Idle) {
            log::warn!("talkie: capture failed in {from:?} but the recorder had already moved on");
        }
    }

    /// What the shortcut does on a press when push-to-talk is off.
    pub fn toggle(self: &Arc<Self>) {
        match self.state() {
            RecorderState::Idle => self.start(),
            RecorderState::Recording => self.stop(),
            // A press during transcription is ignored rather than queued: the
            // model is single-threaded and the capture is already committed.
            RecorderState::Transcribing => {}
        }
    }

    /// Begin a capture. Returns immediately; the microphone opens on a worker.
    pub fn start(self: &Arc<Self>) {
        if self.state() != RecorderState::Idle {
            return;
        }
        if models::status(&self.data_dir) != ModelStatus::Ready {
            self.fail(
                RecorderState::Idle,
                &anyhow!(
                    // COPY: capture.no_model
                    "speech model not yet installed — finish first run to download it"
                ),
            );
            return;
        }

        // Claim the state before the worker starts so a second press cannot
        // open the microphone twice. Losing the claim means the other press
        // is already doing this.
        if !self.transition(RecorderState::Idle, RecorderState::Recording) {
            return;
        }

        let this = Arc::clone(self);
        std::thread::spawn(move || {
            if let Err(e) = this.open_and_start() {
                this.fail(RecorderState::Recording, &e);
            }
        });
    }

    fn open_and_start(&self) -> Result<()> {
        let mut guard = self
            .audio
            .lock()
            .map_err(|e| anyhow!("audio mutex poisoned: {e}"))?;

        if guard.is_none() {
            let recorder = AudioRecorder::new().with_vad(voice_detector(&self.vad_path)?);
            *guard = Some(recorder);
        }

        let recorder = guard.as_mut().expect("recorder created above");
        let device = self.selected_device();
        recorder.open(device).map_err(|e| {
            let message = e.to_string();
            if crate::audio_toolkit::is_microphone_access_denied(&message) {
                anyhow!("Talkie needs microphone access — grant it in System Settings › Privacy & Security › Microphone")
            } else if crate::audio_toolkit::is_no_input_device_error(&message) {
                anyhow!("no microphone is available")
            } else {
                anyhow!("could not open the microphone: {message}")
            }
        })?;

        let ready = recorder
            .start()
            .map_err(|e| anyhow!("could not start recording: {e}"))?;

        // A stop or cancel that took the audio lock before this thread did
        // found nothing running to stop. The state has already moved on, so
        // nobody else will end the capture that just began: end it here.
        if self.state() != RecorderState::Recording {
            if let Err(e) = recorder.stop() {
                log::warn!("talkie: could not stop a capture that was already over: {e}");
            }
            return Ok(());
        }
        drop(guard);

        // Chime only once samples are actually flowing, so the sound never
        // promises a recording the hardware has not begun.
        match ready.recv_timeout(MIC_READY_TIMEOUT) {
            Ok(()) => {}
            // The consumer dropped the acknowledgement: a stop got there
            // first and the capture is already being finished elsewhere.
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
            Err(RecvTimeoutError::Timeout) => {
                // Take the consumer out of recording mode before reporting, or
                // it would keep collecting frames for a capture nobody will
                // ever stop.
                self.discard_capture();
                return Err(anyhow!(
                    "the microphone delivered no audio for {}s",
                    MIC_READY_TIMEOUT.as_secs()
                ));
            }
        }
        if self.settings_snapshot().play_sounds {
            sounds::play_start();
        }
        Ok(())
    }

    /// Abandon a capture: nothing is transcribed, no hook runs, no chime
    /// plays. Returns whether there was a capture to abandon.
    ///
    /// This is the double-tap's first half being taken back, and a short
    /// push-to-talk tap that turned out not to be one.
    pub fn cancel(self: &Arc<Self>) -> bool {
        if !self.transition(RecorderState::Recording, RecorderState::Idle) {
            return false;
        }
        // Off the caller's thread: the worker may still hold the audio lock
        // while a slow microphone opens, and the caller is the shortcut engine.
        let this = Arc::clone(self);
        std::thread::spawn(move || {
            let Ok(guard) = this.audio.lock() else {
                return;
            };
            // A new capture may have begun while this thread waited for the
            // lock. Its own start resets the consumer, so stopping it here
            // would end the wrong capture.
            if this.state() != RecorderState::Idle {
                return;
            }
            if let Some(recorder) = guard.as_ref() {
                if let Err(e) = recorder.stop() {
                    log::warn!("talkie: could not stop a cancelled capture: {e}");
                }
            }
        });
        true
    }

    /// Stop the consumer and throw away whatever it collected.
    fn discard_capture(&self) {
        let Ok(guard) = self.audio.lock() else {
            return;
        };
        if let Some(recorder) = guard.as_ref() {
            if let Err(e) = recorder.stop() {
                log::warn!("talkie: could not stop an abandoned capture: {e}");
            }
        }
    }

    /// Resolve the configured microphone, falling back to the system default.
    fn selected_device(&self) -> Option<cpal::Device> {
        let name = self.settings_snapshot().microphone?;
        let devices = crate::audio_toolkit::list_input_devices().ok()?;
        let found = devices.into_iter().find(|d| d.name == name);
        if found.is_none() {
            log::warn!("talkie: microphone {name:?} is not available; using the system default");
        }
        found.map(|d| d.device)
    }

    fn settings_snapshot(&self) -> talkie_shared::Settings {
        self.app
            .state::<SettingsState>()
            .0
            .lock()
            .expect("settings mutex poisoned")
            .clone()
    }

    /// End a capture and run the pipeline on it.
    pub fn stop(self: &Arc<Self>) {
        if !self.transition(RecorderState::Recording, RecorderState::Transcribing) {
            return;
        }

        let this = Arc::clone(self);
        std::thread::spawn(move || {
            if this.settings_snapshot().play_sounds {
                sounds::play_stop();
            }
            match this.finish_capture() {
                Ok(()) => {
                    let moved = this.transition(RecorderState::Transcribing, RecorderState::Idle);
                    debug_assert!(moved, "nothing else may leave Transcribing");
                }
                Err(e) => this.fail(RecorderState::Transcribing, &e),
            }
        });
    }

    fn finish_capture(&self) -> Result<()> {
        let samples = {
            let guard = self
                .audio
                .lock()
                .map_err(|e| anyhow!("audio mutex poisoned: {e}"))?;
            let recorder = guard
                .as_ref()
                .ok_or_else(|| anyhow!("the microphone was never opened"))?;
            recorder
                .stop()
                .map_err(|e| anyhow!("could not stop the recording: {e}"))?
        };

        // The consumer caps what it keeps; anything longer here means the cap
        // was bypassed and the model is about to be handed unbounded audio.
        debug_assert!(
            samples.len() <= MAX_CAPTURE_SAMPLES,
            "capture of {} samples exceeds the cap of {MAX_CAPTURE_SAMPLES}",
            samples.len()
        );

        // VAD may legitimately return nothing (a capture of pure silence), and
        // a stray tap produces a handful of frames. Neither is worth a model
        // run or an entry in the file.
        if samples.len() < MIN_CAPTURE_SAMPLES {
            log::info!(
                "talkie: capture too short ({} samples after VAD); nothing written",
                samples.len()
            );
            return Ok(());
        }

        let model_dir = models::model_path(&self.data_dir)?;
        let result = self.transcriber.transcribe(&model_dir, &samples);
        // Scheduled on the error path too: a failed transcription still leaves
        // the model resident.
        self.transcriber.schedule_idle_unload();
        let text = result?;

        let settings = self.settings_snapshot();
        let path = note::resolve(&settings.note_path);

        match land(&self.data_dir, &path, &text)? {
            // The app outlives any hook, so `after-capture` finishes on its own.
            Landed::Written { text, after: _ } => log::info!(
                "talkie: appended {} characters to {}",
                text.len(),
                path.display()
            ),
            Landed::Dropped => {
                log::info!("talkie: the on-capture hook dropped the capture; nothing written");
            }
            Landed::Blank => log::info!("talkie: the model heard nothing; nothing written"),
        }
        Ok(())
    }
}

/// The voice detector every capture runs through: Silero, smoothed so a
/// capture keeps the breath before a word and the tail after it.
pub fn voice_detector(vad_path: &Path) -> Result<Box<dyn VoiceActivityDetector>> {
    let silero = SileroVad::new(vad_path, VAD_THRESHOLD)
        .map_err(|e| anyhow!("could not start voice detection: {e}"))?;
    Ok(Box::new(SmoothedVad::new(
        Box::new(silero),
        VAD_PREFILL_FRAMES,
        VAD_HANGOVER_FRAMES,
        VAD_ONSET_FRAMES,
    )))
}

/// What became of a transcript on its way into the notes file.
pub enum Landed {
    /// The entry is in the file. `text` is what was written, which the
    /// `on-capture` hook may have changed, and `after` is the `after-capture`
    /// hook still running, if one is installed.
    Written {
        text: String,
        after: Option<JoinHandle<()>>,
    },
    /// The `on-capture` hook printed nothing: it said there was nothing to keep.
    Dropped,
    /// The transcript was blank, so the file was not touched.
    Blank,
}

/// The tail of every capture, whoever made it — the shortcut or `Talkie
/// capture` on the command line: the `on-capture` hook, the write, then the
/// `after-capture` hook.
pub fn land(data_dir: &Path, path: &Path, text: &str) -> Result<Landed> {
    // The one place a user's own code gets a say before the file changes.
    let Some(text) = hooks::on_capture(data_dir, path, text) else {
        return Ok(Landed::Dropped);
    };
    let written = note::prepend(path, &text)
        .with_context(|| format!("could not write to the notes file at {}", path.display()))?;
    if !written {
        return Ok(Landed::Blank);
    }
    let after = hooks::after_capture(data_dir, path, &text);
    Ok(Landed::Written { text, after })
}

/// Post a failure as a system notification, asking for permission the first
/// time. Best-effort: a notification that cannot be shown must never take the
/// capture path down with it.
fn notify(app: &AppHandle, message: &str) {
    use tauri_plugin_notification::{NotificationExt, PermissionState};

    let notification = app.notification();
    let permitted = match notification.permission_state() {
        Ok(PermissionState::Granted) => true,
        Ok(PermissionState::Denied) | Err(_) => false,
        Ok(_) => matches!(
            notification.request_permission(),
            Ok(PermissionState::Granted)
        ),
    };
    if !permitted {
        return;
    }

    if let Err(e) = notification
        .builder()
        .title(&app.package_info().name)
        .body(message)
        .show()
    {
        log::warn!("talkie: could not show the failure as a notification: {e}");
    }
}
