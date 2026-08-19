//! Parakeet V3, loaded lazily and kept warm.
//!
//! Loading the ONNX session costs a second or two, so the first capture pays it
//! and every later one does not. The model stays resident afterwards; the
//! idle-unload timer is an M3 concern.

use std::path::Path;
use std::sync::Mutex;

use anyhow::{anyhow, Result};
use transcribe_rs::onnx::parakeet::{ParakeetModel, ParakeetParams, TimestampGranularity};
use transcribe_rs::onnx::Quantization;

/// Owns the loaded engine. One capture runs at a time, so a mutex is the whole
/// concurrency story.
pub struct Transcriber {
    engine: Mutex<Option<ParakeetModel>>,
}

impl Transcriber {
    pub fn new() -> Self {
        Self {
            engine: Mutex::new(None),
        }
    }

    /// Transcribe 16 kHz mono samples. Blocking and CPU-bound — call it off the
    /// UI thread.
    pub fn transcribe(&self, model_dir: &Path, samples: Vec<f32>) -> Result<String> {
        let mut guard = self
            .engine
            .lock()
            .map_err(|_| anyhow!("transcriber lock poisoned"))?;

        if guard.is_none() {
            let model = ParakeetModel::load(model_dir, &Quantization::Int8)
                .map_err(|e| anyhow!("could not load the speech model from {model_dir:?}: {e}"))?;
            *guard = Some(model);
        }

        let model = guard.as_mut().expect("model loaded above");
        // Segment granularity is the cheapest option that still returns text;
        // Talkie never renders timestamps from the model, it writes its own.
        let params = ParakeetParams {
            timestamp_granularity: Some(TimestampGranularity::Segment),
            ..Default::default()
        };
        let result = model
            .transcribe_with(&samples, &params)
            .map_err(|e| anyhow!("transcription failed: {e}"))?;

        Ok(result.text.trim().to_string())
    }

    /// Drop the ONNX sessions and their memory. Nothing calls this yet; it is
    /// the hook M3's idle timer needs.
    #[allow(dead_code)]
    pub fn unload(&self) {
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
