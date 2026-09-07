# Talkie — Implementation Plan

*Drafted 2026-08-18 from talkie_plan.md + codebase review of the Handy clone. Decisions to date: separate companion app · single talkie.md · zero-chrome macOS-native editor · silent append · fresh Tauri app porting Handy's pipeline · **Leptos (CSR) frontend via Trunk, zero-node toolchain, handwritten CSS, CodeMirror vendored as a single frozen asset**.*

## What we're building

Talkie is a standalone menu-bar companion app. A global shortcut records your voice, a **local** Parakeet V3 model transcribes it, and the text is **silently appended** as a timestamped entry to one long `talkie.md` file. A second surface — a deliberately minimal, buttonless, macOS-native markdown editor — reads and edits that same file. Handy stays installed and untouched for paste-into-app dictation; Talkie owns the record-to-document use case. The markdown file is the integration surface for everything else (Obsidian just indexes it; an OpenClaw agent just watches it — no in-app agent features).

## Language & toolchain stance

One language, one toolchain: **Rust end-to-end**. The UI is Leptos compiled to WASM by **Trunk**; the backend is the Tauri host. No node, no npm, no package.json anywhere in the repo. The single exception to "no JS" is CodeMirror, which enters the repo as **one prebuilt, frozen ESM asset** (a build artifact, like a font file) behind a thin typed `wasm-bindgen` wrapper — because every mature editor component is JS, and building a text editor from scratch is exactly the scope-creep tar pit talkie_plan.md warns about.

Known costs, accepted with eyes open:

- **No HMR.** Trunk rebuilds the WASM (a few seconds at this app size) and reloads the page. UI polish loops are slower than Vite. Tolerable for one window; Dioxus is the escape hatch if it ever isn't.
- **One untyped edge.** The wasm-bindgen boundary into the CodeMirror wrapper is hand-declared. Mitigation: keep the wrapper surface tiny (~8 functions) and stable.
- WASM debugging is printf-flavored; Leptos `view!` macro errors can be gnarly. Small app keeps both contained.

## Why the pipeline is cheap (verified in Handy's source)

| Need | Source | Notes |
| --- | --- | --- |
| Mic capture, device enum, resampling, VAD | Handy's `src-tauri/src/audio_toolkit/` | Self-contained module (cpal 0.16, rubato 0.16.2, vad-rs). MIT — port with copyright notice retained. |
| Local ASR (Parakeet V3) | `transcribe-rs = "0.3.8"`, `features=["onnx"]` | Same crate Handy uses: `engines::parakeet::{ParakeetModel, ParakeetParams}`. Pure ONNX — **no whisper.cpp, no cmake**. |
| Model files | `https://blob.handy.computer/parakeet-v3-int8.tar.gz` (extracts to `parakeet-tdt-0.6b-v3-int8/`) + `silero_vad_v4.onnx` | Same URLs as Handy's catalog. Mirror before public distribution. |
| Global shortcut | `handy-keys 0.3.4` | Pressed/Released → toggle **and** push-to-talk. Side-specific modifiers (right ⌘) and modifier-only hotkeys, neither of which Carbon's `RegisterEventHotKey` — and so `tauri-plugin-global-shortcut` — can express. **Needs macOS Accessibility** (M1.5 reversed the original no-accessibility stance); mic permission too. |
| Feedback sounds | rodio + Handy's chime approach | Start/stop chimes are the "did it hear me?" signal in the silent flow. |

Deliberately omitted vs Handy: paste/accessibility, whisper.cpp models, LLM post-processing, history database (the md file *is* the history), i18n (v1 English), recording overlay (v1: tray icon state + chimes).

## Workspace architecture

