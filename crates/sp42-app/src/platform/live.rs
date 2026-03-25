use sp42_core::LiveOperatorView;

use crate::components::filter_bar::PatrolFilterParams;

#[cfg(target_arch = "wasm32")]
use super::http::get_bytes;

const LIVE_OPERATOR_URL_PREFIX: &str = "/operator/live";

#[cfg(target_arch = "wasm32")]
pub async fn fetch_live_operator_view(
    wiki_id: &str,
    filters: &PatrolFilterParams,
) -> Result<LiveOperatorView, String> {
    let query_string = filters.to_query_string();
    let url = if query_string.is_empty() {
        format!("{LIVE_OPERATOR_URL_PREFIX}/{wiki_id}")
    } else {
        format!("{LIVE_OPERATOR_URL_PREFIX}/{wiki_id}?{query_string}")
    };
    let bytes = get_bytes(&url, "fetch live operator view").await?;

    serde_json::from_slice(&bytes).map_err(|error| format!("parse live operator view: {error}"))
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn fetch_live_operator_view(
    _wiki_id: &str,
    _filters: &PatrolFilterParams,
) -> Result<LiveOperatorView, String> {
    Err("Live operator view is only available in the browser runtime.".to_string())
}
