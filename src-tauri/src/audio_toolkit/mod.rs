//! Microphone capture, resampling, and voice-activity detection.
//!
//! Ported from Handy (<https://github.com/cjpais/Handy>), MIT licensed:
//!
//! > Copyright (c) 2025 CJ Pais
//! >
//! > Permission is hereby granted, free of charge, to any person obtaining a copy
//! > of this software and associated documentation files (the "Software"), to deal
//! > in the Software without restriction, including without limitation the rights
//! > to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
//! > copies of the Software, and to permit persons to whom the Software is
//! > furnished to do so, subject to the following conditions:
//! >
//! > The above copyright notice and this permission notice shall be included in
//! > all copies or substantial portions of the Software.
//!
//! Talkie's copy is trimmed to the record-to-file path: no spectrum visualiser,
//! no level callbacks, no streaming-transcription frame callback, and one VAD
//! profile instead of two. What remains — the cpal worker, the FFT resampler,
//! and the Silero VAD wrapper — is Handy's code with its comments intact.

pub mod audio;
pub mod constants;
pub mod utils;
pub mod vad;

pub use audio::{
    is_microphone_access_denied, is_no_input_device_error, list_input_devices, AudioRecorder,
};
pub use utils::get_cpal_host;
