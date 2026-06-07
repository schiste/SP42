use sp42_core::{MediaDiffReport, RenderedHunkPreview, StructuredDiff, routes};
use sp42_reporting::LiveOperatorView;

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
