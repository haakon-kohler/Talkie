/// Every engine in the pipeline — Silero VAD and Parakeet alike — wants 16 kHz
/// mono, so the resampler always targets this rate.
pub const SAMPLE_RATE: u32 = 16000;
