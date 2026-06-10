//! Bare-URL repair bridge routes (PRD-0008): propose and (Phase 5) apply.
//!
//! FCIS: classification and rendering are pure `sp42_core::bare_url_repair`
//! calls; this module owns the imperative edges — the per-wiki gate, the
//! sequential Citoid fetch (1 req/s etiquette), and the route glue. Citoid
//! failures decline the affected reference; they never fail the response.

use std::time::Duration;

use sp42_core::{
    ActionError, BareUrlDeclined, BareUrlOutcome, BareUrlProposal, BareUrlProposalsRequest,
    BareUrlProposalsResponse, WikitextEditor, WikitextNodeKind, WikitextNodeLocator,
    WikitextPageRef, bare_url_references, build_citoid_header, build_citoid_request,
    citoid_language, render_bare_url_citation,
};

use crate::action_routes::action_error_from_editor;

/// Citoid etiquette: at most one request per second on the live service.
#[allow(dead_code)]
const CITOID_PACE: Duration = Duration::from_secs(1);

/// The configured bare-URL citation template, or the per-wiki gate refusal.
///
/// Presence of `templates.bare_url_citation` is the whole gate (PRD-0008):
/// a wiki without it (every production config) refuses before any wiki or
/// Citoid traffic.
#[allow(dead_code)]
fn bare_url_template(config: &sp42_core::WikiConfig) -> Result<String, ActionError> {
    config.templates.bare_url_citation.clone().ok_or_else(|| ActionError::Execution {
        message: format!(
            "bare-URL repair is not enabled for wiki {}",
            config.wiki_id
        ),
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
#[allow(dead_code)]
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

/// Enumerate the revision's references, classify the bare ones, and build
/// proposals (or declines) for each — the testable core of the route.
///
/// # Errors
///
/// Returns the gate refusal for a wiki without `bare_url_citation`, or an
/// `editor-*` mapped error when reference enumeration fails. Per-reference
/// Citoid failures are **not** errors; they become declined entries.
#[allow(dead_code)]
pub(crate) async fn collect_bare_url_proposals(
    client: &reqwest::Client,
    citoid_base_override: Option<&str>,
    config: &sp42_core::WikiConfig,
    editor: &dyn WikitextEditor,
    access_date_iso: &str,
    pace: Option<Duration>,
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
        if let Some(pace) = pace.filter(|_| index > 0) {
            tokio::time::sleep(pace).await;
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
            BareUrlOutcome::Proposed { replacement_wikitext } => {
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use sp42_core::{
        ActionError, BareUrlDeclineReason, BareUrlProposalsRequest, ScriptedWikitextEditor,
        ScriptedWikitextNode, WikitextNodeKind,
    };

    use super::{bare_url_template, collect_bare_url_proposals};

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
        let addr = listener.local_addr().expect("mock citoid should expose addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("mock citoid should serve");
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
        let ActionError::Execution { code, http_status, retryable, .. } = error;
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
        assert_eq!(response.proposals.len(), 2, "duplicate URLs must each get a proposal");
        assert_eq!(response.proposals[0].locator.ordinal, 0);
        assert_eq!(response.proposals[1].locator.ordinal, 2);
        assert_eq!(
            response.proposals[1].locator.expected_text,
            "https://example.org/article"
        );
        assert_eq!(response.proposals[0].current_anchor, "https://example.org/article");
        assert!(response.proposals[0].replacement_wikitext.contains("|title=Headline"));
        assert!(response.proposals[0].replacement_wikitext.contains("|access-date=2026-06-09"));
        assert_eq!(citoid.requests.load(Ordering::SeqCst), 2, "one fetch per bare reference");
    }

    #[tokio::test]
    async fn citoid_failure_declines_only_the_affected_reference() {
        let citoid = spawn_mock_citoid(vec![
            ("ok.example", 200, include_str!("../../../fixtures/citoid/basic.json")),
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
        assert_eq!(response.declined[0].reason, BareUrlDeclineReason::MetadataUnavailable);
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
        assert_eq!(response.declined[0].reason, BareUrlDeclineReason::NoUsableTitle);
    }

    #[tokio::test]
    async fn gate_refusal_touches_neither_editor_backend_nor_citoid() {
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
        assert_eq!(citoid.requests.load(Ordering::SeqCst), 0, "no Citoid traffic on refusal");
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
}
