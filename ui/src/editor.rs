//! The editor window: no toolbar, no buttons, no status bar. Just the text.
//!
//! M0 mounts CodeMirror on a sample document to prove the vendored bundle and
//! the wasm-bindgen edge work end to end. M2 replaces the sample with talkie.md
//! and adds autosave plus reload-on-external-change.

use std::cell::RefCell;
use std::rc::Rc;

use leptos::html::Div;
use leptos::prelude::*;
use wasm_bindgen::prelude::*;

use crate::cm::Editor;

/// Stands in for talkie.md until M2. Lorem ipsum in the exact shape of the
/// document contract, so the markdown tinting gets exercised; the real sample
/// text lives in `COPY.md` (`editor.sample`) and is synced in from there.
const SAMPLE: &str = "\
# talkie.md

## 2026-08-18 09:14
Lorem ipsum dolor sit amet, consectetur adipiscing elit.

## 2026-08-18 09:31
Sed do **eiusmod tempor** incididunt ut labore et dolore magna aliqua. Ut enim ad
minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea
commodo consequat.

## 2026-08-18 09:40
Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu
fugiat nulla pariatur.
";

#[component]
pub fn EditorPage() -> impl IntoView {
    let host: NodeRef<Div> = NodeRef::new();
    let editor: Rc<RefCell<Option<Editor>>> = Rc::new(RefCell::new(None));

    Effect::new({
        let editor = editor.clone();
        move |_| {
            let Some(element) = host.get() else { return };
            if editor.borrow().is_some() {
                return;
            }

            let mounted = Editor::mount(&element, SAMPLE, prefers_dark(), || {
                // M2 debounces this into a save. For now it is the proof that
                // the JS → WASM direction of the bridge works.
                web_sys::console::log_1(&"talkie: document edited".into());
            });
            mounted.focus();
            *editor.borrow_mut() = Some(mounted);

            follow_system_theme(editor.clone());
        }
    });

    view! {
        <div class="editor-window">
            // Hidden titlebar: this strip is the drag region under the traffic lights.
            <div class="titlebar" data-tauri-drag-region></div>
            <div class="editor-host" node_ref=host></div>
        </div>
    }
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
