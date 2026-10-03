//! The editor window: no toolbar, no buttons, no status bar. Just the text.
//!
//! It is a view onto `talkie.md` — the same file the capture pipeline appends
//! to, Obsidian indexes, and agents watch. Three rules keep those from fighting:
//!
//! - **Autosave, debounced.** Typing writes the whole file 600 ms after the last
//!   keystroke, and immediately when the window loses focus.
//! - **Reload only when clean.** An external change is applied only if there is
//!   nothing unsaved locally. Unsaved edits win; the pending save lands and the
//!   file converges.
//! - **Insert rather than replace, when it is an insert.** A silent capture
//!   arrives as text added at the top of the file, so it is applied as an
//!   insert — which keeps the cursor where it was, keeps undo history, and
//!   scrolls the new entry into view. Anything else replaces the document.
//! - **Every save names its file.** The notes path can move in Settings while
//!   edits are waiting; they go to the file they were typed into, and only
//!   then does the editor open the new one — as a fresh editor, so undo cannot
//!   carry one file's text into the other.
//!
//! Where "the top" is, and what counts as a capture rather than an edit, come
//! from `talkie_shared::document` — the same code the host saves through, so the
//! two halves cannot disagree about the shape of the file.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use leptos::html::Div;
use leptos::leptos_dom::helpers::{set_timeout_with_handle, TimeoutHandle};
use leptos::prelude::*;
use leptos::task::spawn_local;
use talkie_shared::{commands, document, events, Note, RecorderState, Settings, WriteNoteArgs};
use wasm_bindgen::prelude::*;

use crate::cm::Editor;
use crate::ipc;

/// How long after the last keystroke to write the file. Long enough that a burst
/// of typing is one write, short enough that closing the lid is not a gamble.
const DEBOUNCE: Duration = Duration::from_millis(600);

#[component]
pub fn EditorPage() -> impl IntoView {
    let host: NodeRef<Div> = NodeRef::new();
    let autosave = Autosave::new();
    // The last capture that failed, until the next one starts. Not part of
    // `Autosave`: it is the pipeline's failure, not the editor's, and it clears
    // on a different signal.
    let capture_failure = RwSignal::new(String::new());
    follow_capture_failures(capture_failure);

    Effect::new({
        let autosave = autosave.clone();
        move |_| {
            let Some(element) = host.get() else { return };
            if autosave.editor.borrow().is_some() {
                return;
            }

            let autosave = autosave.clone();
            spawn_local(async move {
                let note = match ipc::fetch::<Note>(commands::READ_NOTE).await {
                    Ok(note) => note,
                    Err(e) => {
                        autosave.trouble.set(e);
                        return;
                    }
                };

                *autosave.host.borrow_mut() = Some(element.into());
                autosave.ledger.borrow_mut().path = note.path;
                // No scrolling to do: CodeMirror opens at the top, and the top
                // is where the newest capture is.
                autosave.open(&note.text);
                if let Some(editor) = autosave.editor.borrow().as_ref() {
                    editor.focus();
                }

                follow_system_theme(autosave.editor.clone());
                flush_on_blur(autosave.clone());
                follow_external_changes(autosave.clone());
                follow_note_path(autosave);
            });
        }
    });

    view! {
        <div class="editor-window">
            // Hidden titlebar: this strip is the drag region under the traffic lights.
            <div class="titlebar" data-tauri-drag-region></div>
            <div class="editor-host" node_ref=host></div>
            // The one exception to "no chrome": a save that failed has to say so,
            // or the window quietly becomes a text box that eats your writing.
            // A capture that failed shares the strip — the pipeline is silent by
            // design, and this is the only place it can say something went
            // wrong. A save failure wins when both are pending: it is the one
            // that concerns the text on screen.
            <Show when={
                let trouble = autosave.trouble;
                move || !trouble.get().is_empty() || !capture_failure.get().is_empty()
            }>
                <div class="editor-trouble">{move || {
                    let trouble = autosave.trouble.get();
                    if trouble.is_empty() { capture_failure.get() } else { trouble }
                }}</div>
            </Show>
        </div>
    }
}

