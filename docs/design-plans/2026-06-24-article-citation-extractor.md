# Article Citation Extractor & Page Orchestrator Design

## Summary

SP42's existing citation verifier operates on a single use-site at a time: a
caller supplies one claim, one source URL, and one context object, and gets back
one verdict. This design adds the layer above that: given a wiki page
identifier, fetch the page through Parsoid, decompose every ref-bearing
paragraph into its constituent claim sentences, and fan the existing
per-use-site verifier over all of them to produce a page-level report. No edits
are made; the route is purely read-only.

The central engineering challenge is that the Parsoid DOM is `!Send` — it cannot
cross thread or async boundaries — so all heuristic logic (sentence splitting,
claim-to-ref association, context assembly, orchestration) must live in the pure
`sp42-core` crate where it can be unit-tested without a real DOM. The design
solves this with a functional-core / imperative-shell cut: the
`ParsoidWikitextEditor` does one synchronous DOM pass and emits a plain
`Vec<ParsoidBlock>`, then hands off to pure core functions that are oblivious to
Parsoid. A secondary concern is fetch efficiency: because many refs on a page
can cite the same URL, the orchestrator dedupes HTTP fetches before fanning
verification, sharing the fetched body through an additive `prefetched` option
on the existing `VerifyOptions` struct — leaving the single-use-site call path
byte-identical.

## Definition of Done

A read-only server route accepts a `{wiki_id, title, rev_id}` and returns a
page-level citation report: for every URL-bearing `<ref>` on the page, the
claim sentence it supports is verified against its source via the existing
per-use-site verifier, with section title and preceding sentences passed as
`ClaimContext`. Non-URL refs and extraction failures are reported separately,
never silently dropped. The route performs no edits. All extraction and
association logic lives in pure `sp42-core` and is unit-tested without a live
Parsoid DOM. The single-claim `verify_citation_use_site` path remains
byte-identical for callers that don't pre-fetch.

## Glossary

