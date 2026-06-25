# ADR-0011: Article-level citation verification (the review path)

**Status:** Proposed
**Date:** 2026-06-25
**Author:** Luis Villa

ADR-0006–0009 settled how SP42 verifies **one** citation use-site (a claim and
its cited source → a categorical verdict with a grounded passage). ADR-0010
added a read-only propose/confirm lane for *generating* content. What was
missing is the layer a reviewer actually wants: read a whole page and verify
**every** citation on it. This ADR records the decisions made building that
article-level path (the `verify-page` route).

This ADR is **retrospective by design**, and that was the right call here.
Most of these decisions were discovered by building the path and smoke-testing
it against real New-Pages-Patrol (enwiki) and reviewer-queue (frwiki) articles —
not derivable from a whiteboard. Three in particular would have been wrong or
absent in an up-front ADR: (a) a cite-template's `archive-url` is a *fallback*,
not a second citation; (b) real Parsoid carries the transclusion `data-mw` on a
`<link>`, not the `<span>` a hand-built fixture assumed; (c) the
`SOURCE_UNAVAILABLE` verdict was conflating a dead link with a source we
fetched but could not read. The engineering detail lives in
`docs/design-plans/2026-06-24-article-citation-extractor.md` and the matching
implementation plans.

## Context

The per-use-site verifier (ADR-0007/0008) takes a `CitationVerificationRequest`
(claim + source URL + page identity) and returns a read-only `CitationFinding`.
To review a page we need to (1) read the revision, (2) find every citation and
the claim it supports, (3) verify each, and (4) return one page-level report —
without writing anything. The Parsoid DOM SP42 already uses for node-anchored
editing (ADR-0003) is `!Send` (kuchikiki), which constrains where the work can
live. The route fetches arbitrary, wiki-supplied source URLs server-side, which
is a server-side request forgery (SSRF) surface the single-claim CLI path did
not expose to an unauthenticated caller.

## Decision

1. **Read-only article extractor + page orchestrator.** A new route
   `POST /dev/citation/verify-page` (`DEV_CITATION_VERIFY_PAGE_PATH`) takes
   `{wiki_id, title, rev_id}` and returns a `PageVerificationReport`
   (`findings`, `skipped`, `extraction_failures`, `stats`). It performs no
   writes — it mirrors the read-only *proposal* side of ADR-0010, not the apply
   side, and has no apply counterpart.

2. **FCIS / `!Send` split.** The Parsoid editor does one DOM pass and emits
   plain `Send` data (`ParsoidBlock`: block text with ref markers removed but
   their byte offsets recorded, the enclosing heading stack, and each ref's
   structured source URLs). Everything heuristic — sentence segmentation,
   claim↔ref association, `ClaimContext` assembly, and the verify fan-out — is
   pure `sp42-core`, unit-tested with no live DOM. No DOM value crosses an
   `.await`. This extends ADR-0003 (the editor is the sole Parsoid boundary) and
   honors ADR-0004's crate boundaries.

3. **Structured URL extraction; `archive-url` is a fallback, not a citation.**
   Source URLs are read from the cite template's `data-mw` via the `parsoid`
   crate (matching **any** element bearing the `mw:Transclusion` typeof — real
   Parsoid carries it on a `<link …templatestyles mw:Transclusion>`), never by
   scraping rendered HTML. A citation's `archive-url` (Wayback, wikiwix) is a
   *fallback* for its live `url`, consulted **only when the live URL is
   `SourceUnavailable`**. The extractor emits one use-site per cited source
   (a bundled ref with multiple distinct cite templates still yields several);
   refs with no extractable URL (books/ISBN) are recorded as `skipped`, not
   verified.

4. **Reuse the per-use-site verifier unchanged; add SIDE-style context.** Each
   use-site carries a `ClaimContext` (article/section title + preceding
   sentences) passed *alongside* the claim — co-reference material, never a
   source and never grounding. The orchestrator fans the existing
   `verify_citation_use_site` (ADR-0007/0008) over the use-sites, fetching each
   distinct source URL once (dedup) and sharing the body. The verdict semantics
   and anti-fabrication grounding gate of ADR-0007 are untouched.

