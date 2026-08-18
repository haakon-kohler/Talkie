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

Not started.

## M2 — The editor

Not started.

## M3 — Settings & robustness

Not started.

## M4 — Shippable

Not started.
