# 03 — sp42-core layout, module registration, tests, and the lint bar

Research notes for porting wikiharness citation verification into `sp42-core`
(and consuming crates). Goal: new code compiles clean under SP42's CI gate
(`warnings = "deny"`, `clippy::pedantic = "deny"`).

Tree researched: `/var/home/louie/Projects/Volunteering-Consulting/SP42-impl-citation`
Toolchain: pinned `1.92.0` (`rust-toolchain.toml`), edition `2024`, workspace
`resolver = "3"`, `rust-version = "1.92"`.

---

## 1. How modules are declared / exported in `lib.rs`

### 1a. The layout reality: sp42-core is FLAT

Every `sp42-core` module is a single `crates/sp42-core/src/<name>.rs` file.
**There are NO `mod.rs` subdirectories anywhere in `sp42-core`** (verified with
`find`). The only `mod.rs` directory pattern in the whole workspace is in
`sp42-app` (`src/pages/mod.rs`, `src/components/mod.rs`, `src/platform/mod.rs`),
and those use `pub(crate) mod article;` style.

So a "citation module with submodules" can be done two ways; **prefer the
directory form to match Rust 2024 and keep files small**, but a single flat file
is also idiomatic here:

**Option A — directory module (recommended for multi-file citation work):**
Create `crates/sp42-core/src/citation/` with these files:
```
crates/sp42-core/src/citation.rs        # the module root (declares submodules)
crates/sp42-core/src/citation/verify.rs
crates/sp42-core/src/citation/prompts.rs
crates/sp42-core/src/citation/body_classifier.rs
crates/sp42-core/src/citation/voting.rs
```
In `citation.rs` (the module root — note: NOT `mod.rs`; the workspace uses the
2018+ `<name>.rs` + `<name>/` form, consistent with edition 2024):
```rust
//! Citation verification: parsing, prompts, body classifier, voting.

pub mod body_classifier;
pub mod prompts;
pub mod verify;
pub mod voting;
```
Submodule files (`citation/verify.rs`) reference siblings via
`use crate::citation::prompts::...;` and crate-wide types via `use crate::...;`.

**Option B — single flat file:** add `crates/sp42-core/src/citation.rs` with
inline `mod`s, mirroring `liftwing.rs`/`scoring_engine.rs`.

### 1b. Registering the module in `lib.rs` — exact steps

`crates/sp42-core/src/lib.rs` has TWO blocks, both alphabetically ordered:

1. **`pub mod` declarations** (lines 27–52). Insert alphabetically:
```rust
pub mod citation;
```
(goes between `pub mod branding;` and `pub mod context_builder;` — `c` order.)

2. **`pub use` re-exports** (lines 54–160), one block per module, also alpha by
   module path. Add a grouped re-export, e.g.:
```rust
pub use citation::{
    CitationVerdict, CitationFinding, build_citation_verify_request,
    execute_citation_verify, parse_citation_verify_response,
    // ...keep the list alphabetized inside the braces, matching existing blocks
};
```
Existing blocks (e.g. `pub use liftwing::{...}`, `pub use scoring_policy::{...}`)
show the convention: every public type/fn is re-exported flat from the crate root
so downstream crates write `use sp42_core::CitationVerdict;`. **The `lib.rs` doc
example block (lines 5–25) is a doctest** — if you add a type used in a new
doctest, keep it compiling; you don't have to touch the existing one.

3. `lib.rs` line 1 is `#![forbid(unsafe_code)]` — no `unsafe` in any new code.

---

## 2. FULL `WikiConfig` + how to add an `Option<Url>` endpoint field + serde defaults

