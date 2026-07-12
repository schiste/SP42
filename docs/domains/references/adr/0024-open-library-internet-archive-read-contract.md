# ADR-0024: Open Library / Internet Archive read contract

**Status:** Proposed
**Date:** 2026-07-07
**Author:** Luis Villa (drafted by Claude Code)

Spawned by PRD-0009 (book-citation grounding and Open Library enrichment),
resolved Q6(a): the **read-contract** ADR. It fixes how SP42 turns a book
citation's identifier into an Open Library catalog record, how it decides
whether an Internet Archive scan is usable for grounding, and how a
search-inside snippet enters the existing verification pipeline. The **write**
side (enriching an Open Library record) is the separate apply-contract ADR
PRD-0009 also names; nothing here authorizes a write.

## Context

The article-level verifier (ADR-0011) records a book citation as
`skipped { NonUrlSource }` — a `{{cite book}}` with an ISBN but no `url=`
yields nothing fetchable, so it gets no verdict and no affordance. PRD-0009
turns that dead end into resolution (Layer 1) and full-text grounding
(Layer 2), both read-only. Three properties of the external surfaces force a
recorded contract rather than ad-hoc client code:

1. **One documented Open Library "read" path has a write side-effect.** The
   `/isbn/{isbn}.json` endpoint is documented under *Import by ISBN* and can
   attempt an import or stage metadata when the ISBN is not in the catalog. A
   resolver that "just GETs the obvious URL" silently violates the read-only,
   no-auth promise of Layers 1–2 and the PRD's enrich-existing-only boundary.
2. **The Read API conflates two kinds of emptiness and two kinds of match.**
   An empty `items` list does not mean "no catalog record" — it means "no
   usable online scan." And a returned scan may be only a `similar` match
   (another edition of the same work), which must never ground a
   page-specific citation against the wrong edition.
3. **IA search-inside snippets are shorter than the generic body floor.** The
   fetched-page usability gate short-circuits bodies below a generic length
   floor (`ShortBody`) before verification. A verbatim OCR snippet is a valid
   grounding body at well below that floor, so routing it through the generic
   gate would misclassify real book evidence as unusable.

## Decision

### 1. Resolution is gated on a positive, validated identifier

- The only inputs to catalog resolution are identifiers read **from the cite
  template's own parameters** via Parsoid `data-mw` — `isbn`, `oclc`, `lccn`,
  and `ol` (the Open Library id param) — the same structured-extraction path
  ADR-0011 uses for `url=`/`archive-url=`. Citoid is not in this path (it is a
  URL→metadata sidecar; a URL-less book ref never reaches it).
- Extracted values are **normalized and shape-validated at construction**:
  ISBNs are stripped of hyphens/spaces and must pass the ISBN-10 (mod 11) or
  ISBN-13 (mod 10) checksum; LCCNs get the Library of Congress normalization
  (lowercase, space-free, `/`-suffix dropped, serial zero-padded); OCLC
  numbers are digits with the `ocm`/`ocn`/`on` prefixes stripped; `ol` values
  are canonicalized to `OL…M` / `OL…W` form. An **author** Open Library id
  (`OL…A`) is *not* a book identifier and is rejected — consistent with
  PRD-0009's author-context exclusion.
- A value that fails validation yields **no identifier** — a garbled ISBN is
  treated as "no identifier to resolve," not sent upstream to see what
  happens. **No title/author guessing, ever** (PRD-0009 Alternatives): a ref
  with no valid identifier stays skipped, with the skip reason distinguishing
  "no identifier" from "carries a book identifier."

### 2. Catalog resolution: the Books API only, side-effect-free

- The catalog lookup is
  `GET https://openlibrary.org/api/books?bibkeys={SCHEME}:{value}&jscmd=data&format=json`
  with the bibkey scheme matching the trusted identifier (`ISBN:`, `OCLC:`,
  `LCCN:`, `OLID:`). An explicit OLID may equivalently be read directly as
  `GET /books/OL…M.json`; both are pure reads.
- The resolver **must not** call `/isbn/{isbn}.json` (import-on-miss; Context
  §1). A lookup miss is **"no record found," never a create** — the empty
  `{}` Books API response maps to a `None` resolution, and no import,
  staging, or retry-with-write endpoint follows. Tests assert the built
  requests never address the import path.
- The parsed record keeps the fields the later layers need — edition key and
  record URL, title, authors, publishers, publish date, page count,
  ISBN-10/13, subjects, cover — parsed defensively (any missing field is
  simply absent; a malformed body is a failed parse, not a panic).

### 3. Scan availability: the Read API, exact matches only, after resolution

- Scan availability is a **separate, subsequent** question from catalog
  existence:
  `GET https://openlibrary.org/api/volumes/brief/{scheme}/{value}.json`
  (schemes `isbn`, `oclc`, `lccn`, `olid`). It is consulted only for
  readable/borrowable scan discovery, never as the catalog lookup.
- Returned items are partitioned by their `match` field. Only **`exact`**
  matches are eligible to feed grounding (Layer 2). A `similar` match (a scan
  of a *different edition* of the same work) may be surfaced to the operator
  as context but never becomes the cited book's source body; a similar-only
  response degrades grounding to `SourceUnavailable` with a refined
  "similar edition only" reason.
- An **empty `items` list is not a resolution failure**: the catalog record
  (from Decision 2) still exists and remains enrichable; only grounding
  degrades to `SourceUnavailable`. The two APIs' outcomes are therefore kept
  as separate values, never collapsed into one boolean.

