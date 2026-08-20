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

- [x] `note.rs`: `read`, atomic `write` (temp + rename), and the trailing-newline contract
- [x] `note::reconcile`: decide a save against what is on disk and what the editor last saw
- [x] `watcher.rs`: watch the note's *directory* (renames move the inode), settle a burst, emit `NOTE_CHANGED_EXTERNALLY`
- [x] Distinguish Talkie's own writes from everyone else's, without timing hacks
- [x] `read_note` / `write_note` commands; re-arm the watcher when the note path changes
- [x] Editor mounts on the real `talkie.md`, scrolled to the newest entry
- [x] Debounced autosave (600 ms), flushed immediately on window blur
- [x] Reload on external change, but only with nothing unsaved
- [x] A capture that lands mid-edit is carried over instead of overwritten
- [x] Apply an append as an append: cursor, undo history and scroll survive it
- [x] The one permitted piece of chrome: a save failure says so
- [x] **End-to-end: the editor opens the real file, edits save, captures land while it is open**

### M2 notes

- The editor is not the only writer, so the save is not a plain write. It
  compares three versions — what the editor sends, what is on disk, and what the
  editor last read — and carries an external *append* over onto the end of its
  own text. That is exactly the capture-lands-while-you-type case, and it is the
  one thing this app must never lose. Anything less clear-cut refuses the write
  and says so rather than picking a winner. `note::reconcile` is pure and tested.
- The watcher tracks the last-seen *content*, not a hash of it. A hash is enough
  to recognise Talkie's own writes, but not to serve as the base of that merge.
- Watching the directory rather than the file is deliberate: every editor worth
  the name saves by rename, Talkie's own `note::write` included, and a watch on
  the file would silently detach the first time that happened.
- End-to-end passed on 2026-08-20 with one gap: *truly simultaneous* editing —
  Talkie and Obsidian typing into the file at the same instant — was not
  exercised, so `note::reconcile`'s conflict branch has only ever run in tests.
  Left there deliberately; it is an esoteric case and the code refuses rather
  than guesses, so the failure mode is a visible message, not lost text.
- The tray's "Open Notes" and the hidden-titlebar window already existed from M0.
  The plan's optional second global shortcut for the editor is not built — the
  tray and ⌘W are enough, and it would need a second recorder in settings.

## M2.5 — Newest first

The capture log reads better upside down: put a new entry at the *top*, so
scrolling down walks backwards through time and the thing you just said is the
thing you are looking at. Decided immediately after M2, and taken then rather
than later — it changes the document contract, which M4 documents publicly, and
every file written in the old order is one more file with a seam in it.

- [x] `shared/src/document.rs`: the contract as code — insertion point, splice, capture detection, save reconciliation
- [x] Insert below YAML frontmatter and a leading `#` title, not at byte zero
- [x] `note.rs` becomes the disk half: `append` → `prepend`, built on the shared rules
- [x] `document::reconcile` inverted: a capture now arrives at the head, not the tail
- [x] Vendored bundle regenerated: `appendAndReveal` + `scrollToEnd` → `insertAndReveal(view, pos, text)`
- [x] `cm.rs` down to eight externs; byte offsets converted to UTF-16 at the boundary
- [x] Editor opens at the top and applies a capture as an insert, keeping the cursor
- [x] Contract updated in `README.md`, `AGENTS.md`, `ui/assets/vendor/README.md`
- [x] End-to-end: speak twice, confirm the newer entry is on top and the older one is untouched

### M2.5 notes

- The insertion rules moved into `shared` rather than staying host-side. Both
  halves need them now — the host to save, the editor to apply a capture at the
  right offset — and a second copy of "where does an entry go" is exactly the
  kind of thing that drifts silently and corrupts a file.
- Existing files are left alone, so a `talkie.md` written before today has one
  seam: newest-first above, oldest-first below. A migration that reversed the
  file was considered and rejected — it is code that rewrites your notes, runs
  once, and is hard to test against files it has never seen.
- Frontmatter is the trap. Inserting at byte zero would push a `---` block down
  and stop it being frontmatter, breaking the vault the file sits in. A leading
  `#` title is treated the same way. An unterminated `---` is a horizontal rule,
  not frontmatter, and is left alone — there is a test for it.
- The bundle rebuild resolved every direct *and* transitive dependency to the
  versions already pinned in `ui/assets/vendor/README.md`, so the diff in that
  checked-in artifact is the facade and nothing else.
- `insertAndReveal` deliberately does not move the cursor. CodeMirror maps the
  existing selection through the insertion, so someone mid-sentence when a
  capture lands keeps their place.

## M3 — Settings & robustness

Not started.

## M4 — Shippable

Not started.