### 2a. Full struct (from `crates/sp42-core/src/types.rs:393–411`)

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikiConfig {
    pub wiki_id: String,
    pub display_name: String,
    pub api_url: Url,
    pub eventstreams_url: Url,
    pub oauth_authorize_url: Url,
    pub oauth_token_url: Url,
    pub liftwing_url: Option<Url>,
    pub coordination_url: Option<Url>,
    #[serde(default)]
    pub namespace_allowlist: Vec<i32>,
    #[serde(default = "default_scoring_policy_ref")]
    pub scoring_policy_ref: String,
    #[serde(default)]
    pub scoring: ScoringConfig,
    #[serde(default)]
    pub templates: WikiTemplates,
}
```
Key facts:
- `Url` is `url::Url` (re-exported via `use url::Url;`); `url` has the `serde`
  feature on (workspace dep `url = { ..., features = ["serde"] }`), so `Url` and
  `Option<Url>` serde directly.
- Derives `Eq` — so any **new field type must be `Eq`** (no bare `f32`/`f64`
  fields, no `HashMap`-with-float; `Url`, `String`, `Option<Url>`, `bool`,
  ints, and nested `Eq` structs are fine). If you need a model list, use a
  `Vec<ModelRef>` where `ModelRef` derives `Eq`.
- `coordination_url: Option<Url>` is the EXACT template for a new optional
  endpoint field. It has NO `#[serde(default)]` because `Option<Url>` already
  defaults to `None` via serde's `Option` handling **when the key is present but
  null** AND the YAML always lists the key (see `configs/frwiki.yaml` has
  `coordination_url:` with empty value → `None`). **To be safe for a NEW field
  that older configs won't have a key for, ADD `#[serde(default)]`** so a missing
  key deserializes to `None`.

### 2b. Adding a model-panel / inference-endpoint field — exact pattern

Add to `WikiConfig`:
```rust
    /// Optional inference endpoint for citation verification (ADR-0006).
    #[serde(default)]
    pub inference_url: Option<Url>,
```
Place it after `coordination_url` to group endpoints. Because of `#[serde(default)]`,
existing configs (`configs/frwiki.yaml`) deserialize fine without the key.

**You MUST also update two places or compilation/doctests break:**
1. The `lib.rs` doctest literal (lines 8–21) constructs `WikiConfig { ... }` with
   ALL fields named. A new non-`default`-constructed field added to a struct
   literal that lists every field will fail to compile. Either (a) give the field
   `#[serde(default)]` AND add it to the doctest literal (`inference_url: None,`),
   or (b) add it to the doctest. **Adding `inference_url: None,` to the doctest is
   required regardless** — struct literals must name every field.
2. `crates/sp42-core/src/test_fixtures.rs` builds a `WikiConfig` via
   `serde_yaml::from_str(include_str!("../../../configs/frwiki.yaml"))` — that's
   deserialization, so a `#[serde(default)]` field needs NO fixture change. Good.

For a `Vec<ModelRef>` panel config, prefer:
```rust
    #[serde(default)]
    pub citation_models: Vec<ModelRef>,
```
where `ModelRef` is a new `#[derive(Debug, Clone, PartialEq, Eq, Serialize,
Deserialize)]` struct `{ provider: String, model: String, version: String }`
(ADR-0006 Decision 8). `Vec<T>` + `#[serde(default)]` → empty vec when absent.

### 2c. Default-fn convention (for non-`Option` defaults)

If a new field needs a non-empty default, follow the file's free-function pattern
(e.g. `default_scoring_policy_ref` at `types.rs:413`, `default_citation_needed`
at `types.rs:433`):
```rust
fn default_inference_provider() -> String { "openrouter".to_string() }
```
and `#[serde(default = "default_inference_provider")]`. `Option` fields use plain
`#[serde(default)]` (→ `None`); `Vec`/`bool` use plain `#[serde(default)]`.

`FlagState` (a bool-newtype enum, `types.rs:9–45`) is the project's preferred
"on/off config flag" type over raw `bool`: it derives `Default = Disabled`,
serdes `from/into bool`, and has `#[serde(default)]`-friendly `Default`. Use
`FlagState` for a boolean toggle like "citation verification enabled".

---

## 3. Exact test / CI commands

### 3a. There is NO `integration` cargo feature anywhere in the workspace.

Verified by grep across all `Cargo.toml` and `*.rs`. The only feature flags in
the workspace are:
- `sp42-wiki`: `[features] test-fixtures = []`
- `sp42-app` (wasm) features (not relevant to core tests)

