# Talkie — What's Left

## Features

- ⌘B / ⌘I from the keyboard, no toolbar — regenerates the frozen CodeMirror bundle
- one H2 per minute instead of per capture — changes the document contract
- keyboard route to the notepad — mechanism undecided, not a double-tap
- notes file location picker — needs the dialog plugin
- microphone picker — host already resolves the setting, UI and device enumeration missing
- model idle-unload timer — `transcriber::unload` is the hook
- capture-failure surfacing in the UI
- real app icon, tray still on Tauri's default
- tray icon states for recording and transcribing, not just the tooltip
- launch at login, host side
- single-instance guard
- stop macOS offering Mic Mode for a few-seconds capture
- document contract back in `README.md`
- Obsidian setup and Handy coexistence in `README.md`
- ad-hoc signed DMG
- model mirrored off `blob.handy.computer` before distributing
- notarization
- OpenClaw integration docs and hooks
- Windows / Linux pass
- archive rotation once one file gets unwieldy

## Bugs

- launch-at-login checkbox persists and nothing reads it — the setting lies
- `CAPTURE_FAILED` emitted by `recorder.rs:63`, no listener anywhere in `ui/src` — a failed capture is silent
- `README.md` lost the document contract in the description rewrite
- notes path is unvalidated free text — a typo saves clean and fails at capture time, the shape that already bit once with the accelerator field
- existing installs stay on `push_to_talk: false` — serde fills defaults only for absent fields
- `cm.rs::open_search` has no caller — the bundle's own `searchKeymap` owns ⌘F
