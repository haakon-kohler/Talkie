# AGENTS.md

Guidance for AI coding assistants working in the Talkie repo.

## What Talkie is

A macOS menu-bar companion app. A global shortcut records your voice, a local
Parakeet V3 model transcribes it, and the text is **silently appended** as a
timestamped entry to one long `talkie.md`. A second surface — a deliberately
buttonless, macOS-native markdown editor — reads and edits that same file.

The markdown file is the integration surface for everything else. Obsidian just
indexes it; an agent just watches it. There are no in-app agent features and no
paste-into-the-active-app dictation (Handy, installed alongside, still owns that).

Full plan and milestones: `talkie_implementation_plan.md`. Current state:
`PROGRESS.md`.

## Toolchain — read this first

One language, one toolchain: **Rust end to end**. The UI is Leptos (CSR)
compiled to WASM by Trunk; the host is Tauri 2. There is no node, no npm, and no
`package.json` in this repo, and none should ever be added.

Rust must come from **rustup**, not Homebrew: the Homebrew toolchain ships no
`wasm32-unknown-unknown` std, so the UI fails to build with ``can't find crate
for `core` ``. `rust-toolchain.toml` pins the channel and the wasm target, which
rustup installs on demand. (The Homebrew rust that used to shadow rustup on this
machine has been uninstalled.)

## Commands

```sh
cargo tauri dev            # run the app (spawns `trunk serve` itself)
cargo tauri build          # bundle a .app/.dmg

trunk build                # frontend only
trunk serve                # frontend only, on :1420

cargo check -p talkie      # host
cargo fmt && cargo clippy  # before committing
```

There is no HMR. Trunk rebuilds the WASM (a few seconds) and reloads the page.
This is a known, accepted cost of the zero-node stance.

## Layout

```
Cargo.toml        workspace: shared · ui · src-tauri   (release profile lives here)
Trunk.toml        builds ui/index.html → dist/
shared/           serde types + command/event names used by BOTH sides
ui/               Leptos CSR crate (wasm32)
  src/main.rs       mount; routes on the Tauri window label
  src/ipc.rs        ~90-line typed invoke/listen shim over window.__TAURI__
  src/cm.rs         typed wasm-bindgen wrapper over the vendored CodeMirror bundle
  src/editor.rs     the zero-chrome editor
  src/settings.rs   settings form
  src/onboarding.rs first run
  assets/vendor/    frozen codemirror.bundle.js + its build inputs and README
  styles/app.css    the entire stylesheet, handwritten
src-tauri/src/
  lib.rs            setup: plugins, state, tray, windows, close-to-hide
  commands.rs       every command the webview can call
  settings.rs       tauri-plugin-store; the host owns settings, the UI never caches them
  windows.rs        show/hide + the macOS Accessory/Regular Dock dance
  tray.rs           menu-bar item and its menu
```

## Rules that are easy to get wrong

- **No JavaScript** beyond `ui/assets/vendor/codemirror.bundle.js`, which is a
  frozen build artifact (like a font file). Its regeneration recipe and pinned
  versions are in `ui/assets/vendor/README.md`. Node runs only there, only
  out-of-repo, and never as part of a Talkie build.
- **`cm.rs` and the bundle's exports must match.** That is the one untyped edge
  in the app; keep it at nine functions unless there is a real reason.
- **Command and event names live in `shared`**, never as string literals on one
  side only.
- **Settings state is host-side only.** The UI reads via `get_settings` and
  writes via `set_settings`; it keeps no store of its own.
- **Three windows, one WASM bundle.** Each window routes on its own label
  (`talkie_shared::WindowLabel`).
- **The editor has no chrome.** No toolbar, no buttons, no status bar. If a
  feature needs a button, it probably does not belong in v1.
- **The document contract is public API.** One H2 per capture, local time, blank
  line before each entry, file ends with a newline. Changing it breaks Obsidian
  setups and any agent watching the file.

## Code style

- Rust: `cargo fmt`, `cargo clippy`, no `unwrap` in paths that can fail at
  runtime, doc comments on public items.
- CSS: handwritten, tokens at the top of `app.css`, light/dark via
  `prefers-color-scheme`.
- Commits: conventional prefixes (`feat:`, `fix:`, `docs:`, `refactor:`,
  `chore:`), message says *why*.

## Attribution

`audio_toolkit/` (arriving in M1) is ported from
[Handy](https://github.com/cjpais/Handy) (MIT). Keep its copyright notice.