```
talkie/                         ← new sibling repo (cargo workspace)
├── Cargo.toml                  # workspace: shared, ui, src-tauri
├── Trunk.toml                  # builds ui/ → dist/; `trunk serve` on :1420 in dev
├── shared/                     # serde types + command-name consts used by BOTH sides
│   └── src/lib.rs              #   Settings, RecorderState, events — no codegen, just one crate
├── ui/                         # Leptos CSR crate (wasm32-unknown-unknown)
│   ├── index.html
│   ├── src/
│   │   ├── main.rs             # mount; route by window label (editor / settings / onboarding)
│   │   ├── ipc.rs              # ~50-line typed invoke/listen shim over window.__TAURI__
│   │   ├── editor.rs           # editor page: CM host component, autosave, reload-on-external-change
│   │   ├── cm.rs               # #[wasm_bindgen(module=…)] typed wrapper over the vendored bundle
│   │   ├── settings.rs         # settings form
│   │   └── onboarding.rs       # mic permission + model download progress
│   ├── assets/vendor/
│   │   ├── codemirror.bundle.js   # frozen prebuilt ESM (checked in)
│   │   └── README.md              # exact pinned versions + one-time regen instructions
│   └── styles/app.css          # handwritten CSS (system font stack, CM theme, macOS chrome)
└── src-tauri/src/
    ├── lib.rs                  # setup: tray, windows, managers, single-instance, withGlobalTauri
    ├── audio_toolkit/          # ported from Handy (capture, VAD, resample)
    ├── transcriber.rs          # ParakeetModel wrapper: lazy load, idle unload
    ├── models.rs               # downloader: parakeet tar.gz + silero onnx, progress, checksum
    ├── recorder.rs             # state machine: Idle → Recording → Transcribing
    ├── note.rs                 # append engine + entry formatting + notify file watcher
    ├── shortcut.rs             # handy-keys engine thread: binding (toggle + PTT) + the recorder
    ├── tray.rs                 # icon states; menu: Open Notes · Record · Settings · Quit
    └── settings.rs             # tauri-plugin-store; settings live backend-side, UI reads/writes via commands
```

Frontend↔backend contract: `withGlobalTauri: true` exposes the Tauri API globally so the wasm binds to it with no npm package; `ipc.rs` wraps it as `async fn invoke<Args: Serialize, R: DeserializeOwned>(cmd, args)` (via serde-wasm-bindgen), and `shared::commands` holds the command names + arg/return types so both sides reference one definition. Settings state lives **only** in the backend (no zustand-style mirror) — the UI is a thin view over typed commands and events.

### CodeMirror vendoring

- One-time bundle (rollup on any machine — node never enters this repo) of the `@codemirror/*` packages into `codemirror.bundle.js`, exposing a small factory API: `init(parent, doc, onDocChanged)`, `getDoc`, `setDoc`, `appendAndReveal`, `scrollToEnd`, `openSearch`, `setTheme`. Committed with pinned versions + regen instructions in `assets/vendor/README.md`.
- `cm.rs` declares those eight-ish externs; everything above that line is typed Rust.
- Visual theme lives in `app.css` targeting CM's classes — the hand-CSS choice keeps full control of the tinting and macOS look.

## The document contract (stable, documented in README)

Every capture appends exactly:

```markdown

## 2026-08-18 09:14
Remember to email Sam about the demo Thursday.
```

- H2 heading = one capture, local time, blank line before each entry, file ends with newline.
- Default path `~/Documents/Talkie/talkie.md`; settings let you point it anywhere (e.g. inside an Obsidian vault — that alone delivers the Obsidian integration).
- Single-writer discipline inside the app: if the editor is open with unsaved changes, the append goes through the editor buffer then saves; otherwise Rust appends directly. The `notify` watcher emits a Tauri event on external changes (Obsidian, agent); the editor reloads when its buffer is clean and never clobbers a dirty buffer.
- This contract is what an OpenClaw agent consumes later — nothing in-app required.

## The editor (zero chrome, macOS-native)