So integration tests are NOT feature-gated. They live in normal `#[cfg(test)]`
modules and (where present) `tests/` dirs, run by plain `cargo test`.

### 3b. CI runner = the `xtask` crate at the workspace ROOT (`xtask/`, not `crates/xtask`).

Entry: `cargo run -p xtask -- <command>`. The CI command is `ci-all`
(`xtask/src/main.rs:345 fn ci_all`). It runs, in order, with `--profile ci`:
1. `cargo build --workspace --all-targets --profile ci`
2. `cargo test --workspace --profile ci`
3. `cargo clippy --workspace --all-targets --all-features --profile ci -- -D warnings`
4. `cargo doc --workspace --no-deps --profile ci`
5. wasm build of `sp42-app` (`--target wasm32-unknown-unknown --profile ci`)
6. `trunk build`
7. tauri contract build of `sp42-desktop/src-tauri`

**The clippy step is `--all-features` and `-D warnings`** on top of the
workspace's `clippy::pedantic = "deny"` lint. **The `cargo doc` step means a
broken or missing rustdoc reference fails CI** — keep `# Errors` doc sections
valid and intra-doc links resolvable.

### 3c. Commands you will actually run while iterating

Fast unit tests for the core crate only:
```sh
cargo test -p sp42-core
```
The repo's focused script (`scripts/check-focused.sh`) runs check + test across
the host crates, **single-threaded by default** (`RUST_TEST_THREADS=1`):
```sh
./scripts/check-focused.sh
```
The full CI-shaped gate (what pre-push runs):
```sh
./scripts/ci-all.sh          # → cargo run -p xtask -- ci-all --locked ...
```
Clippy exactly as CI sees it (run this before claiming clean):
```sh
cargo clippy --workspace --all-targets --all-features -- -D warnings
```
Formatting (pre-commit checks it): `cargo fmt --all -- --check`.

### 3d. Git hooks (husky, `core.hooksPath=.husky`)

- **pre-commit**: whitespace check, `scripts/check-doc-consistency.sh`, then for
  non-docs changes: `cargo fmt --all -- --check`, `audit-release-tree.sh`,
  `./scripts/check-focused.sh`. Docs-only (`.md/.log/.txt`) commits skip the
  compile-heavy steps.
- **pre-push**: `audit-release-tree.sh` + full `./scripts/ci-all.sh` for any
  non-docs push.
- Bypass for emergencies only: `SP42_SKIP_GIT_HOOKS=1`.
- `scripts/check-doc-consistency.sh` greps `README.md`, `docs/STATUS.md`,
  `docs/DEVELOPER_SURFACE.md` for required marker lines — citation work shouldn't
  disturb those, but **don't delete/rephrase those marker lines**.

### 3e. The only GitHub Actions workflow is `desktop-release.yml`

It is `workflow_dispatch` + tag-triggered (`desktop-v*`) — it builds desktop
bundles only. **There is no push/PR CI workflow in `.github/workflows/`.** The
actual quality gate is enforced LOCALLY via the husky pre-push hook running
`ci-all`. So: pushing without the hook would not be caught by Actions; run
`./scripts/ci-all.sh` yourself.

---

## 4. Deps already available to sp42-core (avoid adding new ones)

From `crates/sp42-core/Cargo.toml`:

**Runtime deps (all `.workspace = true`):**
- `async-trait` 0.1 — for `#[async_trait]` on traits (HttpClient etc.)
- `base64` 0.22
- `serde` 1 (derive)
- `serde_json` 1
- `serde_yaml` 0.9
- `sha2` 0.10 — **use this for content-addressed source snapshots / grounding
  hashes; do NOT add another hashing crate.**
- `similar` 2.7 — diffing (already used by `diff_engine`)
- `sp42-types` (path) — HTTP traits + transport types + errors live here
- `thiserror` 2 — all error enums use `#[derive(Error)]`
- `tracing` 0.1
- `url` 2.5 (with `serde`) — `Url` type

