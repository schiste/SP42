# Wikidata

A SP42 domain: making **Wikidata** a first-class target — reviewable in the patrol
workflow, and a structured, referenced source/sink of facts for the citation work.
Wikidata revisions are entity JSON (labels, descriptions, aliases, statements,
sitelinks), not wikitext, so the domain builds on a shared **platform** read
mechanism (entity revision read + `EntityDiff` + content-model routing/gating,
ADR-0015) rather than reimplementing it. Continues
[ADR-0014](../../platform/adr/0014-wikimedia-oauth-and-any-project.md)
(resolve any Wikimedia project) — Wikidata is already resolvable; this domain makes
it *usable*.

Per the "reuse-by-design ⇒ platform" rule ([adding-a-domain](../../process/adding-a-domain.md)),
the read mechanism is platform; this domain owns only the Wikidata-native
**workflows** (policy/config), each operator-confirmed and reversible where it
writes (ADR-0010).

## Product Requirements

- [PRD-0011 — Wikidata as a first-class SP42 target](prd/0011-wikidata-first-class-target.md)
  — the read module + patrol-read MVP, and the use-case family (book/entity
  enrichment, citation→Wikidata facts, Wikidata→sources)

## Architecture Decision Records

- ADR-0015 (platform, forthcoming) — Wikidata entity content-model: revision read +
  `EntityDiff` mechanism, content-model routing, and capability gating. Spawned by
  PRD-0011; to be drafted with the read-module implementation.

## Relation to other domains

- **References / citation verification** — the citation→Wikidata-facts and
  Wikidata→sources workflows bridge into the citation family (ADR-0007
  anti-fabrication gate, ADR-0010 propose/confirm). PRD-0009 already uses Wikidata as
  book-metadata context.
- **Patrolling** — Wikidata patrol consumes the shipped review workflow (ingestion,
  actions, coordination); the only new piece it needs is the entity read/render path.
