# PRD-0011: Wikidata as a first-class SP42 target

**Drafter:** Claude Code
**Editor:** Luis Villa
**Date:** 2026-07-01
**State:** Draft
**Discussion:** design conversation 2026-07-01; PR to follow. Continues
[ADR-0014](../../../platform/adr/0014-wikimedia-oauth-and-any-project.md)
(resolve any Wikimedia project).
**Spawned ADRs:**
- [ADR-0016](../../../platform/adr/0016-wikidata-entity-content-model.md) (platform,
  Proposed) — Wikidata entity content-model: revision read + `EntityDiff` mechanism,
  per-revision content-model routing, and capability gating.
- [ADR-0017](../../../platform/adr/0017-wikidata-statement-proposal-write-contract.md)
  (platform, Proposed) — Wikidata statement-proposal write contract (propose/confirm
  for entity statements: drift against the entity revision, reference attachment),
  reusing ADR-0010's discipline and ADR-0007's grounding gate. Its concrete
  fact-extraction workflow is the citation→facts follow-on PRD (resolved Q5/Q6).

Both ADRs are drafted up front (Proposed) so this PRD's forward references are
concrete before review. This PRD owns the user-facing intent; the ADRs own the
structural contracts.

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

### Platform foundation — the read module (→ ADR-0016)

- **Content-model-aware read path.** Fetch an entity revision and its parent via the
  Action API (`prop=revisions`, entity JSON) and parse into a structured
  **`EntityDiff`**: statements, labels, descriptions, aliases, and sitelinks, each
  classified added / removed / changed. Statements are diffed at **full depth**
  (resolved Q4) — main property/value **plus qualifiers, rank, and references** —
  so an edit that touches only a qualifier, flips a rank, or strips a reference is
  never rendered as a no-op (the SP42 honesty invariant: a real change is never shown
  as unchanged). A reusable platform mechanism paralleling the wikitext
  `diff_engine`/`media_diff`, but for entities.
- **Content-model routing + capability gating.** Keyed on the revision's
  `content_model` (`wikibase-item` vs `wikitext`), the platform routes to the entity
  path or the wikitext path; the capability profile (ADR-0014 machinery) declares
  which features a target supports, so Wikidata switches *off* the wikitext-only
  signals and (for the MVP) scoring rather than misapplying them. The wikitext path
  is untouched for wikitext targets.

### MVP — Wikidata patrol (the read experience)

- Picking Wikidata in the **existing** picker gives a real, readable patrol
  experience. **Reads point at production `wikidatawiki`** (resolved Q2) — public
  data, zero write risk, and the only source of enough real change volume to validate
  the entity diff — while live **writes gate to `testwikidatawiki`**, matching the
  established posture of reading production enwiki/frwiki but gating edits to testwiki
  (PRD-0008). The queue ingests Wikidata recentchanges (already generic) and each
  change renders as a **human-readable entity diff** — "added statement *educated at*
  → *University of X*, referenced to …", "changed the English label", "removed a
  sitelink" — not raw JSON.
- **The queue filters out bot edits** (resolved Q3) via the recentchanges query
  (`rcshow=!bot`) — a query flag, not a model, so it does not reopen the scoring
  decision — and orders the remaining human/tool edits chronologically. Wikidata's
  edit stream is overwhelmingly bots; without this filter a chronological queue is
  unusable. Richer *ranking* rides the scoring follow-on.
- Reviewer **actions** (patrol / rollback) already route through the generic Action
  API and work as-is; the MVP's write acceptance gate is on `testwikidatawiki`, and
  undo's entity-restore semantics are a flagged follow-on.
- **Scoring is gated off** for Wikidata in the MVP: the Wikipedia-trained revertrisk
  model does not describe entity edits, so the queue is ordered chronologically
  (over the bot-filtered stream), without a damage score, until a
  Wikidata-appropriate signal exists. An honest unranked queue beats a fabricated
  score.

### Use-case family (roadmap — each a follow-on PRD reusing the read module + the ADR-0010 confirm discipline)

- **Book / entity metadata enrichment — PRD-0009 (already specified).** Wikidata as
  sourced context for enriching Open Library records.
