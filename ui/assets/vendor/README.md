# Vendored CodeMirror bundle

`codemirror.bundle.js` is a **frozen build artifact**, checked in the way a font
file is. It is the single exception to Talkie's no-JavaScript rule: every mature
text-editor component is JS, and writing one from scratch is exactly the
scope-creep the plan warns about.

Nothing in this repo builds it. Node never runs as part of `trunk build` or
`cargo tauri build`. Regenerating it is a deliberate, occasional, out-of-repo act
(see below).

## How Rust consumes it

`ui/src/cm.rs` declares the bundle's exports as `#[wasm_bindgen(module = "/assets/vendor/codemirror.bundle.js")]`
externs. wasm-bindgen copies the file into its `snippets/` output at build time
and the generated glue imports it as an ES module. The extern list and the
bundle's export list must stay in sync — that is the one untyped edge in the app,
kept deliberately small:

`init · getDoc · setDoc · insertAndReveal · openSearch · setTheme · focusEditor · destroy`

## Build inputs (checked in, never shipped)

`src/cm-entry.mjs` is Talkie's own facade — the only module rollup is pointed at.
`src/rollup.config.mjs` and `src/package.json` complete the recipe. These are
inputs to the one-time build; they are not served, imported, or executed at
runtime.

## Pinned versions

Rebuilt 2026-08-22 with node v24.18.0, npm 11.16.0 — same versions as the
original 2026-08-18 build, direct and transitive, so that rebuild changed
nothing but the facade: ⌘B/⌘I got an inline bold/italic toggle keymap (no new
exports — the keymap lives inside `init`). Holding the transitive set still
took one pin: `@marijn/find-cluster-break` 1.0.4 had shipped the day before
with new Unicode tables, so it is held at 1.0.3 by an npm override (see the
recipe below). The 2026-08-20 rebuild was the `insertAndReveal` facade change.

Direct dependencies:

| package | version |
| --- | --- |
| `@codemirror/commands` | 6.11.0 |
| `@codemirror/lang-markdown` | 6.5.2 |
| `@codemirror/language` | 6.12.4 |
| `@codemirror/search` | 6.7.1 |
| `@codemirror/state` | 6.7.1 |
| `@codemirror/view` | 6.43.9 |
| `@lezer/highlight` | 1.2.3 |
| `@rollup/plugin-node-resolve` | 16.0.3 |
| `rollup` | 4.62.4 |

Transitive CodeMirror/Lezer packages actually inside the bundle:

| package | version |
| --- | --- |
| `@codemirror/autocomplete` | 6.20.3 |
| `@codemirror/lang-css` | 6.3.1 |
| `@codemirror/lang-html` | 6.4.12 |
| `@codemirror/lang-javascript` | 6.2.5 |
| `@codemirror/lint` | 6.9.7 |
| `@lezer/common` | 1.5.2 |
| `@lezer/css` | 1.3.6 |
| `@lezer/html` | 1.3.13 |
| `@lezer/javascript` | 1.5.4 |
| `@lezer/lr` | 1.4.10 |
| `@lezer/markdown` | 1.7.2 |
| `@marijn/find-cluster-break` | 1.0.3 *(held back by an npm override)* |
| `crelt` | 1.0.7 |
| `style-mod` | 4.1.3 |
| `w3c-keyname` | 2.2.8 |

## Regenerating

Anywhere outside this repo (a scratch dir), with node available:

```sh
mkdir /tmp/cm && cd /tmp/cm
cp "<repo>/ui/assets/vendor/src/"* .
npm install --no-fund --no-audit \
  @codemirror/state @codemirror/view @codemirror/commands \
  @codemirror/language @codemirror/lang-markdown @codemirror/search \
  @lezer/highlight rollup @rollup/plugin-node-resolve
npx rollup -c
cp codemirror.bundle.js "<repo>/ui/assets/vendor/"
```

Install the direct dependencies at the exact pinned versions above, then check
the installed transitives against the second table before copying anything —
one drifting utility package is enough to bury the real diff (the checked-in
`src/package.json` carries an npm `overrides` pin for exactly that reason).

Then update the version tables above, and `ui/src/cm.rs` if the facade's exports
changed.

## Deliberate omissions

- **`@codemirror/language-data`** — would pull a parser for every language just to
  highlight the inside of fenced code blocks. Fences render as plain monospace.
- **No theme package.** Colors live in `ui/styles/app.css`; the bundle's highlight
  style only maps syntax tags to `cm-md-*` classes, so the macOS look stays in CSS.
- **No vim keymap, no minimap.** Decided against for v1; the minimap is listed as
  later polish in the implementation plan and would mean a regen.
- **Unminified on purpose.** A checked-in artifact should stay greppable when
  debugging the wasm-bindgen boundary. Size is irrelevant for a local app.
