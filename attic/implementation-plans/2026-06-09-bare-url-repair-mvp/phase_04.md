# Bare-URL Repair MVP Implementation Plan — Phase 4: Proposal route

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** A new gated server route `POST /dev/citation/bare-url-proposals` that enumerates a revision's references, identifies bare ones, fetches Citoid metadata sequentially, and returns `{proposals, declined}` — with per-reference Citoid failures degrading to declined entries, never errors.

**Architecture:** New module `crates/sp42-server/src/citation_routes.rs` (deliberately separate from the ~900-line `action_routes.rs`). The testable core is `collect_bare_url_proposals(...)`, which takes the reqwest client, an explicit Citoid base-URL override, the wiki config, the editor, the access date, and a pacing knob as **explicit parameters** — tests call it directly with a mock Citoid server and `pace: None` (the house pattern: `execute_inline_edit_action` tests call the function, not the router; env-var overrides would race under parallel `cargo test`). The axum handler is thin glue that passes production values (`None` override → the lifted client's canonical `en.wikipedia.org` URL; `Some(1s)` pacing for the documented 1 req/s Citoid etiquette).

**Auth note (decision):** the proposals route performs **only public reads** (Parsoid enumeration + Citoid fetch) and is **not session-gated**, like the other read-only dev-bridge GETs; the apply route (Phase 5) is fully session+CSRF-gated like `post_execute_action`. This also lets the router-level gate test run without session scaffolding.

**Error contract:** gate refusal and editor failures reuse `ActionError::Execution` → `action_error_response` (400 with `code` in body): `bare-url-repair-not-enabled` for the gate, `editor-*` codes via the existing `action_error_from_editor`.

**Tech Stack:** axum 0.8-style routing (existing `routes.rs` patterns), reqwest (`state.http_client` already carries `branding::USER_AGENT` + 5s timeout), tokio. Tests: `ScriptedWikitextEditor` (sp42-core) + an ephemeral axum mock Citoid (the `spawn_mock_parsoid` pattern), `tower::ServiceExt::oneshot` for the router-level test.

**Scope:** Phase 4 of 7. Depends on Phases 1–3.

**Codebase verified:** 2026-06-09, branch `louie/bare-url-repair` @ `2ed57b3`.

---

**Working directory for every command:** `/var/home/louie/Projects/Volunteering-Consulting/SP42/.worktrees/bare-url-repair`

**Existing machinery you will reuse (verified):**

- `crate::config_for_state_wiki(&state, wiki_id)` (defined in `main.rs:1114`) resolves a wiki config or returns a ready `(StatusCode, Json)` error.
- `crate::action_routes::action_error_from_editor(&WikitextEditorError) -> ActionError` (action_routes.rs:454) and `crate::action_routes::action_error_response(&ActionError) -> (StatusCode, Json<serde_json::Value>)` (action_routes.rs:914) — both already `pub(crate)`.
- `AppState` fields: `http_client: reqwest::Client`, `clock: Arc<dyn Clock>` (`.now_ms()`), `wikitext_editor: Arc<dyn WikitextEditor>` (state.rs:28–47).
- `WikitextEditor::enumerate_nodes(&self, config, page: &WikitextPageRef, kind) -> Result<Vec<WikitextNodeDescriptor>, WikitextEditorError>`.
- The lifted `build_citoid_request(source_url)` returns an `HttpRequest` whose `url` is `https://en.wikipedia.org/api/rest_v1/data/citation/mediawiki-basefields/{encoded}`; for tests the helper swaps the base for the mock's `http://127.0.0.1:{port}` while keeping the canonical path.
- Route registration: `dev_bridge_routes()` in `crates/sp42-server/src/routes.rs` (lines ~183–210); path constants live in `crates/sp42-core/src/routes.rs` (imported in the server as `route_contracts`).
- `ScriptedWikitextEditor::new(nodes: Vec<ScriptedWikitextNode>, serialized_wikitext: String)`; `ScriptedWikitextNode { kind, anchor_text }`; ordinals are assigned per-kind in vector order.
- Test config: `sp42_wiki::WikiRegistry::embedded_default().expect(...).default_config()` (frwiki-shaped, **no** `bare_url_citation` — the disabled state); enable by setting `config.templates.bare_url_citation = Some("cite web".to_string())` (the `config_with_parsoid` mutation pattern from parsoid_editor.rs:649).

### Task 1: Route path constants in `sp42-core`

**Files:**
- Modify: `crates/sp42-core/src/routes.rs`

**Step 1: Add the constants**

After the existing dev constants (line 78 area):

```rust
pub const DEV_ACTION_EXECUTE_PATH: &str = "/dev/actions/execute";
```

add:

```rust
pub const DEV_CITATION_BARE_URL_PROPOSALS_PATH: &str = "/dev/citation/bare-url-proposals";
pub const DEV_CITATION_BARE_URL_APPLY_PATH: &str = "/dev/citation/bare-url-apply";
```

(The apply constant is registered in Phase 5; declaring both here keeps the wire contract in one commit. `pub` consts don't trip dead-code.)

**Step 2: Verify**

```bash
cargo check -p sp42-core
```

**Step 3: Commit**

```bash
git add crates/sp42-core/src/routes.rs
git commit -m "feat: add bare-url citation route path contracts"
```

### Task 2: `citation_routes.rs` — gate + collect core (test-driven)

**Files:**
- Create: `crates/sp42-server/src/citation_routes.rs`
- Modify: `crates/sp42-server/src/main.rs` (add `mod citation_routes;` alongside the other route-module declarations, alphabetically — after the `mod` for auth/action routes; match the existing list's style)

**Step 1: Write the failing tests**

Create `crates/sp42-server/src/citation_routes.rs` with the module doc, the test module, and stubs to be filled in Step 3. Tests first:

```rust
//! Bare-URL repair bridge routes (PRD-0008): propose and (Phase 5) apply.
//!
//! FCIS: classification and rendering are pure `sp42_core::bare_url_repair`
//! calls; this module owns the imperative edges — the per-wiki gate, the
//! sequential Citoid fetch (1 req/s etiquette), and the route glue. Citoid
//! failures decline the affected reference; they never fail the response.

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
```

**Note:** `WikitextEditorError::NotConfigured { wiki_id: String }` is the verified variant shape (wikitext_editor.rs:154–157). Confirm the `WikitextEditor` trait's exact method signatures in the same file and mirror them in `FailingEditor` (the trait has exactly the three methods shown at wikitext_editor.rs:281–333).

Add the module declaration in `crates/sp42-server/src/main.rs`: find the block of `mod` declarations (e.g. `mod action_routes;` …) and insert `mod citation_routes;` in alphabetical position.

**Step 2: Run to verify failure**

```bash
cargo test -p sp42-server citation_routes
```

Expected: compile errors — `bare_url_template` and `collect_bare_url_proposals` not found.

**Step 3: Implement the core**

Add above the tests module in `citation_routes.rs`:

```rust
use std::time::Duration;

use sp42_core::{
    ActionError, BareUrlDeclined, BareUrlOutcome, BareUrlProposal, BareUrlProposalsRequest,
    BareUrlProposalsResponse, WikitextEditor, WikitextNodeKind, WikitextNodeLocator,
    WikitextPageRef, bare_url_references, build_citoid_header, build_citoid_request,
    citoid_language, render_bare_url_citation,
};

use crate::action_routes::action_error_from_editor;

/// Citoid etiquette: at most one request per second on the live service.
const CITOID_PACE: Duration = Duration::from_secs(1);

/// The configured bare-URL citation template, or the per-wiki gate refusal.
///
/// Presence of `templates.bare_url_citation` is the whole gate (PRD-0008):
/// a wiki without it (every production config) refuses before any wiki or
/// Citoid traffic.
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
```

Semantics note (matches Phase 3 and the design's decline rules): a 200 response whose object yields no meaningful header at all (`build_citoid_header → None`) declines as `metadata-unavailable`; a 200 with *some* fields but no usable title declines as `no-usable-title`.

**Step 4: Run the tests to verify they pass**

```bash
cargo test -p sp42-server citation_routes
```

Expected: 6 tests pass.

**Step 5: Commit**

```bash
git add crates/sp42-server/src/citation_routes.rs crates/sp42-server/src/main.rs
git commit -m "feat: add bare-url proposal collection with per-wiki gate"
```

### Task 3: Handler + route registration

**Files:**
- Modify: `crates/sp42-server/src/citation_routes.rs` (handler)
- Modify: `crates/sp42-server/src/routes.rs` (registration)
- Test: `crates/sp42-server/src/tests.rs` (router-level gate test)

**Step 1: Write the failing router-level test**

In `crates/sp42-server/src/tests.rs`, near the other dev-route tests (the `put_session_is_disabled_for_single_user_local_token_path` area), add:

```rust
#[tokio::test]
async fn bare_url_proposals_route_is_registered_and_gated() {
    let router = build_router(test_state());
    let response = router
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/dev/citation/bare-url-proposals")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "wiki_id": "frwiki",
                        "title": "Exemple",
                        "rev_id": 42
                    })
                    .to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should complete");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should read");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("body should be json");
    assert_eq!(json["code"], "bare-url-repair-not-enabled");
}
```

(The registry default config is frwiki-shaped with no `bare_url_citation`, so the gate refuses before any editor/Citoid traffic — this proves registration, request deserialization, and the gate end-to-end with zero auth scaffolding.)

**Step 2: Run to verify failure**

```bash
cargo test -p sp42-server bare_url_proposals_route_is_registered_and_gated
```

Expected: FAIL — the route does not exist yet (404/405 instead of 400).

**Step 3: Implement the handler and register the route**

In `citation_routes.rs`, add the axum imports and handler:

```rust
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};

use crate::action_routes::action_error_response;
use crate::config_for_state_wiki;
use crate::state::AppState;
```

(Merge these into the existing `use` lines at the top of the file in house style — one `use crate::action_routes::{action_error_from_editor, action_error_response};` line.)

```rust
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
```

In `crates/sp42-server/src/routes.rs`:
- import the handler at the top with the other route imports: `use crate::citation_routes::post_bare_url_proposals;`
- in `dev_bridge_routes()`, after the `DEV_ACTION_EXECUTE_PATH` route entry, add:

```rust
        .route(
            route_contracts::DEV_CITATION_BARE_URL_PROPOSALS_PATH,
            axum::routing::post(post_bare_url_proposals),
        )
```

**Step 4: Run the tests to verify they pass**

```bash
cargo test -p sp42-server bare_url
```

Expected: all bare-url tests pass (Task 2's six + this router test).

**Step 5: Full crate verification**

```bash
cargo test -p sp42-server
cargo clippy -p sp42-server --all-targets --all-features -- -D warnings
```

Expected: green.

**Step 6: Commit**

```bash
git add crates/sp42-server/src/citation_routes.rs crates/sp42-server/src/routes.rs crates/sp42-server/src/tests.rs
git commit -m "feat: add gated bare-url-proposals bridge route"
```