/// The save side of the editor: what is mounted, the timer that turns typing
/// into one write, and the IO behind whatever the [`Ledger`] decides.
#[derive(Clone)]
struct Autosave {
    /// Where the editor is mounted, kept for opening a fresh one in its place.
    host: Rc<RefCell<Option<web_sys::Element>>>,
    editor: Rc<RefCell<Option<Editor>>>,
    ledger: Rc<RefCell<Ledger>>,
    /// Set while Talkie itself is changing the document. CodeMirror reports a
    /// `setDoc` or an insert through the same listener as a keystroke; without
    /// this every reload would count as an edit, mark the document dirty, and
    /// write the file straight back 600 ms later.
    applying: Rc<Cell<bool>>,
    timer: Rc<Cell<Option<TimeoutHandle>>>,
    /// The ledger's trouble, where the view can see it. Also where a first
    /// read that failed says so, before there is anything to save.
    trouble: RwSignal<String>,
}

impl Autosave {
    fn new() -> Self {
        Self {
            host: Rc::new(RefCell::new(None)),
            editor: Rc::new(RefCell::new(None)),
            ledger: Rc::new(RefCell::new(Ledger::default())),
            applying: Rc::new(Cell::new(false)),
            timer: Rc::new(Cell::new(None)),
            trouble: RwSignal::new(String::new()),
        }
    }

    /// The document changed. Restart the clock.
    fn touch(&self) {
        debug_assert!(
            self.editor.borrow().is_some(),
            "a change before the editor mounted"
        );
        if self.applying.get() {
            return;
        }
        self.ledger.borrow_mut().edited();
        if let Some(pending) = self.timer.take() {
            pending.clear();
        }

        let me = self.clone();
        match set_timeout_with_handle(move || me.flush(), DEBOUNCE) {
            Ok(handle) => self.timer.set(Some(handle)),
            // No timer means no autosave, which is worse than saving eagerly.
            Err(_) => self.flush(),
        }
    }

    /// The editor's text, if it is mounted.
    fn doc(&self) -> Option<String> {
        self.editor.borrow().as_ref().map(Editor::doc)
    }

    /// Mount a fresh editor on `text`, in place of any before it. A replace
    /// would keep the undo history, and one ⌘Z after a switch would put the
    /// old file's text into the new one.
    fn open(&self, text: &str) {
        let Some(host) = self.host.borrow().clone() else {
            return;
        };
        // Torn down first, so the old view has left the page before the new
        // one goes in.
        let old = self.editor.borrow_mut().take();
        drop(old);
        let on_change = {
            let me = self.clone();
            move || me.touch()
        };
        let mounted = Editor::mount(&host, text, prefers_dark(), on_change);
        *self.editor.borrow_mut() = Some(mounted);
    }

    /// Make `change` to the mounted editor as Talkie's own, not the user's.
    /// CodeMirror calls the change listener synchronously inside the dispatch,
    /// so the flag is back off before this returns.
    fn apply(&self, change: Change) {
        let (insert_at, text) = match change {
            Change::Open(text) => return self.open(&text),
            Change::Insert { at, text } => (Some(at), text),
            Change::Replace(text) => (None, text),
        };
        let editor = self.editor.borrow();
        let Some(editor) = editor.as_ref() else {
            return;
        };
        debug_assert!(!self.applying.get(), "nested programmatic change");
        self.applying.set(true);
        match insert_at {
            Some(at) => editor.insert_and_reveal(at, &text),
            None => editor.set_doc(&text),
        }
        self.applying.set(false);
    }

    /// Write now, if there is anything to write.
    fn flush(&self) {
        if let Some(pending) = self.timer.take() {
            pending.clear();
        }
        let Some(args) = self.ledger.borrow_mut().begin_save(|| self.doc()) else {
            return;
        };

        let me = self.clone();
        spawn_local(async move {
            let saved = ipc::call::<WriteNoteArgs, String>(commands::WRITE_NOTE, &args).await;
            let change = me
                .ledger
                .borrow_mut()
                .finish_save(&args.text, saved, || me.doc());
            me.trouble.set(me.ledger.borrow().trouble.clone());
            if let Some(change) = change {
                me.apply(change);
            }
            if me.ledger.borrow_mut().catch_up() {
                me.follow();
            }
        });
    }