- **Parsoid**: A MediaWiki service (and corresponding Rust crate used here) that converts wikitext to structured HTML with machine-readable annotations. SP42 uses it to read page content without hand-parsing wikitext.
- **`data-mw`**: A JSON attribute Parsoid attaches to template nodes in the output HTML, carrying the structured parameters of wiki templates (e.g., `url=`, `archive-url=`). The design reads citation URLs from here rather than scraping raw HTML text.
- **`!Send`**: A Rust trait-marker meaning a type cannot be transferred to another thread. The kuchikiki DOM (the HTML tree that Parsoid produces) is `!Send`, which is the root reason heuristic logic must be separated into a thread-safe crate.
- **Functional-core / imperative-shell (FCIS)**: An architectural pattern where side-effecting code (I/O, DOM access, network) is confined to a thin outer shell, and all logic that can be is written as pure functions in an inner core. Used here to make the sentence-segmentation and association logic unit-testable without a live DOM or network.
- **`sp42-core`**: The pure-Rust library crate in this repo that holds all domain logic with no side effects. Can be tested without a running server or Parsoid instance.
- **`sp42-server`**: The axum-based HTTP server crate. Holds Parsoid-dependent code, route handlers, and server state. The `!Send` DOM is confined here.
- **`kuchikiki`**: The Rust HTML DOM library used by the `parsoid` crate. Its tree nodes are `!Send`, driving the editor-boundary design.
- **`ParsoidBlock`**: New intermediate type emitted by the editor after its DOM pass. A plain-data struct representing one prose block (paragraph, list item, table cell) with its ref markers stripped out and recorded separately as `BlockRef` values.
- **`BlockRef`**: The plain-data record for one inline `<ref>` within a `ParsoidBlock`, carrying its character offset into the cleaned text and the source URLs read from structured Parsoid data.
- **`char_offset`**: The character position within a block's cleaned text where a ref marker sat. Used to associate a ref with the sentence that immediately precedes it.
- **`CitationUseSite`**: The per-ref unit handed to the verifier: one claim sentence, one source URL, and one `ClaimContext`. A single ref with multiple URLs produces multiple use-sites.
- **`ClaimContext`**: Existing SP42 type carrying the article title, section title, and up to three preceding sentences — the window of context passed to the LLM verifier alongside the claim.
- **`PageVerificationReport`**: The top-level output of the new route: all verdicts, all skipped refs (e.g., book citations with no URL), all extraction failures, and summary stats.
- **`verify_citation_use_site`**: The existing single-use-site verifier function in `sp42-core`. This design calls it in a fan-out; its signature is unchanged.
- **`map_with_concurrency`**: Existing SP42 utility in `sp42-core/src/citation/concurrency.rs` that runs async tasks with a bounded concurrency limit. Reused here to fan verification over use-sites and to dedupe-fetch source URLs.
- **`VerifyOptions`**: Existing options struct passed to `verify_citation_use_site`. This design adds one additive field (`prefetched`) to allow the orchestrator to supply a pre-fetched source body, avoiding redundant HTTP requests.
- **`FetchedSource`**: The type representing an already-fetched source document. When passed via `prefetched`, the verifier skips its own HTTP fetch.
- **axum**: The async Rust web framework used for SP42's HTTP server. Route handlers are thin axum handler functions.
- **`ExtLink`**: A Parsoid node type representing a bare external URL in wikitext (i.e., a URL used directly as a ref rather than wrapped in a citation template). The design reads URLs from these nodes for bare-URL refs.
- **Named ref (`<ref name="…">`)**: A wikitext feature allowing one ref definition to be reused at multiple points in an article. The design records these as `named: bool` on `BlockRef`; each use-site is still verified independently.
- **Panel voting**: SP42's existing pattern of querying multiple LLM models and aggregating their verdicts. `verify_page` fans over use-sites using the same panel infrastructure.
- **`WikitextEditor` trait**: The core trait in `sp42-core` that the `ParsoidWikitextEditor` implements. Defines the interface between server-side DOM code and core logic. `extract_blocks` is the new method added here.
- **`collect_bare_url_proposals` / bare-url route**: The existing read-only citation route in `citation_routes.rs` that this design mirrors structurally. Understanding it explains the route shape: no session gating, no apply side, editor + config injected.
- **`route_contracts`**: The SP42 module holding HTTP path constants shared between server and client code. `DEV_CITATION_VERIFY_PAGE_PATH` is added here.
- **`block_ordinal`**: Document-order index of a `ParsoidBlock` within the page. Used to preserve and report document ordering in the output.
- **`SkippedRef`**: Report entry for a ref that was intentionally not verified — primarily non-URL refs (books, ISBNs) that have no source URL to fetch.
- **`BlockFailure`**: Report entry for a block or ref that could not be processed due to a parsing or extraction error. Distinct from `SkippedRef` (intentional) vs. failure (unexpected).
- **Molecular Facts / multi-claim decomposition**: A deferred future pass that would split a single sentence into atomic sub-claims for finer-grained verification. Explicitly out of scope for v1; a single sentence is treated as one claim.

## Architecture

The feature reads a wiki page through Parsoid, extracts every citation
use-site, and fans the existing per-use-site verifier over them to produce a
read-only page report. Work is split across the `!Send` Parsoid DOM boundary
following a functional-core / imperative-shell (FCIS) cut: the editor (shell)
does one DOM pass and emits plain `Send` data; everything heuristic (sentence
segmentation, claim↔ref association, context assembly, orchestration) is pure
`sp42-core` (functional core), unit-tested against hand-built fixtures.

```
sp42-server (has Parsoid DOM, !Send)        sp42-core (pure, Send, unit-tested)
────────────────────────────────────        ──────────────────────────────────
ParsoidWikitextEditor
  .extract_blocks(config, page)  ────────►  Vec<ParsoidBlock>   (new trait method)
                                                │
                                                ▼
                                         extract_use_sites(blocks, title)
                                           ├─ segment_sentences
                                           ├─ associate ref → sentence (char_offset)
                                           └─ build ClaimContext
                                                │
                                                ▼
                                         Vec<CitationUseSite>
                                                │
post_verify_page route  ───────────────────►  verify_page(fetch, model, …, use_sites)
  (mirrors bare-url proposals)                 ├─ dedupe fetches by URL
                                               └─ map_with_concurrency over
                                                  verify_citation_use_site
                                                │
                                                ▼
                                         PageVerificationReport  (read-only)
```

**Components and responsibilities:**

- **`ParsoidWikitextEditor::extract_blocks`** (new method on the
  `WikitextEditor` trait, `crates/sp42-core/src/wikitext_editor.rs`; Parsoid
  impl in `crates/sp42-server/src/parsoid_editor.rs`) — single DOM pass, emits
  `Vec<ParsoidBlock>`. The only component that understands Parsoid structure.
  URL extraction is structured, via the `parsoid` crate's `data-mw`
  cite-template params (and structured ExtLink nodes for bare-URL refs), never
  HTML scraping — consistent with how `enumerate_nodes`/`set_template_params`
  already read structured nodes.

