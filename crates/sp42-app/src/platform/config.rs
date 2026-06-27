const FALLBACK_DEFAULT_WIKI_ID: &str = "frwiki";

#[must_use]
pub fn api_url(path: &str) -> String {
    join_base_and_path(&configured_api_base_url(), path)
}

#[must_use]
pub fn configured_default_wiki_id() -> String {
    runtime_default_wiki_id()
        .or_else(build_default_wiki_id)
        .unwrap_or_else(|| FALLBACK_DEFAULT_WIKI_ID.to_string())
}

/// The wiki the workspace is pointed at: a `?wiki=<dbname>` URL override (set by
/// the wiki picker) when present, otherwise the configured default. Lets SP42
/// target any Wikimedia project the server can resolve (ADR-0014).
#[must_use]
pub fn selected_wiki_id() -> String {
    wiki_override_from_query().unwrap_or_else(configured_default_wiki_id)
}

/// Point the workspace at `wiki_id` by setting the `?wiki=` override and
/// reloading (the patrol surface binds its wiki at construction).
#[cfg(target_arch = "wasm32")]
pub fn request_wiki_switch(wiki_id: &str) {
    let trimmed = wiki_id.trim();
    if trimmed.is_empty() {
        return;
    }
    if let Some(window) = web_sys::window() {
        let encoded = String::from(js_sys::encode_uri_component(trimmed));
        // Preserve the view hash (#view=…); replace the query.
        let hash = window.location().hash().unwrap_or_default();
        let _ = window
            .location()
            .set_href(&format!("/?wiki={encoded}{hash}"));
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn request_wiki_switch(_wiki_id: &str) {}

#[cfg(target_arch = "wasm32")]
fn wiki_override_from_query() -> Option<String> {
    let search = web_sys::window()?.location().search().ok()?;
    query_param(&search, "wiki")
}

#[cfg(not(target_arch = "wasm32"))]
fn wiki_override_from_query() -> Option<String> {
    None
}

fn query_param(search: &str, key: &str) -> Option<String> {
    let search = search.strip_prefix('?').unwrap_or(search);
    for pair in search.split('&') {
        let mut parts = pair.splitn(2, '=');
        if parts.next() == Some(key) {
            return non_empty_string(parts.next().unwrap_or(""));
        }
    }
    None
}

/// Whether the server is running in `local` deployment mode (from runtime
/// config). The local-setup window only makes sense — and the server only
/// permits credential writes — in local mode. Defaults to `true` when the mode
/// is unknown (local dev serves the runtime config, so this is reliable there).
#[must_use]
pub fn is_local_deployment() -> bool {
    deployment_mode().is_none_or(|mode| mode == "local")
}

#[cfg(target_arch = "wasm32")]
fn deployment_mode() -> Option<String> {
    runtime_config_string("deploymentMode")
}

#[cfg(not(target_arch = "wasm32"))]
fn deployment_mode() -> Option<String> {
    None
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
    option_env!("SP42_API_BASE_URL").and_then(non_empty_string)
}

fn build_default_wiki_id() -> Option<String> {
    option_env!("SP42_DEFAULT_WIKI_ID").and_then(non_empty_string)
}

fn non_empty_string(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
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
fn runtime_default_wiki_id() -> Option<String> {
    runtime_config_string("defaultWikiId").or_else(|| runtime_meta_content("sp42-default-wiki-id"))
}

#[cfg(not(target_arch = "wasm32"))]
fn runtime_default_wiki_id() -> Option<String> {
    None
}

#[cfg(target_arch = "wasm32")]
fn runtime_api_base_url_from_window() -> Option<String> {
    runtime_config_string("apiBaseUrl")
}

#[cfg(target_arch = "wasm32")]
fn runtime_config_string(field: &str) -> Option<String> {
    use wasm_bindgen::JsValue;

    let window = web_sys::window()?;
    let config =
        js_sys::Reflect::get(&window, &JsValue::from_str("__SP42_RUNTIME_CONFIG__")).ok()?;
    if config.is_undefined() || config.is_null() {
        return None;
    }

    js_sys::Reflect::get(&config, &JsValue::from_str(field))
        .ok()
        .and_then(|value| value.as_string())
        .and_then(|value| non_empty_string(&value))
}

#[cfg(target_arch = "wasm32")]
fn runtime_api_base_url_from_meta() -> Option<String> {
    runtime_meta_content("sp42-api-base-url")
}

#[cfg(target_arch = "wasm32")]
fn runtime_meta_content(name: &str) -> Option<String> {
    let document = web_sys::window()?.document()?;
    let selector = format!("meta[name=\"{name}\"]");
    let element = document.query_selector(&selector).ok()??;
    element
        .get_attribute("content")
        .and_then(|value| non_empty_string(&value))
}

#[cfg(test)]
mod tests {
    use super::{configured_default_wiki_id, join_base_and_path, normalize_base_url};

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

    #[test]
    fn defaults_to_frwiki_when_no_runtime_or_build_value_is_available() {
        assert_eq!(configured_default_wiki_id(), "frwiki");
    }
}
