# Bare-URL Repair MVP Implementation Plan — Phase 1: Citoid lift

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** The Citoid client (`citation/citoid.rs` + `citation/urls.rs`) compiles and passes its tests on this branch, lifted **byte-identical** from the `impl/citation-verification` branch so the eventual merge auto-resolves.

**Architecture:** Pure, I/O-free client modules land in `sp42-core` (FCIS core). `citoid.rs` builds an `HttpRequest` value and parses response bytes — the actual fetch happens in the server shell (Phase 4). Only `crates/sp42-core/src/citation.rs` (the module-declaration file) intentionally diverges from the source branch; everything else is copied byte-for-byte via `git show`.

**Tech Stack:** Rust workspace; clippy `pedantic = deny`, rustc `warnings = deny` (workspace lints). Tests are in-module `#[cfg(test)]`, no fixture files needed for this phase.

**Scope:** Phase 1 of 7 from `docs/design-plans/2026-06-09-bare-url-repair-mvp.md`.

**Codebase verified:** 2026-06-09, branch `louie/bare-url-repair` @ `2ed57b3` (codebase-investigator + direct file reads).

---

**Working directory for every command:** `/var/home/louie/Projects/Volunteering-Consulting/SP42/.worktrees/bare-url-repair`

**Context you need:**

- The source branch `impl/citation-verification` exists locally (checked out at `.worktrees/louie/citation-work`). Copy file contents with `git show impl/citation-verification:<path>` so bytes are exact regardless of worktree state.
- `citoid.rs` (219 lines) imports `super::urls::encode_uri_component`, `crate::types::{HttpMethod, HttpRequest}`, serde, serde_json, url — all already available in this branch except the new module files themselves.
- `urls.rs` (370 lines) imports `regex::Regex` (a **new dependency** for `sp42-core`) and `crate::errors::CitationVerificationError` (a **new enum** for `errors.rs`).
- Neither file does I/O; both have self-contained in-module tests (6 in citoid.rs, 11 in urls.rs) using only inline literals.
- We lift **both** `CitationVerificationError` and `CitationStorageError` even though only the former is needed to compile: they are adjacent in the source branch's `errors.rs` and re-exported on the same `lib.rs` line, so taking both keeps those hunks byte-identical and lets the future merge auto-resolve. Both are `pub` and re-exported, so no dead-code lint fires.

### Task 1: Lift the Citoid client

**Files:**
- Modify: `crates/sp42-core/Cargo.toml` (add `regex` dependency)
- Modify: `crates/sp42-core/src/errors.rs` (insert two enums between `LiftWingError` and `DiffError`)
- Create: `crates/sp42-core/src/citation.rs`
- Create: `crates/sp42-core/src/citation/citoid.rs` (byte-identical copy)
- Create: `crates/sp42-core/src/citation/urls.rs` (byte-identical copy)
- Modify: `crates/sp42-core/src/lib.rs` (module decl + re-exports)

**Step 1: Add the `regex` dependency**

In `crates/sp42-core/Cargo.toml`, the `[dependencies]` section currently reads:

```toml
[dependencies]
async-trait.workspace = true
base64.workspace = true
serde.workspace = true
```

