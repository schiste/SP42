use sp42_core::{PageVerificationReport, routes};

#[cfg(target_arch = "wasm32")]
use super::{config::api_url, http::post_json_bytes};
#[cfg(target_arch = "wasm32")]
use sp42_core::PageVerificationRequest;

/// Verify every URL-bearing citation on a revision and return the page report.
///
/// The `/dev/citation/verify-page` route is session+CSRF gated; `post_json_bytes`
/// already attaches the remembered CSRF token and includes credentials, so the
/// call rides the app's established bridge session.
#[cfg(target_arch = "wasm32")]
pub async fn fetch_page_report(
    wiki_id: &str,
    title: &str,
    rev_id: u64,
) -> Result<PageVerificationReport, String> {
    let request = PageVerificationRequest {
        wiki_id: wiki_id.to_string(),
        title: title.to_string(),
        rev_id,
    };
    let body = serde_json::to_string(&request)
        .map_err(|error| format!("encode verify-page request: {error}"))?;
    let bytes = post_json_bytes(
        &api_url(routes::DEV_CITATION_VERIFY_PAGE_PATH),
        body,
        "verify page citations",
    )
    .await?;

    serde_json::from_slice(&bytes).map_err(|error| format!("parse page report: {error}"))
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn fetch_page_report(
    _wiki_id: &str,
    _title: &str,
    _rev_id: u64,
) -> Result<PageVerificationReport, String> {
    let _ = routes::DEV_CITATION_VERIFY_PAGE_PATH;
    Err("Page citation verification is only available in the browser runtime.".to_string())
}
