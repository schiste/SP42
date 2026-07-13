# Bibliography-indirection book identifiers — design sketch

**Date:** 2026-07-13
**Status:** Sketch (pre-implementation)
**Governs the *how* for:** the PRD-0009 Layer-1 amendment of the same date
(identifiers carried by indirection). No new PRD: user-facing behavior,
outcome semantics, and honesty rules are PRD-0009's unchanged; this widens
which refs the extractor can feed it. No new ADR unless the short-cite
template list later moves into `sp42-wiki` capability profiles.

## Problem

The merged book lane (PRD-0009 Layers 1–3) extracts book identifiers only
from cite templates *inside* a `<ref>`. Articles using shortened footnotes
put the `{{cite book}}` (and its ISBN) in a bibliography section and cite it
by anchor: `{{sfn|Roxburgh|2014|pp=113–116}}`. Measured effect (2026-07-12):
Eurovision Song Contest 1973 converts **0 of 73** book refs — every one is
an `{{sfn}}`. Shortened footnotes are ~5% of enwiki overall but the house
style of book-sourced quality articles — exactly the GA-review target class.

## Research grounding (2026-07-13, fixtures saved)

Live Parsoid HTML probes of en/fr/de quality articles, plus the convention
pages (en Help:Shortened footnotes; fr Conventions bibliographiques /
Modèle:Sfn; de Zitierregeln / Vorlage:Literatur):

- **enwiki**: the ref's own `data-mw` carries the full `{{sfn}}` invocation
  (positional author params, year, `p`/`pp`); the ref body renders a literal
  `<a href="…#CITEREFRoxburgh2014" class="mw-selflink-fragment">`; the
  bibliography `<cite id="CITEREFRoxburgh2014">`'s template wrapper carries
  the complete `{{Cite book}}` params including `isbn`.
- **frwiki**: structurally identical via `{{harvsp}}`/`{{sfn}}` →
  `<span class="ouvrage" id="Martin-Demézil1986">` with `{{Ouvrage}}` params
  including `isbn` — but the anchor has **no CITEREF prefix**. Anchor-key
  *reconstruction* is therefore the wrong abstraction; following the ref
  body's **literal fragment href** is wiki-agnostic.
- **dewiki**: template-averse; no anchor convention in the wild. ISBNs
  surface as structured **magiclinks**
  (`<a href="./Special:BookSources/<digits>" class="… mw-magiclink-isbn">`),
  in refs and in the Literatur section. `{{Literatur}}` (param `ISBN`)
  exists but is optional and contested.
- CITEREF ids are real DOM `id` attributes (on the `<cite>`/`<span
  class="ouvrage">`, **not** the `<li>`, which gets an opaque Parsoid id).

## Design: two lanes, one abstraction

A ref's book sources = **ref-local sources ∪ linked-bibliography sources**.

**Lane A — link-following (en/fr structured indirection).**
1. Detect short-citation refs: the ref's `data-mw` template target is in the
   short-cite family list (`sfn`, `sfnp`, `harvsp`, `harvnb`, `harv`,
   `harvtxt`, case-insensitive; a shared cross-wiki constant with a config
   override seam — deliberately NOT per-wiki config until a wiki needs it).
2. Index bibliography candidates: every element in the document carrying a
   DOM `id` and a cite-template `data-mw` (or an ISBN magiclink), keyed by id.
3. Resolve: the ref body's first same-page fragment href, matched literally
   against the index. Fallback cross-check: a reconstructed key from the
   short-cite params tried with and without the `CITEREF` prefix (covers a
   missing/mangled body link). No match → the ref keeps its current skip.
4. Extract identifiers from the matched element: template params (`isbn`,
   `ISBN`, `oclc`, `lccn`, `ol` — the existing validated-identifier parse) ∪
   ISBN magiclinks inside the element. `cited_page` comes from the
   short-cite `p`/`pp`/`page`/`pages`/`loc` param — richer than the direct
   lane (sfn pages are per-use-site, feeding search-inside's
   cited-page-first pass exactly).

**Lane B — ref-local structured identifiers (dewiki primary; universal
fallback).** ISBN magiclinks and `{{ISBN}}`-template transclusions inside
the ref body itself become `BookSource`s directly (identifier from the
`Special:BookSources/<digits>` href — already normalized — validated by the
existing checksum). Page: trailing `S. <n>`/`p. <n>` pattern in the ref text
is NOT parsed in the MVP; `cited_page` stays `None` for lane B.

**Explicit non-goal (disclosed, not guessed):** dewiki-style *unlinked*
free-text short refs ("Müller 1999, S. 55" whose full citation lives only in
the Literatur section, connected by nothing machine-readable). Fuzzy
name/year text matching is deferred; these keep a refined skip reason so the
appendix's honesty arms name them.

## Placement

- `sp42-parsoid`: the document-level bibliography-id index, ref-body
  fragment-link reading, magiclink/ISBN-template extraction — mechanism,
  where `book_sources` are already populated today. `BlockRef.book_sources`
  gains entries; the contract type is unchanged (`BookSource { identifiers,
  cited_page }`), so downstream (extract → resolve → ground → report) needs
  **no changes** — the whole feature is upstream of the existing lane.
- Provenance nuance: a lane-A `BookSource` is the *bibliography entry's*
  book cited *at* the sfn's page. Nothing in the current contract records
  "via indirection"; an additive provenance note is a follow-up only if the
  report needs to disclose it (the verdict semantics don't change either way).
- Fixtures: trimmed fragments of the probe HTML (en sfn + bibliography, fr
  harvsp + ouvrage, de magiclink ref) checked in as parsoid test fixtures.

## Done when

- ESC 1973's `{{sfn}}` refs produce `BookSource`s with identifiers from the
  bibliography and per-ref `cited_page`s (fixture test on the trimmed en
  fragment; live smoke re-run recorded in the PR).
- The fr fixture resolves without any CITEREF assumption (prefix-less id).
- The de fixture yields ref-local magiclink `BookSource`s.
- An unresolvable short-cite ref (no matching id) keeps a skip with a
  refined reason, never a guessed identifier.
- Existing direct-template extraction is byte-identically unaffected
  (regression: the current book-lane tests all pass unchanged).