    /// Bring the document in line with the notes file — whichever file
    /// Settings names now. Put off while anything is unsaved or still being
    /// saved: the read makes what it returns the base of the next save, so it
    /// must not happen under text that has not reached its own file yet.
    fn follow(&self) {
        if !self.ledger.borrow_mut().may_read() {
            return;
        }
        let me = self.clone();
        spawn_local(async move {
            let Ok(note) = ipc::fetch::<Note>(commands::READ_NOTE).await else {
                return;
            };
            let Some(doc) = me.doc() else {
                return;
            };
            // Asked after the read, not before: the read was a round trip, and
            // a keystroke during it would make this reload a clobber.
            let change = me.ledger.borrow_mut().loaded(&doc, note);
            if let Some(change) = change {
                me.apply(change);
            }
        });
    }
}

/// The editor's decisions, without the editor: whether anything is unsaved,
/// whether the last save failed, and what a text from the host does to the
/// document. Strings in, answers out — no CodeMirror, no timer, no IPC — so
/// the rules at the top of this file can be tested off the webview.
/// [`Autosave`] carries out whatever it answers.
#[derive(Default)]
struct Ledger {
    /// Unsaved local edits. Also the flag that makes an external change wait.
    dirty: bool,
    /// Empty unless a save failed.
    trouble: String,
    /// The file the document is the text of, as `read_note` named it. Every
    /// save names it, so edits land in the file they were typed into even
    /// after Settings has moved on to another.
    path: String,
    /// Saves sent and not answered yet.
    in_flight: usize,
    /// A read was put off because something was unsaved or in flight. It
    /// happens once the last save lands with nothing left unsaved.
    behind: bool,
}

/// What to do to the document to bring it in line with the file.
#[derive(Debug, PartialEq, Eq)]
enum Change {
    /// Text added at the top — the shape of a silent capture — inserted at this
    /// byte offset, so the cursor and the undo history survive it.
    Insert { at: usize, text: String },
    /// Anything else replaces the document.
    Replace(String),
    /// Another file altogether: a fresh editor on its text.
    Open(String),
}

impl Ledger {
    /// The user edited the document.
    fn edited(&mut self) {
        self.dirty = true;
    }

    /// The text to write, if there is anything to write. `doc` is asked only
    /// when there is, so a blur with nothing unsaved stringifies nothing.
    fn begin_save(&mut self, doc: impl FnOnce() -> Option<String>) -> Option<WriteNoteArgs> {
        if !self.dirty {
            return None;
        }
        // Normalised before sending, so the saved text comes back identical to
        // what was sent and an ordinary save is never mistaken for someone
        // else's capture.
        let text = document::normalized(&doc()?);
        // Clean before the write, not after: an edit made while the write is
        // in flight has to leave the document dirty again, or it would be lost.
        self.dirty = false;
        self.in_flight += 1;
        Some(WriteNoteArgs {
            path: self.path.clone(),
            text,
        })
    }

    /// A save came back. `sent` is what [`Ledger::begin_save`] handed out, and
    /// `doc` the editor's text now, which may have moved on since.
    ///
    /// When the host saved something other than what was sent, it carried over
    /// a capture that landed mid-edit. The carried-over part comes back as an
    /// insert, which holds even if the user kept typing while the save was in
    /// flight — exactly when this happens. Their keystrokes stay, the spoken
    /// entry stays, and nothing has to be thrown away to reconcile the two.
    fn finish_save(
        &mut self,
        sent: &str,
        saved: Result<String, String>,
        doc: impl FnOnce() -> Option<String>,
    ) -> Option<Change> {
        debug_assert!(self.in_flight > 0, "a save came back that was never sent");
        self.in_flight = self.in_flight.saturating_sub(1);
        let saved = match saved {
            Ok(saved) => saved,
            Err(e) => {
                // Still dirty: the next keystroke retries, and no external
                // change may overwrite what did not reach disk.
                self.dirty = true;
                self.trouble = e;
                return None;
            }
        };
        self.trouble.clear();
        if saved == sent {
            return None;
        }
        match document::inserted_at_head(sent, &saved) {
            Some(inserted) => Some(Change::Insert {
                at: document::insertion_offset(&doc()?),
                text: inserted.to_string(),
            }),
            // Not a capture after all. Only safe with nothing unsaved.
            None if !self.dirty => Some(Change::Replace(saved)),
            None => None,
        }
    }

