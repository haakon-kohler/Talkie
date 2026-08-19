//! The start and stop chimes.
//!
//! In a flow with no window and no paste, these two tones are the entire
//! feedback channel: one says "I'm listening", the other says "I've stopped".
//! They are synthesised rather than shipped as assets — two sine blips with a
//! short fade need no files, no licences, and no attribution.

use std::time::Duration;

use rodio::source::{SineWave, Source};
use rodio::OutputStreamBuilder;

/// A rising two-note blip: recording has started.
const START_TONES: [f32; 2] = [660.0, 880.0];
/// The same interval falling: recording has stopped.
const STOP_TONES: [f32; 2] = [880.0, 660.0];

const TONE_MS: u64 = 70;
/// Quiet on purpose. This is a background app; a loud chime in a silent room is
/// worse than no chime at all.
const AMPLITUDE: f32 = 0.12;

pub fn play_start() {
    play(&START_TONES);
}

pub fn play_stop() {
    play(&STOP_TONES);
}

/// Play a sequence of tones on a detached thread.
///
/// Audio output is opened per chime and dropped afterwards: holding an output
/// stream open for the life of a menu-bar app keeps the audio device awake (and
/// on macOS can keep a Bluetooth headset in the wrong profile) for the sake of
/// two blips a minute. Failures are logged and swallowed — a missing chime must
/// never take a capture down with it.
fn play(tones: &'static [f32]) {
    std::thread::spawn(move || {
        let stream = match OutputStreamBuilder::open_default_stream() {
            Ok(stream) => stream,
            Err(e) => {
                log::warn!("talkie: no audio output for the chime: {e}");
                return;
            }
        };

        let sink = rodio::Sink::connect_new(stream.mixer());
        for &freq in tones {
            let tone = SineWave::new(freq)
                .take_duration(Duration::from_millis(TONE_MS))
                .amplify(AMPLITUDE)
                // Without a fade the abrupt start and end of a sine burst click.
                .fade_in(Duration::from_millis(8));
            sink.append(tone);
        }
        sink.sleep_until_end();
    });
}
