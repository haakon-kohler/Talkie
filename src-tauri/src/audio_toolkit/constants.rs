/// Every engine in the pipeline — Silero VAD and Parakeet alike — wants 16 kHz
/// mono, so the resampler always targets this rate.
pub const SAMPLE_RATE: u32 = 16000;

/// The most audio one capture may keep, in seconds of post-VAD speech.
///
/// A capture has no natural upper bound: a missed key release, or a forgotten
/// toggle in a room with the radio on, keeps the consumer collecting speech
/// frames for as long as the app runs. The transcriber then runs the whole
/// buffer through the encoder in one pass, whose attention memory grows with
/// the square of the length — a multi-hour capture is a machine-freezing
/// allocation, not a long note. Ten minutes is far past any voice note and
/// well inside what the model handles.
pub const MAX_CAPTURE_SECONDS: usize = 10 * 60;

/// `MAX_CAPTURE_SECONDS` in samples at `SAMPLE_RATE`: about 38 MB of `f32`.
pub const MAX_CAPTURE_SAMPLES: usize = SAMPLE_RATE as usize * MAX_CAPTURE_SECONDS;
