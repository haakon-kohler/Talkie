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
use talkie_shared::{commands, document, events, RecorderState, WriteNoteArgs};
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
                let text = match ipc::fetch::<String>(commands::READ_NOTE).await {
                    Ok(text) => text,
                    Err(e) => {
                        autosave.trouble.set(e);
                        return;
                    }
                };

                let on_change = {
                    let autosave = autosave.clone();
                    move || autosave.touch()
                };
                // No scrolling to do: CodeMirror opens at the top, and the top
                // is where the newest capture is.
                let mounted = Editor::mount(&element, &text, prefers_dark(), on_change);
                mounted.focus();
                *autosave.editor.borrow_mut() = Some(mounted);

                follow_system_theme(autosave.editor.clone());
                flush_on_blur(autosave.clone());
                follow_external_changes(autosave);
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

/// The save side of the editor: what is mounted, whether it has unsaved changes,
/// and the timer that turns typing into one write.
#[derive(Clone)]
struct Autosave {
    editor: Rc<RefCell<Option<Editor>>>,
    /// Unsaved local edits. Also the flag that makes an external change wait.
    dirty: Rc<Cell<bool>>,
    timer: Rc<Cell<Option<TimeoutHandle>>>,
    /// Empty unless a save failed.
    trouble: RwSignal<String>,
}

impl Autosave {
    fn new() -> Self {
        Self {
            editor: Rc::new(RefCell::new(None)),
            dirty: Rc::new(Cell::new(false)),
            timer: Rc::new(Cell::new(None)),
            trouble: RwSignal::new(String::new()),
        }
    }

    /// The document changed. Restart the clock.
    fn touch(&self) {
        self.dirty.set(true);
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

    /// Write now, if there is anything to write.
    fn flush(&self) {
        if let Some(pending) = self.timer.take() {
            pending.clear();
        }
        if !self.dirty.get() {
            return;
        }
        let Some(text) = self
            .editor
            .borrow()
            .as_ref()
            // Normalised before sending, so the saved text comes back
            // identical to what was sent and an ordinary save is never mistaken
            // for someone else's capture.
            .map(|editor| document::normalized(&editor.doc()))
        else {
            return;
        };

        // Cleared before the write, not after: an edit made while the write is
        // in flight has to leave the document dirty again, or it would be lost.
        self.dirty.set(false);

        let me = self.clone();
        spawn_local(async move {
            let sent = text.clone();
            let args = WriteNoteArgs { text };
            match ipc::call::<WriteNoteArgs, String>(commands::WRITE_NOTE, &args).await {
                Ok(saved) => {
                    me.trouble.set(String::new());
                    if saved != sent {
                        me.reconcile(&sent, &saved);
                    }
                }
                Err(e) => {
                    // Still dirty: the next keystroke retries, and no external
                    // change may overwrite what did not reach disk.
                    me.dirty.set(true);
                    me.trouble.set(e);
                }
            }
        });
    }

    /// The host saved something other than what was sent — it carried over a
    /// capture that landed mid-edit.
    ///
    /// The carried-over part is applied to the editor as an insert, which holds
    /// even if the user kept typing while the save was in flight — exactly when
    /// this happens. Their keystrokes stay, the spoken entry stays, and nothing
    /// has to be thrown away to reconcile the two.
    fn reconcile(&self, sent: &str, saved: &str) {
        let editor = self.editor.borrow();
        let Some(editor) = editor.as_ref() else {
            return;
        };
        match document::inserted_at_head(sent, saved) {
            Some(inserted) => {
                let at = document::insertion_offset(&editor.doc());
                editor.insert_and_reveal(at, inserted);
            }
            // Not a capture after all. Only safe with nothing unsaved.
            None if !self.dirty.get() => editor.set_doc(saved),
            None => {}
        }
    }
}

/// Apply what is now on disk.
///
/// Text added at the top — the shape of a silent capture — is applied as an
/// insert, so the cursor and the undo history survive it. Anything else is a
/// replacement.
fn apply(editor: &Editor, incoming: &str) {
    let current = editor.doc();
    // Compared normalised: the trailing newline the document contract puts on
    // disk is not a change the editor needs to hear about, and treating it as
    // one would edit the document and take the cursor with it.
    if document::normalized(&current) == incoming {
        return;
    }
    match document::inserted_at_head(&current, incoming) {
        Some(inserted) => {
            let at = document::insertion_offset(&current);
            editor.insert_and_reveal(at, inserted);
        }
        None => editor.set_doc(incoming),
    }
}

/// Reload when the file changes underneath us — a capture, Obsidian, an agent —
/// unless there are unsaved edits, which win.
fn follow_external_changes(autosave: Autosave) {
    ipc::listen::<(), _>(events::NOTE_CHANGED_EXTERNALLY, move |()| {
        if autosave.dirty.get() {
            return;
        }
        let autosave = autosave.clone();
        spawn_local(async move {
            let Ok(text) = ipc::fetch::<String>(commands::READ_NOTE).await else {
                return;
            };
            // Checked again: the read was a round trip, and a keystroke during
            // it would make this reload a clobber.
            if autosave.dirty.get() {
                return;
            }
            if let Some(editor) = autosave.editor.borrow().as_ref() {
                apply(editor, &text);
            }
        });
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
