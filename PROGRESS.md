# Talkie — Progress

Tracking against `talkie_implementation_plan.md`. One checklist per milestone; keep it current.

## M0 — Scaffold

- [x] Rename Handy clone → `0 - Projects/Handy/`; new workspace at `0 - Projects/Talkie/`
- [x] Install tooling (`create-tauri-app`, `tauri-cli` v2; trunk + wasm32 target already present)
- [x] Generate create-tauri-app Leptos template as seed (in a temp dir)
- [x] Restructure into cargo workspace: `shared/`, `ui/`, `src-tauri/` + `Trunk.toml`, `.gitignore`
- [x] `shared` crate: `Settings`, `RecorderState`, command/event name consts
- [x] `ui` crate: Leptos CSR, `index.html`, window-label routing in `main.rs`
- [x] `ipc.rs` — typed invoke/listen shim over `window.__TAURI__` (`withGlobalTauri: true`)
- [x] CodeMirror: one-time rollup bundle (outside repo) → `ui/assets/vendor/codemirror.bundle.js` + pinned-version README
- [x] `cm.rs` — typed wasm-bindgen wrapper over the vendored bundle (~8 externs)
- [x] `src-tauri`: three windows (editor = hidden titlebar/Overlay, settings, onboarding)
- [x] Tray icon + menu: Open Notes · Record · Settings · Quit
- [x] Backend-side settings via `tauri-plugin-store`; UI reads/writes via commands
- [x] Verify: `trunk build` clean, `cargo tauri dev` launches, tray works, IPC round-trip proven
- [x] Repo docs: `AGENTS.md`, `CLAUDE.md`, `README.md`, `LICENSE`
- [x] `git init` + initial commit

### M0 notes

- Resolved a dual-Rust collision: Homebrew's rust (no wasm32 std) shadowed
  rustup on PATH. Homebrew rust is uninstalled; rustup stable (1.97.1) is now
  the only toolchain, and `rust-toolchain.toml` pins the wasm target.
- Tray menu carries a disabled **Record** item; it activates in M1.
- Harmless build note: `proc-macro-error2` (transitive, via Leptos macros) emits a
  future-incompatibility warning. Upstream's to fix; nothing to do here.
- Tray uses the default Tauri icon for now — a real template icon is an M3 task.
- The editor mounts CodeMirror on a sample document. Wiring it to talkie.md,
  autosave, and the file watcher are M2.

## M1 — Shortcut → local model → talkie.md

- [x] Port `audio_toolkit` from Handy (cpal capture, FFT resampler, Silero VAD)
- [x] Bundle `silero_vad_v4.onnx` as a Tauri resource
- [x] `models.rs`: Parakeet V3 downloader — stream, SHA-256 verify, staged unpack, progress events
- [x] `transcriber.rs`: lazy `ParakeetModel` load + transcribe
- [x] `note.rs`: append engine and the document contract (with tests)
- [x] `recorder.rs`: Idle → Recording → Transcribing state machine
- [x] `shortcut.rs`: global shortcut, toggle **and** push-to-talk
- [x] `sounds.rs`: synthesised start/stop chimes
- [x] Onboarding: mic permission step + model download with progress bar
- [x] Tray: Record item enabled, tooltip reflects capture state
- [x] **End-to-end run: download the model, speak, confirm the entry lands**

### M1 notes

- The plan had the Silero VAD model as a second download; Handy ships it as a
  1.7 MB bundled resource and Talkie now does the same, so first run needs one
  download instead of two and voice detection works before it finishes.
- `transcribe-rs` 0.3.8 exposes `onnx::parakeet::ParakeetModel::load` +
  `transcribe_with`, not the `ParakeetEngine` shape Handy's newer code uses.
- Chimes are synthesised sine blips (rodio), not shipped audio files: no assets,
  no licences, and the output stream is opened per chime so the audio device is
  not held awake between captures.
- Trimmed out of the port: spectrum visualiser, level/streaming callbacks, the
  second (streaming) VAD profile, `lang_id.rs`, `text.rs`. Handy's own unit
  tests for the resampler and recorder came across and pass.
- Captures under 250 ms of post-VAD audio, and transcriptions that come back
  empty, are dropped without touching the file.
- End-to-end pass (2026-08-19) surfaced two fixes: `env_logger` is now
  initialised (failures were invisible before), and a bad accelerator in the
  store — `"Command"` typed into the free-text field — left the app with no
  hotkey. `set_settings` now rejects an unparseable shortcut before persisting,
  and `shortcut::apply` falls back to `DEFAULT_SHORTCUT` if the stored value
  won't parse.
