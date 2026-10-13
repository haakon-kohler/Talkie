//! Parakeet V3, loaded lazily and kept warm.
//!
//! Loading the ONNX session costs a second or two, so the first capture pays it
//! and every later one does not. The model stays resident for five idle
//! minutes, then unloads — a burst of captures pays one load, and the memory
//! comes back once the burst is over.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Result};
use transcribe_rs::onnx::parakeet::{ParakeetModel, ParakeetParams, TimestampGranularity};
use transcribe_rs::onnx::Quantization;

use crate::audio_toolkit::constants::MAX_CAPTURE_SAMPLES;

/// How long the model stays resident after its last use. Deliberately not a
/// setting.
const IDLE_UNLOAD: Duration = Duration::from_mins(5);

/// Owns the loaded engine. One capture runs at a time, so a mutex is the whole
/// concurrency story.
pub struct Transcriber {
    engine: Mutex<Option<ParakeetModel>>,
    /// Bumped on every use; an idle timer only unloads when it still holds the
    /// generation it was scheduled with, so any use in between cancels it.
    generation: AtomicU64,
}

impl Transcriber {
    pub fn new() -> Self {
        Self {
            engine: Mutex::new(None),
            generation: AtomicU64::new(0),
        }
    }

    /// Transcribe 16 kHz mono samples. Blocking and CPU-bound — call it off the
    /// UI thread.
    pub fn transcribe(&self, model_dir: &Path, samples: &[f32]) -> Result<String> {
        // The recorder drops captures too short to be worth a model run, so
        // this is a bug upstream; it fails the one capture, not the app.
        anyhow::ensure!(!samples.is_empty(), "transcribe called with no audio");
        debug_assert!(
            samples.len() <= MAX_CAPTURE_SAMPLES,
            "the recorder must cap a capture before it reaches the model"
        );
        // Bumped on entry, not on completion: an idle timer that fires while
        // this is running must already see the model as in use, or it would
        // queue up on the mutex and unload a model the moment it finished.
        self.generation.fetch_add(1, Ordering::SeqCst);

        let mut guard = self
            .engine
            .lock()
            .map_err(|e| anyhow!("transcriber lock poisoned: {e}"))?;

        let model = match guard.take() {
            Some(model) => model,
            None => ParakeetModel::load(model_dir, &Quantization::Int8).map_err(|e| {
                anyhow!(
                    "could not load the speech model from {}: {e}",
                    model_dir.display()
                )
            })?,
        };
        let model = guard.insert(model);
        // Segment granularity is the cheapest option that still returns text;
        // Talkie never renders timestamps from the model, it writes its own.
        let params = ParakeetParams {
            timestamp_granularity: Some(TimestampGranularity::Segment),
            ..Default::default()
        };
        let result = model
            .transcribe_with(samples, &params)
            .map_err(|e| anyhow!("transcription failed: {e}"))?;

        Ok(result.text.trim().to_string())
    }

    /// Start (or push back) the idle clock: five minutes from now, unload —
    /// unless another capture used the model in between.
    pub fn schedule_idle_unload(self: &Arc<Self>) {
        let scheduled = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let this = Arc::clone(self);
        std::thread::spawn(move || {
            std::thread::sleep(IDLE_UNLOAD);
            if this.generation.load(Ordering::SeqCst) == scheduled {
                this.unload();
                log::info!("talkie: speech model unloaded after five idle minutes");
            }
        });
    }

    /// Drop the ONNX sessions and their memory.
    fn unload(&self) {
        if let Ok(mut guard) = self.engine.lock() {
            guard.take();
        }
    }
}

impl Default for Transcriber {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An empty capture is a bug upstream, and it fails that one capture: no
    /// panic, and no attempt to load a model to transcribe nothing.
    #[test]
    fn no_audio_is_an_error_not_a_panic() {
        let transcriber = Transcriber::new();
        let error = transcriber
            .transcribe(Path::new("/nonexistent/model"), &[])
            .expect_err("nothing to transcribe");
        assert_eq!(error.to_string(), "transcribe called with no audio");
        assert!(
            transcriber.engine.lock().unwrap().is_none(),
            "no model was loaded"
        );
    }
}
