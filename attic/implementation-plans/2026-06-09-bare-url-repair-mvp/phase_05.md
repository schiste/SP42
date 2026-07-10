# Bare-URL Repair MVP Implementation Plan — Phase 5: Apply route

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** A gated, session+CSRF-authenticated `POST /dev/citation/bare-url-apply` that replays a proposal's locator + replacement **verbatim** through the ADR-0003 node-anchored editor: `baserevid`-guarded save, patrol of the original revision, default summary `"SP42: bare-URL repair"` (operator note wins), and drift / out-of-range / gate refusals with **zero wiki writes**.

**Architecture:** The design's "refactor to expose the shared core if needed" is needed and small: extract the replace-or-refuse body of `node_anchored_replacement` (action_routes.rs:420–452) into a `pub(crate) replace_node_or_refuse(...)` helper, and make `patrol_original_edit_if_possible` `pub(crate)`. The new `execute_bare_url_apply(...)` in `citation_routes.rs` then mirrors `execute_inline_edit_action` exactly: gate → CSRF token fetch → replace-or-refuse → save with `baserevid` → patrol. The handler is session+CSRF-gated like `post_execute_action` and returns the execute-action outcome shape minus the session-action `kind` (`BareUrlApplyResponse`, Phase 3) — adding a `SessionActionKind` variant would ripple through exhaustive matches across shells, including wasm-gated code invisible to host builds; deliberately avoided in the MVP. Action-history logging is likewise an acknowledged MVP omission (the PRD DoD does not require it).

**Tech Stack:** axum, `BearerHttpClient`, `execute_fetch_token` / `execute_wiki_page_save` / `parse_action_response_summary` from sp42-core. Tests: `spawn_mock_wiki_backend` + `ScriptedWikitextEditor` in `tests.rs` (the `inline_edit_with_locator_saves_editor_output` / `inline_edit_with_drifted_locator_refuses_without_saving` templates at tests.rs:2902–2973).

**Scope:** Phase 5 of 7. Depends on Phase 4 (module + gate exist) and ADR-0003 machinery (shipped).

**Codebase verified:** 2026-06-09, branch `louie/bare-url-repair` @ `2ed57b3`.

---

**Working directory for every command:** `/var/home/louie/Projects/Volunteering-Consulting/SP42/.worktrees/bare-url-repair`

### Task 1: Refactor — expose the node-replacement core (no behavior change)

**Files:**
- Modify: `crates/sp42-server/src/action_routes.rs:420-452, 486-502`

**Step 1: Extract `replace_node_or_refuse`**

`node_anchored_replacement` currently reads (action_routes.rs:420–452):

```rust
async fn node_anchored_replacement(
    config: &sp42_core::WikiConfig,
    title: &str,
    payload: &SessionActionExecutionRequest,
    locator: &WikitextNodeLocator,
    editor: &dyn WikitextEditor,
) -> Result<String, ActionError> {
    let Some(replacement) = payload.replacement_text.clone() else {
        return Err(ActionError::Execution {
            message: "replacement_text is required for node-anchored inline edit".to_string(),
            code: Some("invalid-input".to_string()),
            http_status: Some(400),
            retryable: false,
        });
    };
    let page = WikitextPageRef {
        title: title.to_string(),
        rev_id: payload.rev_id,
    };
    let outcome = editor
        .replace_node(config, &page, locator, &replacement)
        .await
        .map_err(|e| action_error_from_editor(&e))?;
    match outcome {
        WikitextEditOutcome::Applied { new_wikitext } => Ok(new_wikitext),
        WikitextEditOutcome::Refused(refusal) => Err(ActionError::Execution {
            message: refusal.message(),
            code: Some(refusal.code().to_string()),
            http_status: Some(409),
            retryable: false,
        }),
    }
}
```

Replace it with:

