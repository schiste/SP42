//! Bare-URL repair bridge routes (PRD-0008): propose and (Phase 5) apply.
//!
//! FCIS: classification and rendering are pure `sp42_core::bare_url_repair`
//! calls; this module owns the imperative edges — the per-wiki gate, the
//! sequential Citoid fetch (1 req/s etiquette), and the route glue. Citoid
//! failures decline the affected reference; they never fail the response.

use std::time::Duration;

use sp42_core::{
    ActionError, ActionResponseSummary, BareUrlApplyRequest, BareUrlApplyResponse, BareUrlDeclined,
    BareUrlOutcome, BareUrlProposal, BareUrlProposalsRequest, BareUrlProposalsResponse,
    CitoidMetadata, FlagState, PageVerificationRequest, TokenKind, VerifyOptions,
    WikiPageSaveRequest, WikitextEditor, WikitextNodeKind, WikitextNodeLocator, WikitextPageRef,
    bare_url_references, build_citoid_header, build_citoid_request, citoid_language,
    execute_fetch_token, execute_wiki_page_save, extract_use_sites, parse_action_response_summary,
    render_bare_url_citation, verify_page,
};

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};

use sp42_types::{HttpClient, HttpMethod, HttpRequest, HttpResponse};

use crate::action_routes::{
    action_error_from_editor, action_error_response, patrol_original_edit_if_possible,
    replace_node_or_refuse,
};
use crate::config_for_state_wiki;
use crate::http_errors::{forbidden_error, unauthorized_error};
use crate::runtime_adapters::BearerHttpClient;
use crate::runtime_adapters::PlainHttpClient;
use crate::session_runtime::{current_session_snapshot, validate_csrf_header};
use crate::state::AppState;

/// Citoid etiquette: at most one request per second on the live service.
const CITOID_PACE: Duration = Duration::from_secs(1);

/// Page-level verify fan-out: how many use-sites verify concurrently on the
/// verify-page route. Each runs its own `options.concurrency`-wide model panel,
/// so keep `PAGE_VERIFY_CONCURRENCY * options.concurrency` within the model
/// endpoint's rate limit.
const PAGE_VERIFY_CONCURRENCY: usize = 8;

/// Default edit summary when the operator supplies no note.
const BARE_URL_DEFAULT_SUMMARY: &str = "SP42: bare-URL repair";

/// The configured bare-URL citation template, or the per-wiki gate refusal.
///
/// Presence of `templates.bare_url_citation` is the whole gate (PRD-0008):
/// a wiki without it (every production config) refuses before any wiki or
/// Citoid traffic.
fn bare_url_template(config: &sp42_core::WikiConfig) -> Result<String, ActionError> {
    config
        .templates
        .bare_url_citation
        .clone()
        .ok_or_else(|| ActionError::Execution {
            message: format!("bare-URL repair is not enabled for wiki {}", config.wiki_id),
            code: Some("bare-url-repair-not-enabled".to_string()),
            http_status: Some(400),
            retryable: false,
        })
}