    /// Whether the file may be read into the document now. If not, the read
    /// is remembered and [`Ledger::catch_up`] says when it can happen.
    fn may_read(&mut self) -> bool {
        if self.dirty || self.in_flight > 0 {
            self.behind = true;
            return false;
        }
        true
    }

    /// Whether a read put off by [`Ledger::may_read`] can happen now.
    fn catch_up(&mut self) -> bool {
        if !self.behind || self.dirty || self.in_flight > 0 {
            return false;
        }
        self.behind = false;
        true
    }

    /// `read_note` came back with `note`, and the document now reads `doc`.
    /// Another file than the document's opens fresh; the same file is an
    /// external change. Nothing happens to a document that picked up unsaved
    /// edits or a save during the read — the read is put off again instead.
    fn loaded(&mut self, doc: &str, note: Note) -> Option<Change> {
        if self.dirty || self.in_flight > 0 {
            self.behind = true;
            return None;
        }
        if note.path != self.path {
            self.path = note.path;
            return Some(Change::Open(note.text));
        }
        self.external(doc, &note.text)
    }

    /// The file changed underneath the editor and now reads `incoming`. `None`
    /// when the document already says the same thing — or has unsaved edits,
    /// which win.
    fn external(&self, doc: &str, incoming: &str) -> Option<Change> {
        // Compared normalised: the trailing newline the document contract puts
        // on disk is not a change the editor needs to hear about, and treating
        // it as one would edit the document and take the cursor with it.
        if self.dirty || document::normalized(doc) == incoming {
            return None;
        }
        Some(match document::inserted_at_head(doc, incoming) {
            Some(inserted) => Change::Insert {
                at: document::insertion_offset(doc),
                text: inserted.to_string(),
            },
            None => Change::Replace(incoming.to_string()),
        })
    }
}

/// Reload when the file changes underneath us — a capture, Obsidian, an agent —
/// unless there are unsaved edits, which win.
fn follow_external_changes(autosave: Autosave) {
    ipc::listen::<(), _>(events::NOTE_CHANGED_EXTERNALLY, move |()| {
        autosave.follow();
    });
}

/// Open the new file when Settings moves the notes path (#5). Whatever is
/// still unsaved goes to the old file first: the save names it, and the read
/// waits for the save. Any other settings change reads the same file back,
/// which costs one round trip and changes nothing.
fn follow_note_path(autosave: Autosave) {
    ipc::listen::<Settings, _>(events::SETTINGS_CHANGED, move |_| {
        autosave.flush();
        autosave.follow();
    });
}

/// Show a failed capture, and take it down again when the next capture begins —
/// a message about the last attempt has nothing to say about this one.
///
/// `CAPTURE_FAILED` is emitted to every window; the editor is the one the tray
/// opens to see whether a capture landed, so it is the one that answers.
fn follow_capture_failures(capture_failure: RwSignal<String>) {
    ipc::listen::<String, _>(events::CAPTURE_FAILED, move |message| {
        capture_failure.set(message);
    });
    ipc::listen::<RecorderState, _>(events::RECORDER_STATE, move |state| {
        if state == RecorderState::Recording {
            capture_failure.set(String::new());
        }
    });
}

/// Losing focus is the last reliable moment before ⌘W hides the window, so it is
/// where the debounce gets cut short.
fn flush_on_blur(autosave: Autosave) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let handler = Closure::wrap(Box::new(move |_: web_sys::Event| {
        autosave.flush();
    }) as Box<dyn FnMut(web_sys::Event)>);

    let _ = window.add_event_listener_with_callback("blur", handler.as_ref().unchecked_ref());
    handler.forget();
}

fn prefers_dark() -> bool {
    web_sys::window()
        .and_then(|w| w.match_media("(prefers-color-scheme: dark)").ok().flatten())
        .map(|mql| mql.matches())
        .unwrap_or(false)
}

/// Page colors come from CSS; this only keeps CodeMirror's own internals (caret,
/// selection layer) on the right side of the light/dark line.
fn follow_system_theme(editor: Rc<RefCell<Option<Editor>>>) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(Some(mql)) = window.match_media("(prefers-color-scheme: dark)") else {
        return;
    };

    let handler = Closure::wrap(Box::new(move |_: web_sys::Event| {
        let dark = prefers_dark();
        if let Some(editor) = editor.borrow().as_ref() {
            editor.set_theme(dark);
        }
    }) as Box<dyn FnMut(web_sys::Event)>);

    mql.set_onchange(Some(handler.as_ref().unchecked_ref()));
    handler.forget();
}

