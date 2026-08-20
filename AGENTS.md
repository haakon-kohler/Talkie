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
  src/document.rs   the document contract as code: where a capture goes, how a save merges
ui/               Leptos CSR crate (wasm32)
  src/main.rs          mount; routes on the Tauri window label
  src/ipc.rs           ~90-line typed invoke/listen shim over window.__TAURI__
  src/cm.rs            typed wasm-bindgen wrapper over the vendored CodeMirror bundle
  src/editor.rs        the zero-chrome editor
  src/settings.rs      settings form
  src/shortcut.rs      the click-and-press shortcut recorder
  src/accessibility.rs the Accessibility grant — settings AND onboarding mount it
  src/model.rs         the model download — settings AND onboarding mount it
  src/onboarding.rs    first run
  assets/vendor/       frozen codemirror.bundle.js + its build inputs and README
  styles/app.css       the entire stylesheet, handwritten
src-tauri/src/
  lib.rs            setup: plugins, state, tray, windows, close-to-hide
  commands.rs       every command the webview can call
  shortcut.rs       handy-keys engine thread: the global hotkey + the recorder
  note.rs           disk half of talkie.md: prepend a capture, read, atomic write
  watcher.rs        watches talkie.md for changes Talkie's editor did not make
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
  in the app; keep it at eight functions unless there is a real reason. (It was
  nine until newest-first ordering retired `scrollToEnd` and turned
  `appendAndReveal` into `insertAndReveal`.)
- **Command and event names live in `shared`**, never as string literals on one
  side only.
- **Settings state is host-side only.** The UI reads via `get_settings` and
  writes via `set_settings`; it keeps no store of its own.
- **Three windows, one WASM bundle.** Each window routes on its own label
  (`talkie_shared::WindowLabel`).
- **The editor has no chrome.** No toolbar, no buttons, no status bar. If a
  feature needs a button, it probably does not belong in v1.
- **`data-wasm-opt-params` in `ui/index.html` is load-bearing.** Release builds
  only: `wasm-opt` 123 validates its input against MVP unless told otherwise and
  rejects wasm-bindgen's `memory.copy` with `Fatal: error validating input`. The
  explicit `--enable-bulk-memory --enable-reference-types
  --enable-nontrapping-float-to-int` is what lets `cargo tauri build` finish.
- **The hotkey engine is `handy-keys`, not a Tauri plugin.** It owns a
  `Receiver`, so it is not `Sync` and lives on its own thread behind a channel —
  never in managed state. The reason for the swap is that Carbon hotkeys cannot
  express a side-specific modifier (right ⌘) or a modifier-only shortcut, and
  Talkie's shortcut is meant to be recorded by pressing it. The price is macOS
  **Accessibility permission**, which the app now asks for in onboarding; the
  earlier "no accessibility permission" stance is gone. Talkie still never types
  into another app.
- **`cargo tauri dev` cannot hold the Accessibility grant.** The dev binary is a
  bare executable with no `Info.plist` and no bundle id, launched as a child of
  the terminal, so macOS attributes the request to the terminal and adding
  `target/debug/talkie` to the pane does nothing. Test the hotkey against a
  bundle instead:

  ```sh
  cargo tauri build --debug --bundles app
  codesign --force --deep --sign - target/debug/bundle/macos/Talkie.app
  open target/debug/bundle/macos/Talkie.app
  ```

  The `codesign` step matters: Tauri leaves the bundle linker-signed with its
  `Info.plist` unbound, which gives TCC nothing stable to key on. After it, the
  identity is `com.haakonkohler.talkie`. The grant still dies on each rebuild
  (the code hash changes) — remove the row and re-add it.
- **The editor is never the only writer.** Captures go into `talkie.md` while
  the editor may be open with unsaved edits, and Obsidian may be in the file too.
  Saves go through `document::reconcile`, which carries an external capture over
  into the editor's text and refuses anything it cannot merge. Do not "simplify"
  that back into a plain write.
- **The document contract is public API.** One H2 per capture, local time,
  **newest first**, blank line between entries, file ends with a newline, and
  insertion happens below any YAML frontmatter and any leading `#` title.
  Changing it breaks Obsidian setups and any agent watching the file. The rules
  live in `shared/src/document.rs` — as code, tested, in one place, because the
  host and the UI both apply them.

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