**Dev-deps:**
- `futures` 0.3 (`futures::executor::block_on` is how async fns are tested in
  unit tests — see `liftwing.rs` tests; **no `tokio` in sp42-core tests**)
- `proptest` 1.9 (NOT `.workspace` — pinned literally `"1.9.0"` in the crate)

**NOT available to sp42-core (do not reach for them here):** `reqwest`, `tokio`,
`rand`, axum, leptos, etc. Network/HTTP is abstracted behind the `HttpClient`
trait (see §6 of the request/response design). A real HTTP client impl belongs
in a non-core crate (e.g. `sp42-wiki`/`sp42-live`/`sp42-server`), NOT sp42-core.

**License gate (`deny.toml`):** allowed licenses are Apache-2.0, BSL-1.0, CC0,
CDLA-Permissive-2.0, BSD-2/3, ISC, MIT, Unicode-3.0, Zlib, **GPL-3.0-only**.
Adding any new crate dep requires its license to be on that list (and a
`Cargo.lock` update). Strongly prefer reusing the deps above to avoid touching
`deny.toml` / lockfile. (Targets checked: linux-gnu + wasm32.)

---

## 5. Clippy `pedantic = deny` gotchas (and how existing code satisfies them)

Workspace lints (`Cargo.toml`):
```toml
[workspace.lints.rust]
warnings = "deny"
[workspace.lints.clippy]
pedantic = "deny"
```
Each crate opts in with `[lints] workspace = true`. **`sp42-core/Cargo.toml` has
`[lints] workspace = true`** — your new code is under pedantic-deny.

Concrete patterns the existing code uses to stay clean (copy these):

1. **`#[must_use]` on every public pure fn/method that returns a value and has no
   side effects.** Examples: `build_scoring_context` (`context_builder.rs:14`),
   `normalize_liftwing_probability` (`:36`), `FlagState::is_enabled` /
   `from_bool` (`types.rs:18,23`), `StubHttpClient::new` (`types/traits.rs:55`),
   `TokenKind::api_value`, all the `routes.rs` path builders. Pedantic's
   `must_use_candidate` fires otherwise.

2. **`# Errors` doc section REQUIRED on every public fn returning `Result`**
   (clippy `missing_errors_doc`). Pattern from `liftwing.rs` / `scoring_engine.rs`:
   ```rust
   /// Build a ... request from a configured wiki.
   ///
   /// # Errors
   ///
   /// Returns [`LiftWingError`] when the revision ID is invalid or ...
   pub fn build_liftwing_score_request(...) -> Result<HttpRequest, LiftWingError> {
   ```
   Use the intra-doc link form `[`ErrorType`]`. (No `# Panics` section needed if
   you don't panic; avoid `.unwrap()`/`.expect()` in non-test code — tests may
   use `.expect("...")`.)

3. **`const fn` where possible** — pedantic pushes `missing_const_for_fn`.
   `FlagState::is_enabled`, `WarningLevel::severity`, `SessionActionKind::label`,
   `TokenKind::api_value` are all `pub const fn`. Make trivial accessors `const`.

4. **No raw float fields in `Eq` structs** (and `WikiConfig` derives `Eq`). The
   codebase keeps `f32` only where the struct does NOT derive `Eq`
   (`ScoringContext` has `liftwing_risk: Option<f32>` and derives only
   `PartialEq`, not `Eq`). If your verdict/finding type needs `Eq`, keep floats
   out of it; if it carries a score/probability, derive only `PartialEq` (and be
   aware that breaks use in any `Eq` context).

5. **Error enums via `thiserror`** with `#[error("...")]` messages, struct-style
   variants `{ message: String }` or `{ field: &'static str, message: String }`,
   and `#[from]` / `#[error(transparent)]` for wrapping (see `errors.rs`). Put a
   new `CitationError` in `crates/sp42-core/src/errors.rs` and re-export it from
   `lib.rs`'s `pub use errors::{...}` block. Pattern:
   ```rust
   #[derive(Debug, Error)]
   pub enum CitationError {
       #[error("citation verify request is invalid: {message}")]
       InvalidRequest { message: String },
       #[error("citation verify response is invalid: {message}")]
       InvalidResponse { message: String },
       #[error("citation serialization failed: {0}")]
       Json(#[from] serde_json::Error),
   }
   ```

