# Article Citation Extractor Implementation Plan — Phase 6: Server Route + Inference Wiring

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

> ⚠️ **DECISION TO CONFIRM WITH LUIS BEFORE EXECUTING THIS PHASE.** Phases 1–5 are fully determined. Phase 6 hits a real architectural gap: the server has **no inference wiring at all**, and `GenaiModelClient` lives *inside the CLI binary* (`sp42-cli/src/main.rs:695`), not a shared lib. To run inference server-side it must be relocated. This plan extracts a small **`sp42-inference`** lib crate (which also directly enables the committed CLI fast-follow). The alternative — duplicating the genai adapter into `sp42-server` — is faster but creates drift. Get sign-off on the crate-extraction approach first.

**Goal:** Expose `verify_page` as a read-only `POST /dev/citation/verify-page`, mirroring the bare-url proposals route, with a per-request model client + panel built from `SP42_INFERENCE_*` env vars.

**Architecture:** Extract `GenaiModelClient` + `ModelEndpointConfig` env loading + panel-from-env into `sp42-inference` (shared by CLI and server). The server handler builds the editor (`ParsoidWikitextEditor`) and a model client per request, calls `extract_blocks` → `extract_use_sites` → `verify_page`, returns the `PageVerificationReport`.

**Tech Stack:** `axum`, `genai`, `sp42-core`, `sp42-types`, `sp42-inference` (new).

**Scope:** Phase 6 of 6.

**Codebase verified:** 2026-06-24
- `AppState` (`crates/sp42-server/src/state.rs:28`) holds `http_client`, `clock`, `wikitext_editor: Arc<dyn WikitextEditor>`, `wiki_registry`, … but **no model client / inference config**. Server has zero `verify_citation_use_site` / `ModelClient` / `GenaiModelClient` references today (inference is CLI-only).
- Bare-url handler `post_bare_url_proposals(State(state), Json(payload))` (`citation_routes.rs:159`) → `config_for_state_wiki(&state, &payload.wiki_id)` (`main.rs:1115`) → `state.wiki_registry.config(wiki_id)`. It uses `state.wikitext_editor.as_ref()` and `state.http_client`.
- Route constants in `crates/sp42-core/src/routes.rs:81` (`DEV_CITATION_BARE_URL_PROPOSALS_PATH`); registered in `crates/sp42-server/src/routes.rs:199` (`dev_bridge_routes`).
- CLI builds panel + client (`main.rs:840–891`): `SP42_INFERENCE_URL`/`MODELS`/`PROVIDER`/`TOKEN`/`CAPABILITY`/`MODE` → `Vec<ModelRef>` + `GenaiModelClient::new(ModelEndpointConfig{ mode, base_url, auth_token, capability_tag })`.
- `GenaiModelClient { client: genai::Client, endpoint: ModelEndpointConfig }` (`main.rs:695`); `ModelEndpointConfig` is already in `sp42-types` (`model.rs:205`).

---

## Task 1: Create the `sp42-inference` crate

**Files:**
- Create: `crates/sp42-inference/Cargo.toml`
- Create: `crates/sp42-inference/src/lib.rs`
- Modify: root `Cargo.toml` (workspace `members`)

**Step 1: Determine the genai version**

Run: `rg -n "^genai" crates/sp42-cli/Cargo.toml` — note the exact `genai` version/features the CLI uses; reuse them verbatim.

**Step 2: Create the crate manifest**

`crates/sp42-inference/Cargo.toml`:

```toml
[package]
name = "sp42-inference"
version = "0.1.0"
edition = "2021"

[dependencies]
sp42-types = { path = "../sp42-types" }
genai = "<same version as sp42-cli>"
async-trait = "<workspace version>"
```

(Match `edition`, and the `async-trait`/`genai` versions, to the existing crates — copy from `crates/sp42-cli/Cargo.toml`.)

**Step 3: Move the genai adapter and env loaders into `lib.rs`**

Cut `struct GenaiModelClient`, its `impl GenaiModelClient`, and its `#[async_trait] impl ModelClient for GenaiModelClient` from `crates/sp42-cli/src/main.rs` into `crates/sp42-inference/src/lib.rs`, making them `pub`. Add two env-driven constructors so both binaries share the wiring:

