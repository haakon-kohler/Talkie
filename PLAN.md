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
- Obsidian setup and Handy coexistence in `README.md`
- ad-hoc signed DMG
- model mirrored off `blob.handy.computer` before distributing
- notarization
- OpenClaw integration docs and hooks
- Windows / Linux pass
- archive rotation once one file gets unwieldy

## Bugs

- launch-at-login checkbox persists and nothing reads it — the setting lies
