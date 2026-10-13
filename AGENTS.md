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

Current state: `PROGRESS.md`.

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

# the same two as Talkie Nightly (see "Channels" below)
cargo tauri dev   --config src-tauri/tauri.nightly.conf.json
cargo tauri build --config src-tauri/tauri.nightly.conf.json

trunk build                # frontend only
trunk serve                # frontend only, on :1420

cargo check -p talkie      # host

# before committing — both clippy runs must be clean
cargo fmt
cargo clippy --workspace --all-targets
cargo clippy -p talkie-ui --all-targets --target wasm32-unknown-unknown
```

There is no HMR. Trunk rebuilds the WASM (a few seconds) and reloads the page.
This is a known, accepted cost of the zero-node stance.

## Layout

```
Cargo.toml        workspace: shared · ui · src-tauri   (release profile and clippy lints live here)
clippy.toml       what tests may do that the code may not; product names doc_markdown accepts
Trunk.toml        builds ui/index.html → dist/
.github/workflows/
  ci.yml            fmt, both clippy runs and the tests, on a Mac, for every PR and push
  release.yml       a push to nightly publishes Talkie Nightly; a vX.Y.Z tag publishes Talkie
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
  panel.rs          the native NSOpenPanel behind the notes-file field; a folder means talkie.md inside it
  hooks.rs          the plugin system: executables in app-data/hooks/, run at the edges of a capture
  journal.rs        the last ten warnings and panics, in app-data/debug.log; `Talkie --debug-log` prints it
  cli.rs            `Talkie transcribe` / `Talkie capture`: the pipeline from a terminal, no app
  login_item.rs     Start at Login via SMAppService; macOS, not the store, is the truth for that switch
  watcher.rs        watches talkie.md for changes Talkie's editor did not make
  settings.rs       tauri-plugin-store; the host owns settings, the UI never caches them
  windows.rs        show/hide + the macOS Accessory/Regular Dock dance
  tray.rs           menu-bar item and its menu
  test_support.rs   helpers shared by the host's tests
