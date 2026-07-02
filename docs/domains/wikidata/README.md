# Wikidata

Making **Wikidata** a first-class target — reviewable in the patrol workflow, and a
structured, referenced source/sink of facts for the citation work. Wikidata
revisions are entity JSON (labels, descriptions, aliases, statements, sitelinks),
not wikitext, so the work builds on a shared **platform** read mechanism (entity
revision read + `EntityDiff` + content-model routing/gating, ADR-0016) rather than
reimplementing it. Continues
[ADR-0014](../../platform/adr/0014-wikimedia-oauth-and-any-project.md)
(resolve any Wikimedia project) — Wikidata is already resolvable; this makes it
*usable*.

**Crate placement (resolved in PRD-0011 Q1 — hybrid/defer):** the read mechanism is
**platform**; **patrol-Wikidata extends the patrolling domain** (patrol policy on a
new content model, reusing the shipped workflow). No dedicated `sp42-wikidata` crate
is created for the MVP; whether the write-lane workflows (citation→facts,
Wikidata→sources) get their own domain or extend references is deferred until they
are specified. This doc area is their home meanwhile — records refile by ID, not
path.

## Product Requirements

- [PRD-0011 — Wikidata as a first-class SP42 target](prd/0011-wikidata-first-class-target.md)
  — the read module + patrol-read MVP, and the use-case family (book/entity
  enrichment, citation→Wikidata facts, Wikidata→sources)

## Architecture Decision Records

- [ADR-0016](../../platform/adr/0016-wikidata-entity-content-model.md) (platform,
  Proposed) — Wikidata entity content-model: revision read + `EntityDiff` mechanism,
  per-revision content-model routing, and capability gating. Spawned by PRD-0011.
- [ADR-0017](../../platform/adr/0017-wikidata-statement-proposal-write-contract.md)
  (platform, Proposed) — Wikidata statement-proposal write contract (propose/confirm
  for entity statements; drift vs. the entity revision; reference attachment),
  reusing ADR-0010 discipline + ADR-0007 grounding. Its fact-extraction workflow is
  the citation→facts follow-on PRD.

## Relation to other domains

- **References / citation verification** — the citation→Wikidata-facts and
  Wikidata→sources workflows bridge into the citation family (ADR-0007
  anti-fabrication gate, ADR-0010 propose/confirm). PRD-0009 already uses Wikidata as
  book-metadata context.
- **Patrolling** — Wikidata patrol consumes the shipped review workflow (ingestion,
  actions, coordination) and extends it; the only new piece it needs is the entity
  read/render path (platform).
