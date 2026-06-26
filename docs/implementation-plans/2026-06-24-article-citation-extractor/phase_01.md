# Article Citation Extractor Implementation Plan — Phase 1: Types

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** Define the plain-data contracts that cross the editor→core boundary and the orchestrator's output.

**Architecture:** FCIS split. The Parsoid `!Send` DOM is confined to `sp42-server`; the editor emits plain `Send` `ParsoidBlock` data; all heuristic logic and the page report live in pure `sp42-core`. This phase adds only types — no behavior.

**Tech Stack:** Rust workspace; `serde`, `url::Url`. Inline `#[cfg(test)] mod tests` with `#[test]` (sync) per `sp42-core` convention.

**Scope:** Phase 1 of 6.

**Codebase verified:** 2026-06-24

- `crates/sp42-core/src/wikitext_editor.rs` holds the `WikitextEditor` trait and `WikitextNodeDescriptor` (lines 65–75). New block types belong here, beside the trait that returns them.
- `crates/sp42-core/src/citation/` has **no `mod.rs`**; submodules (`verify`, `prompts`, `concurrency`, …) are declared in a module file and re-exported in `crates/sp42-core/src/lib.rs` (existing block: `pub use citation::verify::{ … };`).
- `CitationFinding`, `CitationVerificationRequest`, `VerificationOutcome` derive `Debug, Clone, PartialEq, Eq, Serialize, Deserialize` (`verify.rs`). `ClaimContext` derives `Debug, Clone, Default, PartialEq, Eq` and **does not** derive serde (`prompts.rs:21`).
- `url::Url` serde is enabled in the workspace (`CitationVerificationRequest.source_url: Url` already serializes).

---

## Task 1: Block intermediate types (`ParsoidBlock`, `BlockRef`, `BlockKind`)

**Files:**
- Modify: `crates/sp42-core/src/wikitext_editor.rs` (add after `WikitextNodeDescriptor`, ~line 75)

**Step 1: Add the types**

```rust
/// Kind of prose-bearing block a citation can appear in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Paragraph,
    ListItem,
    TableCell,
    Other,
}

/// One inline `<ref>` within a [`ParsoidBlock`], in document order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockRef {
    /// Byte offset into [`ParsoidBlock::text`] where the marker sat — the
    /// position of the punctuation it follows. Anchors claim↔ref association.
    pub offset: usize,
    /// Stable cite id of the inline marker, e.g. `"cite_ref-smith_3-0"`.
    pub ref_id: String,
    /// Source URL(s) read from the ref's structured `data-mw` cite-template
    /// params (`url=`, `archive-url=`) via the parsoid crate; for a bare-URL
    /// ref with no template, from the structured ExtLink node. Empty ⇒ a
    /// non-URL ref (book/ISBN) that the core records as skipped.
    pub source_urls: Vec<url::Url>,
    /// Rendered text of the marker (e.g. `"[3]"`), for provenance.
    pub ref_text: String,
    /// `true` when this is a reuse of a `<ref name="…">`.
    pub named: bool,
}

/// A single prose-bearing block emitted by the editor's one DOM pass.
/// Plain `Send` data: no DOM handles. Ref markers are removed from `text`
/// but their positions are preserved as [`BlockRef::offset`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsoidBlock {
    /// Visible text of the block with ref markers removed.
    pub text: String,
    /// Heading stack from page root to this block, outermost first
    /// (e.g. `["History", "Early life"]`).
    pub section_path: Vec<String>,
    /// Inline refs in this block, in document order.
    pub refs: Vec<BlockRef>,
    pub block_kind: BlockKind,
    /// Document-order index of the block within the page.
    pub block_ordinal: usize,
}
```

**Step 2: Re-export from lib.rs**

In `crates/sp42-core/src/lib.rs`, find the existing `pub use wikitext_editor::{ … };` block (use `rg -n "pub use wikitext_editor" crates/sp42-core/src/lib.rs`) and add `BlockKind, BlockRef, ParsoidBlock` to it.

**Step 3: Verify it compiles**

Run: `PATH="$HOME/.cargo/bin:$PATH" cargo build -p sp42-core`
Expected: builds without errors.

**Step 4: Commit**

```bash
git add crates/sp42-core/src/wikitext_editor.rs crates/sp42-core/src/lib.rs
SP42_SKIP_GIT_HOOKS=1 git commit -m "feat(citation): add ParsoidBlock/BlockRef/BlockKind intermediate types

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_015NtcEzaj8J373GGsv3FC7y"
```

---

## Task 2: Use-site and extraction-outcome types

**Files:**
- Create: `crates/sp42-core/src/citation/extract.rs`
- Modify: the citation module file that declares submodules (find with `rg -n "pub mod verify" crates/sp42-core/src`) — add `pub mod extract;`
- Modify: `crates/sp42-core/src/lib.rs` (re-exports)

**Step 1: Create `extract.rs` with the types**