Add `regex = "1.12"` after `base64.workspace = true` (matching the source branch's placement and pinning):

```toml
[dependencies]
async-trait.workspace = true
base64.workspace = true
regex = "1.12"
serde.workspace = true
```

(`urls.rs` uses `LazyLock<Regex>` statics. Do **not** add `unicode-normalization` or move `futures` — those serve citation modules not lifted in this phase.)

**Step 2: Insert the citation error enums**

In `crates/sp42-core/src/errors.rs`, find this boundary (currently around lines 74–88):

```rust
#[derive(Debug, Error)]
pub enum LiftWingError {
    #[error("liftwing request is invalid: {message}")]
    InvalidRequest { message: String },
    #[error("liftwing response is invalid: {message}")]
    InvalidResponse { message: String },
    #[error("liftwing serialization failed: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Error)]
pub enum DiffError {
```

Insert the following between the closing `}` of `LiftWingError` and the `#[derive(Debug, Error)]` of `DiffError`. This block must be **byte-identical** to the source branch (it is `impl/citation-verification`'s `errors.rs` lines 84–109):

```rust
/// Errors raised by the per-model citation-verification edge (ADR-0008 §4).
///
/// `SourceUnavailable` carries a fixed, deterministic reason token so the caller can
/// short-circuit to a `source_unavailable` verdict without a model call (ADR-0007 §4).
#[derive(Debug, Error)]
pub enum CitationVerificationError {
    #[error("citation verify request is invalid: {message}")]
    InvalidRequest { message: String },
    #[error("citation source unavailable: {reason}")]
    SourceUnavailable { reason: &'static str },
    #[error("citation verify response is invalid: {message}")]
    InvalidResponse { message: String },
    #[error("citation verify serialization failed: {0}")]
    Json(#[from] serde_json::Error),
}

/// Errors raised when persisting/loading source snapshots and verdict records (ADR-0009).
#[derive(Debug, Error)]
pub enum CitationStorageError {
    #[error("citation storage input is invalid: {message}")]
    InvalidInput { message: String },
    #[error("citation storage serialization failed: {message}")]
    Serialize { message: String },
    #[error("citation storage backend failed: {message}")]
    Storage { message: String },
}
```

**Step 3: Copy the module files byte-identically**

```bash
mkdir -p crates/sp42-core/src/citation
git show impl/citation-verification:crates/sp42-core/src/citation/citoid.rs > crates/sp42-core/src/citation/citoid.rs
git show impl/citation-verification:crates/sp42-core/src/citation/urls.rs > crates/sp42-core/src/citation/urls.rs
```

Expected: both commands succeed silently; `wc -l` reports 219 and 370 lines respectively.

**Step 4: Create the module-declaration file**

Create `crates/sp42-core/src/citation.rs` with exactly this content. (This is the **one intentionally divergent file**: the source branch declares 12 submodules; this branch lifts only two. The design plan records the resulting merge conflict as trivially take-theirs.)

```rust
//! Citation support: the Citoid bibliographic-metadata client and citation
//! URL helpers, lifted verbatim from the `impl/citation-verification` branch.
//!
//! Only the `citoid` and `urls` submodules exist on this branch; the full
//! citation-verification module set arrives when that branch merges. This
//! declaration file then takes that branch's version (a known take-theirs
//! conflict, recorded in the bare-URL repair design plan).

pub mod citoid;
pub mod urls;
```

**Step 5: Wire up `lib.rs`**

Three edits to `crates/sp42-core/src/lib.rs`:

(a) In the module-declaration block (currently lines 28–54), add `pub mod citation;` between `branding` and `context_builder`:

```rust
pub mod branding;
pub mod citation;
pub mod context_builder;
```

(b) Insert the citation re-export blocks between the `article_inventory` and `context_builder` re-export blocks. The current text reads:

```rust
pub use article_inventory::{
    ArticleInventory, ArticleReference, article_inventory_notes, build_article_inventory,
};
pub use context_builder::{ContextInputs, build_scoring_context};
```

After the `article_inventory` block's closing `};`, insert these two blocks **byte-identical to the source branch's lines**:

```rust
pub use citation::citoid::{
    CitoidMetadata, build_citoid_header, build_citoid_request, parse_citoid_response,
};
pub use citation::urls::{
    ResolvedUrl, build_article_html_url, check_fetchable_source_url, is_archive_url,
    is_valid_wiki_code, parse_revision_from_etag, resolve_citation_url, rewrite_wayback_url,
};
```

(c) Replace the `pub use errors::{...}` block with the source branch's version (adds the two new names in alphabetical position). Old:

```rust
pub use errors::{
    ActionError, BacklogRuntimeError, DevAuthError, DiffError, EventSourceError, HttpClientError,
    LiftWingError, OAuthError, PublicDocumentError, RecentChangesError, ReviewWorkbenchError,
    ScoringError, ScoringEvaluationError, ScoringPolicyError, StorageError, StreamIngestorError,
    StreamRuntimeError, TrainingDataError, UserAnalysisError, WebSocketError, WikiStorageError,
};
```

New (byte-identical to `impl/citation-verification`):

```rust
pub use errors::{
    ActionError, BacklogRuntimeError, CitationStorageError, CitationVerificationError,
    DevAuthError, DiffError, EventSourceError, HttpClientError, LiftWingError, OAuthError,
    PublicDocumentError, RecentChangesError, ReviewWorkbenchError, ScoringError,
    ScoringEvaluationError, ScoringPolicyError, StorageError, StreamIngestorError,
    StreamRuntimeError, TrainingDataError, UserAnalysisError, WebSocketError, WikiStorageError,
};
```

**Step 6: Verify byte-identical lift**

```bash
diff crates/sp42-core/src/citation/citoid.rs /var/home/louie/Projects/Volunteering-Consulting/SP42/.worktrees/louie/citation-work/crates/sp42-core/src/citation/citoid.rs
diff crates/sp42-core/src/citation/urls.rs /var/home/louie/Projects/Volunteering-Consulting/SP42/.worktrees/louie/citation-work/crates/sp42-core/src/citation/urls.rs
```

Expected: **no output** from either diff. (If the worktree diverges from the branch head, prefer the `git show` bytes and note it; the DoD comparison target is the branch.)

**Step 7: Run the lifted tests**

```bash
cargo test -p sp42-core citation::
```

Expected: 17 tests pass (6 `citation::citoid::tests::*`, 11 `citation::urls::tests::*`), 0 failures, **unmodified** test code.

**Step 8: Full crate check**

```bash
cargo test -p sp42-core
cargo clippy -p sp42-core --all-targets --all-features -- -D warnings
```

Expected: all tests pass; clippy reports no warnings.

**Step 9: Commit**

```bash
git add crates/sp42-core/Cargo.toml Cargo.lock crates/sp42-core/src/errors.rs crates/sp42-core/src/citation.rs crates/sp42-core/src/citation/ crates/sp42-core/src/lib.rs
git commit -m "feat: lift Citoid client verbatim from impl/citation-verification"
```

(Include `Cargo.lock` only if the `regex` addition changed it.)
