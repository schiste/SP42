# Node-Anchored Wikitext Editing (ADR-0003) — Phase 5: Route Content Edits Through the Editor

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** Wire `WikitextEditor` into the server and route `InlineEdit` through node-anchored replacement when the request carries a `node_locator` (ADR-0003 Decision 2), keeping the Phase-1-guarded literal path as the fallback. Refusals surface as clean 4xx action errors. Finish with the documentation/ADR record updates.

**Architecture:** `AppState` carries `Arc<dyn WikitextEditor>` (production: `ParsoidWikitextEditor`; tests: `ScriptedWikitextEditor`). The action dispatch threads `&dyn WikitextEditor` into `execute_inline_edit_action`. `TagCitationNeeded` stays literal-only (it tags prose spans, not structural nodes) and explicitly rejects locators.

**Tech Stack:** No new dependencies.

**Scope:** Phase 5 of 5 from ADR-0003. Requires Phases 1-4.

**Codebase verified:** 2026-06-09.

---

## Context for the implementer

- `AppState` is at `crates/sp42-server/src/state.rs:27-52`; the production literal is at `crates/sp42-server/src/main.rs:310-328`; test literals are in `crates/sp42-server/src/tests.rs` at `test_state()` (line ~78) and **seven** inline constructions (lines ~692, ~1201, ~1269, ~1360, ~1535, ~1618, ~1707 — eight literals total). Struct literals are exhaustive — `cargo check -p sp42-server` enumerates any missed site; treat the compiler's list as authoritative over these line numbers. (`crates/sp42-server/tests/operator_live.rs` does not construct `AppState` — it drives the server externally; if the compiler disagrees, give it the same treatment as `tests.rs`.)
- Action dispatch: `execute_session_action` at `crates/sp42-server/src/action_routes.rs:248-324`; `execute_inline_edit_action` at `:370-419` (post-Phase-1 it uses `replace_exactly_once`); validation at `validate_action_request` `:772-848`; error mapping at `action_error_response` `:850-871` (`http_status: Some(400..=499)` → HTTP 400, otherwise 502).
- Refusal mapping decision: locator refusals (`node-drift`, `node-out-of-range`) become `ActionError::Execution { http_status: Some(409), retryable: false }` — they reach the client as HTTP 400 with the refusal code and `http_status: 409` in the body, telling the caller to re-enumerate. Editor hard errors map to codes `editor-unavailable` (retryable per backend), `editor-missing-target` (404 in body), `editor-not-configured` (502), `editor-unsupported` (400 in body).
- House mock-backend pattern: spawn a real axum router on an ephemeral port (`mock_capability_server` in `tests.rs:260-291`, `start_mock_backend` in `tests/operator_live.rs:73-230` — the latter already mocks MediaWiki `meta=tokens` and action endpoints; mirror its JSON shapes if the ones below disagree with the parsers in `sp42_core::action_executor`).
- `DevAuthCapabilityReport` and its nested capability structs all derive `Default` (`crates/sp42-core/src/dev_auth.rs:64-117`) — build test reports with struct-spread.

---

## Task 1: Carry the editor in `AppState`

**Files:**
- Modify: `crates/sp42-server/src/state.rs`
- Modify: `crates/sp42-server/src/main.rs:310-328`
- Modify: `crates/sp42-server/src/tests.rs` (helper + 8 literals)

**Step 1: Add the field**

`crates/sp42-server/src/state.rs`:
1. Extend the `sp42_core` import (line 7): `use sp42_core::{ActionExecutionLogEntry, DevAuthCapabilityReport, WikitextEditor};`
2. Add to `AppState` after `pub(crate) wiki_registry: WikiRegistry,`:

```rust
    pub(crate) wikitext_editor: Arc<dyn WikitextEditor>,
```

**Step 2: Construct it everywhere**

1. `crates/sp42-server/src/main.rs` — in the `AppState { … }` literal (line ~310), after `wiki_registry,`:

```rust
        wikitext_editor: Arc::new(parsoid_editor::ParsoidWikitextEditor::new()),
```

2. `crates/sp42-server/src/tests.rs` — add a helper next to `test_wiki_registry()` (line ~73):