```rust
async fn node_anchored_replacement(
    config: &sp42_core::WikiConfig,
    title: &str,
    payload: &SessionActionExecutionRequest,
    locator: &WikitextNodeLocator,
    editor: &dyn WikitextEditor,
) -> Result<String, ActionError> {
    let Some(replacement) = payload.replacement_text.clone() else {
        return Err(ActionError::Execution {
            message: "replacement_text is required for node-anchored inline edit".to_string(),
            code: Some("invalid-input".to_string()),
            http_status: Some(400),
            retryable: false,
        });
    };
    replace_node_or_refuse(config, title, payload.rev_id, locator, &replacement, editor).await
}

/// Replace one node-anchored target, mapping editor failures to `editor-*`
/// codes and ADR-0003 refusals (drift / out-of-range) to a 409-in-body
/// `ActionError` — shared by inline edits and bare-URL repair (PRD-0008).
pub(crate) async fn replace_node_or_refuse(
    config: &sp42_core::WikiConfig,
    title: &str,
    rev_id: u64,
    locator: &WikitextNodeLocator,
    replacement: &str,
    editor: &dyn WikitextEditor,
) -> Result<String, ActionError> {
    let page = WikitextPageRef {
        title: title.to_string(),
        rev_id,
    };
    let outcome = editor
        .replace_node(config, &page, locator, replacement)
        .await
        .map_err(|e| action_error_from_editor(&e))?;
    match outcome {
        WikitextEditOutcome::Applied { new_wikitext } => Ok(new_wikitext),
        WikitextEditOutcome::Refused(refusal) => Err(ActionError::Execution {
            message: refusal.message(),
            code: Some(refusal.code().to_string()),
            http_status: Some(409),
            retryable: false,
        }),
    }
}
```

**Step 2: Widen patrol visibility**

At action_routes.rs:486, change:

```rust
async fn patrol_original_edit_if_possible(
```

to:

```rust
pub(crate) async fn patrol_original_edit_if_possible(
```

**Step 3: Verify no behavior change**

```bash
cargo test -p sp42-server
cargo clippy -p sp42-server --all-targets --all-features -- -D warnings
```

Expected: the existing inline-edit tests (`inline_edit_with_locator_saves_editor_output`, `inline_edit_with_drifted_locator_refuses_without_saving`, and the rest of the suite) pass unchanged.

**Step 4: Commit**

```bash
git add crates/sp42-server/src/action_routes.rs
git commit -m "refactor: expose node replacement core for reuse"
```

### Task 2: `execute_bare_url_apply` (test-driven)

**Files:**
- Modify: `crates/sp42-server/src/citation_routes.rs`
- Test: `crates/sp42-server/src/tests.rs` (MockWikiBackend lives there; these tests sit beside the inline-edit ones at tests.rs:2902+)

**Step 1: Write the failing tests**

Add to `crates/sp42-server/src/tests.rs`, after `inline_edit_with_drifted_locator_refuses_without_saving` (reuse the existing private helpers `spawn_mock_wiki_backend`, `wiki_config_for_backend`; note `wiki_config_for_backend` returns the registry default config — frwiki-shaped, gate **disabled** — so enabled tests set the template explicitly):

