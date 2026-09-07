//! The capture state machine: Idle → Recording → Transcribing → Idle.
//!
//! This is the module the shortcut, the tray, and the UI all drive. It owns the
//! microphone, decides what a press means, and — when a capture finishes — hands
//! the samples to the transcriber and the text to the append engine.
//!
//! Everything slow happens off the calling thread. A shortcut press returns
//! immediately; the microphone opens, the model runs, and the file is written on
//! a worker, with `RECORDER_STATE` events narrating the transitions.

use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context, Result};
use talkie_shared::{events, ModelStatus, RecorderState};
use tauri::{AppHandle, Emitter, Manager};

use crate::audio_toolkit::vad::{
    SileroVad, SmoothedVad, VAD_HANGOVER_FRAMES, VAD_ONSET_FRAMES, VAD_PREFILL_FRAMES,
    VAD_THRESHOLD,
};
use crate::audio_toolkit::AudioRecorder;
use crate::settings::SettingsState;
use crate::transcriber::Transcriber;
use crate::{models, note, sounds, tray};

/// Captures shorter than this are treated as a slip of the finger — a
/// double-tap on the shortcut, a key held for a moment — and dropped without
/// running the model.
const MIN_CAPTURE_SAMPLES: usize = 16000 / 4; // 250 ms at 16 kHz

pub struct Recorder {
    app: AppHandle,
    state: Mutex<RecorderState>,
    audio: Mutex<Option<AudioRecorder>>,
    transcriber: Arc<Transcriber>,
}

impl Recorder {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            state: Mutex::new(RecorderState::Idle),
            audio: Mutex::new(None),
            transcriber: Arc::new(Transcriber::new()),
        }
    }

    pub fn state(&self) -> RecorderState {
        *self.state.lock().expect("recorder state mutex poisoned")
    }

    fn set_state(&self, next: RecorderState) {
        *self.state.lock().expect("recorder state mutex poisoned") = next;
        let _ = self.app.emit(events::RECORDER_STATE, next);
        tray::set_state(&self.app, next);
    }

    /// Report a failure the ways a silent app can: the log, an event the UI may
    /// be listening to, and a system notification — the windows are usually all
    /// closed when a capture fails. Never panics the capture path.
    fn fail(&self, error: anyhow::Error) {
        let message = format!("{error:#}");
        log::error!("talkie: {message}");
        notify(&self.app, &message);
        let _ = self.app.emit(events::CAPTURE_FAILED, message);
        self.set_state(RecorderState::Idle);
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
        if models::status(&self.app) != ModelStatus::Ready {
            self.fail(anyhow!(
                // COPY: capture.no_model
                "speech model not yet installed — finish first run to download it"
            ));
            return;
        }

        // Claim the state before the worker starts so a second press cannot
        // open the microphone twice.
        self.set_state(RecorderState::Recording);

        let this = Arc::clone(self);
        std::thread::spawn(move || {
            if let Err(e) = this.open_and_start() {
                this.fail(e);
            }
        });
    }

    fn open_and_start(&self) -> Result<()> {
        let mut guard = self
            .audio
            .lock()
            .map_err(|_| anyhow!("audio mutex poisoned"))?;

        if guard.is_none() {
            let vad_path = models::vad_path(&self.app)?;
            let silero = SileroVad::new(&vad_path, VAD_THRESHOLD)
                .map_err(|e| anyhow!("could not start voice detection: {e}"))?;
            let smoothed = SmoothedVad::new(
                Box::new(silero),
                VAD_PREFILL_FRAMES,
                VAD_HANGOVER_FRAMES,
                VAD_ONSET_FRAMES,
            );
            let recorder = AudioRecorder::new()
                .map_err(|e| anyhow!("could not create the recorder: {e}"))?
                .with_vad(Box::new(smoothed));
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
        drop(guard);

        // Chime only once samples are actually flowing, so the sound never
        // promises a recording the hardware has not begun.
        let _ = ready.recv();
        if self.settings_snapshot().play_sounds {
            sounds::play_start();
        }
        Ok(())
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
        if self.state() != RecorderState::Recording {
            return;
        }
        self.set_state(RecorderState::Transcribing);

        let this = Arc::clone(self);
        std::thread::spawn(move || {
            if this.settings_snapshot().play_sounds {
                sounds::play_stop();
            }
            match this.finish_capture() {
                Ok(()) => this.set_state(RecorderState::Idle),
                Err(e) => this.fail(e),
            }
        });
    }

    fn finish_capture(&self) -> Result<()> {
        let samples = {
            let guard = self
                .audio
                .lock()
                .map_err(|_| anyhow!("audio mutex poisoned"))?;
            let recorder = guard
                .as_ref()
                .ok_or_else(|| anyhow!("the microphone was never opened"))?;
            recorder
                .stop()
                .map_err(|e| anyhow!("could not stop the recording: {e}"))?
        };

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

        let model_dir = models::model_path(&self.app)?;
        let result = self.transcriber.transcribe(&model_dir, samples);
        // Scheduled on the error path too: a failed transcription still leaves
        // the model resident.
        self.transcriber.schedule_idle_unload();
        let text = result?;

        let settings = self.settings_snapshot();
        let path = note::resolve(&settings.note_path);
        let written = note::prepend(&path, &text)
            .with_context(|| format!("could not write to the notes file at {}", path.display()))?;

        if written {
            log::info!(
                "talkie: appended {} characters to {}",
                text.len(),
                path.display()
            );
        } else {
            log::info!("talkie: the model heard nothing; nothing written");
        }
        Ok(())
    }
}

/// Post a failure as a system notification, asking for permission the first
/// time. Best-effort: a notification that cannot be shown must never take the
/// capture path down with it.
fn notify(app: &AppHandle, message: &str) {
    use tauri_plugin_notification::{NotificationExt, PermissionState};

    let notification = app.notification();
    let permitted = match notification.permission_state() {
        Ok(PermissionState::Granted) => true,
        Ok(PermissionState::Denied) => false,
        Ok(_) => matches!(
            notification.request_permission(),
            Ok(PermissionState::Granted)
        ),
        Err(_) => false,
    };
    if !permitted {
        return;
    }

    let _ = notification
        .builder()
        .title(talkie_shared::APP_NAME)
        .body(message)
        .show();
}