/// Fetch and parse the Citoid object for one source URL; `None` on any
/// failure (transport error, non-2xx, unparseable body) — the caller
/// declines that reference instead of erroring.
///
/// `base_override` swaps the canonical endpoint's scheme/host for tests
/// while keeping the lifted client's exact path encoding.
async fn fetch_citoid_object(
    client: &reqwest::Client,
    base_override: Option<&str>,
    source_url: &str,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let canonical = build_citoid_request(source_url);
    let url = match base_override {
        None => canonical.url.to_string(),
        Some(base) => format!("{}{}", base.trim_end_matches('/'), canonical.url.path()),
    };
    let response = client.get(&url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let body = response.bytes().await.ok()?;
    sp42_core::parse_citoid_response(&body)
}

/// Fetch Citoid metadata for each distinct source URL, deduped (caller passes
/// distinct URLs) and paced at [`CITOID_PACE`] (Wikimedia API etiquette — the
/// shared service expects ~1 req/s). Best-effort: a URL Citoid cannot resolve is
/// simply absent from the map, never an error.
async fn fetch_page_metadata(
    client: &reqwest::Client,
    urls: Vec<String>,
) -> std::collections::HashMap<String, CitoidMetadata> {
    let mut metadata = std::collections::HashMap::new();
    for (index, url) in urls.iter().enumerate() {
        if index > 0 {
            tokio::time::sleep(CITOID_PACE).await;
        }
        if let Some(object) = fetch_citoid_object(client, None, url).await
            && let Some(header) = build_citoid_header(&object, url)
        {
            metadata.insert(url.clone(), header);
        }
    }
    metadata
}

/// Enumerate the revision's references, classify the bare ones, and build
/// proposals (or declines) for each — the testable core of the route.
///
/// # Errors
///
/// Returns the gate refusal for a wiki without `bare_url_citation`, or an
/// `editor-*` mapped error when reference enumeration fails. Per-reference
/// Citoid failures are **not** errors; they become declined entries.
pub(crate) async fn collect_bare_url_proposals(
    client: &reqwest::Client,
    citoid_base_override: Option<&str>,
    config: &sp42_core::WikiConfig,
    editor: &dyn WikitextEditor,
    access_date_iso: &str,
    pacing: Option<Duration>,
    request: &BareUrlProposalsRequest,
) -> Result<BareUrlProposalsResponse, ActionError> {
    let template = bare_url_template(config)?;
    let page = WikitextPageRef {
        title: request.title.clone(),
        rev_id: request.rev_id,
    };
    let descriptors = editor
        .enumerate_nodes(config, &page, WikitextNodeKind::Reference)
        .await
        .map_err(|error| action_error_from_editor(&error))?;

    let mut response = BareUrlProposalsResponse::default();
    for (index, reference) in bare_url_references(&descriptors).into_iter().enumerate() {
        if let Some(duration) = pacing.filter(|_| index > 0) {
            tokio::time::sleep(duration).await;
        }
        let raw = fetch_citoid_object(client, citoid_base_override, &reference.url).await;
        let metadata = raw
            .as_ref()
            .and_then(|object| build_citoid_header(object, &reference.url));
        let language = raw.as_ref().and_then(citoid_language);
        match render_bare_url_citation(
            &template,
            metadata.as_ref(),
            language.as_deref(),
            access_date_iso,
        ) {
            BareUrlOutcome::Proposed {
                replacement_wikitext,
            } => {
                response.proposals.push(BareUrlProposal {
                    locator: WikitextNodeLocator {
                        kind: WikitextNodeKind::Reference,
                        ordinal: reference.ordinal,
                        expected_text: reference.anchor_text.clone(),
                    },
                    url: reference.url,
                    current_anchor: reference.anchor_text,
                    replacement_wikitext,
                });
            }
            BareUrlOutcome::Declined { reason } => {
                response.declined.push(BareUrlDeclined {
                    ordinal: reference.ordinal,
                    url: reference.url,
                    reason,
                });
            }
        }
    }
    Ok(response)
}

/// `POST /dev/citation/bare-url-proposals` — read-only proposal generation.
///
/// Not session-gated: it performs only public reads (Parsoid enumeration and
/// Citoid metadata). The apply route is the authenticated, CSRF-checked path.
pub(crate) async fn post_bare_url_proposals(
    State(state): State<AppState>,
    Json(payload): Json<BareUrlProposalsRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let config = config_for_state_wiki(&state, &payload.wiki_id)?;
    let access_date = sp42_core::iso_date_from_epoch_ms(state.clock.now_ms());
    let response = collect_bare_url_proposals(
        &state.http_client,
        None,
        &config,
        state.wikitext_editor.as_ref(),
        &access_date,
        Some(CITOID_PACE),
        &payload,
    )
    .await
    .map_err(|error| action_error_response(&error))?;
    Ok(Json(response))
}

/// Resolve a page's current revision id via the wiki action API (a public read).
/// Used when a verify-page request leaves `rev_id` at `0` ("latest").
async fn fetch_latest_revid(
    client: &(impl HttpClient + ?Sized),
    config: &sp42_core::WikiConfig,
    title: &str,
) -> Result<u64, ActionError> {
    let mut url = config.api_url.clone();
    url.query_pairs_mut()
        .append_pair("action", "query")
        .append_pair("prop", "revisions")
        .append_pair("titles", title)
        .append_pair("rvprop", "ids")
        .append_pair("rvlimit", "1")
        .append_pair("format", "json")
        .append_pair("formatversion", "2");

    let response = client
        .execute(HttpRequest {
            method: HttpMethod::Get,
            url,
            headers: std::collections::BTreeMap::new(),
            body: Vec::new(),
        })
        .await
        .map_err(|error| ActionError::Execution {
            message: format!("latest-revision lookup failed: {error}"),
            code: None,
            http_status: None,
            retryable: true,
        })?;

    if !(200..300).contains(&response.status) {
        return Err(ActionError::Execution {
            message: format!("latest-revision lookup HTTP {}", response.status),
            code: None,
            http_status: Some(response.status),
            retryable: response.status >= 500,
        });
    }

    let value: serde_json::Value =
        serde_json::from_slice(&response.body).map_err(|error| ActionError::Execution {
            message: format!("latest-revision JSON failed: {error}"),
            code: None,
            http_status: None,
            retryable: false,
        })?;

    value
        .pointer("/query/pages/0/revisions/0/revid")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| ActionError::Execution {
            message: format!("no current revision found for: {title}"),
            code: Some("missing-revision".to_string()),
            http_status: None,
            retryable: false,
        })
}

