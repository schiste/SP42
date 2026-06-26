# Bare-URL Repair MVP (PRD-0008) Design

## Summary

SP42 is a Wikipedia patrolling tool that helps reviewers act on problems they
find in the revision queue. One common problem is a bare-URL reference —
`<ref>https://example.org/article</ref>` — where a citation template filled
with title, author, and date is needed but producing one currently requires
leaving SP42 to look up the page manually. This design adds a
propose-and-confirm repair flow: the operator requests proposals for a
revision, SP42 fetches citation metadata from Citoid (the same service
MediaWiki's own visual editor uses), renders a `{{cite web}}` template from
that metadata, and presents the result for the operator to inspect before
anything is written. If the operator confirms, the replacement is applied
under their own session and rights; if the article changed between proposal
and confirm, SP42 refuses rather than guessing.

The implementation builds on three existing mechanisms: the Citoid client
already developed in the `impl/citation-verification` branch (lifted
verbatim), the node-anchored wikitext editing machinery from ADR-0003 (which
addresses edits by document-order ordinal with an anti-drift re-check), and
the per-wiki presence-gated config pattern already used for
`citation_needed`. New code follows the FCIS (Functional Core, Imperative
Shell) principle: pure proposal and rendering logic lives in `sp42-core` with
no I/O, server routes handle Citoid fetches and wiki writes in `sp42-server`,
and a pair of CLI flag-modes give the operator a keyboard-reachable surface
without coupling the MVP to frontend work. The scope is deliberately narrow —
testwiki only, CLI-first, no production wiki writes — to prove the end-to-end
repair loop before enabling any production wiki.

## Definition of Done

The MVP's full Definition of Done is PRD-0008's (docs/domains/references/prd/0008-bare-url-repair.md,
Draft — implementation proceeding at-risk pending reviewer reaction): testwiki-only,
CLI-first, eight DoD items covering proposal generation, ordinal targeting,
exact-replacement saves, zero-write refusals, sparse-metadata decline,
service-failure degradation, per-wiki config gating, and a recorded live repair
on test.wikipedia.org.

**Tonight's confirmed execution slice (2026-06-09):** core service modules land
on `louie/bare-url-repair` with tests green:

1. Citoid client in-tree — **verbatim lift** of `citoid.rs` + `urls.rs` (+ the
   small `CitationVerificationError` enum and `lib.rs` glue) from
   `impl/citation-verification`, byte-identical so the later merge of that
   branch auto-resolves.
2. Bare-URL reference detection (plain-URL rule from PRD-0008 Resolved
   question 1).
3. Metadata → `{{cite web}}` renderer with decline-on-no-usable-title
   (none, or URL echoed as title).
4. Proposal builder: enumerates the revision's references, identifies bare
   ones, emits locator + replacement proposals.
5. Per-wiki enablement gating (testwiki only; production configs refuse on
   both proposal and apply paths).

6. The CLI flag-modes (scope expanded by the Editor mid-design,
   2026-06-09 evening): `--bare-url-preview --title <T> --rev <N>` and
   `--bare-url-execute --ordinal <K> [--action-note <summary>]`, thin drivers
   over the bridge routes following the house flag-mode pattern, with their
   render functions unit-tested.

All clippy-clean (`-D warnings`), with replayed-fixture tests (no live network
in tests).

**Explicitly not tonight (follows per PRD DoD):** the live
test.wikipedia.org repair gate (needs an operator at the keyboard).

**Also in scope for the design (executed with the CLI follow-up):** the
proposed CLI surface folded back into PRD-0008, and a GitHub issue proposing
the "PRDs contain proposed CLI surfaces" convention for docs/process/prd-protocol.md
(issue filed on the Editor's instruction).

## Glossary

- **ADR (Architecture Decision Record):** A short document recording a
  significant architectural choice and its rationale. SP42 uses a numbered
  series in `docs/adr/`.
- **Anti-drift re-check:** Before applying an edit, the node-anchored editor
  compares the targeted node's current text against the `expected_text`
  captured at proposal time; a mismatch refuses the edit rather than touching
  the wrong content. The primary guard against TOCTOU races.
- **`baserevid`:** The MediaWiki revision ID the proposal was generated
  against, sent with the save so the API rejects writes over unseen edits.
- **Bare-URL reference:** A `<ref>` whose content, after trimming, is exactly
  one plain `http(s)` URL. Bracket-wrapped forms and references with any other
  prose are excluded.
- **Bridge / dev-auth bridge (ADR-0002):** The local server-side mechanism
  holding the operator's OAuth token; the CLI drives authenticated server
  routes via `--bridge-base-url` so tokens never live in the CLI process.
- **Citoid:** The Wikimedia REST service returning structured citation
  metadata for a URL (SP42 calls the per-wiki `mediawiki-basefields` endpoint
  read-only, one request per second). The same service the visual editor's
  Cite tool uses.
- **`CitoidMetadata`:** The Rust struct for a parsed Citoid response, in the
  `citation/citoid.rs` module lifted from `impl/citation-verification`.
- **`{{cite web}}` / `{{Lien web}}`:** MediaWiki citation templates (English
  default and its French equivalent). SP42 renders the template the target
  wiki's config names.
- **`decline-to-propose` / `Declined`:** The structured non-error outcome when
  a bare-URL reference cannot get a usable proposal (no title, or the URL
  echoed as title). The reference stays a finding rather than receiving a bad
  edit.
- **FCIS (Functional Core, Imperative Shell):** Pure, I/O-free domain logic in
  core modules; side effects confined to the shells. `sp42-core` is the core;
  `sp42-server` and `sp42-cli` are shells (Constitution Art. 2).
- **Flag-mode (CLI):** A mutually-exclusive top-level flag (e.g.
  `--bare-url-preview`) that performs one operation and exits instead of
  entering the queue loop — SP42's pattern for one-shot operator commands.
- **`frwiki`:** French Wikipedia, the eventual production target — explicitly
  not enabled by this MVP (no `bare_url_citation` in its config).
- **Gate / per-wiki enablement gating:** The config check before any proposal
  or apply action; a wiki without `bare_url_citation` gets
  `bare-url-repair-not-enabled` without the wiki being touched.
- **`impl/citation-verification`:** The parallel SP42 branch where the Citoid
  client was developed; its files are copied byte-identical so the eventual
  merge auto-resolves.
- **`node-drift` / `node-out-of-range`:** Refusal codes (HTTP 400 with
  `http_status: 409` in the body) when the anti-drift re-check fails or the
  ordinal no longer exists; zero wiki writes on these paths.
- **Operator:** The human using SP42 — a patroller acting under their own
  account. Every write requires their explicit confirmation.
- **Ordinal:** The zero-based document-order position of a node within its
  kind (here, `<ref>` elements), used with the text anchor to address an edit
  (ADR-0003).
- **Parsoid:** The MediaWiki wikitext↔HTML service backing ADR-0003's editor;
  both the proposal flow (reference enumeration) and the apply path run
  through SP42's Parsoid-backed `WikitextEditor`.
- **PRD (Product Requirements Document):** SP42's feature-intent document;
  PRD-0008 is what this design implements.
- **Proposal / propose-confirm pattern:** Two-step flow: generate a proposed
  edit, then apply only when the operator replays exactly that payload.
  Nothing reaches the wiki without a confirmation bound to the exact proposed
  wikitext.
- **Replayed-fixture tests:** Tests running against pre-recorded responses
  checked into the repo; no live network anywhere in the test suite.
- **`ScriptedWikitextEditor` / `MockWikiBackend`:** In-crate test doubles: the
  former enforces the real locator contract for enumeration/apply logic; the
  latter captures wiki save requests and proves zero-write refusals.
- **`testwiki` (`test.wikipedia.org`):** The Wikimedia sandbox wiki — the only
  wiki enabled in the MVP; the PRD's final DoD item is a confirmed live repair
  there.
- **TOCTOU (Time-of-Check to Time-of-Use):** The race where the article
  changes between proposal and confirm; mitigated by the anti-drift re-check
  plus the `baserevid` guard.
- **`WikitextNodeLocator`:** The kind + ordinal + expected_text struct that
  addresses one node and anchors an edit against drift (`sp42-core`), reused
  verbatim in the proposal payload.
- **`WikiTemplates.bare_url_citation`:** The new optional per-wiki config
  field; presence (e.g. `"cite web"`) both enables the feature and names the
  template to render.

## Architecture

Server-seam shape (Approach A, selected in brainstorming): pure proposal logic
in `sp42-core`, orchestration and I/O in `sp42-server` behind two new gated
routes, and the CLI as a thin authenticated driver over the existing dev
bridge. The apply path reuses ADR-0003's node-anchored editing machinery
unchanged.

Data flow (proposal): CLI/operator → `POST /dev/citation/bare-url-proposals`
→ gate check (`config.templates.bare_url_citation` present) →
`state.wikitext_editor.enumerate_nodes(Reference)` → pure
`bare_url_references()` scan → per bare reference, sequential Citoid fetch
(plain reqwest, `branding::USER_AGENT`, no auth, respects the 1 req/s limit) →
pure `render_bare_url_citation()` → `{proposals, declined}` response. Each
proposal carries a `WikitextNodeLocator` (kind/ordinal/expected_text) plus the
replacement wikitext — a replayable, drift-guarded edit payload.

Data flow (apply): CLI/operator → `POST /dev/citation/bare-url-apply` → gate
check → delegate to the same internals as `execute_inline_edit_action` with
the proposal's locator + replacement replayed verbatim, default summary
"SP42: bare-URL repair" (operator note wins) → `baserevid`-guarded save →
patrol of the original revision. Drift and out-of-range refuse exactly as
ADR-0003 shipped (`node-drift`/`node-out-of-range`, HTTP 400 with
`http_status: 409` in body, zero wiki writes).

Decline rules (core-pure): metadata unavailable (Citoid 520/404/non-200), no
title, or title == URL (Citoid's documented degenerate fallback) →
`Declined { reason }` entries, never errors; a junk URL cannot fail the whole
proposal response.

### Contracts

Core module `crates/sp42-core/src/bare_url_repair.rs` (pure, no I/O):

```rust
pub fn classify_bare_url(anchor_text: &str) -> Option<&str>;
pub fn bare_url_references(descriptors: &[WikitextNodeDescriptor]) -> Vec<BareUrlReference>;
pub fn render_bare_url_citation(
    template_name: &str,
    metadata: Option<&CitoidMetadata>,
    access_date_iso: &str,   // shell passes the fetch date; core stays clock-free
) -> BareUrlOutcome;

pub struct BareUrlReference { pub ordinal: usize, pub url: String, pub anchor_text: String }
pub enum BareUrlOutcome {
    Proposed { replacement_wikitext: String },
    Declined { reason: BareUrlDeclineReason },  // code(): "no-usable-title" | "metadata-unavailable"
}
pub struct BareUrlProposal {           // wire type, serde
    pub locator: WikitextNodeLocator,  // kind=reference, ordinal, expected_text=current anchor
    pub url: String,
    pub current_anchor: String,
    pub replacement_wikitext: String,
}
```

Renderer field mapping (default `{{cite web}}` map, the only one in the MVP):
`|url=`, `|title=`, `|website=` (websiteTitle, fallback publicationTitle),
`|author=` (formatted from `[["First","Last"],…]`), `|date=` (ISO, partial
precision passes through), `|access-date=` (ISO), `|language=` (when present
and not the wiki's own; default map treats "en" as own-language).

Route contracts (`crates/sp42-server/src/citation_routes.rs`, new):

```text
POST /dev/citation/bare-url-proposals
  req:  { wiki_id, title, rev_id }
  resp: { proposals: [BareUrlProposal], declined: [{ ordinal, url, reason }] }
  4xx:  bare-url-repair-not-enabled (gate), editor-* codes (backend)

POST /dev/citation/bare-url-apply
  req:  { wiki_id, title, rev_id, locator, replacement_wikitext, summary? }
  resp: action outcome (same shape as execute-action)
  4xx:  bare-url-repair-not-enabled, node-drift/node-out-of-range (409-in-body)
```

Config: `WikiTemplates` gains `bare_url_citation: Option<String>` —
`Some("cite web")` in `fixtures/testwiki.yaml` enables the feature with that
template; absent in `configs/frwiki.yaml` disables both routes. One field is
both gate and map (the `citation_needed` precedent).

CLI flag-modes (`crates/sp42-cli/src/main.rs`, house pattern: hand-rolled
flags, early-exit before queue flow, `--format text|json|markdown`):

```text
--bare-url-preview --title <T> --rev <N> [--bridge-base-url <URL>]
--bare-url-execute --ordinal <K> --title <T> --rev <N> [--action-note <summary>]
```

`--bare-url-execute` re-fetches the proposal for ordinal K and replays it
against the apply route (fresh anchor, narrows the TOCTOU window); the CLI
never holds a token — auth rides the bridge session (ADR-0002), exactly like
`--action-execute` today.

## Existing Patterns

- **FCIS / crate boundaries (Constitution Art. 2, ADR-0004):** pure logic in
  `sp42-core`, I/O adapters in `sp42-server` — same split as
  `wikitext_editor` (core contract) / `parsoid_editor` (server adapter).
- **Presence-gated per-wiki template config:** `WikiTemplates.citation_needed`
  (`crates/sp42-core/src/types.rs:422-438`, consumed in
  `action_routes.rs:344`) is the model for `bare_url_citation`.
- **Verbatim lift from the citation branch:** `citation/citoid.rs` (219
  lines) + `citation/urls.rs` (370 lines) + `CitationVerificationError`
  copied byte-identical from `impl/citation-verification` so the eventual
  merge auto-resolves; only the thin `citation.rs` module-declaration file
  diverges (trivial take-theirs conflict, noted below).
- **Mock-backend test patterns:** `ScriptedWikitextEditor` for enumeration,
  ephemeral axum mock for Citoid (the `spawn_mock_parsoid` pattern in
  `parsoid_editor.rs` tests), `MockWikiBackend` in `tests.rs` for the apply
  path. No live network in tests.
- **CLI flag-modes:** mutually-exclusive mode flags with early exit and the
  `OutputFormat` enum (the citation branch's `--claim`/`--locate-probe`
  additions are the direct precedent); render functions are pure and
  unit-tested (`render_verify_text` precedent).
- **Bridge-mediated auth:** the CLI drives authenticated server routes via
  `--bridge-base-url` (existing `--action-execute` path, main.rs:1504);
  tokens never live in the CLI (ADR-0002).
- **Divergence — new `citation_routes.rs`:** bare-URL routes get their own
  module rather than growing `action_routes.rs` (which is already ~900
  lines); same axum patterns, separate file.

## Implementation Phases

### Phase 1: Citoid lift
**Goal:** The Citoid client compiles and passes its tests in this branch.

**Components:** `crates/sp42-core/src/citation.rs` (module decl, 2 submodules),
`crates/sp42-core/src/citation/citoid.rs` + `citation/urls.rs` (byte-identical
lifts), `CitationVerificationError` in `crates/sp42-core/src/errors.rs`,
re-exports in `crates/sp42-core/src/lib.rs`.

**Dependencies:** none (first phase).

**Done when:** lifted tests pass unmodified; `cargo test -p sp42-core` and
clippy `-D warnings` green; `diff` against the citation worktree confirms
byte-identical citoid.rs/urls.rs.

### Phase 2: Per-wiki enablement config
**Goal:** `bare_url_citation` gates the feature per wiki.

**Components:** `WikiTemplates` in `crates/sp42-core/src/types.rs`;
`fixtures/testwiki.yaml` (`bare_url_citation: "cite web"`);
`configs/frwiki.yaml` untouched (absent = disabled);
`docs/platform/RUNTIME_CONFIGURATION.md` documents the knob; parse/default tests in
`crates/sp42-wiki/src/config.rs` if mapping plumbing requires it.

**Dependencies:** none (parallel to Phase 1).

**Done when:** testwiki config parses with the field, frwiki parses without
it (defaults to None), workspace check green.

### Phase 3: `bare_url_repair` core module
**Goal:** Pure classification, rendering, and proposal types with full
decline semantics.

**Components:** `crates/sp42-core/src/bare_url_repair.rs` (contracts above);
checked-in Citoid fixture JSON (normal 200, degenerate title==url, partial
dates, author arrays, no-title); lib.rs re-exports.

**Dependencies:** Phase 1 (CitoidMetadata type).

**Done when:** fixture tests prove: plain-URL rule (brackets/labels/prose
rejected), every renderer field mapping, both decline reasons, ISO access
date pass-through; clippy green.

### Phase 4: Proposal route
**Goal:** Gated server route returning proposals + declines for a revision.

**Components:** `crates/sp42-server/src/citation_routes.rs` (new; route
handler, citoid fetch helper, gate check), route registration in
`routes.rs`, request/response wire types (serde) in the same module.

**Dependencies:** Phases 1-3.

**Done when:** tests (ScriptedWikitextEditor + mock Citoid) prove: proposals
for a fixture with a duplicated URL target the right ordinal; per-reference
Citoid failure degrades to a declined entry; frwiki-shaped config refuses
with `bare-url-repair-not-enabled`; editor errors map to `editor-*` codes.

### Phase 5: Apply route
**Goal:** Gated confirm path that replays a proposal verbatim.

**Components:** apply handler in `citation_routes.rs` delegating to the
inline-edit execution internals (`action_routes.rs` refactor to expose the
shared core if needed), default summary handling.

**Dependencies:** Phase 4 (module exists), ADR-0003 machinery (shipped).

**Done when:** MockWikiBackend tests prove: exact proposed replacement +
`baserevid` in the save body; drift refusal with zero wiki writes; gating
refusal on frwiki-shaped config; summary defaulting.

### Phase 6: CLI flag-modes
**Goal:** Operator-reachable preview/execute from the command line.

**Components:** flag parsing + mode handlers in
`crates/sp42-cli/src/main.rs`; pure render functions (text/json/markdown) for
proposal lists and apply outcomes, unit-tested; `--bridge-base-url` reuse.

**Dependencies:** Phases 4-5 (routes exist).

**Done when:** render-function tests pass; CLI compiles; a manual smoke
against a locally running server succeeds (documented, not automated).

### Phase 7: Records and fold-backs
**Goal:** The paper trail matches the code.

**Components:** PRD-0008 gains "Proposed CLI surface" section; thin
propose/confirm contract ADR drafted in `docs/adr/` (number per the series
state at draft time); GitHub issue proposing the "PRDs contain proposed CLI
surfaces" convention for `docs/process/prd-protocol.md` (filed on the Editor's
instruction); `docs/STATUS.md` bullet.

**Dependencies:** Phases 1-6 (records describe what shipped).

**Done when:** `scripts/check-doc-consistency.sh` passes; issue URL recorded
in the PRD discussion trail.

## Additional Considerations

**Live gate is out of this plan.** PRD-0008's final DoD item (confirmed
repair on test.wikipedia.org) needs an operator at the keyboard with a
running server and a real session; it runs manually after this plan lands and
gates the PRD's move to `Implemented`, not this plan's completion.

**Citoid pacing:** the proposal route fetches sequentially; with the 1 req/s
service limit, a reference-heavy revision is seconds, not minutes, and the
flow is operator-paced. No batching machinery in the MVP.

**Known trivial merge conflict:** `crates/sp42-core/src/citation.rs` (module
declarations) will conflict when `impl/citation-verification` merges —
resolution is take-theirs. All other lifted content is byte-identical and
auto-resolves.

**ADR numbering:** the propose/confirm ADR takes its number at draft time;
the citation-verification series holds unmerged drafts through ADR-0009.

**Language parameter heuristic:** the default map hardcodes "en" as
own-language (testwiki). Per-wiki own-language comes with the per-wiki
mapping work in the frwiki-enablement follow-on; not modeled in config now.