```rust
fn test_wikitext_editor() -> std::sync::Arc<dyn sp42_core::WikitextEditor> {
    std::sync::Arc::new(sp42_core::ScriptedWikitextEditor::new(Vec::new(), String::new()))
}
```

then add `wikitext_editor: test_wikitext_editor(),` to all **eight** `AppState { … }` literals (`test_state()` plus the seven inline ones — run `cargo check -p sp42-server 2>&1 | grep "missing field"` and fix every site it reports).

**Step 3: Verify operationally**

Run: `cargo check -p sp42-server && cargo test -p sp42-server`
Expected: compiles; existing tests stay green.

**Step 4: Commit**

```bash
git add crates/sp42-server/src/state.rs crates/sp42-server/src/main.rs crates/sp42-server/src/tests.rs
git commit -m "feat(server): carry WikitextEditor in AppState (ADR-0003 D2)"
```

---

## Task 2: Locator-aware request validation

**Files:**
- Modify: `crates/sp42-server/src/action_routes.rs:772-848` (`validate_action_request`)
- Test: `crates/sp42-server/src/tests.rs`

**Step 1: Write the failing tests**

Add to `crates/sp42-server/src/tests.rs` (plain `#[test]`s — `validate_action_request` is pure; import `crate::action_routes::validate_action_request` and the `sp42_core` types following the file's existing import style):

```rust
fn capability_report_allowing_edit() -> sp42_core::DevAuthCapabilityReport {
    sp42_core::DevAuthCapabilityReport {
        checked: true,
        wiki_id: "frwiki".to_string(),
        capabilities: sp42_core::DevAuthDerivedCapabilities {
            editing: sp42_core::DevAuthEditCapabilities {
                can_edit: true,
                can_undo: true,
            },
            ..Default::default()
        },
        ..Default::default()
    }
}

fn inline_edit_request(
    node_locator: Option<sp42_core::WikitextNodeLocator>,
    selected_text: Option<String>,
    replacement_text: Option<String>,
) -> sp42_core::SessionActionExecutionRequest {
    sp42_core::SessionActionExecutionRequest {
        wiki_id: "frwiki".to_string(),
        kind: sp42_core::SessionActionKind::InlineEdit,
        rev_id: 42,
        title: Some("Exemple".to_string()),
        target_user: None,
        undo_after_rev_id: None,
        summary: None,
        selected_text,
        batch_rev_ids: None,
        replacement_text,
        node_locator,
    }
}

fn template_locator() -> sp42_core::WikitextNodeLocator {
    sp42_core::WikitextNodeLocator {
        kind: sp42_core::WikitextNodeKind::Template,
        ordinal: 0,
        expected_text: "{{cite web|url=https://example.org/a|title=Example A}}".to_string(),
    }
}

#[test]
fn validate_accepts_inline_edit_with_node_locator() {
    let payload = inline_edit_request(Some(template_locator()), None, Some("{{lang|fr|x}}".to_string()));
    let report = capability_report_allowing_edit();
    assert!(crate::action_routes::validate_action_request(&payload, &report).is_ok());
}

#[test]
fn validate_rejects_inline_edit_without_selected_text_or_locator() {
    let payload = inline_edit_request(None, None, Some("x".to_string()));
    let report = capability_report_allowing_edit();
    let (status, body) = crate::action_routes::validate_action_request(&payload, &report)
        .expect_err("missing target must be rejected");
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    assert!(
        body.0["error"]
            .as_str()
            .expect("error body should carry a message")
            .contains("selected_text or node_locator")
    );
}

#[test]
fn validate_rejects_node_locator_with_empty_expected_text() {
    let mut locator = template_locator();
    locator.expected_text = "   ".to_string();
    let payload = inline_edit_request(Some(locator), None, Some("x".to_string()));
    let report = capability_report_allowing_edit();
    let (status, _body) = crate::action_routes::validate_action_request(&payload, &report)
        .expect_err("empty expected_text must be rejected");
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
}

#[test]
fn validate_rejects_node_locator_without_replacement_text() {
    let payload = inline_edit_request(Some(template_locator()), None, None);
    let report = capability_report_allowing_edit();
    let (status, body) = crate::action_routes::validate_action_request(&payload, &report)
        .expect_err("missing replacement_text must be rejected");
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    assert!(
        body.0["error"]
            .as_str()
            .expect("error body should carry a message")
            .contains("replacement_text")
    );
}

#[test]
fn validate_rejects_node_locator_for_citation_tagging() {
    let mut payload = inline_edit_request(
        Some(template_locator()),
        Some("une phrase".to_string()),
        None,
    );
    payload.kind = sp42_core::SessionActionKind::TagCitationNeeded;
    let report = capability_report_allowing_edit();
    let (status, body) = crate::action_routes::validate_action_request(&payload, &report)
        .expect_err("citation tagging must reject locators");
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    assert!(
        body.0["error"]
            .as_str()
            .expect("error body should carry a message")
            .contains("not supported")
    );
}
```

**Step 2: Run the tests to verify they fail**

Run: `cargo test -p sp42-server validate_`
Expected: `validate_accepts_inline_edit_with_node_locator`, `validate_rejects_node_locator_*` FAIL (today's InlineEdit arm unconditionally demands `selected_text`; citation tagging ignores locators).

**Step 3: Implement**

In `validate_action_request` (`action_routes.rs`):

1. In the `SessionActionKind::TagCitationNeeded` arm, after the `selected_text` check:

```rust
            if payload.node_locator.is_some() {
                return Err(invalid_payload(
                    "node_locator is not supported for citation tagging",
                ));
            }
```

2. Replace the `SessionActionKind::InlineEdit` arm's `selected_text` check (keep the `title` and `can_edit` checks) with:

```rust
            match payload.node_locator.as_ref() {
                Some(locator) => {
                    if locator.expected_text.trim().is_empty() {
                        return Err(invalid_payload(
                            "node_locator.expected_text must not be empty",
                        ));
                    }
                    if payload.replacement_text.is_none() {
                        return Err(invalid_payload(
                            "replacement_text is required for node-anchored inline edit",
                        ));
                    }
                }
                None => {
                    if payload.selected_text.as_deref().is_none_or(str::is_empty) {
                        return Err(invalid_payload(
                            "selected_text or node_locator is required for inline edit",
                        ));
                    }
                }
            }
```

**Step 4: Run the tests to verify they pass**

Run: `cargo test -p sp42-server validate_`
Expected: all 5 pass.

**Step 5: Commit**

```bash
git add crates/sp42-server/src/action_routes.rs crates/sp42-server/src/tests.rs
git commit -m "feat(server): validate node-locator inline edits (ADR-0003 D2/D3)"
```

---

## Task 3: Node-anchored `InlineEdit` execution

**Files:**
- Modify: `crates/sp42-server/src/action_routes.rs` (imports, `execute_session_action`, `execute_inline_edit_action`, new helpers)
- Modify: `crates/sp42-server/src/action_routes.rs:50-100` (`post_execute_action` passes the editor)
- Test: `crates/sp42-server/src/tests.rs`

**Step 1: Write the failing tests**

Add to `crates/sp42-server/src/tests.rs` (`tokio::net`, `axum`, and `Arc` are already imported there; follow existing import style for anything missing). The mock wiki backend mirrors `tests/operator_live.rs:73-230` — cross-check the token/edit JSON shapes against that file and `sp42_core::action_executor`'s parsers if a parse error surfaces:

```rust
struct MockWikiBackend {
    base_url: String,
    edit_bodies: Arc<std::sync::Mutex<Vec<String>>>,
}

async fn spawn_mock_wiki_backend(page_wikitext: &'static str) -> MockWikiBackend {
    let edit_bodies = Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorded = edit_bodies.clone();
    let handler = move |request: axum::extract::Request| {
        let recorded = recorded.clone();
        async move {
            let query = request.uri().query().unwrap_or_default().to_string();
            let body_bytes = axum::body::to_bytes(request.into_body(), usize::MAX)
                .await
                .expect("mock body should read");
            let body = String::from_utf8_lossy(&body_bytes).to_string();
            let json = if query.contains("meta=tokens") {
                serde_json::json!({
                    "batchcomplete": true,
                    "query": { "tokens": {
                        "csrftoken": "test-csrf-token+\\",
                        "patroltoken": "test-patrol-token+\\"
                    } }
                })
            } else if query.contains("prop=revisions") {
                serde_json::json!({
                    "batchcomplete": true,
                    "query": { "pages": [ { "title": "Exemple", "revisions": [
                        { "slots": { "main": { "content": page_wikitext } } }
                    ] } ] }
                })
            } else if body.contains("action=edit") {
                recorded
                    .lock()
                    .expect("mock edit log should lock")
                    .push(body);
                serde_json::json!({
                    "edit": { "result": "Success", "pageid": 1, "title": "Exemple", "newrevid": 4243 }
                })
            } else if body.contains("action=patrol") {
                serde_json::json!({ "patrol": { "rcid": 7, "ns": 0, "title": "Exemple" } })
            } else {
                serde_json::json!({ "error": { "code": "unmocked", "info": format!("query={query} body={body}") } })
            };
            axum::Json(json)
        }
    };
    let app = axum::Router::new().fallback(handler);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("mock wiki backend should bind");
    let addr = listener
        .local_addr()
        .expect("mock wiki backend should expose addr");
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("mock wiki backend should serve");
    });
    MockWikiBackend {
        base_url: format!("http://{addr}"),
        edit_bodies,
    }
}

fn wiki_config_for_backend(base_url: &str) -> sp42_core::WikiConfig {
    let mut config = test_wiki_registry().default_config();
    config.api_url = format!("{base_url}/w/api.php")
        .parse()
        .expect("mock api url should parse");
    config
}

#[tokio::test]
async fn inline_edit_with_locator_saves_editor_output() {
    let backend = spawn_mock_wiki_backend("unused page text").await;
    let config = wiki_config_for_backend(&backend.base_url);
    let editor = sp42_core::ScriptedWikitextEditor::new(
        vec![sp42_core::ScriptedWikitextNode {
            kind: sp42_core::WikitextNodeKind::Template,
            anchor_text: "{{cite web|url=https://example.org/a|title=Example A}}".to_string(),
        }],
        "NEWPAGEWIKITEXT".to_string(),
    );
    let client = crate::runtime_adapters::BearerHttpClient::new(
        reqwest::Client::new(),
        "test-access-token".to_string(),
    );
    let payload = inline_edit_request(
        Some(template_locator()),
        None,
        Some("{{cite web|url=https://example.org/b|title=Example B}}".to_string()),
    );

    let response =
        crate::action_routes::execute_inline_edit_action(&client, &config, &payload, &editor)
            .await
            .expect("node-anchored inline edit should succeed");

    assert_eq!(response.status, 200);
    let invocations = editor.invocations();
    assert_eq!(invocations.len(), 1);
    assert_eq!(invocations[0].operation, "replace_node");
    assert_eq!(
        invocations[0].payload,
        "{{cite web|url=https://example.org/b|title=Example B}}"
    );
    let edits = backend.edit_bodies.lock().expect("mock edit log should lock");
    assert_eq!(edits.len(), 1, "exactly one save must reach the wiki");
    assert!(edits[0].contains("NEWPAGEWIKITEXT"), "save must carry the editor output: {}", edits[0]);
    assert!(edits[0].contains("baserevid=42"), "save must stay baserevid-guarded: {}", edits[0]);
}

#[tokio::test]
async fn inline_edit_with_drifted_locator_refuses_without_saving() {
    let backend = spawn_mock_wiki_backend("unused page text").await;
    let config = wiki_config_for_backend(&backend.base_url);
    let editor = sp42_core::ScriptedWikitextEditor::new(
        vec![sp42_core::ScriptedWikitextNode {
            kind: sp42_core::WikitextNodeKind::Template,
            anchor_text: "{{cite web|url=https://example.org/DIFFERENT|title=Drifted}}"
                .to_string(),
        }],
        "NEVERUSED".to_string(),
    );
    let client = crate::runtime_adapters::BearerHttpClient::new(
        reqwest::Client::new(),
        "test-access-token".to_string(),
    );
    let payload = inline_edit_request(Some(template_locator()), None, Some("x".to_string()));

    let error =
        crate::action_routes::execute_inline_edit_action(&client, &config, &payload, &editor)
            .await
            .expect_err("drifted locator must refuse");

    let sp42_core::ActionError::Execution { code, http_status, retryable, .. } = error;
    assert_eq!(code.as_deref(), Some("node-drift"));
    assert_eq!(http_status, Some(409));
    assert!(!retryable);
    assert!(
        backend.edit_bodies.lock().expect("mock edit log should lock").is_empty(),
        "a refused edit must never reach the wiki"
    );
}

#[tokio::test]
async fn inline_edit_without_locator_refuses_ambiguous_literal_target() {
    let backend = spawn_mock_wiki_backend("le mot, le mot, deux fois").await;
    let config = wiki_config_for_backend(&backend.base_url);
    let editor = sp42_core::ScriptedWikitextEditor::new(Vec::new(), String::new());
    let client = crate::runtime_adapters::BearerHttpClient::new(
        reqwest::Client::new(),
        "test-access-token".to_string(),
    );
    let payload = inline_edit_request(None, Some("le mot".to_string()), Some("la phrase".to_string()));

    let error =
        crate::action_routes::execute_inline_edit_action(&client, &config, &payload, &editor)
            .await
            .expect_err("ambiguous literal target must refuse");

    let sp42_core::ActionError::Execution { code, .. } = error;
    assert_eq!(code.as_deref(), Some("text-ambiguous"));
    assert!(backend.edit_bodies.lock().expect("mock edit log should lock").is_empty());
}

#[test]
fn editor_errors_map_to_action_error_codes() {
    let mapped = crate::action_routes::action_error_from_editor(
        sp42_core::WikitextEditorError::NotConfigured {
            wiki_id: "frwiki".to_string(),
        },
    );
    let sp42_core::ActionError::Execution { code, http_status, retryable, .. } = mapped;
    assert_eq!(code.as_deref(), Some("editor-not-configured"));
    assert_eq!(http_status, None, "configuration gaps surface as gateway errors");
    assert!(!retryable);

    let mapped = crate::action_routes::action_error_from_editor(
        sp42_core::WikitextEditorError::Unavailable {
            message: "down".to_string(),
            retryable: true,
        },
    );
    let sp42_core::ActionError::Execution { code, retryable, .. } = mapped;
    assert_eq!(code.as_deref(), Some("editor-unavailable"));
    assert!(retryable);
}
```

**Step 2: Run the tests to verify they fail**

Run: `cargo test -p sp42-server inline_edit -- --test-threads=1` (and `cargo test -p sp42-server editor_errors_map`)
Expected: compile errors — `execute_inline_edit_action` has no editor parameter and `action_error_from_editor` does not exist.

**Step 3: Implement**

In `crates/sp42-server/src/action_routes.rs`:

1. Extend the `sp42_core` import list with `WikitextEditOutcome, WikitextEditor, WikitextEditorError, WikitextNodeLocator, WikitextPageRef` (keep alphabetical order).

2. `post_execute_action` (line ~71): pass the editor through —

```rust
    let outcome =
        execute_session_action(&client, &config, &payload, state.wikitext_editor.as_ref()).await;
```

3. `execute_session_action`: add the parameter and forward it —

```rust
async fn execute_session_action(
    client: &BearerHttpClient,
    config: &sp42_core::WikiConfig,
    payload: &SessionActionExecutionRequest,
    editor: &dyn WikitextEditor,
) -> Result<HttpResponse, ActionError> {
```

and change the `InlineEdit` arm to:

```rust
        SessionActionKind::InlineEdit => {
            execute_inline_edit_action(client, config, payload, editor).await
        }
```

4. Rework `execute_inline_edit_action` (make it `pub(crate)` so `tests.rs` can drive it):

```rust
pub(crate) async fn execute_inline_edit_action(
    client: &BearerHttpClient,
    config: &sp42_core::WikiConfig,
    payload: &SessionActionExecutionRequest,
    editor: &dyn WikitextEditor,
) -> Result<HttpResponse, ActionError> {
    let token = execute_fetch_token(client, config, TokenKind::Csrf).await?;
    let title = payload.title.clone().unwrap_or_default();
    let updated_text = if let Some(locator) = payload.node_locator.as_ref() {
        node_anchored_replacement(config, &title, payload, locator, editor).await?
    } else {
        let original = payload.selected_text.clone().unwrap_or_default();
        let replacement = payload.replacement_text.clone().unwrap_or_default();
        if original.trim().is_empty() {
            return Err(ActionError::Execution {
                message: "selected_text (original) is required for inline edit".to_string(),
                code: Some("invalid-input".to_string()),
                http_status: None,
                retryable: false,
            });
        }
        let page_text = crate::fetch_page_wikitext(client, config, &title).await?;
        replace_exactly_once(&page_text, &original, &replacement)?
    };
    let summary = payload
        .summary
        .clone()
        .unwrap_or_else(|| "SP42: inline edit".to_string());
    let save_response = execute_wiki_page_save(
        client,
        config,
        &WikiPageSaveRequest {
            title,
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

5. Add the two helpers (near `execute_inline_edit_action`):

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
        .map_err(action_error_from_editor)?;
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

pub(crate) fn action_error_from_editor(error: WikitextEditorError) -> ActionError {
    let (code, http_status, retryable) = match &error {
        WikitextEditorError::Unavailable { retryable, .. } => {
            ("editor-unavailable", None, *retryable)
        }
        WikitextEditorError::MissingTarget { .. } => ("editor-missing-target", Some(404), false),
        WikitextEditorError::NotConfigured { .. } => ("editor-not-configured", None, false),
        WikitextEditorError::Unsupported { .. } => ("editor-unsupported", Some(400), false),
    };
    ActionError::Execution {
        message: error.to_string(),
        code: Some(code.to_string()),
        http_status,
        retryable,
    }
}
```

**Step 4: Run the tests to verify they pass**

Run: `cargo test -p sp42-server`
Expected: all green, including the three new async tests and the mapping test.

**Step 5: Commit**

```bash
git add crates/sp42-server/src/action_routes.rs crates/sp42-server/src/tests.rs
git commit -m "feat(server): route InlineEdit through node-anchored WikitextEditor (ADR-0003 D2)"
```

---

## Task 4: Record the outcome (ADR status, docs)

**Files:**
- Modify: `docs/adr/0003-node-anchored-wikitext-editing.md:3` (Status)
- Modify: `docs/DEVELOPER_SURFACE.md` ("Action Boundary" section, lines ~41-47)
- Modify: `docs/STATUS.md` (latest phase section)

**Step 1: Update the records**

1. ADR-0003 header: change `**Status:** Proposed` to `**Status:** Accepted`, and directly under the `**Date:**`/`**Author:**` block add:

```markdown
**Implementation note (2026-06-09):** Implemented (Decisions 1-6). The open
licensing gate resolved via ADR-0001 §3 — SP42 is `GPL-3.0-only`, so the
`parsoid` crate (`GPL-3.0-or-later`) is linked directly and recorded in
`deny.toml`.
```

2. `docs/DEVELOPER_SURFACE.md`, end of the "Action Boundary" section, append:

```markdown
Node-anchored content editing (ADR-0003) follows the same split: the
`WikitextEditor` contract, locator types, and the deterministic scripted
double live in `sp42-core::wikitext_editor`; the Parsoid REST adapter lives
in `sp42-server::parsoid_editor`.
```

3. `docs/STATUS.md`: in the most recent phase section, append a bullet:

```markdown
- node-anchored wikitext editing (ADR-0003) is implemented: a `WikitextEditor`
  contract with a Parsoid-backed adapter; `InlineEdit` accepts an optional
  node locator, and the literal fallback refuses ambiguous matches
```

**Step 2: Verify**

Run: `bash scripts/check-doc-consistency.sh` (the docs-consistency check the pre-push hook runs for docs-only pushes) — expected clean.

**Step 3: Commit**

```bash
git add docs/adr/0003-node-anchored-wikitext-editing.md docs/DEVELOPER_SURFACE.md docs/STATUS.md
git commit -m "docs: record ADR-0003 implementation (status accepted)"
```

---

## Phase verification (final, whole feature)

Run, from the worktree root:

```bash
./scripts/check-focused.sh
cargo clippy --workspace --all-targets -- -D warnings
cargo deny check licenses
```

Expected: all green (check-focused runs the serial workspace tests plus the trunk frontend build, matching the pre-commit gate).

**Done when:** an `InlineEdit` request with a `node_locator` flows reviewer → validation → `WikitextEditor::replace_node` → `baserevid`-guarded save; drift/out-of-range refusals return `node-drift`/`node-out-of-range` without touching the wiki; literal edits stay exactly-once-guarded; `TagCitationNeeded` rejects locators; the ADR records the implementation.
