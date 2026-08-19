# Talkie — Copy

Every string a user can see, in one place. **This file is the source of truth for
prose.** The app currently ships lorem ipsum in the slots marked *(placeholder)*;
edit the text here, tell me to resync, and I'll copy it into the code.

Conventions:

- Each string has a stable ID (`onboarding.lede`). The code carries that ID in a
  `// COPY:` comment next to the placeholder, so resyncing is mechanical.
- **Placeholder** = the app shows lorem ipsum today, waiting on your text.
  **Live** = short functional label, already in the app verbatim; edit here and
  it gets resynced the same way.
- Don't renumber or rename IDs — that's what makes the resync safe.
- Same routine for every later revision: edit here, say "resync copy".

---

## Onboarding window

Window title (`onboarding.window_title`) — *live*

> Welcome to Talkie

Heading (`onboarding.title`) — *live*

> Talkie

Lede (`onboarding.lede`) — *placeholder*

> The modern notepad. 

Body (`onboarding.body`) — *placeholder*

> Press the shortcut and you can record directly to a markdown notepad. Want to enable obsidian or openclaw integration? Point the document at the home folder!

Footnote (`onboarding.note`) — *placeholder*

> 

First run now runs in three steps, and the page shows one at a time: grant the
microphone, download the model, then finish. The lede and body above sit at the
top of all three.

### Step 1 — microphone

Body (`onboarding.microphone.body`) — *placeholder*

> Talkie needs the microphone, and nothing else. It never types into other apps,
> so macOS never asks for accessibility.

Button (`onboarding.microphone.cta`) — *placeholder*

> Allow Microphone

Denied-permission error (`onboarding.microphone.denied`) — *live*

> macOS denied the microphone. Open System Settings › Privacy & Security ›
> Microphone and switch Talkie on.

### Step 2 — the speech model

Body (`onboarding.model.body`) — *placeholder*

> Parakeet V3 runs entirely on this Mac. It is a 456 MB download, once.

Button (`onboarding.model.cta`) — *placeholder*. Reads "Downloading…" while busy.

> Download Model

Progress labels — *live*

| ID | Text |
| --- | --- |
| `onboarding.model.progress` | 123 of 456 MB |
| `onboarding.model.unpacking` | Unpacking… |
| `onboarding.model.ready` | Ready. |

### Step 3 — finish

Body (`onboarding.done.body`) — *placeholder*

> That's everything. Press ⌃⌥Space anywhere and start talking.

Primary button (`onboarding.cta`) — *live*

> Start Writing

---

## Settings window

Window title (`settings.window_title`) — *live*

> Talkie Settings

Heading (`settings.title`) — *live*

> Settings

Loading state (`settings.loading`) — *live*

> Loading…

### Notes file

Label (`settings.note_path.label`) — *live*

> Notes file

Hint (`settings.note_path.hint`) — *placeholder*

> 

### Shortcut

Label (`settings.shortcut.label`) — *live*

> Shortcut

Hint (`settings.shortcut.hint`) — *placeholder*

> 

### Toggles

`settings.push_to_talk.label` — *live*

> Toggle Push-to-Talk (Push Twice Instead Of Tap-and-Hold )

`settings.play_sounds.label` — *live*

> Play Sound When Recording Starts/Stops

`settings.launch_at_login.label` — *live*

> Start at Login

### Buttons and status

`settings.save` — *live*

> Save

`settings.saved` — *live*

> Saved.

`settings.error.read` — *live* (`{e}` is the underlying error)

> Could not read settings: {e}

`settings.error.write` — *live* (`{e}` is the underlying error)

> Could not save: {e}

---

## Editor window

Window title (`editor.window_title`) — *live*

> Talkie

Sample document (`editor.sample`) — *placeholder*

The editor mounts on this text until M2 wires it to the real `talkie.md`. It has
to obey the document contract (one H2 per capture, blank line before each entry,
trailing newline) so the markdown tinting gets exercised.

```markdown
# talkie.md

## 2026-08-18 09:14
Remember to email Sam about the demo Thursday.

## 2026-08-18 09:31
The **document contract** is the whole integration surface: one H2 per capture,
local time, a blank line before each entry, file ends with a newline. Obsidian
just indexes this file; an agent just watches it.

## 2026-08-18 09:40
Nothing here is saved yet — M0 only proves the editor mounts. Type into it and
watch the console for the change callback.
```

---

## Capture errors

The pipeline is silent by design, so these are the only sentences it can say.
All *live*, emitted on `talkie://capture-failed`.

| ID | Text |
| --- | --- |
| `capture.no_model` | the speech model is not installed yet — finish first run to download it |
| `capture.no_microphone_permission` | Talkie needs microphone access — grant it in System Settings › Privacy & Security › Microphone |
| `capture.no_microphone` | no microphone is available |

---

## Menu bar

Tooltip (`tray.tooltip`) — *live*. Reflects the capture state.

| ID | Text |
| --- | --- |
| `tray.tooltip.idle` | Talkie |
| `tray.tooltip.recording` | Talkie — recording |
| `tray.tooltip.transcribing` | Talkie — transcribing |

Menu items — *live*

| ID | Text |
| --- | --- |
| `tray.open_notes` | Notepad |
| `tray.record` | Record |
| `tray.settings` | Settings… |
| `tray.quit` | Quit Talkie |

---

## System prompts

macOS microphone prompt (`system.microphone_usage`) — *live*. Lives in
`src-tauri/Info.plist`, not in the Rust or the UI.

> Talkie records your voice so it can transcribe it into your notes file,
> entirely on this Mac.

---

## Bundle metadata

App name (`bundle.product_name`) — *live*

> Talkie

Short description (`bundle.short_description`) — *live* (shows in Finder, so it
stays real rather than lorem)

> The modern notepad.

---

## Fallbacks

`app.unknown_window` — *live*. Only reachable if a window is created without a
matching label.

> Unknown window.
