//! The whole frontend↔backend bridge, in one file.
//!
//! `withGlobalTauri: true` puts Tauri's JS API on `window.__TAURI__`, so the
//! WASM binds straight to it — no npm package, no generated bindings. Command
//! names and payload types come from `talkie_shared`, which makes this shim the
//! only untyped-by-construction code on the Rust side, and it is deliberately
//! small.

use serde::{de::DeserializeOwned, Serialize};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], catch)]
    async fn invoke(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "event"], js_name = "listen")]
    fn tauri_listen(event: &str, handler: &JsValue) -> js_sys::Promise;

    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "window"], js_name = "getCurrentWindow")]
    fn current_window() -> JsValue;
}

fn describe(value: &JsValue) -> String {
    value
        .as_string()
        .unwrap_or_else(|| format!("{value:?}").trim_matches('"').to_string())
}

/// The label Tauri gave this window — how each surface knows what it is.
pub fn current_window_label() -> String {
    js_sys::Reflect::get(&current_window(), &JsValue::from_str("label"))
        .ok()
        .and_then(|value| value.as_string())
        .unwrap_or_default()
}

/// Call a command that takes arguments and returns a value.
// First used in M1's model-download progress query. Kept complete so the shim stays
// symmetric rather than growing a hole-shaped gap later.
#[allow(dead_code)]
pub async fn call<A: Serialize, R: DeserializeOwned>(cmd: &str, args: &A) -> Result<R, String> {
    let args = serde_wasm_bindgen::to_value(args).map_err(|e| e.to_string())?;
    let value = invoke(cmd, args).await.map_err(|e| describe(&e))?;
    serde_wasm_bindgen::from_value(value).map_err(|e| e.to_string())
}

/// Call a command that takes arguments and returns nothing worth reading.
pub async fn call_void<A: Serialize>(cmd: &str, args: &A) -> Result<(), String> {
    let args = serde_wasm_bindgen::to_value(args).map_err(|e| e.to_string())?;
    invoke(cmd, args)
        .await
        .map(|_| ())
        .map_err(|e| describe(&e))
}

/// Call a command that takes no arguments.
pub async fn fetch<R: DeserializeOwned>(cmd: &str) -> Result<R, String> {
    let value = invoke(cmd, js_sys::Object::new().into())
        .await
        .map_err(|e| describe(&e))?;
    serde_wasm_bindgen::from_value(value).map_err(|e| e.to_string())
}

/// Call a command that takes no arguments and returns nothing.
pub async fn fire(cmd: &str) -> Result<(), String> {
    invoke(cmd, js_sys::Object::new().into())
        .await
        .map(|_| ())
        .map_err(|e| describe(&e))
}

/// Subscribe to a backend event for the lifetime of the window.
///
/// The unlisten handle is intentionally dropped: every listener Talkie sets up
/// lives as long as its window does.
// First used in M1 (recorder state) and M2 (external file changes).
#[allow(dead_code)]
pub fn listen<T, F>(event: &'static str, mut on_event: F)
where
    T: DeserializeOwned + 'static,
    F: FnMut(T) + 'static,
{
    let handler = Closure::wrap(Box::new(move |raw: JsValue| {
        let payload =
            js_sys::Reflect::get(&raw, &JsValue::from_str("payload")).unwrap_or(JsValue::UNDEFINED);
        match serde_wasm_bindgen::from_value::<T>(payload) {
            Ok(value) => on_event(value),
            Err(e) => web_sys::console::error_1(
                &format!("talkie: unreadable payload on {event}: {e}").into(),
            ),
        }
    }) as Box<dyn FnMut(JsValue)>);

    let _ = tauri_listen(event, handler.as_ref());
    handler.forget();
}