5. **Server-side source fetch is SSRF-guarded (SP42#34 floor).** Because the
   route fetches arbitrary citation URLs server-side, a dedicated guarded HTTP
   client applies the SP42#34 floor — http/https only; refuse
   loopback/private/link-local/`localhost`/metadata — to the initial URL **and
   re-checks every redirect hop** (a public URL must not 302 into
   `169.254.169.254`), GET-only, with a response-size cap and the
   `SP42_FETCH_ALLOW_PRIVATE=1` dev escape hatch. The guard is the single
   canonical `check_fetchable_source_url`, shared by the CLI and the server (no
   parallel implementation to drift). The route is read-only and ungated like
   bare-url's proposal route; the fetch guard is what makes that safe, and `/dev`
   routes must not be exposed to untrusted networks.

6. **Shared inference edge: the `sp42-inference` crate.** The genai-backed
   `ModelClient` and the `SP42_INFERENCE_*` env-driven panel/client construction
   moved out of the CLI binary into a `sp42-inference` library used by both the
   CLI and the server route. `sp42-core` and `sp42-types` keep no dependency on
   the `genai` adapter, preserving the pure-core boundary (ADR-0004).

7. **`SourceUnavailable` carries a reason.** A `source_unavailable_reason` on
   `CitationFinding` (set only for that verdict) is derived from
   `provenance.http_status`: `Unreachable` (missing/non-2xx → the link is dead,
   the *citation* is actionable) vs `Unusable` (fetched 2xx but the panel could
   not use the content — PDF, JavaScript viewer shell, or a wrong/redirected
   page → a *tool* limitation, the citation may be fine). This is additive and
   serde-back-compatible (ADR-0009 replay); it does not change the verdict enum
   or panel voting (ADR-0006/0007 are preserved). Reading PDF and Google-Books
   sources is out of scope and tracked separately (#52, #53).

8. **Two-level concurrency.** Page-level concurrency (how many use-sites and
   distinct fetches are in flight) is a separate knob from the per-use-site
   panel concurrency (`VerifyOptions.concurrency`, models per use-site). They
   nest multiplicatively, so the route sizes the product (8 use-sites × a
   3-model panel = 24 in flight) against the model endpoint's rate limit.

## Relation to the citation series and SP42#34

- **ADR-0003 (node-anchored Parsoid editing):** the editor stays the sole
  Parsoid/`!Send` boundary; `extract_blocks` is a read-only sibling of
  `enumerate_nodes`.
- **ADR-0006 (using LLMs):** the model panel and measured agreement are reused
  as-is; this ADR adds no new model semantics.
- **ADR-0007 (verdict + anti-fabrication semantics):** unchanged. The
  `source_unavailable_reason` is a derived annotation on the existing
  `SourceUnavailable` verdict, not a new verdict class.
- **ADR-0008 (per-use-site contract):** reused verbatim. The
  `PageVerificationReport` is a new read-only *aggregate* surface over many
  ADR-0008 findings; per Constitution Art. 9.1 it is a versioned serde contract
  in `sp42-core`.
- **ADR-0009 (snapshot storage):** unchanged. The new finding field follows the
  same `serde(default, skip_serializing_if)` replay discipline.
- **ADR-0010 (read-only proposal precedent):** the verify-page route follows the
  read-only-proposal shape; it never reaches the apply lane.
- **SP42#34 (SSRF floor):** Decision 5 is the first server-side application of
  the floor to arbitrary URLs, including per-hop redirect re-checking.

## Consequences

- Reviewers get a page-level read-only report distinguishing supported,
  partial, not-supported, skipped (non-URL), and unavailable (unreachable vs
  unusable) citations — validated end-to-end on real enwiki and frwiki
  articles, including cross-lingual grounding (French/Dutch/Spanish sources).
- The extractor is heuristic. Sentence segmentation and claim↔ref association
  are intentionally iterable; known limitations include single-letter initials
  (`J.R. Ewing`) over-splitting and a whole-block claim fallback for
  terminator-less list items.
- Known source-readability gaps are tracked, not silently dropped: PDF (#52) and
  Google Books (#53) currently read as `SourceUnavailable (unusable)`.
- The crate graph gains `sp42-inference`. The CLI's verify behavior is
  unchanged (it now consumes the shared crate).
- A CLI page-reader is a committed fast-follow; it additionally needs the
  Parsoid editor reachable from the CLI (a separate relocation).

## Non-Goals

- **No editing/apply path.** This is read-only; proposing/applying fixes is
  ADR-0010 territory and a future pass.
- **No claim rewrite or multi-claim decomposition.** Context is passed
  alongside (SIDE-style), not rewritten; the "Molecular Facts" decomposition
  pass is deferred.
- **No PDF / JavaScript source reading yet** (#52, #53).
- **No DNS-rebinding protection.** The SSRF floor is literal/host-based; a
  public hostname resolving to a private IP is not caught (pre-existing,
  applies to the CLI path too; tracked with SP42#34).
- **No bias / contested-topic handling** beyond the existing open-weight panel.
