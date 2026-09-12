# Wikidata Statement-Proposal Write Contract Design

Implements [ADR-0017](../platform/adr/0017-wikidata-statement-proposal-write-contract.md)
(Proposed) — the write-contract half only. The fact-extraction producer remains the
citation→facts follow-on PRD (PRD-0011 Q5).

## Summary

<!-- TO BE GENERATED after body is written -->

## Definition of Done

**Deliverable** — a second operator-confirmed write lane (entity statements) alongside
ADR-0010's wikitext lane: shared wire types in `sp42-platform`, a propose route and an
apply route, CLI preview/execute, gated to `testwikidatawiki`.

**Done when:**

1. `configs/testwikidatawiki.yaml` enables the lane; production `wikidatawiki` omits
   the key and both routes refuse with a structured code and **zero writes**.
2. Propose fetches the entity, captures `last_revid` as the drift baseline, requires
   non-empty ADR-0007 grounding (empty → **structured decline**, not an error), and
   returns a replayable payload with a server-rendered before/after built from the
   existing `EntityDiff` renderer.
3. Apply replays that payload **verbatim** under the operator's own session — session +
   CSRF + `editpage` capability gates, MediaWiki CSRF token, a single write carrying
   `baserevid`.
4. Entity drift refuses with **zero writes**, asserted against an in-process mock
   Wikibase whose edit log is empty.
5. `sp42-cli` gains preview/execute subcommands mirroring `dispatch_bare_url`.
6. A live acceptance write on `test.wikidata.org` is performed and recorded in the
   closing PR.

**Out of scope** — fact extraction (citation→facts PRD), entity/edition creation, FRBR
work-vs-edition resolution (ADR-0017 Decision 8), statement deletion/merge/rank edits,
batch apply, the `sp42-app` UI.

**Resolved up front (clarification phase):**

- Grounding is **required and recorded, not re-verified** at apply; re-verification is
  the producer's job.
- The write gate lives in a new `configs/testwikidatawiki.yaml`, mirroring ADR-0010
  Decision 4 ("production configs simply omit the key"), because Wikidata configs are
  otherwise derived from the embedded SiteMatrix with `templates: None`.
- Both a propose (preview) route and an apply route ship; the server renders the
  before/after so the drift baseline is server-captured, not client-trusted.

## Glossary

<!-- TO BE GENERATED after body is written -->
