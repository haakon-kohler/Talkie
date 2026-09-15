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
  hooks.rs          the plugin system: executables in app-data/hooks/, run at the edges of a capture
  journal.rs        the last ten warnings and panics, in app-data/debug.log; `Talkie --debug-log` prints it
  login_item.rs     Start at Login via SMAppService; macOS, not the store, is the truth for that switch
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
  in the app; keep it at seven functions unless there is a real reason. (It was
  nine until newest-first ordering retired `scrollToEnd` and turned
  `appendAndReveal` into `insertAndReveal`, and eight until an `openSearch`
  nobody called went too — ⌘F lives inside the bundle's own keymap.)
- **Command and event names live in `shared`**, never as string literals on one
  side only.
- **Settings state is host-side only.** The UI reads via `get_settings` and
  writes via `set_settings`; it keeps no store of its own.
- **Three windows, one WASM bundle.** Each window routes on its own label
  (`talkie_shared::WindowLabel`).
- **The editor has no chrome.** No toolbar, no buttons, no status bar. If a
  feature needs a button, it probably does not belong in v1.
- **The hotkey engine is `handy-keys`, not a Tauri plugin.** It owns a
  `Receiver`, so it is not `Sync` and lives on its own thread behind a channel —
  never in managed state. The reason for the swap is that Carbon hotkeys cannot
  express a side-specific modifier (right ⌘) or a modifier-only shortcut, and
  Talkie's shortcut is meant to be recorded by pressing it. The price is macOS
  **Accessibility permission**, which the app now asks for in onboarding; the
  earlier "no accessibility permission" stance is gone. Talkie still never types
  into another app.
- **`cargo tauri dev` cannot hold the Accessibility or Microphone grant.** The
  dev binary is a bare executable with no `Info.plist` and no bundle id,
  launched as a child of the terminal, so macOS attributes the request to the
  terminal and adding `target/debug/talkie` to the pane does nothing. Test
  either permission against a bundle instead:

  ```sh
  cargo tauri build --debug --bundles app
  open target/debug/bundle/macos/Talkie.app
  ```

  `tauri.conf.json` sets `signingIdentity: "-"` so the bundler ad-hoc signs
  the app itself, binding the `Info.plist` and giving it the identity
  `com.haakonkohler.talkie`; without that the bundle is only linker-signed
  under a random identifier and TCC has nothing stable to key on. The
  hardened runtime is on, so the microphone also needs
  `src-tauri/entitlements.plist`: a hardened-runtime app without the
  `audio-input` entitlement is refused the microphone, prompt or not. Both
  grants still die on each rebuild:
  an ad-hoc signature's designated requirement is the code hash, and TCC keeps
  the stale row with its switch shown *on* while refusing the new build.
  `tccutil reset Microphone com.haakonkohler.talkie` (and `Accessibility`)
  clears it. Only a certificate — Developer ID, or at least an Apple
  Development one — makes a grant survive rebuilds, and there is none yet.
  Set `APPLE_SIGNING_IDENTITY` to use one; it overrides the config.
- **Start at Login only works from a bundle.** `login_item.rs` registers the
  app with `SMAppService` (macOS 13+, hence `minimumSystemVersion`), which
  needs a bundle identity; the `cargo tauri dev` binary reports `NotFound`
  and switching it on fails with the system's own message. Switching it off
  in that state is a no-op, so every other setting still saves in dev.
- **Quit Talkie before rebuilding the bundle or running `tccutil reset`.**
  Revoking Accessibility under a running instance does not silence its event
  tap: WindowServer refuses every event the tap returns ("Sender is
  prohibited from synthesizing events"), disables the tap, `handy-keys`
  re-enables it from the callback, and the two ping-pong at over 100 Hz until
  the machine hangs. That is what froze this Mac on 2026-09-12 — a rebuild
  and a `tccutil reset` while the 15:31 build was still running. The engine
  thread now drops its tap within five seconds of losing the grant
  (`shortcut::TRUST_CHECK`), which bounds the damage but is not a reason to
  rely on it.
- **When trying to build a new version make sure to quit the existing
  instance because duplicate instances are automatically closed.** The
  single-instance guard (`tauri-plugin-single-instance`, registered first in
  `lib.rs`) keys on the bundle identifier via
  `/tmp/com_haakonkohler_talkie_si.sock`, so a fresh debug bundle or
  `cargo tauri dev` launched beside a running Talkie hands off to the running
  one — it pops the notepad — and exits before it builds anything. Your new
  build never ran; the old one is still what is answering the shortcut.
- **Bundled Talkie has no stderr; read `debug.log` instead.** Every
  `log::warn!`/`error!` and every panic lands in a ten-line
  `~/Library/Application Support/com.haakonkohler.talkie/debug.log`, the
  file rewritten on each entry so a crash keeps its last line. Two severities
  only: `warn` (handled, carried on) and `fatal` (panic). Print it with
  `/Applications/Talkie.app/Contents/MacOS/Talkie --debug-log` — the flag
  is answered before the single-instance guard, so it works while Talkie is
  running. `open --args` will not show it; it needs a terminal. There is no
  UI for it and there should not be one.
- **`data-wasm-opt-params` in `ui/index.html` is load-bearing.** Release builds
  only: `wasm-opt` 123 validates its input against MVP unless told otherwise and
  rejects wasm-bindgen's `memory.copy` with `Fatal: error validating input`. The
  explicit `--enable-bulk-memory --enable-reference-types
  --enable-nontrapping-float-to-int` is what lets `cargo tauri build` finish.
- **The editor is never the only writer.** Captures go into `talkie.md` while
  the editor may be open with unsaved edits, and Obsidian may be in the file too.
  Saves go through `document::reconcile`, which carries an external capture over
  into the editor's text and refuses anything it cannot merge. Do not "simplify"
  that back into a plain write.
- **`PROGRESS.md` keeps no backlog.** Track the current milestone and the next
  one. Do not add a "later" section, and do not reinstate one you find deleted —
  deferred work either comes back on its own or was never worth listing.
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