- The Notepad window still shows the sample document by design; the file on
  disk is the pipeline's output. The editor reads the real file in M2.

## M1.5 — Shortcut recorder & hotkey engine

Slotted between M1 and M2. The free-text accelerator field was the wrong shape:
it could be typed wrong (it was, once), and it can't express the shortcut the
user actually wants — a held right-hand modifier.

**Engine decision.** `tauri-plugin-global-shortcut` is out; `handy-keys 0.3.4`
(crates.io, same crate Handy ships) is in. The plugin sits on Carbon
`RegisterEventHotKey`, which has no left/right modifier bits and refuses a
hotkey with no non-modifier key — so ⌘-right-held is unbindable through it, no
matter how good the recorder is. `handy-keys` gives side-specific modifiers,
modifier-only hotkeys, and a real press/release edge for push-to-talk. The cost
is macOS **Accessibility permission**, which Talkie previously did not need;
that becomes an onboarding step and a documented change of stance.

- [x] Swap the dependency: drop `tauri-plugin-global-shortcut`, add `handy-keys = "0.3.4"`
- [x] `shortcut.rs`: manager thread owning `HotkeyManager`, mpsc register/unregister, Pressed/Released → recorder
- [x] Accessibility: `check_accessibility` / `open_accessibility_settings` behind two commands
- [x] Onboarding: an Accessibility step alongside the mic step
- [x] The same Accessibility block in settings, and a `retry_shortcut` command so a
      grant that arrives late binds the hotkey without a restart
- [x] Recording mode: `KeyboardListener` on a poll thread, key events → `talkie://shortcut-capture`
- [x] Live hotkey unregistered while recording, restored on stop or cancel
- [x] `shared`: `ShortcutCapture` payload + a dependency-free `format_shortcut` glyph helper
- [x] `set_settings` validation: must parse; a bare key with no modifiers is refused
- [x] Settings UI: the text field becomes a click-to-record field — live glyph preview, auto-commit on release, Esc cancels
- [x] Commit rule: a keyed combo commits on its key's release; a modifier-only one when the last modifier is released
- [x] Migration: existing `"Control+Alt+Space"` still parses, so stored settings carry over untouched
- [x] Docs: `AGENTS.md` (permission stance), `talkie_implementation_plan.md` (engine table), `COPY.md` (new strings)
- [x] End-to-end: record ⌘-right held, speak, confirm the entry lands

### M1.5 notes

- Blocking mode is on (`HotkeyManager::new_with_blocking`): a bound shortcut is
  Talkie's alone and does not also reach the app in front. That is the point of
  a held-modifier shortcut — but it does mean binding right ⌘ takes right ⌘ away
  from ⌘C and friends, which is the deal Handy makes too.
- The recorder waits for *every* key to come up before committing, not the first
  release. Committing on the first release is how a combination silently saves
  as just its modifiers (Handy's issue #1578); waiting for the keyboard to be
  empty is also what makes a modifier-only shortcut recordable at all.
- The M1 bug value — a bare `"Command"` in the store — is a legal shortcut now
  rather than an error, and there is a test pinning that so nobody "fixes" it.
- Accessibility cannot be granted to the `cargo tauri dev` binary at all: it has
  no `Info.plist` and no bundle id, and as a child of the terminal macOS blames
  the terminal for the request. Adding `target/debug/talkie` to the pane looks
  like it worked and does nothing. Use `cargo tauri build --debug --bundles app`
  plus an explicit `codesign -s -`; see AGENTS.md for the incantation.
- Bundling turned up an unrelated blocker: `trunk build --release` died in
  `wasm-opt` ("error validating input" on `memory.copy`). Fixed with explicit
  feature flags in `ui/index.html`; the release wasm optimises to 450 KB.
- The Accessibility ask lives in `ui/src/accessibility.rs` and is mounted by
  *both* settings and onboarding, the way `ModelSection` already was. Onboarding
  alone would have been useless: it runs once, and the grant can vanish later.
  The section renders nothing while the permission is in place.

### Deferred by choice

Reset-to-default button, Handy-clash warnings (Handy's own binding moves too
often for a hardcoded list to stay true), and any recorder chrome beyond the
field itself.

## M2 — The editor

Not started.

## M3 — Settings & robustness

Not started.

## M4 — Shippable

Not started.
