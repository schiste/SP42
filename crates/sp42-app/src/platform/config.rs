#[must_use]
pub fn api_url(path: &str) -> String {
    join_base_and_path(&configured_api_base_url(), path)
}

#[must_use]
pub fn configured_api_base_url() -> String {
    runtime_api_base_url()
        .or_else(build_api_base_url)
        .map(|value| normalize_base_url(&value))
        .unwrap_or_default()
}

#[must_use]
pub fn join_base_and_path(base_url: &str, path: &str) -> String {
    let normalized_base = normalize_base_url(base_url);
    let normalized_path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };

    if normalized_base.is_empty() {
        normalized_path
    } else {
        format!("{normalized_base}{normalized_path}")
    }
}

#[must_use]
pub fn normalize_base_url(value: &str) -> String {
    value.trim().trim_end_matches('/').to_string()
}

fn build_api_base_url() -> Option<String> {
    option_env!("SP42_API_BASE_URL")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

#[cfg(target_arch = "wasm32")]
fn runtime_api_base_url() -> Option<String> {
    runtime_api_base_url_from_window().or_else(runtime_api_base_url_from_meta)
}

#[cfg(not(target_arch = "wasm32"))]
fn runtime_api_base_url() -> Option<String> {
    None
}

#[cfg(target_arch = "wasm32")]
fn runtime_api_base_url_from_window() -> Option<String> {
    use wasm_bindgen::JsValue;

    let window = web_sys::window()?;
    let config =
        js_sys::Reflect::get(&window, &JsValue::from_str("__SP42_RUNTIME_CONFIG__")).ok()?;
    if config.is_undefined() || config.is_null() {
        return None;
    }

    js_sys::Reflect::get(&config, &JsValue::from_str("apiBaseUrl"))
        .ok()
        .and_then(|value| value.as_string())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(target_arch = "wasm32")]
fn runtime_api_base_url_from_meta() -> Option<String> {
    let document = web_sys::window()?.document()?;
    let element = document
        .query_selector("meta[name=\"sp42-api-base-url\"]")
        .ok()??;
    element
        .get_attribute("content")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{join_base_and_path, normalize_base_url};

    #[test]
    fn joins_same_origin_paths() {
        assert_eq!(
            join_base_and_path("", "/operator/live/frwiki"),
            "/operator/live/frwiki"
        );
        assert_eq!(
            join_base_and_path("", "operator/live/frwiki"),
            "/operator/live/frwiki"
        );
    }

    #[test]
    fn joins_absolute_base_url_without_double_slashes() {
        assert_eq!(
            join_base_and_path("https://sp42.example.org/", "/debug/runtime"),
            "https://sp42.example.org/debug/runtime"
        );
    }

    #[test]
    fn normalizes_base_url() {
        assert_eq!(
            normalize_base_url(" https://sp42.example.org/// "),
            "https://sp42.example.org"
        );
    }
}
