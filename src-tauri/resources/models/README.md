# Bundled models

`silero_vad_v4.onnx` — [Silero VAD](https://github.com/snakers4/silero-vad) v4,
MIT licensed. 1.7 MB, so it ships inside the app bundle rather than being
downloaded: first run then needs exactly one download (Parakeet, 456 MB) instead
of two, and voice detection works before that download finishes.

Copied from Handy's `resources/models/`, same file, same version.

The speech model is *not* here. Parakeet V3 is fetched at first run into
`<app-data>/models/parakeet-tdt-0.6b-v3-int8/` — see `src/models.rs`.
