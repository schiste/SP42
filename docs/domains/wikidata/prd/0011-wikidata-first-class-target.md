# PRD-0011: Wikidata as a first-class SP42 target

**Drafter:** Claude Code
**Editor:** Luis Villa
**Date:** 2026-07-01
**State:** Draft
**Discussion:** design conversation 2026-07-01; PR to follow. Continues
[ADR-0014](../../../platform/adr/0014-wikimedia-oauth-and-any-project.md)
(resolve any Wikimedia project).
**Spawned ADRs:** ADR-0015 (platform) — Wikidata entity content-model: revision
read + `EntityDiff` mechanism, content-model routing, and capability gating. To be
drafted alongside the read-module implementation (this PRD owns the user-facing
intent; ADR-0015 owns the structural contract).

## Problem

SP42 patrols Wikipedias. It can already *resolve* any Wikimedia project (ADR-0014):
`wikidatawiki` and `testwikidatawiki` are in the embedded SiteMatrix, derive a full
`WikiConfig`, and already appear in the wiki picker. But the moment an operator
picks Wikidata, the experience collapses. A Wikidata revision is **entity JSON**
(labels, descriptions, aliases, statements, sitelinks), not wikitext, so:

- the diff view renders raw JSON as a line-diff — technically a diff, useless for
  review: a patroller cannot judge a statement change from a JSON hunk;
- every wikitext-shaped signal is silent or misapplied — media-reference
  extraction, talk-page warning parsing, citation tagging, Parsoid rendering, and
  the Wikipedia-trained LiftWing revertrisk score all assume wikitext.

So Wikidata is **reachable but not reviewable**.

And Wikidata is not merely one more wiki to patrol — it is the structured hub the
citation work already leans on (PRD-0009 uses it as book-metadata context). Two
operator workflows the citation family wants are impossible today:

- turning a newly-cited source into **sourced Wikidata statements** about the
  subject, and
- **mining Wikidata's existing statement references** to find sources for a claim
  under review.

This is for the same experienced patroller/editor as PRD-0002 and PRD-0008, now
working on Wikidata items — plus the citation reviewer who wants Wikidata as a
source and sink of structured, referenced facts. Books (PRD-0009) are just one case;
the target is Wikidata as a first-class citizen.

## Proposal

Establish Wikidata as a first-class target through **one shared platform mechanism**
(the read module) and a **family of workflows** on top of it, sequenced
read-only-first. The split follows the "reuse-by-design ⇒ platform" rule
(`adding-a-domain.md`): the read mechanism is reused by every Wikidata workflow, so
it is platform; the workflows are thin domain policy.

### Platform foundation — the read module (→ ADR-0015)

- **Content-model-aware read path.** Fetch an entity revision and its parent via the
  Action API (`prop=revisions`, entity JSON) and parse into a structured
  **`EntityDiff`**: statements, labels, descriptions, aliases, and sitelinks, each
  classified added / removed / changed, carrying the property/value and any
  reference. A reusable platform mechanism paralleling the wikitext
  `diff_engine`/`media_diff`, but for entities.
- **Content-model routing + capability gating.** Keyed on the revision's
  `content_model` (`wikibase-item` vs `wikitext`), the platform routes to the entity
  path or the wikitext path; the capability profile (ADR-0014 machinery) declares
  which features a target supports, so Wikidata switches *off* the wikitext-only
  signals and (for the MVP) scoring rather than misapplying them. The wikitext path
  is untouched for wikitext targets.

### MVP — Wikidata patrol (the read experience)

- Picking `wikidatawiki`/`testwikidatawiki` in the **existing** picker gives a real,
  readable patrol experience: the queue ingests Wikidata recentchanges (already
  generic), and each change renders as a **human-readable entity diff** — "added
  statement *educated at* → *University of X*, referenced to …", "changed the English
  label", "removed a sitelink" — not raw JSON.
- Reviewer **actions** (patrol / rollback) already route through the generic Action
  API and work as-is; the MVP exercises them on `testwikidatawiki` — the safe target,
  paralleling how `testwiki` gates live edits — and leaves undo's entity-restore
  semantics as a flagged follow-on.
- **Scoring is gated off** for Wikidata in the MVP: the Wikipedia-trained revertrisk
  model does not describe entity edits, so the queue orders Wikidata changes
  chronologically, without a damage score, until a Wikidata-appropriate signal
  exists. An honest unranked queue beats a fabricated score.

### Use-case family (roadmap — each a follow-on PRD reusing the read module + the ADR-0010 confirm discipline)

- **Book / entity metadata enrichment — PRD-0009 (already specified).** Wikidata as
  sourced context for enriching Open Library records.
- **Citation → Wikidata facts.** When a citation is added to article X, scan the
  cited source for facts about X and propose them as **referenced Wikidata
  statements** on X's item — each a property/value pair carrying the citation as its
  reference. This is the anti-fabrication gate (ADR-0007) in its most constrained
  form: a statement is offered only when the fact is verbatim-locatable in the
  source, and it lands only on operator confirmation (ADR-0010), attributed to the
  operator's own account. Structured triples are *safer* than the free-text
  description crossing in PRD-0009 — there is no prose to synthesize, only a sourced
  triple.
- **Wikidata → sources.** Mine the references already attached to Wikidata's
  statements about X to surface additional candidate sources for a claim under
  review, feeding the existing citation-verification and bare-URL-repair flows.
  Read-only.

Everything reuses the same substrate: the platform read module reads entities; the
workflows are thin domain policy; every write is operator-confirmed, sourced, and
reversible.

## Definition of Done