- **`segment_sentences`** (`crates/sp42-core/src/citation/`) — hand-rolled
  rule-based splitter with an abbreviation guard list; returns sentences with
  char ranges back into the block text. Swappable for a crate later.

- **`extract_use_sites`** (`crates/sp42-core/src/citation/`) — pure association:
  segments each block, attaches each `BlockRef` to the sentence its
  `char_offset` falls in, builds `ClaimContext`. Produces `Vec<CitationUseSite>`
  plus skipped/failure records.

- **`verify_page`** (`crates/sp42-core/src/citation/`) — orchestrator. Dedupes
  source fetches by URL, fans `verify_citation_use_site` over the use-sites with
  the existing `map_with_concurrency`, assembles `PageVerificationReport`. Owns
  no Parsoid and no HTTP wiring; testable with mock fetch/model clients like the
  existing verify tests.

- **`post_verify_page` route** (`crates/sp42-server/src/citation_routes.rs`) —
  thin axum handler mirroring `collect_bare_url_proposals`: read-only, no
  session gating (no apply counterpart, since this never edits).

**Data flow across the boundary.** The editor never returns DOM handles — only
`ParsoidBlock` (plain data). The `!Send` kuchikiki DOM stays confined to the
editor exactly as it does for today's `enumerate_nodes`/`apply_revision_edit`.
Core consumes `Vec<ParsoidBlock>` and produces a report; it can be exercised end
to end with no network and no DOM.

### Contract: editor → core intermediate

```rust
pub struct ParsoidBlock {
    /// Visible text of the block with ref markers REMOVED, so sentence
    /// segmentation sees clean prose.
    pub text: String,
    /// Heading stack from page root down to this block, outermost first:
    /// ["History", "Early life"].
    pub section_path: Vec<String>,
    /// Each inline <ref> in this block, in document order.
    pub refs: Vec<BlockRef>,
    pub block_kind: BlockKind,   // Paragraph | ListItem | TableCell | Other
    pub block_ordinal: usize,    // document-order index of the block
}

pub struct BlockRef {
    /// Char offset into `text` where the marker sat (the position of the
    /// punctuation it follows). Anchors claim↔ref association.
    pub char_offset: usize,
    /// Stable cite id, e.g. "cite_note-smith-3".
    pub ref_id: String,
    /// Source URL(s) read from the ref's structured data-mw cite-template
    /// params (url=, archive-url=) via the parsoid crate; for a bare-URL ref
    /// with no template, from the structured ExtLink node. Empty ⇒ non-URL
    /// ref (book/ISBN) ⇒ core records it skipped.
    pub source_urls: Vec<Url>,
    /// Raw rendered ref text, for provenance/debugging.
    pub ref_text: String,
    pub named: bool,             // <ref name="…"> reuse
}
```

### Contract: core use-site and report

```rust
pub struct CitationUseSite {
    pub use_site_ordinal: u32,           // document order across the page
    pub request: CitationVerificationRequest,  // wiki_id, rev_id, title, claim, source_url
    pub context: ClaimContext,           // article_title, section_title, preceding_sentences
    pub ref_id: String,
}

pub struct PageVerificationReport {
    pub wiki_id: String,
    pub rev_id: u64,
    pub title: String,
    pub findings: Vec<CitationFinding>,        // one per verified use-site
    pub skipped: Vec<SkippedRef>,              // non-URL refs, with reason
    pub extraction_failures: Vec<BlockFailure>,// blocks/refs we couldn't process
    pub stats: PageVerificationStats,          // refs seen, verified, skipped, by-verdict tallies
}
```

### Contract: server route

```
POST /dev/citation/verify-page   (new DEV_CITATION_VERIFY_PAGE_PATH in route_contracts)

Request:  { "wiki_id": String, "title": String, "rev_id": u64 }
Response: PageVerificationReport (serialized; see above)

Read-only. No session/CSRF gating (mirrors bare-url *proposals*).
Hard-fails only on page-level problems: parsoid_url not configured,
revision not found, page unreachable.
```

### Touch to existing code: pre-fetch option

`verify_citation_use_site` fetches its source internally with no cache. To let
the orchestrator dedupe fetches across a page without forking that function, add
one additive field to `VerifyOptions`:

```rust
// VerifyOptions gains:
//   prefetched: Option<&FetchedSource>
// When present, verify_citation_use_site uses it instead of fetching.
// When absent (the single-claim path), behavior is byte-identical to today.
```

## Existing Patterns

Investigation (codebase-investigator, 2026-06-24) confirmed the design follows
established SP42 patterns:

- **Editor as the sole Parsoid boundary.** `ParsoidWikitextEditor`
  (`crates/sp42-server/src/parsoid_editor.rs`) is a zero-sized struct
  implementing the `WikitextEditor` trait (`crates/sp42-core/src/wikitext_editor.rs`).
  The kuchikiki DOM is `!Send` and confined to synchronous DOM passes that never
  cross `.await`. `extract_blocks` is a new trait method following the exact
  shape of the existing `enumerate_nodes` (takes `config` + `page`, returns
  plain `Send` descriptors). Structured node reads via the `parsoid` crate's
  `data-mw` access mirror how `set_template_params` and bare-url already read
  template params.

- **Read-only route mirrors bare-url proposals.**
  `collect_bare_url_proposals` / `post_bare_url_proposals`
  (`crates/sp42-server/src/citation_routes.rs`, registered at
  `DEV_CITATION_BARE_URL_PROPOSALS_PATH`) is the template: editor + config
  injected, read-only, no session gating. The verify-page route reuses this
  shape. Only the *apply* side of bare-url is session/CSRF-gated; this feature
  has no apply side.

- **Bounded fan-out reuses `map_with_concurrency`**
  (`crates/sp42-core/src/citation/concurrency.rs`), the same primitive panel
  voting uses in `verify.rs`.

- **Verifier and context contracts unchanged.** `verify_citation_use_site`,
  `CitationVerificationRequest`, `ClaimContext`, and `CitationFinding`
  (`crates/sp42-core/src/citation/verify.rs`, `prompts.rs`) are reused as-is.
  The only modification is the additive `prefetched` option on `VerifyOptions`,
  which keeps the single-claim path byte-identical.

- **FCIS split** matches the codebase's separation of pure core logic from the
  imperative server shell. The heuristic-heavy, iteration-prone code
  (segmentation, association) lives in core where it is unit-testable.

New surface introduced: `extract_blocks` trait method, the `ParsoidBlock` /
`BlockRef` intermediate, the `CitationUseSite` / `PageVerificationReport` types,
`segment_sentences` / `extract_use_sites` / `verify_page` in core, and the
`post_verify_page` route.

## Implementation Phases

### Phase 1: Intermediate and use-site types
**Goal:** Define the plain-data contracts that cross the editor→core boundary
and the orchestrator's output.

**Components:**
- `ParsoidBlock`, `BlockRef`, `BlockKind` in `crates/sp42-core/src/` (module
  alongside the wikitext/citation types).
- `CitationUseSite`, `PageVerificationReport`, `SkippedRef` (with a reason
  enum incl. `NonUrlSource`), `BlockFailure`, `PageVerificationStats` in
  `crates/sp42-core/src/citation/`.

**Dependencies:** None.

**Done when:** Types compile, derive the serde/Debug traits used by sibling
citation types, and round-trip through serde in a unit test (the report
serializes for the route).

### Phase 2: Sentence segmentation
**Goal:** Split block prose into sentences with char ranges.

**Components:**
- `segment_sentences(text: &str) -> Vec<Sentence>` in
  `crates/sp42-core/src/citation/` (`Sentence { text, range: Range<usize> }`),
  hand-rolled rule splitter with an abbreviation guard list.

**Dependencies:** Phase 1.

**Done when:** Table-driven unit tests pass for terminal punctuation,
abbreviations (`U.S.`, `Dr.`, `c. 1500`), decimals, quotes/parens, and short
fragments; ranges index back into the input correctly.

### Phase 3: Claim↔ref association
**Goal:** Turn `Vec<ParsoidBlock>` into `Vec<CitationUseSite>` plus skip/failure
records.

**Components:**
- `extract_use_sites(blocks: &[ParsoidBlock], article_title: &str)` in
  `crates/sp42-core/src/citation/` — per block: segment, attach each `BlockRef`
  to the sentence containing `char_offset - 1` (paragraph/block-text fallback
  when no sentence matches), build `ClaimContext` (most-specific
  `section_path` entry; up to 3 preceding in-block sentences). Non-URL refs →
  `skipped`.

**Dependencies:** Phases 1–2.