```rust
//! Shared inference edge: the genai-backed `ModelClient` and env-driven
//! construction of an endpoint config + model panel.

use sp42_types::{ModelEndpointConfig, ModelRef};

// ... moved GenaiModelClient (now `pub struct GenaiModelClient`), its `new`,
//     and its `impl ModelClient` ...

/// Endpoint mode parser (moved from the CLI).
pub fn parse_endpoint_mode(raw: Option<&str>) -> Result<sp42_types::EndpointMode, String> {
    // ... moved body ...
}

/// Build the model panel from `SP42_INFERENCE_MODELS` (+ `SP42_INFERENCE_PROVIDER`).
pub fn panel_from_env() -> Result<Vec<ModelRef>, String> {
    let provider =
        std::env::var("SP42_INFERENCE_PROVIDER").unwrap_or_else(|_| "configured".to_string());
    let models = std::env::var("SP42_INFERENCE_MODELS")
        .map_err(|_| "set SP42_INFERENCE_MODELS to a comma-separated list of model ids".to_string())?;
    let panel: Vec<ModelRef> = models
        .split(',')
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .map(|m| ModelRef::new(provider.clone(), m, m))
        .collect();
    if panel.is_empty() {
        return Err("SP42_INFERENCE_MODELS is empty".to_string());
    }
    Ok(panel)
}

/// Build a genai model client from `SP42_INFERENCE_URL`/`TOKEN`/`CAPABILITY`/`MODE`.
pub fn client_from_env() -> Result<GenaiModelClient, String> {
    let base_url = std::env::var("SP42_INFERENCE_URL")
        .map_err(|_| "set SP42_INFERENCE_URL to the model's OpenAI-compatible base URL".to_string())?;
    let auth_token = std::env::var("SP42_INFERENCE_TOKEN").ok();
    let capability_tag = std::env::var("SP42_INFERENCE_CAPABILITY").ok();
    let mode = parse_endpoint_mode(std::env::var("SP42_INFERENCE_MODE").ok().as_deref())?;
    Ok(GenaiModelClient::new(ModelEndpointConfig { mode, base_url, auth_token, capability_tag }))
}
```

**Step 4: Rewire the CLI to use the crate**

- Add `sp42-inference = { path = "../sp42-inference" }` to `crates/sp42-cli/Cargo.toml`.
- In `main.rs`, delete the moved definitions and `use sp42_inference::{GenaiModelClient, client_from_env, panel_from_env, parse_endpoint_mode};`. Replace the inline env-panel/client construction in `run_verify` with `panel_from_env()?` / `client_from_env()?` (behavior-preserving).

**Step 5: Add crate to workspace and build**

- Add `"crates/sp42-inference"` to the workspace `members` in the root `Cargo.toml`.

Run: `PATH="$HOME/.cargo/bin:$PATH" cargo build -p sp42-inference -p sp42-cli`
Expected: both build. The CLI `verify` behaves exactly as before (no functional change).

**Step 6: Commit**

```bash
git add crates/sp42-inference Cargo.toml crates/sp42-cli/Cargo.toml crates/sp42-cli/src/main.rs
SP42_SKIP_GIT_HOOKS=1 git commit -m "refactor(inference): extract sp42-inference crate from CLI

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_015NtcEzaj8J373GGsv3FC7y"
```

---

## Task 2: Route constant + request/handler

**Files:**
- Modify: `crates/sp42-core/src/routes.rs` (add path constant)
- Modify: `crates/sp42-server/src/citation_routes.rs` (handler)
- Modify: `crates/sp42-server/Cargo.toml` (add `sp42-inference` dep)

**Step 1: Add the path constant**

In `crates/sp42-core/src/routes.rs`, beside `DEV_CITATION_BARE_URL_PROPOSALS_PATH`:

```rust
pub const DEV_CITATION_VERIFY_PAGE_PATH: &str = "/dev/citation/verify-page";
```

**Step 2: Add the dependency**

Add `sp42-inference = { path = "../sp42-inference" }` to `crates/sp42-server/Cargo.toml`.

**Step 3: Write the handler**

In `citation_routes.rs`, mirror `post_bare_url_proposals`. The request uses `sp42_core::PageVerificationRequest` (already serde). The handler builds inference per request:

```rust
use sp42_core::{
    extract_use_sites, verify_page, PageVerificationReport, PageVerificationRequest, VerifyOptions,
};
use sp42_core::WikitextPageRef;

pub(crate) async fn post_verify_page(
    State(state): State<AppState>,
    Json(payload): Json<PageVerificationRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let config = config_for_state_wiki(&state, &payload.wiki_id)?;

    // Per-request inference edge from env (dev route).
    let panel = sp42_inference::panel_from_env()
        .map_err(|error| (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({ "error": error }))))?;
    let model_client = sp42_inference::client_from_env()
        .map_err(|error| (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({ "error": error }))))?;

    // Extract blocks via the editor (Parsoid), then use-sites, then verify.
    let page_ref = WikitextPageRef { title: payload.title.clone(), rev_id: payload.rev_id };
    let blocks = state
        .wikitext_editor
        .extract_blocks(&config, &page_ref)
        .await
        .map_err(|error| editor_error_response(&error))?;
    let extract = extract_use_sites(&blocks, &payload);

    let options = VerifyOptions::default();
    let report: PageVerificationReport = verify_page(
        &state.http_client,
        &model_client,
        state.clock.as_ref(),
        &panel,
        &payload,
        extract,
        options,
    )
    .await;

    Ok(Json(report))
}
```