*MVP scope = the Wikidata patrol read experience. Each item binds to a test or
observable; tests replay recorded responses (ADR-0009 discipline), no live network.
The write-lane use cases carry their own DoD in their follow-on PRDs.*

- [ ] Selecting `wikidatawiki`/`testwikidatawiki` in the picker loads a patrol queue
      from Wikidata recentchanges, verified by a replayed recentchanges fixture test.
- [ ] An entity revision + its parent parse into a structured `EntityDiff`
      (statements / labels / descriptions / aliases / sitelinks, each
      added·removed·changed with property, value, and reference), verified over
      replayed entity-revision fixtures; a missing parent (first revision) degrades
      gracefully rather than erroring.
- [ ] The patrol/diff view renders a Wikidata change as a **human-readable entity
      diff, not raw JSON**, verified by a renderer test.
- [ ] Content-model routing selects the entity path for `wikibase-item` and the
      wikitext path for `wikitext`, verified by a routing test over both models.
- [ ] A Wikidata target's capability profile reports the wikitext-only features
      (media-ref, warning parsing, citation extraction) and scoring as
      **unavailable**, and those paths are not invoked, verified by a
      capability-gating test.
- [ ] No revertrisk score is computed for Wikidata changes (no LiftWing request
      built, no fabricated score), verified by an adapter test.
- [ ] A patrol/rollback action targets `testwikidatawiki` through the generic Action
      API under the operator's session, verified by a mock-write-path test; the live
      acceptance gate on `test.wikidata.org` is recorded in the closing PR.

## Alternatives

- *Treat Wikidata as "just another derived wiki" and render the raw JSON line-diff.*
  Rejected: it is reachable today and useless — the entity read module is the whole
  point of the change.
- *Put the entity read/diff in a Wikidata domain crate.* Rejected by
  "reuse-by-design ⇒ platform": the read mechanism is reused by patrol, citation→
  facts, and enrichment alike, so it is a platform mechanism (ADR-0015); only the
  workflows are domain policy.
- *Use the Wikibase REST API instead of the Action API for reads.* Rejected for the
  read/diff path: the REST API is convenient for the current entity but weaker for
  arbitrary historical revisions, and patrol diffs a revision against its parent. The
  Action API `prop=revisions` returns both revisions' entity JSON with parity to the
  existing revision-fetch path. (REST may still suit later write workflows.)
- *Feed Wikidata edits to the existing revertrisk model.* Rejected: the model is
  Wikipedia-trained; the score would be noise. Gate scoring off until a
  Wikidata-appropriate signal exists.
- *Ship all use cases at once.* Rejected: the read module is the shared prerequisite;
  the write-lane workflows are follow-on PRDs that reuse it. The MVP proves the read
  experience first.

## Risks

- **Entity-diff readability is the product.** If the rendered entity diff is not
  genuinely clearer than JSON, the MVP fails its purpose. Mitigation: the DoD binds a
  human-readable renderer test; validate on real `testwikidatawiki` changes
  (statement add with reference, label edit, sitelink change, a vandalism-shaped
  value swap).
- **Scoring gap leaves Wikidata queues unranked.** Without a damage score, Wikidata
  patrol is chronological, not risk-ranked. Mitigation: explicit MVP non-goal; the
  queue still ingests and renders; a Wikidata-appropriate signal is a tracked
  follow-on. An honest unranked queue beats a fabricated one.
- **Undo on entities.** MediaWiki undo may not cleanly restore entity state the way
  it does wikitext. Mitigation: the MVP scopes actions to patrol/rollback (entity
  safe); undo's entity semantics are a flagged follow-on, not silently assumed.
- **Citation → facts writes into a machine-consumed knowledge base.** A wrong
  statement is worse than wrong prose. Mitigation (for that follow-on PRD): sourced
  only, verbatim-grounded (ADR-0007), each statement carries its reference,
  operator-confirmed (ADR-0010), reversible.
- **Platform surface growth.** A new content model widens the platform contract.
  Mitigation: it is additive and routing-gated (the wikitext path is untouched for
  wikitext), pinned by ADR-0015 and enforced by the layer check.

## Open questions

1. **Domain placement.** Do the Wikidata-native workflows (patrol Wikidata;
   citation→facts; Wikidata→sources) warrant their own `sp42-wikidata` domain crate,
   or does patrol-Wikidata extend the patrolling domain while the citation-linked
   workflows extend references? *Proposed:* a thin `sp42-wikidata` domain owns the
   Wikidata-native workflows; the entity **read mechanism lives in platform**
   (ADR-0015); patrol consumes both. React before ADR-0015 is filed.
2. **MVP live target.** `testwikidatawiki` only for the first cut (paralleling
   testwiki), with `wikidatawiki` enabled once the read/render/action loop is proven?
   *Proposed:* yes.
3. **Queue ranking without scoring.** Chronological for the MVP, or a cheap non-model
   heuristic (anonymous edits, statement removals, large value changes)? *Proposed:*
   chronological MVP; a heuristic rides the scoring follow-on.
4. **Entity-diff depth.** Does the MVP render full statement qualifiers/ranks/
   references, or start with property + value + reference and defer qualifiers/ranks?
   *Proposed:* property + value + reference for the MVP; qualifiers/ranks follow.
5. **Which write use case comes first after the read MVP** — citation→facts or
   Wikidata→sources? *Proposed:* Wikidata→sources first (read-only, feeds existing
   citation flows), then citation→facts (write).
6. **ADR split.** One platform ADR-0015 for the read mechanism, with the write-lane
   contract riding ADR-0010, or a second ADR for the statement-proposal contract?
   *Proposed:* ADR-0015 for read now; revisit a write ADR when citation→facts is
   specified.