- **Citation → Wikidata facts — the immediate follow-on to the read MVP (resolved
  Q5).** When a citation is added to article X, scan the cited source for facts about
  X and propose them as **referenced Wikidata statements** on X's item — each a
  property/value pair carrying the citation as its reference. This is the anti-fabrication gate (ADR-0007) in its most constrained
  form: a statement is offered only when the fact is verbatim-locatable in the
  source, and it lands only on operator confirmation (ADR-0010), attributed to the
  operator's own account. Structured triples are *safer* than the free-text
  description crossing in PRD-0009 — there is no prose to synthesize, only a sourced
  triple.
- **Wikidata → sources (follows citation→facts).** Mine the references already
  attached to Wikidata's statements about X to surface additional candidate sources
  for a claim under review, feeding the existing citation-verification and
  bare-URL-repair flows. Read-only — a smaller delta on the read module, sequenced
  second only because Q5 prioritized the write capability.

Everything reuses the same substrate: the platform read module reads entities; the
workflows are thin domain policy; every write is operator-confirmed, sourced, and
reversible.

## Definition of Done

*MVP scope = the Wikidata patrol read experience. Each item binds to a test or
observable; tests replay recorded responses (ADR-0009 discipline), no live network.
The write-lane use cases carry their own DoD in their follow-on PRDs.*

- [ ] Selecting Wikidata loads a patrol queue from **production `wikidatawiki`**
      recentchanges with **bot edits excluded** (`rcshow=!bot`), ordered
      chronologically, verified by a replayed recentchanges fixture test that asserts
      bot edits are filtered out.
- [ ] An entity revision + its parent parse into a structured `EntityDiff`
      (statements / labels / descriptions / aliases / sitelinks, each
      added·removed·changed) at **full statement depth — property, value, qualifiers,
      rank, and references** — verified over replayed entity-revision fixtures; an
      edit touching **only** a qualifier, rank, or reference renders as a change and
      **never as a no-op**, and a missing parent (first revision) degrades gracefully
      rather than erroring.
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
  facts, and enrichment alike, so it is a platform mechanism (ADR-0016); only the
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
  wikitext), pinned by ADR-0016 and enforced by the layer check.

## Resolved questions

All six carry the Editor's decided answers (2026-07-01), folded into the body above;
they remain open to reviewer reaction until acceptance.

1. **Domain placement.** Resolved: **hybrid / defer.** The entity **read mechanism
   lives in platform** (ADR-0016); **patrol-Wikidata extends the patrolling domain**
   (it is patrol policy on a new content model, reusing the shipped workflow). No new
   `sp42-wikidata` crate is stood up for the MVP; whether the *write-lane* workflows
   (citation→facts, Wikidata→sources) get their own domain or extend references is
   **deferred** until those workflows are specified. (This doc area remains their
   home meanwhile; PRDs refile by ID, not path.)
2. **MVP live target.** Resolved: **read production `wikidatawiki`, gate live writes
   to `testwikidatawiki`** — matching the existing read-production / write-test
   posture for Wikipedias (PRD-0008), and giving the entity diff real change volume
   to validate against. Not test-only.
3. **Queue ranking without scoring.** Resolved: **chronological over a bot-filtered
   stream** (`rcshow=!bot`). Filtering bots is a query flag, not a model, so it does
   not reopen the no-scoring decision; it is necessary because Wikidata's bot volume
   would otherwise make a chronological queue unusable. A richer ranking heuristic
   rides the scoring follow-on.
4. **Entity-diff depth.** Resolved: **full depth now** — statements diffed on
   property, value, qualifiers, rank, and references. This also secures the invariant
   that an edit touching only a qualifier/rank/reference is never rendered as a no-op.
5. **Which write use case comes first after the read MVP.** Resolved: **citation→
   facts first** (the write capability), then Wikidata→sources. This makes the
   statement-proposal write lane the immediate follow-on design work.
6. **ADR split.** Resolved: **ADR-0016 (platform) for the read mechanism now; a
   separate thin ADR-0017 for the statement-proposal write contract**, drafted with
   the citation→facts follow-on PRD. ADR-0010's wikitext node-anchor/`baserevid`
   mechanism does not map to entity statements (drift is detected against the entity
   revision), so its principle is reused but its mechanism is not — the same reason
   ADR-0010 itself was a thin ADR over ADR-0003.