6. **`use` ordering / grouping**: std first, then external crates, then `crate::`
   — rustfmt-enforced grouping with blank lines between groups (see top of every
   module). `cargo fmt --all -- --check` is in pre-commit; run it.

7. **Numeric literals get separators in pedantic** (`unreadable_literal`):
   `123_456`, `0.000_001` (see `liftwing.rs` tests, `action_contracts.rs`).

8. **Range `contains` over manual comparisons** (`manual_range_contains`):
   `(200..300).contains(&response.status)`, `(0.0..=1.0).contains(&p)` — see
   `liftwing.rs`. Use the same.

9. **`map_or`/`map_or_else`/`filter`/`and_then` combinators** instead of nested
   `match`/`if let` where pedantic would flag it (`liftwing.rs`,
   `context_builder.rs`, `xtask/src/main.rs:source_date_epoch` use `map_or_else`).

10. **Slices over `&Vec`** in fn params (`ptr_arg`): take `&[u8]` not `&Vec<u8>`
    (`parse_liftwing_score_response(body: &[u8])`).

11. **Async traits** use `#[async_trait]` (re-exported via `async_trait` dep);
    `HttpClient` is `Send + Sync`. Generic HTTP-executing fns are written
    `where C: HttpClient + ?Sized` (see `execute_liftwing_score`,
    `liftwing.rs:59–66`) — copy that signature shape for
    `execute_citation_verify`.

12. **Tests live in-file** under `#[cfg(test)] mod tests { ... }` with explicit
    `use super::{...};` imports (named, not glob where pedantic flags
    `wildcard_imports` — though tests sometimes use `proptest::prelude::*`, which
    is allowed). Use `futures::executor::block_on` to drive async fns; use the
    `StubHttpClient::new([Ok(HttpResponse { ... })])` double for HTTP. Use
    `crate::test_fixtures::fixture_wiki_config()` for a ready `WikiConfig`
    (only available `#[cfg(test)]`; `test_fixtures` is `pub(crate) mod` gated on
    `#[cfg(test)]` in `lib.rs:46–47`).

---

## 6. The request/response contract shape (HttpClient build/parse split)

The proven SP42 pattern for an external service (mirrors ADR-0008 Decision 3 and
exactly matches `liftwing.rs`) is THREE functions:
- `build_<x>_request(config, request) -> Result<HttpRequest, Error>` (pure)
- `execute_<x>(client, config, request) -> Result<Out, Error>` async, generic
  `where C: HttpClient + ?Sized` — calls build, `client.execute(...).await`,
  checks `(200..300).contains(&response.status)`, then parse
- `parse_<x>_response(body: &[u8]) -> Result<Out, Error>` (pure)

`HttpRequest` / `HttpResponse` / `HttpMethod` (from `sp42-types::transport`, re-
exported by `sp42-core::types`):
```rust
pub enum HttpMethod { Get, Post, Put, Patch, Delete }
pub struct HttpRequest { pub method: HttpMethod, pub url: Url,
    #[serde(default)] pub headers: BTreeMap<String,String>,
    #[serde(default)] pub body: Vec<u8> }
pub struct HttpResponse { pub status: u16,
    #[serde(default)] pub headers: BTreeMap<String,String>,
    #[serde(default)] pub body: Vec<u8> }
```
`HttpClient` trait (`sp42-types::traits`, re-exported `sp42-core::traits`):
```rust
#[async_trait]
pub trait HttpClient: Send + Sync {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, HttpClientError>;
}
```
Test double: `StubHttpClient::new(iter_of_Result<HttpResponse, HttpClientError>)`
returns queued responses FIFO. **This is the LLM/inference edge in SP42** — the
ADR set defers a dedicated `ModelClient`; the model endpoint is called over
`HttpClient`. So a citation-verify request is just another `HttpRequest` to the
configured `inference_url`, executed through an injected `HttpClient`, and tested
with `StubHttpClient` — no new trait, no new dep.