```rust
fn bare_url_apply_payload(summary: Option<&str>) -> sp42_core::BareUrlApplyRequest {
    sp42_core::BareUrlApplyRequest {
        wiki_id: "frwiki".to_string(),
        title: "Exemple".to_string(),
        rev_id: 42,
        locator: sp42_core::WikitextNodeLocator {
            kind: sp42_core::WikitextNodeKind::Reference,
            ordinal: 0,
            expected_text: "https://example.org/article".to_string(),
        },
        replacement_wikitext:
            "{{cite web |url=https://example.org/article |title=Headline |access-date=2026-06-09}}"
                .to_string(),
        summary: summary.map(ToString::to_string),
    }
}

fn bare_url_test_editor(anchor: &str) -> sp42_core::ScriptedWikitextEditor {
    sp42_core::ScriptedWikitextEditor::new(
        vec![sp42_core::ScriptedWikitextNode {
            kind: sp42_core::WikitextNodeKind::Reference,
            anchor_text: anchor.to_string(),
        }],
        "NEWPAGEWIKITEXT".to_string(),
    )
}

#[tokio::test]
async fn bare_url_apply_saves_exact_replacement_with_baserevid() {
    let backend = spawn_mock_wiki_backend("unused page text").await;
    let mut config = wiki_config_for_backend(&backend.base_url);
    config.templates.bare_url_citation = Some("cite web".to_string());
    let editor = bare_url_test_editor("https://example.org/article");
    let client = crate::runtime_adapters::BearerHttpClient::new(
        reqwest::Client::new(),
        "test-access-token".to_string(),
    );
    let payload = bare_url_apply_payload(None);

    let response =
        crate::citation_routes::execute_bare_url_apply(&client, &config, &payload, &editor)
            .await
            .expect("bare-url apply should succeed");

    assert_eq!(response.status, 200);
    let invocations = editor.invocations();
    assert_eq!(invocations.len(), 1);
    assert_eq!(invocations[0].operation, "replace_node");
    assert_eq!(
        invocations[0].payload, payload.replacement_wikitext,
        "the proposed replacement must be replayed verbatim"
    );
    let edits = backend.edit_bodies.lock().expect("mock edit log should lock");
    assert_eq!(edits.len(), 1, "exactly one save must reach the wiki");
    assert!(edits[0].contains("NEWPAGEWIKITEXT"), "save must carry the editor output: {}", edits[0]);
    assert!(edits[0].contains("baserevid=42"), "save must stay baserevid-guarded: {}", edits[0]);
    assert!(
        edits[0].contains("bare-URL+repair"),
        "default summary should be applied: {}",
        edits[0]
    );
}

#[tokio::test]
async fn bare_url_apply_operator_summary_wins() {
    let backend = spawn_mock_wiki_backend("unused page text").await;
    let mut config = wiki_config_for_backend(&backend.base_url);
    config.templates.bare_url_citation = Some("cite web".to_string());
    let editor = bare_url_test_editor("https://example.org/article");
    let client = crate::runtime_adapters::BearerHttpClient::new(
        reqwest::Client::new(),
        "test-access-token".to_string(),
    );
    let payload = bare_url_apply_payload(Some("fixed ref per talk"));

    crate::citation_routes::execute_bare_url_apply(&client, &config, &payload, &editor)
        .await
        .expect("bare-url apply should succeed");

    let edits = backend.edit_bodies.lock().expect("mock edit log should lock");
    assert_eq!(edits.len(), 1);
    assert!(
        edits[0].contains("fixed+ref+per+talk"),
        "operator note must win over the default summary: {}",
        edits[0]
    );
    assert!(!edits[0].contains("bare-URL+repair"), "default must not also apply: {}", edits[0]);
}

#[tokio::test]
async fn bare_url_apply_drift_refuses_with_zero_writes() {
    let backend = spawn_mock_wiki_backend("unused page text").await;
    let mut config = wiki_config_for_backend(&backend.base_url);
    config.templates.bare_url_citation = Some("cite web".to_string());
    let editor = bare_url_test_editor("https://example.org/SOMETHING-ELSE");
    let client = crate::runtime_adapters::BearerHttpClient::new(
        reqwest::Client::new(),
        "test-access-token".to_string(),
    );
    let payload = bare_url_apply_payload(None);

    let error = crate::citation_routes::execute_bare_url_apply(&client, &config, &payload, &editor)
        .await
        .expect_err("drifted anchor must refuse");

    let sp42_core::ActionError::Execution { code, http_status, retryable, .. } = error;
    assert_eq!(code.as_deref(), Some("node-drift"));
    assert_eq!(http_status, Some(409));
    assert!(!retryable);
    assert!(
        backend.edit_bodies.lock().expect("mock edit log should lock").is_empty(),
        "a refused apply must never reach the wiki"
    );
}

#[tokio::test]
async fn bare_url_apply_out_of_range_refuses_with_zero_writes() {
    let backend = spawn_mock_wiki_backend("unused page text").await;
    let mut config = wiki_config_for_backend(&backend.base_url);
    config.templates.bare_url_citation = Some("cite web".to_string());
    let editor = sp42_core::ScriptedWikitextEditor::new(Vec::new(), "NEVERUSED".to_string());
    let client = crate::runtime_adapters::BearerHttpClient::new(
        reqwest::Client::new(),
        "test-access-token".to_string(),
    );
    let payload = bare_url_apply_payload(None);

    let error = crate::citation_routes::execute_bare_url_apply(&client, &config, &payload, &editor)
        .await
        .expect_err("missing ordinal must refuse");

    let sp42_core::ActionError::Execution { code, http_status, .. } = error;
    assert_eq!(code.as_deref(), Some("node-out-of-range"));
    assert_eq!(http_status, Some(409));
    assert!(
        backend.edit_bodies.lock().expect("mock edit log should lock").is_empty(),
        "a refused apply must never reach the wiki"
    );
}

#[tokio::test]
async fn bare_url_apply_gate_refuses_with_zero_writes() {
    let backend = spawn_mock_wiki_backend("unused page text").await;
    let config = wiki_config_for_backend(&backend.base_url);
    let editor = bare_url_test_editor("https://example.org/article");
    let client = crate::runtime_adapters::BearerHttpClient::new(
        reqwest::Client::new(),
        "test-access-token".to_string(),
    );
    let payload = bare_url_apply_payload(None);

    let error = crate::citation_routes::execute_bare_url_apply(&client, &config, &payload, &editor)
        .await
        .expect_err("unconfigured wiki must refuse");

    let sp42_core::ActionError::Execution { code, .. } = error;
    assert_eq!(code.as_deref(), Some("bare-url-repair-not-enabled"));
    assert!(editor.invocations().is_empty(), "gate refusal must not touch the editor");
    assert!(
        backend.edit_bodies.lock().expect("mock edit log should lock").is_empty(),
        "gate refusal must not touch the wiki"
    );
}
```

