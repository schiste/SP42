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
- **Wayback / archive-snapshot enrichment of URL citations** — that is a future
  archive enrichment PRD (issue #29). This PRD is about *books*, not about
  attaching web archives to dead links.
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
  Records are readable as JSON (the multi-identifier `/api/books`, the Read API, or
  `/works/OL…W.json` / `/books/OL…M.json` by record id) and writable — either by
  `PUT`ing the record URL as JSON or via `POST /api/import` for a whole edition —
  under a login session. (The `/isbn/{isbn}.json` path is *not* purely read-only — it
  is documented under *Import by ISBN* and can import-on-miss — so the resolve lane
  deliberately avoids it; see Layer 1.) Every edit is attributed and reversible (wiki history).
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

- The key is the **ISBN** (or OCLC/LCCN/OLID) carried by the cite template, read
  **directly from the citation's template parameters** via Parsoid `data-mw` — the same
  structured-extraction path ADR-0011 already uses for a cite's `url`/`archive-url`.
  This is a **new cite-template identifier extractor the MVP must add**, and it is
  precisely what lets a book ref stop being `skipped { NonUrlSource }`: today the
  article extractor drops a ref with no URL before any Citoid call, and Citoid is a
  URL→metadata sidecar that does not carry ISBN — so the identifier cannot come from
  Citoid and must be pulled from the template itself. With an identifier in hand, SP42
  resolves it through a **strictly side-effect-free** Open Library read — the Books API
  (`/api/books?bibkeys=ISBN:{isbn}&jscmd=data&format=json`) or the Read API
  (`/api/volumes/brief/isbn/{isbn}.json`) — reaching the edition, its `works`, and (when
  present) the linked Internet Archive scan `ocaid`. It **must not** use the
  `/isbn/{isbn}.json` endpoint, which Open Library documents under *Import by ISBN* and
  which can **attempt an import or stage metadata on a miss** — a write side-effect that
  would violate this lane's read-only, no-auth promise and the "enrich existing only"
  boundary. A miss is simply "no record found," never a create.
- **Matching is gated on a positive identifier.** With an ISBN (or another unique
  identifier — OCLC, LCCN, an explicit OLID) the resolution is reliable. **Without
  one, SP42 does not guess** from title+author (too error-prone; the classic
  wrong-edition failure) — the citation stays `skipped`, with the skip reason
  refined so the report distinguishes "no identifier to resolve" from "resolved but
  nothing found." No fabricated matches, consistent with how the URL path treats
  unresolvable sources.
- **Grounding requires an exact edition scan.** If the Read API response marks a
  volume as only a `similar` match (a scan of another edition of the same work),
  SP42 may surface that fact to the operator as context, but it does **not** feed
  the scan into Layer 2 as the cited book's source body. For grounding, page
  checks, and `not_supported` conclusions, a similar-edition-only response
  degrades to `SourceUnavailable` with a refined "similar edition only" reason;
  SP42 never verifies a page-specific ISBN citation against a different edition.
- **Wikidata is a secondary structured authority.** When the Open Library record (or
  its work/author) carries a Wikidata link — or one is reachable via a shared
  identifier — SP42 may pull the linked Wikidata item as *additional sourced
  context*. It is not used for matching (identifier gating above still governs
  resolution); it feeds the enrichment lane (Layer 3), where a richer structured
  context is what unlocks a synthesized description.

### Layer 2 — Ground (read-only, no auth)

When resolution yields an Internet Archive scan with searchable full text:

- SP42 queries the scan's **"search inside" API**, which returns matched pages with a
  **verbatim OCR snippet**, the page number, and match coordinates. That snippet is
  the source body handed to the **existing** per-use-site verifier and panel
  (ADR-0006/0007) — no new verdict semantics.
- **Cited page first, then fall back.** When the cite template names a `|page=`, SP42
  scopes the first search pass to that page (higher precision); if it finds nothing
  there — scan pagination often differs from the cited edition (front-matter offset,
  reprints) — it falls back to a whole-book search. The finding records which
  **scanned** page the passage was actually found on, so a page mismatch surfaces to
  the operator rather than causing a false `not_supported`.
- The **anti-fabrication grounding gate is unchanged** (ADR-0007): a `supported`
  or `partial` verdict must carry a passage **verbatim-located in the fetched
  bytes** — here, the returned snippet. `not_supported` is a no-quote verdict
  grounded by fetched-source provenance and the search outcome, not by a
  fabricated "supporting" passage. The snippet is content-hashed and stored under
  the ADR-0009 snapshot/replay discipline, so verdicts replay deterministically.
  Grounding on the snippet (rather than a full-book OCR download) is deliberate:
  **snippet search typically works even for lending-restricted / in-copyright
  scans**, so SP42 can ground a passage without needing to borrow the book.
- The finding records a **deep link to the scanned page** (Internet Archive book
  URLs support page anchors + search highlighting) so the operator can jump to the
  page that supports — or contradicts — the claim.
- **Availability signal (resolved Q4):** SP42 treats a book as groundable when the
  Open Library edition has an `ocaid` **and** the archive.org item is a text item with
  a full-text index (search-inside can run).
- **"No usable body" and "searched, found nothing" are different outcomes (ADR-0007).**
  Two empty results must not be conflated:
  - *No usable body* — no `ocaid`, not a text item, or no full-text index so
    search-inside cannot run at all → `SourceUnavailable`, reusing the `unreachable` vs
    `unusable` split (ADR-0011 Decision 7). SP42 could not read the book.
  - *Searched, nothing found* — the scan **is** indexed and search-inside ran but
    returned **zero matching snippets** (after both the cited-page and whole-book
    passes) → **`not_supported`**, not `SourceUnavailable`: the source exists and was
    searched, it simply yielded no supporting passage. Reporting this as unavailable
    would hide genuine not-supported book citations from the reviewer and the report
    stats. Because keyword/OCR search can miss a differently-worded passage (the same
    false-negative risk as the OCR-miss item in Risks), this stays conservative and the
    scanned-page deep link lets the operator confirm.

### Layer 3 — Enrich (operator-confirmed write; ADR-0010 discipline)

The write lane **enriches records that already exist** (resolved Q2). Creating a
wholly-missing edition via `/api/import` is out of scope for this PRD (dedup risk +
elevated import privileges; see Alternatives). When Layer 1 resolved an existing
Open Library record and SP42 holds **sourced** values that record is missing:

- SP42 computes a **field-level gap proposal** against the live Open Library
  record. Two classes of field, with different provenance rules:
  - **Structured fields — relayed verbatim, never authored.** Missing `subjects`,
    cover, `publish_date`/`number_of_pages`, publisher, an ISBN-13 to sit alongside a
    bare ISBN-10, a link back to the source. **Every such field traces verbatim to a
    source** — Citoid, the resolved scan's title-page/OCR, or Wikidata — and the
    proposal shows that provenance. This is the PRD-0008 "SP42 authors nothing" rule,
    extended to a new target.
  - **Description — synthesized, but only from rich sourced context (resolved Q3).**
    SP42 *may* propose a model-**synthesized** `description`, but **only when it has
    assembled a substantial structured context** for the book — in practice, a linked
    Wikidata item plus the Open Library / Citoid facts. The synthesis is constrained
    to facts present in that assembled context (grounded synthesis, not open-ended
    generation), and the proposal shows the sources it drew from. Where that rich
    context is absent, **no description is proposed.** This is the deliberate
    "judgment in the loop" crossing PRD-0008 foreshadowed (its closing note that
    later features in this family cross the author-nothing boundary "under the spawned
    ADR, when they put judgment in the loop"); it is the one generative affordance in
    the lane and it carries its own DoD and risk treatment below.
- The operator sees the record's current value and the proposed value **field by
  field** — including, for a synthesized description, the sources it was built from —
  and confirms or dismisses. It is a **proposal, not a write**: nothing reaches
  Open Library without the operator confirming that exact change (ADR-0010).
- On confirm, SP42 applies the change through Open Library's write API **under the
  operator's own per-operator Open Library session** (resolved Q1) — connected and
  stored like the existing MediaWiki OAuth session, never a shared/baked-in service
  key — with a descriptive edit comment (`SP42: sourced metadata enrichment`),
  respecting Open Library's rate limits and import/bot etiquette. The edit is
  attributed to the human who confirmed it and is reversible (wiki history) — an
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
      (when present) its Internet Archive scan through a **side-effect-free** read
      (Books/Read API, **not** `/isbn/{isbn}.json`), verified over a replayed
      `/api/books` fixture; a resolution **miss issues no import/write** (asserted by
      the mock client seeing only the read endpoint), and a citation with **no**
      resolvable identifier stays `skipped` with the refined reason.
- [ ] A resolved book with a searchable scan never grounds against a
      Read-API `similar` match: only exact-edition scans enter Layer 2, and a
      similar-edition-only response is reported `SourceUnavailable` with the
      refined reason, verified by a replayed Read API fixture.
- [ ] A resolved book with a searchable exact-edition scan may produce a
      `supported` / `partial` / `not_supported` verdict, but only `supported` and
      `partial` require a supporting passage **verbatim-located in the returned
      search-inside snippet**, verified by a replayed search-inside fixture; the
      ADR-0007 grounding gate rejects a passage not present in the snippet, and a
      `not_supported` verdict is never required to fabricate one.
- [ ] A citation with `|page=` searches that page first and **falls back to a
      whole-book search** when it misses; the finding reports the **scanned** page the
      passage was found on, verified by fixtures for both the page-hit and the
      fallback path.
- [ ] The verdict carries a page-anchored deep link into the scan, verified by a
      renderer test.
- [ ] A resolved book with **no usable body** (no `ocaid`, not a text item, or no
      full-text index) degrades to `SourceUnavailable` (correct `unreachable`/`unusable`
      split), verified by a fixture test.
- [ ] A resolved book whose scan **is** indexed but whose search-inside returns **zero
      matching snippets** (after both passes) is reported `not_supported`, **not**
      `SourceUnavailable`, verified by a fixture test asserting the verdict and that the
      report stats count it as not-supported.
- [ ] A thin Open Library record yields a field-level enrichment proposal whose
      **structured** fields are each populated **verbatim** from a named source
      (Open Library / Citoid / Wikidata), verified by a renderer test over replayed
      fixtures; a record already complete yields **no** proposal.
- [ ] A **synthesized description** is proposed **only** when a rich structured
      context (a linked Wikidata item + Open Library/Citoid facts) is present, shows
      the sources it drew from, and is withheld when that context is absent, verified
      by fixtures for both the rich-context and sparse-context cases.
- [ ] Confirming applies **exactly the proposed field change** to Open Library under
      the operator's **own per-operator session** with the edit comment, verified by a
      mock-write-path test; **no** write occurs without a confirmation bound to that
      exact proposal.
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
- *Let the model write a description from its own open-ended knowledge.* Rejected:
  open-ended generation violates the anti-fabrication posture (ADR-0007). The
  admitted form (resolved Q3) is narrower — a description **synthesized only from an
  assembled, sourced structured context** (a linked Wikidata item + Open Library/
  Citoid facts), constrained to those facts and shown with its provenance, and only
  when that context is rich enough. Subjects and other structured fields are always
  relayed verbatim, never authored.
- *Never propose a description at all (structured fields only).* Considered and not
  taken: it is the strictest reading of PRD-0008, but it leaves the single most
  useful reader-facing field permanently empty even when SP42 holds ample sourced
  facts to ground one. The grounded-synthesis rule above is the deliberate,
  bounded crossing instead — with the operator's confirmation as the gate.
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
- **Duplicating existing Open Library ↔ Wikidata sync.** OL and Wikidata already
  exchange data: the P648 "Open Library ID" property links them, OL runs an official
  Wikidata Integration (pulling author bios, photos, awards, and notable works *from*
  Wikidata and pushing OL IDs *to* it), and `cdrini/openlibrary-wikidata-bot` does
  identifier sync/cleanup. Mitigation: this lane must **not** re-do author-level
  Wikidata sync OL already owns — it targets **edition-level, source-derived fields**
  (ISBN-13 alongside ISBN-10, page count, subjects, cover, and the source-grounded
  description) that the existing sync does not cover. Pulling a linked Wikidata item as
  *description context* (Q3) is consistent with OL already ingesting Wikidata author
  data, not a parallel pipeline. Author-biography enrichment is explicitly out of scope
  (OL's own integration handles it). Sibling note: the Wikidata write direction
  (PRD-0011 / ADR-0017) records the same de-duplication and the FRBR work/edition
  sourcing rule.
- **Synthesized description drifts from its sources (the one fabrication surface).**
  The description is the only generative field, so it is the only place a claim can
  appear that no source backs. Mitigations: it is proposed only when a rich sourced
  context is present; it is constrained to that context (grounded synthesis, not
  open-ended); the proposal shows the sources it drew from; and the operator confirms
  it field-by-field. A candidate mechanical floor — check each salient sentence of the
  synthesized description against the assembled context before offering it (the same
  spirit as ADR-0007's located-passage gate) — is worth carrying into the ADR. This is
  the crossing of PRD-0008's author-nothing line and is recorded here, not buried.
- **Operator habituation (rubber-stamping).** Mitigation: field-level diffs are
  small and sourced; a complete record yields no proposal at all, so proposals stay
  rare and worth reading. The synthesized description is the field most at risk of a
  rubber-stamp, which is the reason its provenance is shown inline.
- **Credential handling for Open Library.** The write lane needs the operator's
  Open Library session (resolved Q1: per-operator, never a shared/baked-in key).
  Mitigation: store and refresh it exactly like the existing per-operator MediaWiki
  session; an operator without a connected Open Library account simply does not see
  the write lane (the read-only Resolve/Ground lanes still work).
- **Three more external services (Open Library, IA search-inside, Wikidata).**
  Mitigation: each sits behind the `HttpClient` trait boundary with pure builders/
  parsers and replayed fixtures (ADR-0004/0008 pattern, as Citoid does); any one
  being unavailable degrades gracefully — a missing Wikidata item just means no
  synthesized description, a missing scan just means no grounding — and never blocks
  the review flow. Server-side fetches honor the SSRF floor (SP42#34) like ADR-0011
  Decision 5.

## Resolved questions

All six carry the Editor's decided answers (2026-07-01), folded into the body above;
they remain open to reviewer reaction until acceptance.

1. **Open Library credential model.** Resolved: **per-operator session**, connected
   and stored like the existing MediaWiki OAuth session, so every enrichment is
   attributed to the human who confirmed it. No shared/baked-in service key.
2. **Enrich-only, or also import missing editions?** Resolved: **enrich existing
   records only** for this PRD. Importing a missing edition (`/api/import`) is
   deferred to its own PRD/DoD (dedup risk + import-privilege requirements).
3. **Where do enrichment fields come from, and is a description ever authored?**
   Resolved in two parts. **Structured fields** (identifiers, publish date, page
   count, publisher, subjects, cover, source link) are **relayed verbatim** from a
   named source (Open Library / Citoid / the scan / Wikidata) and never authored. A
   **description** *may* be **model-synthesized, but only from a rich assembled
   sourced context** — in practice a linked **Wikidata** item plus Open Library/
   Citoid facts — constrained to those facts and shown with provenance; where that
   context is absent, no description is proposed. This adds Wikidata as a secondary
   structured authority (Layer 1) and is the one deliberate crossing of PRD-0008's
   author-nothing line.
4. **Full-text availability signal.** Resolved (by API check): a book is groundable
   when the Open Library edition has an `ocaid` **and** the archive.org item is a text
   item whose **search-inside** endpoint can run. Search-inside returns a **verbatim
   snippet + page** and typically works even for lending-restricted / in-copyright
   scans, so SP42 grounds on the snippet without borrowing. Two empty results differ
   (ADR-0007, per Layer 2): **no usable body** (no `ocaid` / not a text item / no
   full-text index) → `SourceUnavailable` (`unreachable`/`unusable`); but an
   **indexed** scan whose search-inside returns **zero matching snippets** →
   `not_supported`, **not** `SourceUnavailable` — the source was searched and yielded
   no supporting passage.
5. **Grounding a *page-specific* citation.** Resolved: **cited page first, then fall
   back to a whole-book search**, and report the **scanned** page the passage was
   actually found on (so a pagination mismatch surfaces instead of causing a false
   `not_supported`).
6. **Does this need its own ADR now, or ride ADR-0010/0011?** Resolved: **two thin
   ADRs.** (a) A **read-contract ADR** — Internet Archive search-inside as a new
   grounding source type feeding the existing verifier, plus Wikidata as an enrichment
   context source. (b) If the enrichment lane ships, a **separate apply-contract ADR**
   for the Open Library write: ADR-0010's propose/confirm/refuse-on-drift **discipline**
   transfers, but its **mechanism** is MediaWiki-specific (`WikitextNodeLocator` +
   `replacement_wikitext` + `baserevid` + wiki session/CSRF) and does **not** map to a
   field-level Open Library JSON change guarded by the OL record's own revision under an
   OL session — so the OL payload, auth, and drift check get their own thin ADR, exactly
   as the Wikidata statement write does (ADR-0017 over ADR-0010). **Not "reuse ADR-0010
   as-is."** This matches the header's "a second ADR … against a non-wiki target."
   Numbers assigned when drafted.
