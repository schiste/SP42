#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsValue;

#[cfg(target_arch = "wasm32")]
#[must_use]
pub fn browser_window() -> Option<web_sys::Window> {
    web_sys::window()
}

#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn browser_window() -> Option<web_sys::Window> {
    None
}

#[cfg(target_arch = "wasm32")]
#[must_use]
pub fn browser_document() -> Option<web_sys::Document> {
    browser_window()?.document()
}

#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn browser_document() -> Option<web_sys::Document> {
    None
}

#[cfg(target_arch = "wasm32")]
#[must_use]
pub fn browser_navigator() -> Option<web_sys::Navigator> {
    browser_window().map(|window| window.navigator())
}

#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn browser_navigator() -> Option<web_sys::Navigator> {
    None
}

#[cfg(target_arch = "wasm32")]
#[must_use]
pub fn browser_service_worker_container() -> Option<web_sys::ServiceWorkerContainer> {
    browser_navigator().map(|navigator| navigator.service_worker())
}

#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn browser_service_worker_container() -> Option<web_sys::ServiceWorkerContainer> {
    None
}

#[cfg(target_arch = "wasm32")]
pub fn browser_location_search() -> Result<Option<String>, String> {
    let Some(window) = browser_window() else {
        return Err("browser window is unavailable".to_string());
    };
    window.location().search().map(Some).map_err(|error| {
        error
            .as_string()
            .unwrap_or_else(|| "unable to read browser location search".to_string())
    })
}

#[cfg(not(target_arch = "wasm32"))]
pub fn browser_location_search() -> Result<Option<String>, String> {
    Err("browser window is unavailable".to_string())
}

#[cfg(target_arch = "wasm32")]
pub fn browser_is_secure_context() -> bool {
    browser_window()
        .map(|window| window.is_secure_context())
        .unwrap_or(false)
}

#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn browser_is_secure_context() -> bool {
    false
}

#[cfg(target_arch = "wasm32")]
pub fn publish_window_status_lines(key: &str, lines: &[String]) -> Result<(), String> {
    let Some(window) = browser_window() else {
        return Err("browser window is unavailable".to_string());
    };

    let payload = js_sys::Array::new();
    for line in lines {
        payload.push(&JsValue::from_str(line));
    }

    js_sys::Reflect::set(window.as_ref(), &JsValue::from_str(key), &payload.into()).map_err(
        |error| {
            error
                .as_string()
                .unwrap_or_else(|| "failed to publish browser status".to_string())
        },
    )?;

    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn publish_window_status_lines(_key: &str, _lines: &[String]) -> Result<(), String> {
    Err("browser window is unavailable".to_string())
}

#[cfg(target_arch = "wasm32")]
pub fn browser_manifest_href() -> Option<String> {
    browser_document()
        .and_then(|document| {
            document
                .query_selector("link[rel='manifest']")
                .ok()
                .flatten()
        })
        .and_then(|element| element.get_attribute("href"))
}