---

## 7. ADR-0008 §5 attach point — `CitationFinding` → `live_operator_view.rs`

**Confirmed.** ADR-0008 (the request/response contract ADR; note: ADR-0008 itself
is NOT in THIS tree's `docs/adr/` — that dir only holds schiste's 0001–0005;
0008 lives on the `docs/citation-verification-adrs` branch / `../SP42-adr-citation`
worktree per CLAUDE.md). The research-workflow scaffold
(`docs/implementation-notes/research-workflow.mjs:81`) and the task brief both
name `crates/sp42-reporting/src/live_operator_view.rs` as the attach point ADR-0008
§5 designates.

Shape of `LiveOperatorView` (full struct, `live_operator_view.rs:18–55`): it is
the browser/operator payload aggregating `queue: Vec<QueuedEdit>`, `diff`,
`media_diff`, `review_workbench`, `scoring_context`, `scenario_report`,
`session_digest`, `shell_state`, `capabilities`, `auth`, `backend`,
`action_status`/`action_history`/`action_preflight`, `public_documents`,
`heuristic_provenance`, `coordination_room`/`coordination_state`,
`debug_snapshot`, `telemetry`, `notes: Vec<String>`, `next_continue`.

A future `CitationFinding` would attach here as a new field, e.g.:
```rust
    #[serde(default)]
    pub citation_findings: Vec<CitationFinding>,
```
Conventions to honor when/if you add it:
- `#[serde(default)]` so existing serialized views stay compatible (matches
  `public_documents`, `heuristic_provenance`, `debug_snapshot`, `telemetry`).
- `LiveOperatorView` derives `Debug, Clone, PartialEq, Serialize, Deserialize`
  (NOT `Eq` — it transitively holds floats via `scoring_context`), so
  `CitationFinding` only needs those five derives, and **may carry a float
  score** since `Eq` is not required here (unlike `WikiConfig`).
- `CitationFinding` lives in `sp42-core` (the citation module) and is imported by
  `sp42-reporting` via `use sp42_core::{... , CitationFinding};` (the
  `live_operator_view.rs` `use sp42_core::{...}` block, lines 5–9). ADR-0008
  Decision 7 says `CitationFinding` retains `use_site_ordinal` (informational
  anchor for future node-anchored repair).

**First cut is CLI** (`sp42-cli`) per the brief, so the operator-view (browser)
display can be DEFERRED — you do NOT have to wire `live_operator_view.rs` to ship
the CLI citation lens. Land the `sp42-core::citation` types + the
build/execute/parse trio + tests first; the `LiveOperatorView` field is a later,
additive, serde-default step.

---

## Quick checklist for "compiles clean" before claiming done

1. `pub mod citation;` added alphabetically + grouped `pub use citation::{...}`
   in `lib.rs` (alphabetized inside braces).
2. New `CitationError` in `errors.rs` + re-exported in `lib.rs`'s `pub use
   errors::{...}` block.
3. Every public `Result`-returning fn has a `# Errors` doc section; every public
   pure value-returning fn has `#[must_use]`; trivial accessors are `const fn`.
4. No `unsafe` (crate forbids it). No new deps (reuse sha2/serde/serde_json/url/
   thiserror/async-trait); if a dep is unavoidable, its license must be in
   `deny.toml`'s allow-list and `Cargo.lock` updated.
5. `WikiConfig` new field is `Option<Url>`/`Vec<ModelRef>` with `#[serde(default)]`,
   AND the `lib.rs` doctest struct literal updated to name it
   (`inference_url: None,`).
6. Run `cargo fmt --all -- --check` then
   `cargo clippy --workspace --all-targets --all-features -- -D warnings` then
   `cargo test -p sp42-core` then `./scripts/ci-all.sh` (== pre-push gate).