/// `POST /dev/citation/verify-page` — read-only page-level citation verification.
///
/// Session + CSRF gated (like `post_bare_url_apply`). The route only reads from the
/// wiki, but it spends the server's `SP42_INFERENCE_*` credentials on a
/// caller-chosen page — so it is *not* equivalent to the credential-free proposal
/// route and must not be reachable unauthenticated once the server is bound beyond
/// loopback (ADR-0011 §5). The gate runs before any inference client is built.
///
/// Per-request inference edge from env (dev route). Resolves wiki config, then
/// builds a model panel and client from inference environment variables. Extracts
/// blocks via the editor (Parsoid), then use-sites, then runs `verify_page`, and
/// returns a `PageVerificationReport`.
pub(crate) async fn post_verify_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(mut payload): Json<PageVerificationRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let Some(session) = current_session_snapshot(&state, &headers, true).await else {
        return Err(unauthorized_error(
            "No authenticated bridge session is active.",
        ));
    };
    validate_csrf_header(&headers, &session)?;
    let config = config_for_state_wiki(&state, &payload.wiki_id)?;

    // Accept a pasted wiki URL in `title`, not just a bare page title — the action
    // API treats a URL as a literal (missing) title. A bare title parses to itself
    // (idempotent with any client-side parse); an oldid URL also yields a revision,
    // honored only when the request did not already pin one.
    let target = sp42_core::parse_page_target(&payload.title);
    payload.title = target.title;
    if payload.rev_id == 0 {
        payload.rev_id = target.rev_id;
    }

    // Per-request inference edge from env (dev route).
    let panel = sp42_inference::panel_from_env().map_err(|error| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": error })),
        )
    })?;
    let model_client = sp42_inference::client_from_env().map_err(|error| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": error })),
        )
    })?;

    let http_client = PlainHttpClient::new().map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error })),
        )
    })?;

    // `rev_id == 0` means "latest": resolve it to a concrete revision before
    // verifying, so the report records the exact revision that was checked. Use the
    // trusted wiki-API client (not the SSRF-guarded source client) so this works
    // for loopback/private wiki configs the same way a pinned --rev does.
    if payload.rev_id == 0 {
        let wiki_client =
            BearerHttpClient::new(state.http_client.clone(), session.access_token.clone());
        payload.rev_id = fetch_latest_revid(&wiki_client, &config, &payload.title)
            .await
            .map_err(|error| action_error_response(&error))?;
    }

    // Extract blocks via the editor (Parsoid), then use-sites, then verify.
    let page_ref = WikitextPageRef {
        title: payload.title.clone(),
        rev_id: payload.rev_id,
    };
    let blocks = state
        .wikitext_editor
        .extract_blocks(&config, &page_ref)
        .await
        .map_err(|error| action_error_response(&action_error_from_editor(&error)))?;
    let extract = extract_use_sites(&blocks, &payload);

    // Distinct live source URLs for deduped Citoid metadata lookups.
    let metadata_urls: Vec<String> = {
        let mut seen = std::collections::HashSet::new();
        extract
            .use_sites
            .iter()
            .map(|use_site| use_site.request.source_url.to_string())
            .filter(|url| seen.insert(url.clone()))
            .collect()
    };

    // Verify the page and fetch source metadata concurrently. Metadata is a display
    // sidecar (never grounded); it is fetched deduped + paced (CITOID_PACE) so a
    // citation-heavy page does not burst the shared Citoid service, and the pacing
    // latency overlaps the model calls rather than adding to them.
    let options = VerifyOptions::default();
    let verify = verify_page(
        &http_client,
        &model_client,
        state.clock.as_ref(),
        &panel,
        &payload,
        extract,
        options,
        PAGE_VERIFY_CONCURRENCY,
    );
    let metadata = fetch_page_metadata(&state.http_client, metadata_urls);
    let (mut report, metadata_by_url) = tokio::join!(verify, metadata);

    for finding in &mut report.findings {
        if let Some(meta) = metadata_by_url.get(&finding.provenance.url.to_string()) {
            finding.metadata = Some(meta.clone());
        }
    }

    Ok(Json(report))
}

