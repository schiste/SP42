use sp42_core::{MediaDiffReport, RenderedHunkPreview, StructuredDiff, routes};
use sp42_patrol::LiveOperatorView;

use crate::components::filter_bar::PatrolFilterParams;

#[cfg(target_arch = "wasm32")]
use super::{config::api_url, http::get_bytes};

#[cfg(target_arch = "wasm32")]
pub async fn fetch_live_operator_view(
    wiki_id: &str,
    filters: &PatrolFilterParams,
) -> Result<LiveOperatorView, String> {
    let query_string = filters.to_query_string();
    let url = routes::operator_live_path_with_query(wiki_id, &query_string);
    let bytes = get_bytes(&api_url(&url), "fetch live operator view").await?;

    serde_json::from_slice(&bytes).map_err(|error| format!("parse live operator view: {error}"))
}

/// The active wiki's resolved default namespace allowlist (`GET /wikis/{id}`), so
/// the patrol filter UI shows the namespaces the server actually uses for an
/// unfiltered query — configured wikis differ from the universal default. Codex
/// review #90.
#[cfg(target_arch = "wasm32")]
pub async fn fetch_wiki_namespace_defaults(wiki_id: &str) -> Result<Vec<i32>, String> {
    let bytes = get_bytes(
        &api_url(&routes::wiki_defaults_path(wiki_id)),
        "fetch wiki namespace defaults",
    )
    .await?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    let namespaces = value
        .get("namespace_allowlist")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_i64)
                .filter_map(|value| i32::try_from(value).ok())
                .collect()
        })
        .unwrap_or_default();
    Ok(namespaces)
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn fetch_live_operator_view(
    _wiki_id: &str,
    _filters: &PatrolFilterParams,
) -> Result<LiveOperatorView, String> {
    Err("Live operator view is only available in the browser runtime.".to_string())
}

#[cfg(target_arch = "wasm32")]
pub async fn fetch_diff(
    wiki_id: &str,
    rev_id: u64,
    old_rev_id: u64,
) -> Result<Option<StructuredDiff>, String> {
    let url = routes::operator_diff_path(wiki_id, rev_id, old_rev_id);
    let bytes = get_bytes(&api_url(&url), "fetch diff").await?;
    serde_json::from_slice(&bytes).map_err(|error| format!("parse diff: {error}"))
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn fetch_diff(
    _wiki_id: &str,
    _rev_id: u64,
    _old_rev_id: u64,
) -> Result<Option<StructuredDiff>, String> {
    Err("Diff fetch is only available in the browser runtime.".to_string())
}

#[cfg(target_arch = "wasm32")]
pub async fn fetch_media_diff(
    wiki_id: &str,
    rev_id: u64,
    old_rev_id: u64,
) -> Result<Option<MediaDiffReport>, String> {
    let url = routes::operator_media_diff_path(wiki_id, rev_id, old_rev_id);
    let bytes = get_bytes(&api_url(&url), "fetch media diff").await?;
    serde_json::from_slice(&bytes).map_err(|error| format!("parse media diff: {error}"))
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn fetch_media_diff(
    _wiki_id: &str,
    _rev_id: u64,
    _old_rev_id: u64,
) -> Result<Option<MediaDiffReport>, String> {
    Err("Media diff fetch is only available in the browser runtime.".to_string())
}

#[cfg(target_arch = "wasm32")]
pub async fn fetch_rendered_hunk(
    wiki_id: &str,
    rev_id: u64,
    old_rev_id: u64,
    hunk_index: usize,
) -> Result<Option<RenderedHunkPreview>, String> {
    let url = routes::operator_rendered_hunk_path(wiki_id, rev_id, old_rev_id, hunk_index);
    let bytes = get_bytes(&api_url(&url), "fetch rendered hunk").await?;
    serde_json::from_slice(&bytes).map_err(|error| format!("parse rendered hunk: {error}"))
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn fetch_rendered_hunk(
    _wiki_id: &str,
    _rev_id: u64,
    _old_rev_id: u64,
    _hunk_index: usize,
) -> Result<Option<RenderedHunkPreview>, String> {
    Err("Rendered hunk fetch is only available in the browser runtime.".to_string())
}
