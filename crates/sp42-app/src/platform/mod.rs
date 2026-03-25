pub mod auth;
pub mod bootstrap;
pub mod coordination;
pub mod debug;
pub mod globals;
pub(crate) mod http;
pub mod live;
pub mod pwa;
pub mod runtime;

/// Convert a [`wasm_bindgen::JsValue`] error into a human-readable string.
///
/// Tries `as_string()` first, then `JSON.stringify()`, and falls back to a
/// generic message.  Shared by every platform module that handles JS errors.
#[cfg(target_arch = "wasm32")]
pub(crate) fn js_error_message(error: wasm_bindgen::JsValue) -> String {
    error
        .as_string()
        .or_else(|| {
            js_sys::JSON::stringify(&error)
                .ok()
                .and_then(|text| text.as_string())
        })
        .unwrap_or_else(|| "unknown browser error".to_string())
}
