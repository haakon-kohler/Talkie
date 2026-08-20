# Talkie

**Speak, and it lands in your notes.**

Press a shortcut, say the thing, press it again. A local speech model transcribes
what you said and appends it — timestamped — to one long markdown file. Nothing
steals focus, nothing gets pasted into whatever app you were using, nothing
leaves the machine.

Talkie lives in the menu bar. Its second surface is a deliberately buttonless
markdown editor over that same file.

> **Status: M0 (scaffold).** The app builds, runs, and shows its three windows.
> The capture pipeline lands in M1 and the real editor in M2. See `PROGRESS.md`.

## The document contract

Every capture inserts exactly this, **at the top of the file**:

```markdown
## 2026-08-18 09:14
Remember to email Sam about the demo Thursday.

```

- One `##` heading per capture, in local time.
- **Newest first.** Scrolling down walks backwards through time, so the thing
  you just said is the thing you are looking at.
- A blank line between entries; the file always ends with a newline.
- "The top" is below any YAML frontmatter and below a leading `#` title, both of
  which stay where they are — so the file can carry Obsidian properties without
  a capture landing above them and quietly stopping them being frontmatter.
- Default location `~/Documents/Talkie/talkie.md`, changeable in Settings.

That contract is the whole integration story. Point the file inside an Obsidian
vault and Obsidian indexes it. Point an agent at it and the agent watches it.
Talkie itself stays a capture tool and an editor — nothing more.

## Running it

Requires [Rust](https://rustup.rs/) (via rustup) and
[Trunk](https://trunkrs.dev/) (`cargo install trunk`). No node, no npm.

```sh
cargo tauri dev
```

`cargo tauri build` produces the `.app`/`.dmg`.

## Living alongside Handy

Talkie does not replace [Handy](https://github.com/cjpais/Handy) — it does a
different job. Handy pastes dictation into the app you are in; Talkie files it
into a document. Keep both: their default shortcuts are distinct (Talkie
defaults to ⌃⌥Space, clear of Handy's ⌥Space and ⌥⇧Space), and macOS lets both
hold the microphone.

Talkie needs two permissions: the microphone, and Accessibility. Accessibility
is what lets it hear its own shortcut while another app is in front, and it is
the only way a shortcut can be a *held modifier* — right ⌘ and nothing else —
rather than a Carbon-style combination. Talkie still never types into another
app; that stays Handy's job.

## Built with

Rust end to end: [Tauri 2](https://tauri.app) hosting a
[Leptos](https://leptos.dev) CSR frontend compiled to WASM by Trunk. The one
piece of JavaScript is a frozen [CodeMirror 6](https://codemirror.net) bundle
behind a typed wasm-bindgen wrapper — see `ui/assets/vendor/README.md`.

Speech recognition uses Parakeet V3 through
[transcribe-rs](https://crates.io/crates/transcribe-rs); the audio capture,
resampling, and VAD code is ported from Handy (MIT) with its notice retained.

## License

MIT — see `LICENSE`.