#[cfg(test)]
mod tests {
    use super::*;

    const OLDER: &str = "## 2026-08-18 09:14\nOlder\n";
    const ENTRY: &str = "## 2026-08-18 09:41\nNewest\n";
    const CONFLICT: &str = "The notes file changed outside Talkie, so this text was not saved.";

    /// A ledger the way `flush` leaves it: one edit, its save in flight.
    fn saving(text: &str) -> (Ledger, String) {
        let mut ledger = Ledger::default();
        ledger.edited();
        let sent = ledger
            .begin_save(|| Some(text.to_string()))
            .expect("an edit to save");
        (ledger, sent.text)
    }

    fn note(path: &str, text: &str) -> Note {
        Note {
            path: path.to_string(),
            text: text.to_string(),
        }
    }

    /// A ledger the way the first read leaves it: clean, on file `a`.
    fn on_a() -> Ledger {
        Ledger {
            path: "a".to_string(),
            ..Ledger::default()
        }
    }

    #[test]
    fn a_dirty_editor_ignores_an_external_change() {
        let mut ledger = Ledger::default();
        assert_eq!(
            ledger.external(OLDER, "Theirs\n"),
            Some(Change::Replace("Theirs\n".to_string())),
            "a clean editor follows the file"
        );
        ledger.edited();
        assert_eq!(ledger.external(OLDER, "Theirs\n"), None);
    }

    /// The premise of #5: after a conflict nothing but a successful save
    /// unsticks the editor, so it never follows the file again on its own.
    #[test]
    fn a_failed_save_stays_dirty_and_says_so() {
        let (mut ledger, sent) = saving(OLDER);
        let change = ledger.finish_save(&sent, Err(CONFLICT.to_string()), || Some(sent.clone()));
        assert_eq!(change, None);
        assert!(ledger.dirty);
        assert_eq!(ledger.trouble, CONFLICT);
        assert_eq!(ledger.external(OLDER, "Theirs\n"), None);
    }

    #[test]
    fn a_successful_save_clears_the_trouble() {
        let (mut ledger, sent) = saving(OLDER);
        ledger.finish_save(&sent, Err(CONFLICT.to_string()), || Some(sent.clone()));

        let sent = ledger
            .begin_save(|| Some(OLDER.to_string()))
            .expect("still dirty, so the retry has something to send")
            .text;
        let change = ledger.finish_save(&sent, Ok(sent.clone()), || Some(sent.clone()));
        assert_eq!(change, None);
        assert!(!ledger.dirty);
        assert_eq!(ledger.trouble, "");
    }

    #[test]
    fn nothing_unsaved_means_nothing_to_send() {
        let mut ledger = Ledger::default();
        assert_eq!(
            ledger.begin_save(|| unreachable!("a clean ledger read the document")),
            None
        );
    }

    #[test]
    fn a_save_is_sent_normalised() {
        let (_, sent) = saving("## 2026-08-18 09:14\nOlder");
        assert_eq!(sent, OLDER);
    }

    #[test]
    fn an_edit_during_the_save_keeps_the_document_dirty() {
        let (mut ledger, sent) = saving(OLDER);
        ledger.edited();
        ledger.finish_save(&sent, Ok(sent.clone()), || Some(sent.clone()));
        assert!(ledger.dirty);
    }

    /// The user kept typing while the save was in flight; the capture the host
    /// carried over goes in at the top of what they have now.
    #[test]
    fn a_capture_carried_over_by_a_save_comes_back_as_an_insert() {
        let (mut ledger, sent) = saving(&format!("# talkie.md\n\n{OLDER}"));
        ledger.edited();
        let saved = document::splice(&sent, ENTRY);
        let now = format!("# talkie.md\n\n{OLDER}And more.\n");

        assert_eq!(
            ledger.finish_save(&sent, Ok(saved), || Some(now)),
            Some(Change::Insert {
                at: "# talkie.md\n\n".len(),
                text: format!("{ENTRY}\n"),
            })
        );
    }