/// Replay one proposal verbatim: gate → CSRF token → node-anchored replace
/// (anti-drift re-check inside the editor) → `baserevid`-guarded save →
/// patrol of the original revision. Mirrors `execute_inline_edit_action`.
///
/// # Errors
///
/// `bare-url-repair-not-enabled` (gate, before any wiki traffic),
/// `editor-*` codes, `node-drift` / `node-out-of-range` (409-in-body, zero
/// wiki writes), or upstream save failures.
pub(crate) async fn execute_bare_url_apply(
    client: &BearerHttpClient,
    config: &sp42_core::WikiConfig,
    payload: &BareUrlApplyRequest,
    editor: &dyn WikitextEditor,
) -> Result<HttpResponse, ActionError> {
    bare_url_template(config)?;
    let token = execute_fetch_token(client, config, TokenKind::Csrf).await?;
    let updated_text = replace_node_or_refuse(
        config,
        &payload.title,
        payload.rev_id,
        &payload.locator,
        &payload.replacement_wikitext,
        editor,
    )
    .await?;
    let summary = payload
        .summary
        .clone()
        .filter(|summary| !summary.trim().is_empty())
        .unwrap_or_else(|| BARE_URL_DEFAULT_SUMMARY.to_string());
    let save_response = execute_wiki_page_save(
        client,
        config,
        &WikiPageSaveRequest {
            title: payload.title.clone(),
            text: updated_text,
            token,
            summary: Some(summary),
            baserevid: Some(payload.rev_id),
            tags: Vec::new(),
            watchlist: None,
            create_only: FlagState::Disabled,
            minor: FlagState::Disabled,
        },
    )
    .await?;
    patrol_original_edit_if_possible(client, config, payload.rev_id).await;
    Ok(save_response)
}

/// Gate that checks edit capability from a capability report.
/// Returns Ok if `editing.can_edit` is true, or a 403 error otherwise.
fn ensure_bare_url_edit_capability(
    capabilities: &sp42_core::DevAuthCapabilityReport,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if !capabilities.capabilities.editing.can_edit {
        return Err(forbidden_error(
            "The authenticated session does not currently have edit capability on this wiki.",
        ));
    }
    Ok(())
}

/// `POST /dev/citation/bare-url-apply` — the operator-confirmed write path.
/// Session + CSRF gated exactly like `post_execute_action` (ADR-0002).
pub(crate) async fn post_bare_url_apply(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<BareUrlApplyRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let Some(session) = current_session_snapshot(&state, &headers, true).await else {
        return Err(unauthorized_error(
            "No authenticated bridge session is active.",
        ));
    };
    validate_csrf_header(&headers, &session)?;
    let capabilities =
        crate::capability_report_for_session(&state, &session, &payload.wiki_id, false).await;
    ensure_bare_url_edit_capability(&capabilities)?;
    let config = config_for_state_wiki(&state, &payload.wiki_id)?;
    let client = BearerHttpClient::new(state.http_client.clone(), session.access_token.clone());
    let response =
        execute_bare_url_apply(&client, &config, &payload, state.wikitext_editor.as_ref())
            .await
            .map_err(|error| action_error_response(&error))?;
    let summary = parse_action_response_summary(&response, "bare-url-repair")
        .map_err(|error| action_error_response(&error))?;
    Ok((
        StatusCode::OK,
        Json(bare_url_apply_response(
            &payload,
            session.username.clone(),
            &response,
            &summary,
        )),
    ))
}

