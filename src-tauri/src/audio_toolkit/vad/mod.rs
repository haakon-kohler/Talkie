use anyhow::Result;

/// Frames of audio kept from *before* speech was detected, so a capture never
/// clips the first syllable.
pub const VAD_PREFILL_FRAMES: usize = 15;
/// Frames of audio kept after speech stops — the tail of a sentence.
pub const VAD_HANGOVER_FRAMES: usize = 15;
/// Consecutive speech frames required before the detector commits to "speech",
/// which keeps a cough or a door from opening a capture.
pub const VAD_ONSET_FRAMES: usize = 2;
/// Silero speech probability above which a frame counts as voice.
pub const VAD_THRESHOLD: f32 = 0.5;

pub enum VadFrame<'a> {
    /// Speech – may aggregate several frames (prefill + current + hangover)
    Speech(&'a [f32]),
    /// Non-speech (silence, noise). Down-stream code can ignore it.
    Noise,
}

impl VadFrame<'_> {
    #[inline]
    pub fn is_speech(&self) -> bool {
        matches!(self, VadFrame::Speech(_))
    }
}

pub trait VoiceActivityDetector: Send + Sync {
    /// Primary streaming API: feed one 30-ms frame, get keep/drop decision.
    fn push_frame<'a>(&'a mut self, frame: &'a [f32]) -> Result<VadFrame<'a>>;

    fn is_voice(&mut self, frame: &[f32]) -> Result<bool> {
        Ok(self.push_frame(frame)?.is_speech())
    }

    fn reset(&mut self) {}
}

mod silero;
mod smoothed;

pub use silero::SileroVad;
pub use smoothed::SmoothedVad;