    #[test]
    fn a_save_that_came_back_otherwise_replaces_only_when_clean() {
        let (mut ledger, sent) = saving(OLDER);
        let saved = "Something else\n".to_string();
        assert_eq!(
            ledger.finish_save(&sent, Ok(saved.clone()), || Some(sent.clone())),
            Some(Change::Replace(saved.clone()))
        );

        let (mut ledger, sent) = saving(OLDER);
        ledger.edited();
        assert_eq!(
            ledger.finish_save(&sent, Ok(saved), || Some(sent.clone())),
            None
        );
    }

    #[test]
    fn an_external_capture_is_an_insert() {
        let incoming = document::splice(OLDER, ENTRY);
        assert_eq!(
            Ledger::default().external(OLDER, &incoming),
            Some(Change::Insert {
                at: 0,
                text: format!("{ENTRY}\n"),
            })
        );
    }

    #[test]
    fn the_trailing_newline_alone_is_not_an_external_change() {
        let unterminated = OLDER.trim_end_matches('\n');
        assert_eq!(Ledger::default().external(unterminated, OLDER), None);
    }

    #[test]
    fn a_save_names_the_file_it_came_from() {
        let mut ledger = on_a();
        ledger.edited();
        let args = ledger.begin_save(|| Some(OLDER.to_string())).expect("args");
        assert_eq!(args.path, "a");
    }

    /// #5: the notes path moved, so the read names another file. It opens
    /// fresh, and the saves after it name the new file.
    #[test]
    fn another_file_opens_fresh() {
        let mut ledger = on_a();
        assert_eq!(
            ledger.loaded(OLDER, note("b", ENTRY)),
            Some(Change::Open(ENTRY.to_string()))
        );
        assert_eq!(ledger.path, "b");

        ledger.edited();
        let args = ledger.begin_save(|| Some(ENTRY.to_string())).expect("args");
        assert_eq!(args.path, "b");
    }

    #[test]
    fn the_same_file_is_an_external_change() {
        let incoming = document::splice(OLDER, ENTRY);
        assert_eq!(
            on_a().loaded(OLDER, note("a", &incoming)),
            Some(Change::Insert {
                at: 0,
                text: format!("{ENTRY}\n"),
            })
        );
    }

    /// Edits waiting when the path moves are saved to the old file before
    /// the new one is read: the read waits for the edit and for its save.
    #[test]
    fn a_read_waits_for_unsaved_edits_and_their_save() {
        let mut ledger = on_a();
        ledger.edited();
        assert!(!ledger.may_read(), "read under unsaved edits");

        let sent = ledger.begin_save(|| Some(OLDER.to_string())).expect("args");
        assert_eq!(sent.path, "a");
        assert!(!ledger.catch_up(), "read with a save in flight");
        assert!(!ledger.may_read(), "read with a save in flight");

        ledger.finish_save(&sent.text, Ok(sent.text.clone()), || {
            Some(sent.text.clone())
        });
        assert!(ledger.catch_up(), "the put-off read never happened");
        assert!(!ledger.catch_up(), "the put-off read happened twice");
        assert!(ledger.may_read());
    }

    /// A save that fails leaves the read put off: the editor stays on the
    /// file its text belongs to rather than dropping the text.
    #[test]
    fn a_failed_save_keeps_the_editor_on_its_file() {
        let mut ledger = on_a();
        ledger.edited();
        assert!(!ledger.may_read());
        let sent = ledger.begin_save(|| Some(OLDER.to_string())).expect("args");
        ledger.finish_save(&sent.text, Err(CONFLICT.to_string()), || {
            Some(sent.text.clone())
        });
        assert!(!ledger.catch_up());
        assert_eq!(ledger.path, "a");
    }

    /// A keystroke or a save during the read's round trip: the read is not
    /// applied, the document stays on its file, and the read is put off.
    #[test]
    fn a_read_that_lands_on_new_edits_is_put_off() {
        let mut ledger = on_a();
        assert!(ledger.may_read());
        ledger.edited();
        assert_eq!(ledger.loaded(OLDER, note("b", ENTRY)), None);
        assert_eq!(ledger.path, "a");

        let sent = ledger.begin_save(|| Some(OLDER.to_string())).expect("args");
        ledger.finish_save(&sent.text, Ok(sent.text.clone()), || {
            Some(sent.text.clone())
        });
        assert!(ledger.catch_up());
    }
}