**Step 2: Run to verify failure**

```bash
cargo test -p sp42-server bare_url_apply
```

Expected: compile error — `execute_bare_url_apply` not found in `citation_routes`.

**Step 3: Implement `execute_bare_url_apply`**

In `crates/sp42-server/src/citation_routes.rs`, extend imports (merge into the existing `use sp42_core::{...}` line): `BareUrlApplyRequest`, `FlagState`, `TokenKind`, `WikiPageSaveRequest`, `execute_fetch_token`, `execute_wiki_page_save`. Also `use sp42_types::HttpResponse;`, `use crate::runtime_adapters::BearerHttpClient;`, and extend the action_routes import to `use crate::action_routes::{action_error_from_editor, action_error_response, patrol_original_edit_if_possible, replace_node_or_refuse};`.

```rust
/// Default edit summary when the operator supplies no note.
const BARE_URL_DEFAULT_SUMMARY: &str = "SP42: bare-URL repair";

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
```

(Ordering note, verified against the gate DoD: the gate runs **before** the CSRF-token fetch, so a disabled wiki sees zero traffic of any kind. On drift, the token fetch and the editor's revision read have already happened — same as the shipped inline-edit path — but no *write* is issued; the tests assert `edit_bodies` stays empty.)

**Step 4: Run the tests to verify they pass**

```bash
cargo test -p sp42-server bare_url_apply
```

Expected: 5 tests pass.

**Step 5: Commit**

```bash
git add crates/sp42-server/src/citation_routes.rs crates/sp42-server/src/tests.rs
git commit -m "feat: add bare-url apply execution replaying proposals verbatim"
```

### Task 3: Authenticated handler + route registration

**Files:**
- Modify: `crates/sp42-server/src/citation_routes.rs` (handler + response mapping)
- Modify: `crates/sp42-server/src/routes.rs` (registration)
- Test: `crates/sp42-server/src/tests.rs` (auth-required router test)

**Step 1: Write the failing router-level test**

In `tests.rs`:

```rust
#[tokio::test]
async fn bare_url_apply_route_requires_a_session() {
    let router = build_router(test_state());
    let response = router
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/dev/citation/bare-url-apply")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "wiki_id": "frwiki",
                        "title": "Exemple",
                        "rev_id": 42,
                        "locator": {
                            "kind": "reference",
                            "ordinal": 0,
                            "expected_text": "https://example.org/article"
                        },
                        "replacement_wikitext": "{{cite web |url=https://example.org/article |title=T |access-date=2026-06-09}}"
                    })
                    .to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should complete");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
```

**Step 2: Run to verify failure**

```bash
cargo test -p sp42-server bare_url_apply_route_requires_a_session
```

Expected: FAIL — route not registered (404/405).

**Step 3: Implement the handler**

In `citation_routes.rs`, extend imports: `axum::http::HeaderMap`, `use sp42_core::{ActionResponseSummary, BareUrlApplyResponse, parse_action_response_summary};` (merged into existing lines), and `use crate::http_errors::unauthorized_error;`, `use crate::session_runtime::{current_session_snapshot, validate_csrf_header};`.

```rust
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
```

Check `parse_action_response_summary`'s exact signature in `crates/sp42-core/src/action_executor.rs` before wiring (the existing call site is `parse_action_response_summary(&response, payload.kind.label())` at action_routes.rs `handle_action_success` — same two-argument shape with a `&str` label).

In `routes.rs`, `dev_bridge_routes()`, after the proposals route entry:

```rust
        .route(
            route_contracts::DEV_CITATION_BARE_URL_APPLY_PATH,
            axum::routing::post(post_bare_url_apply),
        )
```

and extend the import: `use crate::citation_routes::{post_bare_url_apply, post_bare_url_proposals};`

**Step 4: Run the tests to verify they pass**

```bash
cargo test -p sp42-server bare_url
```

Expected: all bare-url tests pass (Phases 4–5 combined: 6 + 1 + 5 + 1 = 13).

**Step 5: Full crate verification**

```bash
cargo test -p sp42-server
cargo clippy -p sp42-server --all-targets --all-features -- -D warnings
```

Expected: green.

**Step 6: Commit**

```bash
git add crates/sp42-server/src/citation_routes.rs crates/sp42-server/src/routes.rs crates/sp42-server/src/tests.rs
git commit -m "feat: add gated bare-url-apply bridge route"
```