src-tauri/tauri.nightly.conf.json   merged over tauri.conf.json: Talkie Nightly's name, identifier, icons
src-tauri/icons/nightly/            the inverted app icon (dark tile, white mark), from icons/source/*-nightly.svg
```

## Channels: stable and nightly

Talkie ships the way T3 Code does: **Talkie** is the stable app, **Talkie
Nightly** carries what is new, and anyone can install either or both.

- **Two long-lived branches, never more.** `nightly` is where work lands;
  `main` is stable. A feature branches off `nightly`, its PR targets
  `nightly`, and the branch is deleted once merged. `main` only ever moves by
  fast-forwarding to a commit that is already on `nightly`
  (`git push origin <sha>:main`), so stable never runs code nightly users have
  not. A fix for stable goes the same way: into `nightly` first, then promoted.
- **Releasing.** Every push to `nightly` publishes a GitHub prerelease,
  `v<version>-nightly.<yyyymmdd>.<run>`. Stable is a tag: promote `main`, then
  push `vX.Y.Z` on it; `release.yml` refuses a tag that is not on both
  branches. Afterwards, bump `version` in `tauri.conf.json` on `nightly` so the
  nightlies name the release they lead up to.
- **The channel is chosen at build time, not in code.**
  `tauri.nightly.conf.json` changes the product name, the bundle identifier
  (`com.haakonkohler.talkie.nightly`) and the icons, and nothing else. The
  code reads its name from `package_info()`, so the tray says "Talkie Nightly"
  without an `if`. Do not branch on the channel in Rust; if nightly needs to
  behave differently, it is not ready for nightly.
- **Everything keyed on the identifier is separate.** Nightly has its own
  settings, its own copy of the model (one more download), its own
  `debug.log` and hooks folder under
  `~/Library/Application Support/com.haakonkohler.talkie.nightly/`, its own
  single-instance socket, and its own Accessibility and Microphone grants
  (`tccutil reset … com.haakonkohler.talkie.nightly`). The one thing they
  share by default is `~/Documents/Talkie/talkie.md`, which is safe because
  the editor is never the only writer anyway. The single-instance guard does
  not stop one of each from running: with the same shortcut both would
  capture every utterance, so give them different shortcuts or quit one.

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
- **`Talkie transcribe` and `Talkie capture` run the pipeline with no app.**
  `transcribe <file.wav>` prints the transcript; `capture --wav <file>` or
  `capture --text <text>` goes on through the hooks into the notes file, or
  into `--note <path>`. Use them to test a hook, an agent watching the file, or
  the capture path itself without speaking. Like `--debug-log` they are
  answered before the single-instance guard, so they run beside a live Talkie
  and never install the event tap — keep anything added there that way, and
  keep the microphone out of it: a terminal cannot hold the grant. The model
  has to be installed already. On Linux, `XDG_DATA_HOME=/tmp/x` gives them a
  scratch app-data directory instead of the real one.
- **`data-wasm-opt-params` in `ui/index.html` is load-bearing.** Release builds
  only: `wasm-opt` 123 validates its input against MVP unless told otherwise and
  rejects wasm-bindgen's `memory.copy` with `Fatal: error validating input`. The
  explicit `--enable-bulk-memory --enable-reference-types
  --enable-nontrapping-float-to-int` is what lets `cargo tauri build` finish.
- **The editor is never the only writer.** Captures go into `talkie.md` while
  the editor may be open with unsaved edits, and Obsidian may be in the file too.
  Saves go through `document::reconcile`, which carries an external capture over
  into the editor's text and refuses anything it cannot merge. Do not "simplify"
  that back into a plain write. The base it merges against is what the
  *editor* last read or saved, tagged with that file's path, and never what
  the watcher has seen since: a watcher-moved base turned a capture under
  unsaved edits, or a notes-path change, into a silent overwrite (#5). Every
  save names the file its text came from.
- **`PROGRESS.md` keeps no backlog.** Track the current milestone and the next
  one. Do not add a "later" section, and do not reinstate one you find deleted —
  deferred work either comes back on its own or was never worth listing.
- **Clippy is strict, and every lint is `deny`.** The list is
  `[workspace.lints.clippy]` in the root `Cargo.toml`: all of `pedantic`, plus
  lints picked one at a time against panics (`unwrap_used`, `indexing_slicing`,
  `string_slice`, `panic_in_result_fn`, …) and against silent failure
  (`let_underscore_must_use`, `map_err_ignore`). `clippy.toml` lets tests
  panic. `#[allow]` is itself denied. The only way past a lint is
  `#[expect(lint, reason = "…")]` on the narrowest item, and the reason names
  the invariant that makes the code correct ("the offset is a sum of whole
  lines"), not the wish to compile.
- **Never satisfy a lint by making up a value.** `.unwrap_or(0)`,
  `.unwrap_or_default()`, `saturating_sub` or `.get(i).unwrap_or("")` where
  the code used to panic turns a loud crash, which lands in `debug.log`, into
  text written to the wrong place in `talkie.md`. Propagate with `?` where the
  function can fail. Where it cannot, `.expect("why this holds")` is allowed
  (`expect_used` is deliberately off) and says the same thing as a panic, out
  loud. A `let _ =` that drops an error gets the same treatment: handle it, log
  it, or `#[expect(clippy::let_underscore_must_use, reason = "…")]` saying why
  nobody needs to know.
- **Clippy on Linux does not see the macOS code.** `login_item.rs`,
  `panel.rs`, the event-tap half of `shortcut.rs` and `windows.rs` are
  `cfg(target_os = "macos")` and need the macOS SDK to build, so lint on the
  Mac before merging. The Linux stand-ins draw lints the real code does not
  (`unused_async`, `unnecessary_wraps`, `dead_code`): silence those with
  `#[cfg_attr(not(target_os = "macos"), expect(…, reason = "…"))]`. A bare
  `#[expect]` goes unfulfilled, and warns, on the other platform.
- **Bumping the toolchain is a lint change.** `rust-toolchain.toml` pins an
  exact version because every release adds lints and every lint is deny. Bump
  it on purpose, and fix what the new clippy finds in the same commit.
- **The document contract is public API.** One H2 per minute of capture,
  local time, **newest first** — a capture in the minute the top heading
  already names goes directly under it as its own paragraph — blank line
  between entries and between paragraphs, file ends with a newline, and
  insertion happens below any YAML frontmatter and any leading `#` title.
  Changing it breaks Obsidian setups and any agent watching the file. The rules
  live in `shared/src/document.rs` — as code, tested, in one place, because the
  host and the UI both apply them.

## Code style

- Rust: `cargo fmt`, both clippy runs clean (see Commands), doc comments on
  public items.
- CSS: handwritten, tokens at the top of `app.css`, light/dark via
  `prefers-color-scheme`.
- Commits: conventional prefixes (`feat:`, `fix:`, `docs:`, `refactor:`,
  `chore:`), message says *why*.

## Attribution

`audio_toolkit/` is ported from
[Handy](https://github.com/cjpais/Handy) (MIT). Keep its copyright notice.