**Done when:** Unit tests over hand-built `ParsoidBlock` fixtures assert correct
association for: end-of-sentence ref, multiple refs after one sentence
(each its own use-site, shared claim), mid-sentence ref, paragraph-trailing
ref, list-item/table-cell fallback, named-ref reuse, and non-URL skip. Char
offset boundary cases covered.

### Phase 4: Page orchestrator
**Goal:** Fan the existing verifier over use-sites with fetch dedupe; assemble
the report.

**Components:**
- `prefetched: Option<&FetchedSource>` additive field on `VerifyOptions` and
  its use in `verify_citation_use_site` (`crates/sp42-core/src/citation/verify.rs`).
- `verify_page(...)` in `crates/sp42-core/src/citation/` — dedupe distinct
  source URLs, fetch each once via `map_with_concurrency`, fan
  `verify_citation_use_site` with the prefetched body, assemble
  `PageVerificationReport`. Failure isolation: one bad ref never sinks the page.

**Dependencies:** Phases 1–3.

**Done when:** Tests with mock fetch/model clients assert: a URL shared by N
use-sites is fetched once; concurrency is bounded; per-use-site verify failure
becomes a report entry, not a thrown error; report stats tally correctly; the
single-claim `verify_citation_use_site` path is unchanged when `prefetched` is
`None`.

### Phase 5: Parsoid block extraction
**Goal:** Implement the editor's DOM→`ParsoidBlock` pass.

**Components:**
- `extract_blocks(&self, config, page) -> Result<Vec<ParsoidBlock>, _>` added to
  the `WikitextEditor` trait (`crates/sp42-core/src/wikitext_editor.rs`) and
  implemented on `ParsoidWikitextEditor`
  (`crates/sp42-server/src/parsoid_editor.rs`): walk prose-bearing blocks in
  document order, strip ref markers while recording `char_offset`, capture the
  heading stack, read source URLs from structured `data-mw` / ExtLink nodes.

**Dependencies:** Phase 1 (the `ParsoidBlock` type).

**Done when:** Tests against a small set of captured real Parsoid HTML fixtures
assert correct block segmentation, heading stacks, ref char offsets, and URL
extraction (cite-template `url=` and bare-URL cases). This is the only component
needing real-DOM fixtures.

### Phase 6: Server route
**Goal:** Expose the orchestrator as a read-only HTTP route.

**Components:**
- `collect_page_verification(...)` and `post_verify_page` axum handler in
  `crates/sp42-server/src/citation_routes.rs`, mirroring the bare-url proposals
  handler; `DEV_CITATION_VERIFY_PAGE_PATH` constant in `route_contracts`; route
  registration in `routes.rs`. Sources the inference panel/model client from
  server state (introducing that wiring if not already present server-side).

**Dependencies:** Phases 4–5.

**Done when:** An integration test with a stub editor returning canned
`ParsoidBlock`s and a mock model client drives the route end to end and asserts
the serialized `PageVerificationResponse`; route hard-fails cleanly when
`parsoid_url` is unconfigured.

## Additional Considerations

**Failure isolation.** The route hard-fails only on page-level problems
(`parsoid_url` absent, revision not found, page unreachable). A single
unparseable ref or unsegmentable block goes to `extraction_failures`; a fetch or
verify error for one use-site becomes a finding/entry in the report. One bad ref
never sinks the page.

**Fetch dedupe.** A page reuses the same `source_url` across many refs and the
per-call fetcher has no cache, so the orchestrator fetches each distinct URL once
and shares the body via the `prefetched` option. Without this, a 200-ref page
would hammer the same hosts repeatedly.

**Cost/latency.** A large page is N panel-votes. v1 is a synchronous request
(matches bare-url); the report's `stats` surface how many use-sites ran so the
caller sees the scale. Async/streaming is deferred.

**Explicitly out of scope for v1:** clause-level splitting (mid-sentence refs
attach to their containing sentence), cross-block preceding context (context
stays within the block), multi-claim decomposition (the deferred Molecular
Facts pass), the CLI page-reader (committed fast-follow; deferred only because
it needs `sp42-cli` to reach Parsoid — either a dependency on `sp42-server` or
moving `ParsoidWikitextEditor` to a shared crate), and any editing/apply path.

**Server-side inference wiring.** The CLI builds its inference panel from env
vars; the server may not yet hold a model client/panel in state. Phase 6
introduces that wiring if absent — to be confirmed during Phase 6 codebase
verification in the implementation plan.