```rust
//! Article-level claim↔ref extraction: turning the editor's `ParsoidBlock`
//! intermediate into per-use-site verification inputs. Pure, no DOM, no I/O.

use crate::citation::page::PageVerificationRequest;
use crate::citation::prompts::ClaimContext;
use crate::citation::verify::CitationVerificationRequest;

/// One citation use-site: a claim sentence, one source URL, and the context
/// passed alongside it. The unit the orchestrator fans the verifier over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CitationUseSite {
    /// Document-order index across the page.
    pub use_site_ordinal: u32,
    /// Document-order index of the block this use-site came from (provenance;
    /// used to attribute verify errors back to a block in the report).
    pub block_ordinal: usize,
    /// Claim + source URL + page identity for the verifier.
    pub request: CitationVerificationRequest,
    /// Section title + preceding sentences passed alongside the claim.
    pub context: ClaimContext,
    /// The originating ref's marker id, for provenance.
    pub ref_id: String,
}

/// Why a ref produced no use-site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkippedReason {
    /// The ref carries no extractable URL (book/ISBN/offline source).
    NonUrlSource,
}

/// A ref that was intentionally not verified.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SkippedRef {
    pub ref_id: String,
    pub reason: SkippedReason,
    pub block_ordinal: usize,
}

/// A block (or ref) that could not be processed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BlockFailure {
    pub block_ordinal: usize,
    pub reason: String,
}

/// Result of extracting use-sites from a page's blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractOutcome {
    pub use_sites: Vec<CitationUseSite>,
    pub skipped: Vec<SkippedRef>,
    pub failures: Vec<BlockFailure>,
}
```

**Step 2: Declare the module**

Add `pub mod extract;` alongside the existing `pub mod verify;` declaration (location from the `rg` above).

**Step 3: Re-export from lib.rs**

Add to `crates/sp42-core/src/lib.rs` under the citation section:

```rust
pub use citation::extract::{
    BlockFailure, CitationUseSite, ExtractOutcome, SkippedReason, SkippedRef,
};
```

**Step 4: Verify**

Run: `PATH="$HOME/.cargo/bin:$PATH" cargo build -p sp42-core`
Expected: builds (will succeed once Task 3 adds the `page` module it imports; if you run before Task 3, expect "unresolved import `crate::citation::page`" — do Task 3 then build).

---

## Task 3: Page request, report, and stats types

**Files:**
- Create: `crates/sp42-core/src/citation/page.rs`
- Modify: citation module file — add `pub mod page;`
- Modify: `crates/sp42-core/src/lib.rs` (re-exports)
- Test: inline in `page.rs`

**Step 1: Write the failing serde round-trip test**

Create `crates/sp42-core/src/citation/page.rs` with the types and a test:

```rust
//! Page-level verification: request, orchestrator output report, and stats.

use crate::citation::extract::{BlockFailure, SkippedRef};
use crate::citation::verify::CitationFinding;

/// Identity of the page to verify.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PageVerificationRequest {
    pub wiki_id: String,
    pub title: String,
    pub rev_id: u64,
}

/// Counts summarising a page run.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PageVerificationStats {
    pub refs_seen: usize,
    pub use_sites_verified: usize,
    pub skipped: usize,
    pub extraction_failures: usize,
    pub supported: usize,
    pub partial: usize,
    pub not_supported: usize,
    pub source_unavailable: usize,
}

/// Read-only result of verifying every URL-bearing citation on a page.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PageVerificationReport {
    pub wiki_id: String,
    pub rev_id: u64,
    pub title: String,
    pub findings: Vec<CitationFinding>,
    pub skipped: Vec<SkippedRef>,
    pub extraction_failures: Vec<BlockFailure>,
    pub stats: PageVerificationStats,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_round_trips_through_serde() {
        let report = PageVerificationReport {
            wiki_id: "frwiki".to_string(),
            rev_id: 42,
            title: "Exemple".to_string(),
            findings: Vec::new(),
            skipped: Vec::new(),
            extraction_failures: Vec::new(),
            stats: PageVerificationStats::default(),
        };
        let json = serde_json::to_string(&report).expect("serialize");
        let back: PageVerificationReport =
            serde_json::from_str(&json).expect("deserialize");
        assert_eq!(report, back);
    }
}
```

**Step 2: Declare the module and re-export**

- Add `pub mod page;` alongside `pub mod verify;`.
- Add to `lib.rs`:

```rust
pub use citation::page::{
    PageVerificationReport, PageVerificationRequest, PageVerificationStats,
};
```

**Step 3: Run the test**

Run: `PATH="$HOME/.cargo/bin:$PATH" cargo test -p sp42-core report_round_trips_through_serde -- --exact`
Expected: PASS.

**Step 4: Build the whole crate (confirms Task 2's import resolves)**

Run: `PATH="$HOME/.cargo/bin:$PATH" cargo build -p sp42-core`
Expected: builds without errors.

**Step 5: Commit**

```bash
git add crates/sp42-core/src/citation/extract.rs crates/sp42-core/src/citation/page.rs crates/sp42-core/src/lib.rs crates/sp42-core/src/citation*.rs
SP42_SKIP_GIT_HOOKS=1 git commit -m "feat(citation): add use-site, page request, and report types

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_015NtcEzaj8J373GGsv3FC7y"
```

**Done when:** `sp42-core` builds; the serde round-trip test passes; `CitationUseSite`, `ExtractOutcome`, `PageVerificationReport`, `PageVerificationRequest`, `PageVerificationStats`, `SkippedRef`, `BlockFailure`, `ParsoidBlock`, `BlockRef`, `BlockKind` are all public.