/// The execute-action outcome shape (minus session-action `kind`) for one
/// applied bare-URL repair — mirrors `action_response_payload`.
fn bare_url_apply_response(
    payload: &BareUrlApplyRequest,
    actor: String,
    response: &HttpResponse,
    summary: &ActionResponseSummary,
) -> BareUrlApplyResponse {
    let mut warnings = summary.warnings.clone();
    if summary.nochange {
        warnings.push("no change — the edit may have already been reverted".to_string());
    }
    BareUrlApplyResponse {
        wiki_id: payload.wiki_id.clone(),
        rev_id: payload.rev_id,
        accepted: !summary.nochange,
        actor: Some(actor),
        http_status: Some(response.status),
        api_code: summary.api_code.clone(),
        retryable: summary.retryable,
        warnings,
        result: summary.result.clone(),
        message: if summary.nochange {
            Some("no change — the edit may have already been reverted".to_string())
        } else {
            Some(format!("MediaWiki HTTP {}", response.status))
        },
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use axum::http::StatusCode;
    use sp42_core::{
        ActionError, BareUrlDeclineReason, BareUrlProposalsRequest, DevAuthCapabilityReport,
        DevAuthDerivedCapabilities, DevAuthEditCapabilities, ScriptedWikitextEditor,
        ScriptedWikitextNode, WikitextNodeKind,
    };

    use super::{bare_url_template, collect_bare_url_proposals, ensure_bare_url_edit_capability};

    fn disabled_config() -> sp42_core::WikiConfig {
        sp42_wiki::WikiRegistry::embedded_default()
            .expect("embedded wiki registry should load")
            .default_config()
    }

    fn enabled_config() -> sp42_core::WikiConfig {
        let mut config = disabled_config();
        config.templates.bare_url_citation = Some("cite web".to_string());
        config
    }

    fn reference(anchor: &str) -> ScriptedWikitextNode {
        ScriptedWikitextNode {
            kind: WikitextNodeKind::Reference,
            anchor_text: anchor.to_string(),
        }
    }

    fn proposals_request() -> BareUrlProposalsRequest {
        BareUrlProposalsRequest {
            wiki_id: "frwiki".to_string(),
            title: "Exemple".to_string(),
            rev_id: 42,
        }
    }

    struct MockCitoid {
        base_url: String,
        requests: Arc<AtomicUsize>,
    }

    /// Ephemeral Citoid stand-in: each `(needle, status, body)` rule matches
    /// when the request path contains `needle` (URL-encoded source URLs keep
    /// their host readable, so host fragments make good needles).
    async fn spawn_mock_citoid(rules: Vec<(&'static str, u16, &'static str)>) -> MockCitoid {
        let requests = Arc::new(AtomicUsize::new(0));
        let counter = requests.clone();
        let handler = move |request: axum::extract::Request| {
            let counter = counter.clone();
            let rules = rules.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                let path = request.uri().path().to_string();
                for (needle, status, body) in rules {
                    if path.contains(needle) {
                        return axum::response::Response::builder()
                            .status(status)
                            .header("content-type", "application/json")
                            .body(axum::body::Body::from(body))
                            .expect("mock response should build");
                    }
                }
                axum::response::Response::builder()
                    .status(404)
                    .body(axum::body::Body::from(format!("unmocked path: {path}")))
                    .expect("mock response should build")
            }
        };
        let app = axum::Router::new().fallback(handler);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("mock citoid should bind");
        let addr = listener
            .local_addr()
            .expect("mock citoid should expose addr");
        tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("mock citoid should serve");
        });
        MockCitoid {
            base_url: format!("http://{addr}"),
            requests,
        }
    }

    fn test_client() -> reqwest::Client {
        reqwest::Client::builder()
            .user_agent(sp42_core::branding::USER_AGENT)
            .build()
            .expect("reqwest client should build")
    }

    #[test]
    fn gate_yields_the_configured_template() {
        assert_eq!(
            bare_url_template(&enabled_config()).expect("enabled config should pass the gate"),
            "cite web"
        );
        let error = bare_url_template(&disabled_config())
            .expect_err("config without bare_url_citation must refuse");
        let ActionError::Execution {
            code,
            http_status,
            retryable,
            ..
        } = error;
        assert_eq!(code.as_deref(), Some("bare-url-repair-not-enabled"));
        assert_eq!(http_status, Some(400));
        assert!(!retryable);
    }

    #[tokio::test]
    async fn proposals_target_each_bare_reference_including_duplicates() {
        let citoid = spawn_mock_citoid(vec![(
            "example.org",
            200,
            include_str!("../../../fixtures/citoid/basic.json"),
        )])
        .await;
        let editor = ScriptedWikitextEditor::new(
            vec![
                reference("https://example.org/article"),
                reference("Prose citation"),
                reference("https://example.org/article"),
            ],
            String::new(),
        );

        let response = collect_bare_url_proposals(
            &test_client(),
            Some(&citoid.base_url),
            &enabled_config(),
            &editor,
            "2026-06-09",
            None,
            &proposals_request(),
        )
        .await
        .expect("proposals should collect");

        assert_eq!(response.declined.len(), 0);
        assert_eq!(
            response.proposals.len(),
            2,
            "duplicate URLs must each get a proposal"
        );
        assert_eq!(response.proposals[0].locator.ordinal, 0);
        assert_eq!(response.proposals[1].locator.ordinal, 2);
        assert_eq!(
            response.proposals[1].locator.expected_text,
            "https://example.org/article"
        );
        assert_eq!(
            response.proposals[0].current_anchor,
            "https://example.org/article"
        );
        assert!(
            response.proposals[0]
                .replacement_wikitext
                .contains("|title=Headline")
        );
        assert!(
            response.proposals[0]
                .replacement_wikitext
                .contains("|access-date=2026-06-09")
        );
        assert_eq!(
            citoid.requests.load(Ordering::SeqCst),
            2,
            "one fetch per bare reference"
        );
    }

    #[tokio::test]
    async fn citoid_failure_declines_only_the_affected_reference() {
        let citoid = spawn_mock_citoid(vec![
            (
                "ok.example",
                200,
                include_str!("../../../fixtures/citoid/basic.json"),
            ),
            ("fail.example", 520, "{}"),
        ])
        .await;
        let editor = ScriptedWikitextEditor::new(
            vec![
                reference("https://ok.example/a"),
                reference("https://fail.example/b"),
            ],
            String::new(),
        );

        let response = collect_bare_url_proposals(
            &test_client(),
            Some(&citoid.base_url),
            &enabled_config(),
            &editor,
            "2026-06-09",
            None,
            &proposals_request(),
        )
        .await
        .expect("a junk URL must not fail the whole response");

        assert_eq!(response.proposals.len(), 1);
        assert_eq!(response.proposals[0].locator.ordinal, 0);
        assert_eq!(response.declined.len(), 1);
        assert_eq!(response.declined[0].ordinal, 1);
        assert_eq!(response.declined[0].url, "https://fail.example/b");
        assert_eq!(
            response.declined[0].reason,
            BareUrlDeclineReason::MetadataUnavailable
        );
    }

    #[tokio::test]
    async fn degenerate_title_declines_as_no_usable_title() {
        let citoid = spawn_mock_citoid(vec![(
            "degenerate.example",
            200,
            include_str!("../../../fixtures/citoid/degenerate_title_url.json"),
        )])
        .await;
        let editor = ScriptedWikitextEditor::new(
            vec![reference("https://degenerate.example/x")],
            String::new(),
        );

        let response = collect_bare_url_proposals(
            &test_client(),
            Some(&citoid.base_url),
            &enabled_config(),
            &editor,
            "2026-06-09",
            None,
            &proposals_request(),
        )
        .await
        .expect("degenerate metadata should decline, not error");

        assert!(response.proposals.is_empty());
        assert_eq!(response.declined.len(), 1);
        assert_eq!(
            response.declined[0].reason,
            BareUrlDeclineReason::NoUsableTitle
        );
    }

    #[tokio::test]
    async fn gate_refusal_emits_no_citoid_traffic_and_leaves_editor_untouched() {
        let citoid = spawn_mock_citoid(vec![]).await;
        let editor = ScriptedWikitextEditor::new(
            vec![reference("https://example.org/article")],
            String::new(),
        );

        let error = collect_bare_url_proposals(
            &test_client(),
            Some(&citoid.base_url),
            &disabled_config(),
            &editor,
            "2026-06-09",
            None,
            &proposals_request(),
        )
        .await
        .expect_err("disabled wiki must refuse");

        let ActionError::Execution { code, .. } = error;
        assert_eq!(code.as_deref(), Some("bare-url-repair-not-enabled"));
        assert_eq!(
            citoid.requests.load(Ordering::SeqCst),
            0,
            "no Citoid traffic on refusal"
        );
        assert!(
            editor.invocations().is_empty(),
            "gate refusal must not invoke editor"
        );
    }

    #[tokio::test]
    async fn editor_errors_map_to_editor_codes() {
        struct FailingEditor;

        #[async_trait::async_trait]
        impl sp42_core::WikitextEditor for FailingEditor {
            async fn enumerate_nodes(
                &self,
                _config: &sp42_core::WikiConfig,
                _page: &sp42_core::WikitextPageRef,
                _kind: WikitextNodeKind,
            ) -> Result<Vec<sp42_core::WikitextNodeDescriptor>, sp42_core::WikitextEditorError>
            {
                Err(sp42_core::WikitextEditorError::NotConfigured {
                    wiki_id: "frwiki".to_string(),
                })
            }

            async fn replace_node(
                &self,
                _config: &sp42_core::WikiConfig,
                _page: &sp42_core::WikitextPageRef,
                _locator: &sp42_core::WikitextNodeLocator,
                _replacement_wikitext: &str,
            ) -> Result<sp42_core::WikitextEditOutcome, sp42_core::WikitextEditorError>
            {
                unreachable!("proposal collection never replaces nodes")
            }

            async fn set_template_params(
                &self,
                _config: &sp42_core::WikiConfig,
                _page: &sp42_core::WikitextPageRef,
                _locator: &sp42_core::WikitextNodeLocator,
                _params: &[(String, String)],
            ) -> Result<sp42_core::WikitextEditOutcome, sp42_core::WikitextEditorError>
            {
                unreachable!("proposal collection never sets template params")
            }
        }

        let error = collect_bare_url_proposals(
            &test_client(),
            None,
            &enabled_config(),
            &FailingEditor,
            "2026-06-09",
            None,
            &proposals_request(),
        )
        .await
        .expect_err("editor failure must surface");

        let ActionError::Execution { code, .. } = error;
        assert_eq!(code.as_deref(), Some("editor-not-configured"));
    }

    #[test]
    fn ensure_bare_url_edit_capability_accepts_when_can_edit_true() {
        let report = DevAuthCapabilityReport {
            checked: true,
            wiki_id: "frwiki".to_string(),
            capabilities: DevAuthDerivedCapabilities {
                editing: DevAuthEditCapabilities {
                    can_edit: true,
                    can_undo: false,
                },
                ..Default::default()
            },
            ..Default::default()
        };

        let result = ensure_bare_url_edit_capability(&report);

        assert!(result.is_ok());
    }

    #[test]
    fn ensure_bare_url_edit_capability_refuses_when_can_edit_false() {
        let report = DevAuthCapabilityReport {
            checked: true,
            wiki_id: "frwiki".to_string(),
            capabilities: DevAuthDerivedCapabilities {
                editing: DevAuthEditCapabilities {
                    can_edit: false,
                    can_undo: false,
                },
                ..Default::default()
            },
            ..Default::default()
        };

        let result = ensure_bare_url_edit_capability(&report);

        assert!(result.is_err());
        let (status, body) = result.unwrap_err();
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(
            body.0
                .get("error")
                .and_then(|v| v.as_str())
                .is_some_and(|s| s.contains("edit capability")),
            "error message should mention edit capability"
        );
    }
}
