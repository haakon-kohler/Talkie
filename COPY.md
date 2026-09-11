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

## Still to write

The slots below are the only ones still showing lorem ipsum or nothing at all.
Everything else in this file is in the app verbatim.

| ID | Where it shows | Note |
| --- | --- | --- |
| `onboarding.model.installed` | Under "Speech model", once the model is on disk | In **settings** this is the only thing that section ever says, so it carries more weight than its length suggests |
| `editor.trouble` | Bottom-right of the editor, when a save or a capture failed | Currently the host's raw error text |
| `settings.note_path.*` | Under the Save button, when the notes path is refused | Working text, see the Notes file section |
| `onboarding.note` | Footnote under the first-run body | Empty, and nothing renders it yet |
| `settings.note_path.hint` | Under the notes-file field | Empty |
| `settings.shortcut.hint` | Under the shortcut field | Empty |

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

First run now runs in four steps, and the page shows one at a time: grant
Accessibility, grant the microphone, download the model, then finish. The lede
and body above sit at the top of all four. The Accessibility step skips itself
when the permission is already granted.

### Step 1 — accessibility

The settings window mounts this same block, and both hide it when the
permission is already in place. Onboarding runs once, but macOS drops the grant
whenever the binary changes, so settings has to be able to ask for it too.

Body (`onboarding.accessibility.body`) — *live*

> This setting allows Talkie to use specific modifier keys as your shortcut button (like the right Option key). We don't look at any information in other apps. 

Button (`onboarding.accessibility.cta`) — *live*

> Allow Accessibility

### Step 2 — microphone

Body (`onboarding.microphone.body`) — *live*

> Talkie needs the microphone to record voice notes.

Button (`onboarding.microphone.cta`) — *live*

> Allow Microphone

Denied-permission error (`onboarding.microphone.denied`) — *live*

> Microphone permission denied. Open System Settings › Privacy & Security ›
> Microphone and enable microphone access for Talkie.

### Step 3 — the speech model

Body (`onboarding.model.body`) — *live*

> The voice transcription model, Parakeet V3, runs locally, so your data stays on your device.

Button (`onboarding.model.cta`) — *live*. Reads "Downloading…" while busy.

> Download Model

Progress labels — *live*

| ID | Text |
| --- | --- |
| `onboarding.model.progress` | 123 of 456 MB |
| `onboarding.model.unpacking` | Unpacking… |
| `onboarding.model.ready` | Finished. |

Installed state (`onboarding.model.installed`) — *placeholder*. Shown in place of
the download button once the model is on disk — in settings too, where it is the
only thing that section says.

> 

### Step 4 — finish

Body (`onboarding.done.body`) — *live*

> All set! Use the default shortcut ⌃⌥Space and record your first note.

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

Refusals — *placeholder*. Save checks the path the way a capture would use it:
it has to be a full path, not a folder, and its folder has to be creatable and
writable. The folder is created on Save so the failure shows up now, not on
the first capture. `{path}` is what was typed, `{folder}` its parent, `{e}` the
underlying error.

| ID | Text |
| --- | --- |
| `settings.note_path.empty` | The notes file needs a path. |
| `settings.note_path.relative` | `{path}` is not a full path — it has to start with / or ~/. |
| `settings.note_path.folder` | `{path}` is a folder; the notes file has to be a file inside one. |
| `settings.note_path.unwritable` | Could not create the folder {folder}: {e} · Could not write in the folder {folder}: {e} |

### Shortcut

Label (`settings.shortcut.label`) — *live*

> Shortcut

Hint (`settings.shortcut.hint`) — *placeholder*

> 

Empty state, when nothing is bound (`settings.shortcut.empty`) — *live*

> None

While recording, before any key is down (`settings.shortcut.recording`) — *live*

> Press keys…

Refused because it has no modifier (`settings.shortcut.invalid`) — *live*

> A shortcut needs at least one modifier — ⌘, ⌥, ⌃ or ⇧.

The field records rather than reads: click it, press the combination, and it
saves itself when every key comes back up. Escape cancels. A held modifier on
its own (right ⌘) is a legal shortcut and keeps its side.

### Toggles

`settings.push_to_talk.label` — *live*. The control is **inverted**: push-to-talk
is the default, so this box is the way out of it and ships unchecked. The stored
setting is still `push_to_talk`; only the checkbox reads backwards, which is why
the label says "Turn Off".

> Turn Off Push-to-Talk (Toggle Record)

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

Sample document (`editor.sample`) — *retired*

M2 wired the editor to the real `talkie.md`, so there is no sample document any
more. An empty file opens as an empty editor.

Save failure (`editor.trouble`) — *placeholder*

The editor has no chrome by design, with one exception: a save that failed has
to say so, or the window quietly becomes a text box that eats your writing. The
string shown is currently the host's raw error.

The same strip shows a failed capture (the `capture.*` sentences below) until
the next capture starts. A save failure takes precedence if both are pending.

---

## Capture errors

The pipeline is silent by design, so these are the only sentences it can say.
All *live*, emitted on `talkie://capture-failed` and shown in the editor's
trouble strip.

| ID | Text |
| --- | --- |
| `capture.no_model` | speech model not yet installed — finish first run to download it |
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

> Talkie records your voice so it can locally transcribe it into your notes file.

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

> Error: unknown window.