### 4. Grounding source: the search-inside snippet, with a bounded gate bypass

- A book is **groundable** when the resolved edition carries an `ocaid` and
  the archive.org item is a text item whose full-text index allows
  search-inside to run (PRD-0009 resolved Q4). The scan's OCR is never
  downloaded whole; SP42 queries the item's search-inside endpoint (the
  BookReader full-text search, reached via the item's metadata-designated
  server) and receives verbatim OCR snippets with page numbers — which
  typically works even for lending-restricted scans, so grounding needs no
  borrow.
- The returned snippet is a **book-snippet source body**: it feeds the
  existing per-use-site verdict contract unchanged (ADR-0006/0007). The
  anti-fabrication gate is untouched — `supported`/`partial` still require a
  passage verbatim-located in the fetched bytes (here, the snippet), and the
  snippet is content-hashed and stored under the ADR-0009 snapshot/replay
  discipline.
- The book-snippet body **bypasses only the generic `ShortBody` page-body
  floor** (Context §3), via a provenance-checked wrapper: the bypass applies
  exclusively to bodies produced by the search-inside path, never to
  arbitrary short fetched web pages, which continue to short-circuit as
  before.
- Search order for a page-specific citation: the cited `|page=` first, then a
  whole-book fallback; the finding records the **scanned** page the passage
  was found on (PRD-0009 resolved Q5).
- Two empty outcomes stay distinct (ADR-0007 discipline): **no usable body**
  (no `ocaid` / not a text item / no index) → `SourceUnavailable`
  (`unreachable`/`unusable` split per ADR-0011 Decision 7); an **indexed scan
  whose search returns zero snippets** after both passes → `not_supported`.

### 5. House transport and test discipline apply unchanged

- Every request in this contract is a pure `build_*` → `HttpRequest` /
  `parse_*(bytes)` pair executed over the injected `HttpClient` trait, like
  Citoid (`citation/citoid.rs`). Server-side execution rides the guarded
  `sp42-fetch` source face (ADR-0015): openlibrary.org and archive.org hosts
  are reachable via attacker-influenced citation content, so they are treated
  as untrusted fetch targets (SSRF floor, caps, retry), not first-party APIs.
- All tests replay recorded fixtures; no live network (ADR-0009). Open
  Library and IA being third parties, any endpoint drift surfaces as a
  fixture update, not a hidden behavior change.
- These endpoints are best-effort context like Citoid: any failure degrades
  (no resolution / no grounding) and never blocks the URL-citation flow.

## Consequences

- Book refs stop being a uniform `NonUrlSource` dead end: the extractor can
  distinguish "no identifier" from "identifier present," and a resolved
  record gives the report something honest to print even before grounding
  ships.
- The side-effect-free rule is enforceable by test (assert the only endpoints
  addressed are `/api/books`, `/books/…json`, `/works/…json`, and
  `/api/volumes/brief/…`), so the import-on-miss hazard cannot regress
  silently.
- Keeping catalog resolution and scan availability as separate values commits
  the report/UI to representing "record exists, no scan" — mildly more
  plumbing than one boolean, deliberately.
- The `ShortBody` bypass is scoped to a provenance-checked body type, so the
  generic usability gate keeps its meaning for web pages.
- Two more external hosts join the fetch surface; both sit behind the guarded
  edge and the existing fixture-replay pattern.

## Alternatives considered

- **Resolve via `/isbn/{isbn}.json` (the "obvious" URL).** Rejected: its
  import-on-miss behavior is a write side-effect in a read lane (Context §1).
- **Use the Read API as the catalog lookup.** Rejected: it answers
  availability, not existence; an empty `items` would be misread as "no
  record" and would kill enrichment for every unscanned book.
- **Ground against `similar` matches when no exact scan exists.** Rejected:
  verifying a page-specific citation against a different edition is the
  classic wrong-edition failure; honesty (`SourceUnavailable` + context for
  the operator) beats a plausible-but-wrong verdict.
- **Lower the generic `ShortBody` floor instead of a scoped bypass.**
  Rejected: it would weaken the usability gate for every web fetch to
  accommodate one provenance-known source type.
- **Title/author fuzzy resolution.** Rejected for the MVP (PRD-0009
  Alternatives); revisit only with a confidence threshold if identifier
  coverage proves too thin.

## Out of scope / non-goals

- **Any Open Library write** — enrichment proposals and the apply mechanism
  are PRD-0009 Layer 3 and its separate apply-contract ADR.
- **archive.org item-metadata editing** (IA-S3/JSON-Patch) — excluded by
  PRD-0009's scope boundary.
- **Wikidata as enrichment context** — PRD-0009 Layer 1 allows pulling a
  linked book-level Wikidata item as context; that read rides the Wikidata
  entity machinery (ADR-0016) and adds no contract here.
- **Queue-scale book discovery** — this contract serves the revision under
  review (ADR-0011 footprint), not cross-article scanning.

## References

- PRD-0009 (book-citation grounding and Open Library enrichment; resolved
  Q1–Q6), ADR-0011 (article-level verification / skip semantics), ADR-0007
  (grounding gate), ADR-0009 (snapshot/replay), ADR-0015 (guarded fetch
  edge), ADR-0016 (Wikidata entity read).
- External: Open Library Books API (`/api/books`), Read API
  (`/api/volumes/brief`), *Import by ISBN* (`/isbn/{isbn}.json` — avoided),
  Internet Archive BookReader search-inside.