- Vendored CodeMirror 6 with markdown mode: **source stays visible with syntax tinting** — no toolbar, no buttons, no status bar. Just the text.
- Native feel: hidden title bar with overlay traffic lights (`titleBarStyle: Overlay`), system font stack, ~68ch measure, generous padding, native-feeling overlay scrollbars, light/dark follows system (CSS `prefers-color-scheme` + `setTheme`).
- Autosave: Leptos signal debounced ~500 ms → `save_doc` command; also on blur/close. Opens scrolled to the latest entry. ⌘W hides the window (app lives in tray); ⌘F opens CM's search panel.
- Later polish (explicitly deferred): minimap-style scrollbar (added to the vendored bundle when wanted), ⌘⇧P rendered preview.

## Milestones

**M0 — Scaffold (small).** Cargo workspace at `0 - Projects/talkie-app/` (rename folders later if you want the clean `Talkie` name — the Handy clone currently occupies it). create-tauri-app Leptos template as the seed; Trunk wiring; `withGlobalTauri`; `shared` crate + `ipc.rs` shim; **build and commit the CodeMirror bundle**; tray with menu; one hidden-titlebar window; settings store backend-side.

**M1 — The first big task, part A: shortcut → local model → talkie.md.** *(Frontend-agnostic — unchanged by the Leptos decision.)*
Port `audio_toolkit`; model downloader with first-run progress UI (Leptos onboarding page); Parakeet V3 via transcribe-rs; VAD trim; global shortcut (toggle + PTT; default **⌃⌥Space** — no clash with Handy's ⌥Space / ⌥⇧Space); append engine; start/stop chimes; mic-permission onboarding.
*Acceptance: press shortcut, speak, press again → entry lands in talkie.md in ~1–2 s, fully offline, while Handy keeps working normally.*

**M2 — The first big task, part B: the editor.** CM host component in Leptos per the spec above; autosave; watcher → event → reload-if-clean; hand-CSS macOS styling + markdown tinting theme; "Open Notes" from tray + optional second global shortcut.
*Acceptance: feels like a native notes app; Obsidian can edit the same file without conflicts.*

**M3 — Settings & robustness.** Leptos settings form (file location picker via dialog plugin, mic picker, PTT toggle, launch-at-login, model idle-unload timer, error surfacing, app icon. Additions decided since drafting: **⌘B / ⌘I from the keyboard** with no toolbar and no on-screen hint (the one task that touches the frozen CodeMirror bundle; no ⌘U, because CommonMark has no underline), **condensing captures made in the same minute under one heading** (a change to the document contract), **a keyboard route to the notepad** whose mechanism is deliberately undecided, and **stopping macOS from offering Mic Mode** for a capture that only holds the microphone for a few seconds. Details and the traps in each: `PROGRESS.md`.

**M4 — Shippable.** DMG build (ad-hoc signed for personal use; notarization when distributing), README documenting the file contract + Obsidian setup + Handy coexistence, MIT attribution for the ported Handy code.

**M5 — Later (per talkie_plan.md).** OpenClaw integration docs/hooks; Windows/Linux pass (Tauri + Trunk keep this open); archive rotation if the single file ever gets unwieldy; promote/publish.

## Risks & mitigations

- **Model hosting**: blob.handy.computer is cjpais's bucket — fine personally; mirror to HuggingFace/own bucket before distributing.
- **Two dictation apps**: distinct default shortcuts; macOS allows concurrent mic access, so no contention.
- **Iteration speed**: Trunk rebuild+reload instead of HMR — accepted above; revisit (Dioxus) only if it genuinely hurts.
- **wasm-bindgen edge**: the CM wrapper is the one untyped boundary — kept to ~8 functions, exercised constantly in normal use.
- **tauri-sys maturity**: we don't depend on it — the hand-rolled `ipc.rs` shim + `withGlobalTauri` is ~50 lines and fully under our control.
- **Scope creep** (the plan's own stated top risk): v1 cut is exactly M1+M2 — enough to evaluate the "is the dump useful?" hypothesis. Everything else waits.
- **ONNX runtime size**: adds tens of MB to the bundle; acceptable.

## Immediate next steps

1. M0 scaffold in `0 - Projects/talkie-app/` (workspace, Trunk, CM bundle, tray, window).
2. M1 pipeline (audio_toolkit port → model download → transcribe → append).
3. M2 editor.
