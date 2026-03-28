/// Browser console logging helpers for debugging.
///
/// All functions are no-ops on non-wasm targets. On wasm they write
/// structured messages to the browser dev-tools console.

#[cfg(target_arch = "wasm32")]
pub fn debug(msg: &str) {
    web_sys::console::debug_1(&msg.into());
}

#[cfg(not(target_arch = "wasm32"))]
pub fn debug(_msg: &str) {}

#[cfg(target_arch = "wasm32")]
pub fn info(msg: &str) {
    web_sys::console::info_1(&msg.into());
}

#[cfg(not(target_arch = "wasm32"))]
pub fn info(_msg: &str) {}

#[cfg(target_arch = "wasm32")]
pub fn warn(msg: &str) {
    web_sys::console::warn_1(&msg.into());
}

#[cfg(not(target_arch = "wasm32"))]
pub fn warn(_msg: &str) {}
