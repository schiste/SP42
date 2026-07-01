# PRD-0009: Book-citation grounding and Open Library enrichment

**Drafter:** Claude Code (Opus 4.8)
**Editor:** Luis Villa
**Date:** 2026-07-01
**State:** Draft
**Discussion:** design conversation 2026-07-01 (Internet Archive editable book
metadata → SP42 integration); no tracking issue yet.
**Spawned ADRs:** none yet. If this PRD is accepted, expect at least one thin ADR
for the Internet Archive / Open Library read contract (resolve + full-text
grounding) and, if the enrichment lane ships, a second reusing ADR-0010's
propose/confirm apply discipline against a non-wiki target.

## Scope boundary

This PRD owns **what SP42 does with a *book* citation**: today those citations
are recorded as `skipped { reason: NonUrlSource }` by the article-level verifier
(ADR-0011, Decision 3) — a book or bare ISBN yields no fetchable URL, so it is
neither verified nor actionable. This PRD turns that dead end into two capabilities,
both anchored on the Internet Archive's book holdings:

1. **Grounding** — verify a book citation against the *full text* of an Internet
   Archive scan, when one exists, producing the same categorical verdict + grounded
   passage the URL path already produces (ADR-0007).
2. **Enrichment** — when the citation resolves to an [Open Library](https://openlibrary.org)
   record, offer the operator a **sourced, operator-confirmed** improvement to that
   record's metadata (subjects, description, cover, publish details, identifiers).

It deliberately excludes adjacent concerns:

- **archive.org *item* metadata editing.** The Internet Archive also exposes a
  write API over a scanned *item's* own metadata (JSON Patch + IA-S3 keys). That is
  permission-gated to item curators and is a different surface from the globally
  editable Open Library catalog; it is a Non-Goal here (see Non-Goals) and, if ever
  wanted, its own PRD.
- **Wayback / archive-snapshot enrichment of URL citations** — that is PRD-0010
  (issue #29). This PRD is about *books*, not about attaching web archives to dead
  links.
- **The propose→confirm→apply *mechanism*** for the enrichment write — that reuses
  ADR-0010 (operator-confirmed content proposals) against a new (non-MediaWiki)
  target; this PRD relies on that discipline, it does not re-specify it.
- **Finding book citations at scale** — queue ranking and cross-article discovery
  are the citation review queue (issue #26, future). This PRD covers only the
  references of the revision the operator is already reviewing (same footprint as
  PRD-0008 and ADR-0011).
- **Identity/capability gating** for who may enrich — follows the existing gating
  posture (PRD-0004/PRD-0005); the enrichment write lands under the operator's own
  Open Library account, not a shared bot.

## Background: is the "editable book metadata" still there?

Yes, and it is two distinct systems:

- **Open Library** (`openlibrary.org`) — the Internet Archive's wiki-model,
  anyone-with-an-account-can-edit bibliographic catalog of *works* and *editions*.
  Records are readable as JSON (`GET /isbn/{isbn}.json`, `/works/OL…W.json`,
  `/books/OL…M.json`, or the multi-identifier `/api/books`) and writable — either by
  `PUT`ing the record URL as JSON or via `POST /api/import` for a whole edition —
  under a login session. Every edit is attributed and reversible (wiki history).
  **This is the "editable book metadata" the enrichment lane targets.**
- **archive.org item metadata** — metadata on a specific scanned item, writable via
  `POST archive.org/metadata/{identifier}` with a JSON-Patch body and IA-S3
  credentials, but only for items the account has rights on. Out of scope (above).

The *grounding* lane leans on a third, read-only Internet Archive capability: many
Open Library editions link to a full-text-searchable scan on archive.org, so a
book's OCR can be searched for a claim's supporting passage.

## Problem

A patroller reviewing a revision routinely meets claims cited to books:
`<ref>{{cite book |isbn=978-0-14-032872-1 |title=… |page=42}}</ref>`. Today SP42
can do nothing useful with these. ADR-0011 records them honestly as `skipped`
(`NonUrlSource`) — no URL, no fetch, no verdict — so a book citation is a blind
spot in exactly the review a citation workbench exists to support. Two things are
being left on the table:

1. **A verifiable source SP42 is ignoring.** The Internet Archive has millions of
   full-text-searchable scanned books. For a book citation carrying an ISBN, that
   ISBN often resolves to a scan whose OCR SP42 *could* search for the cited
   passage — turning "we can't check this" into a real, grounded verdict, and even
   a deep link to the scanned page. This is the same grounded-passage discipline
   the URL path already enforces (ADR-0007); the source is just a scan instead of an
   HTML page.
2. **A public good SP42 is uniquely positioned to improve.** In the course of
   verifying, SP42 already resolves the book to its Open Library record and already
   holds clean bibliographic fields from Citoid (`citation/citoid.rs`). When that
   record is thin — no subjects, no description, no cover, missing identifiers — SP42
   can offer the operator a **sourced** improvement, at the moment they are already
   looking at that book. The user's framing: *"you checked the citations on page X;
   page X cites books Y and Z; would you like to improve the metadata about Y and Z
   at Internet Archive?"*

This is for the **experienced reviewer/patroller** acting under their own account,
one citation at a time — the same operator and posture as PRD-0008.

## Proposal

Three layers, each usable on its own, sequenced cheapest-and-safest first. The
first two are **read-only** and need no new credentials; only the third writes, and
only under ADR-0010's confirm discipline.

### Layer 1 — Resolve (read-only, no auth)

For a book citation in the revision under review, SP42 resolves it to catalog
records **only from identifiers it already trusts**:

- The key is the **ISBN** carried by the cite template (SP42 already extracts cite
  metadata via Citoid). SP42 calls Open Library's read API (`/isbn/{isbn}.json` →
  edition, then its `works` and, when present, the linked Internet Archive scan
  `ocaid`).
- **Matching is gated on a positive identifier.** With an ISBN (or another unique
  identifier — OCLC, LCCN, an explicit OLID) the resolution is reliable. **Without
  one, SP42 does not guess** from title+author (too error-prone; the classic
  wrong-edition failure) — the citation stays `skipped`, with the skip reason
  refined so the report distinguishes "no identifier to resolve" from "resolved but
  nothing found." No fabricated matches, consistent with how the URL path treats
  unresolvable sources.

### Layer 2 — Ground (read-only, no auth)

When resolution yields an Internet Archive scan with searchable full text:

- SP42 searches the scan's OCR (Internet Archive full-text / "search inside") for
  the claim's supporting passage and runs it through the **existing** per-use-site
  verifier and panel (ADR-0006/0007) — the scan's OCR text is the source body, no
  new verdict semantics.
- The **anti-fabrication grounding gate is unchanged** (ADR-0007): a `supported`
  verdict must carry a passage **verbatim-located in the fetched OCR bytes**. A scan
  is just another byte source under the ADR-0009 snapshot/replay discipline (the
  OCR text is content-hashed and stored, so verdicts replay deterministically).
- The finding records a **deep link to the scanned page** (Internet Archive book
  URLs support page anchors + search highlighting) so the operator can jump to the
  page that supports — or contradicts — the claim.
- A book that resolves but whose scan is **not full-text-searchable** (in-copyright
  / lending-only / no scan) degrades to `SourceUnavailable`, reusing the existing
  reason split (`unreachable` vs `unusable`, ADR-0011 Decision 7) rather than adding
  a book-specific verdict.

### Layer 3 — Enrich (operator-confirmed write; ADR-0010 discipline)

When Layer 1 resolved an Open Library record and SP42 holds **sourced** fields that
record is missing:

- SP42 computes a **field-level gap proposal** against the live Open Library
  record: e.g. missing `subjects`, missing `description`, missing cover, absent
  `publish_date`/`number_of_pages`, an ISBN-13 to sit alongside a bare ISBN-10, a
  link back to the source. **Every proposed field traces to a source** — the Citoid
  metadata, the resolved scan's own title-page/OCR, or another authority — and the
  proposal shows that provenance. **SP42 authors nothing** (the PRD-0008 rule,
  extended to a new target); there is no generative "write me a book blurb" step.
- The operator sees the record's current value and the proposed value **field by
  field** and confirms or dismisses. It is a **proposal, not a write**: nothing
  reaches Open Library without the operator confirming that exact change (ADR-0010).
- On confirm, SP42 applies the change through Open Library's write API **under the
  operator's own Open Library account**, with a descriptive edit comment
  (`SP42: sourced metadata enrichment`), respecting Open Library's rate limits and
  import/bot etiquette. The edit is attributed and reversible (wiki history) — an
  assisted edit, not a bot run.
- If the Open Library record changed under the proposal, SP42 **refuses rather than
  overwrites** and offers to re-propose against the current record (ADR-0010's
  refuse-on-drift, applied via the record's revision rather than a MediaWiki
  `baserevid`).

### Surface

CLI-first, matching PRD-0008 and the `verify-page` CLI (ADR-0011). Layers 1–2 fold
into the page report: a book citation that today prints as `skipped (non-URL)`
instead prints a resolved record, a grounded verdict + page deep link when a scan
is searchable, or an honest `SourceUnavailable`. Layer 3 mirrors the
`bare-url preview`/`execute` pair — a read-only proposal listing and a
confirm-and-apply action under the operator's session. The browser Citations tab
(PR #81) is the eventual home for both, as a follow-on.

## Definition of Done

*Names planned coverage; the closing PR records exact test ids and updates this
list when the PRD moves to `Implemented`. All tests replay recorded
Open Library / Internet Archive responses — no live network in tests (ADR-0009).*

- [ ] A book citation carrying an ISBN resolves to its Open Library edition/work and
      (when present) its Internet Archive scan, verified over a replayed
      `/isbn/{isbn}.json` fixture; a citation with **no** resolvable identifier stays
      `skipped` with the refined reason, verified by a fixture test.
- [ ] A resolved book with a full-text-searchable scan produces a `supported` /
      `partial` / `not_supported` verdict whose supporting passage is
      **verbatim-located in the scan OCR**, verified by a replayed full-text-search
      fixture; the ADR-0007 grounding gate rejects a passage not present in the OCR.
- [ ] The verdict carries a page-anchored deep link into the scan, verified by a
      renderer test.
- [ ] A resolved book whose scan is not full-text-searchable degrades to
      `SourceUnavailable` (correct `unreachable`/`unusable` split), not a fabricated
      verdict, verified by a fixture test.
- [ ] A thin Open Library record yields a field-level enrichment proposal populated
      **only** from sourced fields, verified by a renderer test over replayed record
      + Citoid fixtures; a record already complete yields **no** proposal.
- [ ] Confirming applies **exactly the proposed field change** to Open Library under
      the operator's account with the edit comment, verified by a mock-write-path
      test; **no** write occurs without a confirmation bound to that exact proposal.
- [ ] A proposal whose target record drifted (edited since proposal) **refuses** and
      offers re-proposal, with **zero** writes reaching Open Library, verified by a
      write-path refusal test.
- [ ] The Open Library write path is exercised end-to-end against the real service
      exactly once as the live-edit acceptance gate; the closing PR records the
      target record, the field before/after, and the resulting Open Library revision.

## Alternatives

- *Match books by title + author when no ISBN is present.* Rejected for the MVP:
  fuzzy bibliographic matching hits the wrong edition often enough to poison both a
  verdict and an enrichment write; the citation staying `skipped` is the honest
  outcome. Revisit with a confidence threshold if identifier coverage proves too
  thin in practice.
- *Let the model write missing descriptions/subjects from its own knowledge.*
  Rejected: violates the anti-fabrication posture (ADR-0007) and PRD-0008's
  "SP42 authors nothing." Enrichment relays sourced fields only; an unsourced field
  is simply not proposed.
- *Auto-apply high-confidence enrichments.* Rejected: violates
  operator-confirms-every-edit (PRD-0004, ADR-0010), and a public catalog is exactly
  where an unattended wrong edit is most costly.
- *Edit the archive.org item metadata (JSON-Patch + IA-S3) instead of Open Library.*
  Rejected for this PRD: item metadata is permission-gated to curators of that scan,
  a much narrower surface than the globally editable Open Library catalog, and a poor
  fit for "improve this book's bibliographic record." Its own PRD if ever wanted.
- *Create wholly-missing Open Library editions via `/api/import`.* Deferred:
  importing a new record is higher-stakes than enriching an existing one (dedup
  risk, import-privilege requirements) and warrants its own DoD; the MVP enriches
  records that already exist.
- *Hand off to the Open Library website (open the edit page in a browser).*
  Rejected for the same reason PRD-0008 rejected the visual-editor hand-off: the cost
  being removed is leaving SP42 and losing review context.

## Risks

- **Wrong-edition resolution.** Mitigation: resolve only from a positive identifier
  (ISBN/OCLC/LCCN/OLID); no title/author guessing (MVP). A wrong ISBN in the citation
  itself surfaces as a mismatched record the operator sees before confirming any
  enrichment.
- **Garbage / thin OCR.** Scan OCR quality varies; a bad scan can miss a passage
  that is genuinely in the book (false `SourceUnavailable`/`not_supported`).
  Mitigation: the grounded-passage gate keeps this conservative (SP42 never claims
  support it cannot locate), and the deep link lets the operator confirm by eye. This
  is a *tool limitation* framing (`unusable`), not a claim about the book.
- **Editing a public catalog.** Enrichment writes to a shared, human-curated
  resource. Mitigation: operator-confirmed, operator-attributed, one field at a
  time, sourced-only, with a clear edit comment and Open Library's history/revert
  behind it — an assisted edit, not a bot. Respect Open Library rate limits and
  import/bot policy; if community practice requires more for assisted enrichment, the
  write lane stays disabled until resolved (same posture as PRD-0008's frwiki gate).
- **Operator habituation (rubber-stamping).** Mitigation: field-level diffs are
  small and sourced; a complete record yields no proposal at all, so proposals stay
  rare and worth reading.
- **Credential handling for Open Library.** The write lane needs the operator's
  Open Library session. Mitigation: treat it like the existing per-operator
  MediaWiki session — never a shared/baked-in key (see Open questions).
- **Two more external services (Open Library, IA full-text).** Mitigation: both sit
  behind the `HttpClient` trait boundary with pure builders/parsers and replayed
  fixtures (ADR-0004/0008 pattern, as Citoid does); unavailability degrades to
  "no resolution / no proposal," never blocks the review flow. Server-side fetches
  honor the SSRF floor (SP42#34) like ADR-0011 Decision 5.

## Open questions

1. **Open Library credential model.** Per-operator login session stored like the
   MediaWiki session, or an access/secret pair? Leaning per-operator session so
   every enrichment is attributed to the human who confirmed it.
2. **Enrich-only, or also import missing editions?** MVP leans enrich-only
   (Alternatives); is importing a missing edition (`/api/import`) in scope later, and
   under what privilege check?
3. **Where do enrichment fields come from, in priority order?** Citoid vs the scan's
   own title-page/OCR vs other authorities — and which fields are safe to relay
   (identifiers, publish date, page count, subjects) vs never auto-proposed
   (free-text description?).
4. **Full-text availability signal.** How reliably can SP42 tell "scan exists and is
   searchable" from "scan exists but is lending-restricted/not indexed" before
   spending a search? Determines how often Layer 2 can promise a verdict.
5. **Grounding a *page-specific* citation.** A cite `|page=42` names a location; do we
   scope the OCR search to that page (higher precision, brittle to scan pagination)
   or search the whole book and report the page we found (more robust)? Leaning the
   latter.
6. **Does this need its own ADR now, or ride ADR-0010/0011?** Likely a thin read
   contract ADR (resolve + full-text grounding as a new source type) plus reuse of
   ADR-0010 for the write; assign numbers when drafted.
