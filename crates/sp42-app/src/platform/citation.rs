use sp42_core::{
    BareUrlApplyRequest, BareUrlApplyResponse, BareUrlProposalsRequest, BareUrlProposalsResponse,
    PageVerificationReport, ReverifyFindingRequest, ReverifyFindingResponse, routes,
};

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

/// Re-verify one finding in place (PRD-0014): operator-triggered only, never
/// called automatically. `/dev/citation/reverify` is session+CSRF gated,
/// matching `fetch_page_report`.
///
/// # Errors
///
/// Returns `Err` when the request fails to encode, the network call fails,
/// or the response fails to parse (including a non-2xx error body).
#[cfg(target_arch = "wasm32")]
pub async fn reverify_finding(
    request: &ReverifyFindingRequest,
) -> Result<ReverifyFindingResponse, String> {
    let body = serde_json::to_string(request)
        .map_err(|error| format!("encode reverify request: {error}"))?;
    let bytes = post_json_bytes(
        &api_url(routes::DEV_CITATION_REVERIFY_PATH),
        body,
        "re-verify citation finding",
    )
    .await?;

    serde_json::from_slice(&bytes).map_err(|error| format!("parse reverify response: {error}"))
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn reverify_finding(
    _request: &ReverifyFindingRequest,
) -> Result<ReverifyFindingResponse, String> {
    Err("Re-verify is only available in the browser runtime.".to_string())
}

/// Bare-URL repair proposals for a page (PRD-0008): read-only, not session-gated.
/// The Citations tab's "Fix citation" action filters these down to the one
/// proposal (if any) whose bare URL matches the finding's source.
///
/// # Errors
///
/// Returns `Err` when the request fails to encode, the network call fails,
/// or the response fails to parse (including a non-2xx error body).
#[cfg(target_arch = "wasm32")]
pub async fn fetch_bare_url_proposals(
    wiki_id: &str,
    title: &str,
    rev_id: u64,
) -> Result<BareUrlProposalsResponse, String> {
    let request = BareUrlProposalsRequest {
        wiki_id: wiki_id.to_string(),
        title: title.to_string(),
        rev_id,
    };
    let body = serde_json::to_string(&request)
        .map_err(|error| format!("encode bare-url proposals request: {error}"))?;
    let bytes = post_json_bytes(
        &api_url(routes::DEV_CITATION_BARE_URL_PROPOSALS_PATH),
        body,
        "fetch bare-URL repair proposals",
    )
    .await?;

    serde_json::from_slice(&bytes).map_err(|error| format!("parse bare-url proposals: {error}"))
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn fetch_bare_url_proposals(
    _wiki_id: &str,
    _title: &str,
    _rev_id: u64,
) -> Result<BareUrlProposalsResponse, String> {
    Err("Bare-URL repair proposals are only available in the browser runtime.".to_string())
}

/// Apply one bare-URL repair proposal verbatim (PRD-0008), session+CSRF gated.
///
/// # Errors
///
/// Returns `Err` when the request fails to encode, the network call fails,
/// or the response fails to parse (including a non-2xx error body).
#[cfg(target_arch = "wasm32")]
pub async fn apply_bare_url_proposal(
    request: &BareUrlApplyRequest,
) -> Result<BareUrlApplyResponse, String> {
    let body = serde_json::to_string(request)
        .map_err(|error| format!("encode bare-url apply request: {error}"))?;
    let bytes = post_json_bytes(
        &api_url(routes::DEV_CITATION_BARE_URL_APPLY_PATH),
        body,
        "apply bare-URL repair",
    )
    .await?;

    serde_json::from_slice(&bytes)
        .map_err(|error| format!("parse bare-url apply response: {error}"))
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn apply_bare_url_proposal(
    _request: &BareUrlApplyRequest,
) -> Result<BareUrlApplyResponse, String> {
    Err("Bare-URL repair apply is only available in the browser runtime.".to_string())
}
