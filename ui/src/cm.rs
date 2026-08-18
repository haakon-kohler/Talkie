//! The typed edge over the vendored CodeMirror bundle.
//!
//! This is the one place in Talkie where Rust trusts a hand-written declaration
//! instead of the compiler. Keep it boring: nine externs, mirroring exactly the
//! nine exports of `assets/vendor/codemirror.bundle.js`. If you change one side,
//! change the other — see `assets/vendor/README.md`.

use wasm_bindgen::prelude::*;

#[wasm_bindgen(module = "/assets/vendor/codemirror.bundle.js")]
extern "C" {
    #[wasm_bindgen(js_name = "init")]
    fn cm_init(
        parent: &web_sys::Element,
        doc: &str,
        on_doc_changed: &JsValue,
        dark: bool,
    ) -> JsValue;

    #[wasm_bindgen(js_name = "getDoc")]
    fn cm_get_doc(view: &JsValue) -> String;

    #[wasm_bindgen(js_name = "setDoc")]
    fn cm_set_doc(view: &JsValue, text: &str);

    #[wasm_bindgen(js_name = "appendAndReveal")]
    fn cm_append_and_reveal(view: &JsValue, text: &str);

    #[wasm_bindgen(js_name = "scrollToEnd")]
    fn cm_scroll_to_end(view: &JsValue);

    #[wasm_bindgen(js_name = "openSearch")]
    fn cm_open_search(view: &JsValue);

    #[wasm_bindgen(js_name = "setTheme")]
    fn cm_set_theme(view: &JsValue, dark: bool);

    #[wasm_bindgen(js_name = "focusEditor")]
    fn cm_focus(view: &JsValue);

    #[wasm_bindgen(js_name = "destroy")]
    fn cm_destroy(view: &JsValue);
}

/// A mounted editor. Owns the change callback, so dropping this tears down both
/// the CodeMirror view and the JS closure behind it.
pub struct Editor {
    view: JsValue,
    _on_change: Closure<dyn FnMut()>,
}

// M2 wires the rest of these up to talkie.md; they exist now so the vendored
// bundle's surface is declared once and proven to load.
#[allow(dead_code)]
impl Editor {
    /// Mount an editor into `parent`.
    ///
    /// `on_change` fires on every user edit and carries no payload — the caller
    /// debounces, then pulls the text with [`Editor::doc`], so a long file is
    /// never stringified per keystroke.
    pub fn mount(
        parent: &web_sys::Element,
        doc: &str,
        dark: bool,
        on_change: impl FnMut() + 'static,
    ) -> Self {
        let on_change = Closure::wrap(Box::new(on_change) as Box<dyn FnMut()>);
        let view = cm_init(parent, doc, on_change.as_ref(), dark);
        Self {
            view,
            _on_change: on_change,
        }
    }

    pub fn doc(&self) -> String {
        cm_get_doc(&self.view)
    }

    pub fn set_doc(&self, text: &str) {
        cm_set_doc(&self.view, text);
    }

    /// Append at the end and scroll it into view — how a silent capture shows up
    /// while the editor happens to be open.
    pub fn append_and_reveal(&self, text: &str) {
        cm_append_and_reveal(&self.view, text);
    }

    pub fn scroll_to_end(&self) {
        cm_scroll_to_end(&self.view);
    }

    /// ⌘F is already bound inside CodeMirror; this is for opening search from
    /// elsewhere (a menu item, say).
    pub fn open_search(&self) {
        cm_open_search(&self.view);
    }

    pub fn set_theme(&self, dark: bool) {
        cm_set_theme(&self.view, dark);
    }

    pub fn focus(&self) {
        cm_focus(&self.view);
    }
}

impl Drop for Editor {
    fn drop(&mut self) {
        cm_destroy(&self.view);
    }
}