Executor notes:
- `editor_error_response` — reuse whatever the bare-url path uses to turn a `WikitextEditorError` into an HTTP response; if there's no shared helper, map `WikitextEditorError::NotConfigured` → `400` and others → `502`, matching `action_error_response`'s style.
- Confirm `config_for_state_wiki` is importable here (it lives in `main.rs`; bare-url calls it, so it's in scope for `citation_routes.rs` — match the existing `use`).
- `state.clock` is `Arc<dyn Clock>`; pass `state.clock.as_ref()`.

**Step 4: Build**

Run: `PATH="$HOME/.cargo/bin:$PATH" cargo build -p sp42-server`
Expected: builds.

---

## Task 3: Register the route + integration test

**Files:**
- Modify: `crates/sp42-server/src/routes.rs` (register in `dev_bridge_routes`)
- Test: `crates/sp42-server/src/tests.rs`

**Step 1: Register**

In `dev_bridge_routes` (≈ line 199), add beside the bare-url registration:

```rust
        .route(
            route_contracts::DEV_CITATION_VERIFY_PAGE_PATH,
            axum::routing::post(post_verify_page),
        )
```

Add `post_verify_page` to the `citation_routes` import list at the top of `routes.rs`.

**Step 2: Write a route test driven by a stub editor**

The handler reads `SP42_INFERENCE_*` from env; to keep the test hermetic and avoid real inference, assert the route is **registered and reaches config/editor resolution** without those env vars (it should return `503 SERVICE_UNAVAILABLE` from `panel_from_env`, proving the route exists and is wired). This mirrors the bare-url test's "registered and gated" style (`tests.rs:642`).

```rust
#[tokio::test]
async fn verify_page_route_is_registered() {
    // Ensure inference env is absent so we hit the 503 wiring check deterministically.
    std::env::remove_var("SP42_INFERENCE_MODELS");
    let router = build_router(test_state());
    let response = router
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/dev/citation/verify-page")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "wiki_id": "frwiki", "title": "Exemple", "rev_id": 42 })
                        .to_string(),
                ))
                .expect("request builds"),
        )
        .await
        .expect("request completes");
    // 503 (no inference configured) proves the route is registered and reached
    // the inference-wiring step — not a 404.
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}
```

Executor note: confirm `test_state()` constructs a `wiki_registry` that resolves `frwiki` (the bare-url test uses `frwiki`, so it should). If config resolution fails first with `400`, swap the assertion to `BAD_REQUEST` or give the test a wiki the registry knows — match whatever the bare-url registration test relies on.

**Step 3: Run**

Run: `PATH="$HOME/.cargo/bin:$PATH" cargo test -p sp42-server verify_page_route_is_registered -- --exact`
Expected: PASS.

**Step 4: Full focused check, clippy, fmt, commit**

```bash
PATH="$HOME/.cargo/bin:$PATH" cargo clippy -p sp42-server -p sp42-core -p sp42-inference --all-targets -- -D warnings
PATH="$HOME/.cargo/bin:$PATH" cargo fmt --all
RUST_TEST_THREADS=1 PATH="$HOME/.cargo/bin:$PATH" cargo test -p sp42-core -p sp42-server -p sp42-inference
git add crates/sp42-core/src/routes.rs crates/sp42-server/src/citation_routes.rs crates/sp42-server/src/routes.rs crates/sp42-server/src/tests.rs crates/sp42-server/Cargo.toml
SP42_SKIP_GIT_HOOKS=1 git commit -m "feat(citation): read-only verify-page route

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_015NtcEzaj8J373GGsv3FC7y"
```

**Done when:** `POST /dev/citation/verify-page` is registered; with no `SP42_INFERENCE_*` it returns `503` (proving wiring); with inference env set it extracts blocks via Parsoid, builds use-sites, runs `verify_page`, and returns a `PageVerificationReport`; CLI `verify` still works through the shared `sp42-inference` crate; clippy clean, focused test suite green.

---

## Manual end-to-end validation (after the phase)

With a Parsoid-configured wiki and inference env set:

```bash
export SP42_INFERENCE_URL=https://openrouter.ai/api/v1
export SP42_INFERENCE_MODELS='google/gemma-4-26b-a4b-it,ibm-granite/granite-4.1-8b,mistralai/mistral-small-3.2-24b-instruct'
export SP42_INFERENCE_TOKEN=...   # from alex-cite-checker/.env (never echo)
curl -s -XPOST localhost:8788/dev/citation/verify-page \
  -H 'content-type: application/json' \
  -d '{"wiki_id":"enwiki","title":"Cat","rev_id":<rev>}' | jq '.stats'
```

Expect a `stats` block with `refs_seen`, per-verdict tallies, `skipped`, `extraction_failures`.
