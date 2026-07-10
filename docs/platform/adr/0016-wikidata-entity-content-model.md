# ADR-0016: Wikidata entity content-model — revision read, `EntityDiff`, and content-model routing

**Status:** Proposed
**Date:** 2026-07-01
**Author:** Luis Villa (drafted by Claude Code)

Spawned by PRD-0011 (Wikidata as a first-class SP42 target). This ADR owns the
**read** mechanism; the entity **write** contract is ADR-0017.

**Implementation note (2026-07-10):** Not yet implemented in platform. Only the
`sp42-mcp` `verify_wikidata_statement` verb reads entity JSON today, ad hoc via
`Special:EntityData`; `EntityDiff`, per-revision content-model routing, and the
capability gating this ADR specifies do not exist in the code. The working
design is `docs/design-plans/2026-07-01-wikidata-read-model.md`.

## Context

SP42 already resolves any Wikimedia project (ADR-0014): `wikidatawiki` and
`testwikidatawiki` derive a full `WikiConfig` from the embedded SiteMatrix and appear
in the wiki picker. But the review path assumes **wikitext** end to end. Revisions
are diffed by `StructuredDiff` — a line/char text diff (`diff_engine.rs`) — and the
wikitext-only signals (media-reference extraction, talk-page warning parsing,
citation extraction/Parsoid, and the Wikipedia-trained LiftWing revertrisk score) run
unconditionally. A Wikidata main-namespace revision is **entity JSON** (labels,
descriptions, aliases, statements, sitelinks), so text-diffing it produces a JSON
hunk diff — technically a diff, useless for review — and every wikitext signal
misfires.

Two facts shape the decision:

- **Nothing records a revision's content model today.** `content_model` appears
  nowhere in the codebase; `EditEvent` (`types.rs`) carries `is_bot`, `is_patrolled`,
  `byte_delta`, etc., but no content model.
- **Content model is per-revision, not per-wiki.** `wikidata.org` carries wikitext
  (talk pages, Project/Help namespaces), and Wikipedias carry non-wikitext (JSON
  `.tab`/`Scribunto`). Routing therefore **cannot** key on the wiki id; it must key on
  the revision's actual content model, or a Wikidata talk-page edit would wrongly hit
  the entity path.

PRD-0011 needs a readable entity diff, content-model routing, and feature gating — as
a **reusable platform mechanism** (reuse-by-design ⇒ platform, ADR-0013/0004): the
patrolling domain consumes it now, and the citation↔Wikidata workflows will too.

## Decision

1. **Content model is a first-class, per-revision fact.** Add an additive
   `content_model` to `EditEvent` and to the revision-content read, with serde
   `default` → `wikitext` so existing streams and snapshots deserialize unchanged
   (ADR-0009 discipline). It is populated from the API (`rvprop=contentmodel` on the
   revision-content fetch). **All content-model routing keys on this value, never on
   the wiki id.**

2. **Entity revision read — reuse and promote the entity/statement parser from
   PR #103.** PR #103's `verify_wikidata_statement` (`sp42-mcp/src/wikidata.rs`)
   already reads entity JSON (from the keyless `Special:EntityData/{id}.json`
   endpoint) and parses statements, labels, item-values, and references. Its first
   consumer reads P854 URL references; the shared model keeps the full reference snak
   set so book and non-URL references do not get flattened into that P854-only case.
   That parsing/rendering logic is exactly what `EntityDiff` and the write lane
   (ADR-0017) also need, so this ADR **promotes it out of the `sp42-mcp` shell into
   platform** and reuses it — #103 was simply its first consumer, and the
   reuse-by-design rule puts a twice-used mechanism in platform, not a shell. The diff
   fetch retrieves the change's
   revision **and its parent**: the Action API `prop=revisions&rvslots=main&rvprop=ids|content|contentmodel`
   returns both revisions' entity JSON in one call and carries `contentmodel` for
   Decision 1, while `Special:EntityData/{id}.json?revision={rev}` (the endpoint #103
   already uses) is an equivalent per-revision read — the entity JSON body is the same
   either way, so the shared parser is endpoint-agnostic and the exact fetch endpoint is
   an implementation detail to settle with #103. The Wikibase **REST** API is not used
   for the read/diff path (weaker for arbitrary old revisions; may suit later writes).
   Either way the edge is a pure builder/parser over the injected `HttpClient` trait
   (ADR-0004), fixture-replayed with no live network in tests (ADR-0009).

3. **`EntityDiff` is a new structured type, a sibling to `StructuredDiff` — not an
   extension of it.** It diffs two entity revisions into classified changes over
   labels, descriptions, aliases, sitelinks, and statements; **statements at full
   depth** — main property/value, qualifiers, rank, and references — each
   added / removed / changed (PRD-0011 Q4). The parser/differ guarantees the
   **honesty invariant** over the review contract: every change in modeled
   review-relevant fields surfaces as a classified change, and any unmodeled
   top-level/raw entity delta surfaces as an explicit `UnknownEntityChange`/raw-hash
   change instead of being silently dropped. In particular, an edit touching only a
   qualifier, rank, or reference is never rendered as a no-op. A missing parent (first
   revision) yields an all-added diff, not an error.

4. **Content-model routing via a `ContentDiff` sum.** Diff consumers (the
   `sp42-reporting` renderer, routes, shells) receive a
   `ContentDiff { Text(StructuredDiff) | Entity(EntityDiff) }` selected by the
   revision's content model. The wikitext path is byte-for-byte untouched for
   `wikitext`; only `wikibase-item` / `wikibase-property` route to `EntityDiff`. An
   unknown/other content model falls back to `Text` with a note rather than failing —
   honest degradation over a hard error.

5. **Feature gating by content model, on the capability profile.** Extend the
   capability model with a content dimension recording which content-model-specific
   features apply. For entity content the wikitext-only signals (media-reference
   extraction, warning parsing, citation extraction/Parsoid) and LiftWing revertrisk
   are marked **unavailable**, and those code paths are **not invoked** (rather than
   invoked and their output discarded). This is a **separate axis** from the
   OAuth-grant/rights/token derivation in `derive_wiki_capability_profile`
   (`capabilities.rs`) — content-model capability is a property of the *content*, not
   the *account* — so it is additive and leaves the existing read/editing/moderation
   derivation untouched.

6. **Bot filtering reuses the existing query flag.** `build_recent_changes_request`
   already emits `rcshow=!bot` when `RecentChangesQuery.include_bots` is disabled; the
   Wikidata patrol queue simply sets that flag (PRD-0011 Q3). No new ingestion
   machinery — this is a consumption choice in the patrolling domain, noted here only
   to record that the mechanism already exists.

7. **Scoring is skipped, not faked.** With revertrisk gated off (Decision 5), no
   LiftWing request is built for entity revisions and no score is synthesized; the
   Wikidata queue orders chronologically over the bot-filtered stream. A
   Wikidata-appropriate signal is out of scope (follow-on).

8. **Crate placement.** `content_model`, `EntityDiff`, `ContentDiff`, and the
   content-model capability axis are **platform** types (`sp42-platform` / `sp42-types`)
   consumed by the patrolling domain and shells; the entity read builder/parser follows
   the Citoid / recentchanges precedent (pure functions over `HttpClient`). Per
   ADR-0013 the layer check keeps domains and shells from reaching around the contract.

## Relation to prior ADRs

- **PRD-0010 / PR #103 (citation-verification MCP surface):** #103 ships the *first*
  Wikidata read in the codebase — `verify_wikidata_statement` reads an entity and its
  P854 URL reference, then verifies the statement via the existing `verify_claim`
  pipeline. P854 is that verb's URL-reference case, not the whole platform reference
  model.
  This ADR does **not** compete with it: it **promotes #103's entity/statement parser
  to platform** (Decision 2) so the MCP verb, the patrol entity diff, and the write
  lane (ADR-0017) share one Wikidata read model instead of drifting into two. #103's
  verb (verify an *existing* statement's reference) and this ADR's `EntityDiff` (review
  a *change*) are different consumers of the same read; both stay abstention-biased and
  grounded (ADR-0007). Coordination point: if #103 merges first, the promotion is a
  refactor-in-place of `sp42-mcp/src/wikidata.rs`; if this lands first, #103's verb
  consumes the platform parser from the start. The shared model's type shape and the
  line-by-line mapping from #103's current code are sketched in
  `docs/design-plans/2026-07-01-wikidata-read-model.md`.
- **ADR-0014 (resolve any project):** this makes the resolved-but-unusable Wikidata
  target *usable*; resolution is unchanged.
- **ADR-0004 / ADR-0013 (crate boundaries / layered architecture):** `EntityDiff` is a
  reused mechanism → platform; the entity read edge is a pure builder/parser over the
  `HttpClient` trait, injected by a shell.
- **ADR-0009 (snapshot/replay):** `content_model` and the entity read are additive and
  serde-back-compatible; fixtures replay byte-for-byte.
- **ADR-0006 / scoring:** unchanged — scoring is gated *off* for entity content, not
  modified.
- **ADR-0003 / ADR-0010 (wikitext editing / propose-confirm):** untouched by the read
  path; the entity **write** contract is ADR-0017.

## Consequences

- Selecting Wikidata yields a readable entity diff; wikitext review is byte-for-byte
  unchanged because routing gates on content model.
- The platform contract grows a content-model axis and a `ContentDiff` sum type;
  consumers `match` on it, and the compiler enforces exhaustiveness (the ADR-0010
  wasm-gated-match caution applies — a new arm is a deliberate, compiler-checked
  change).
- Per-revision routing correctly handles mixed content on one wiki (Wikidata talk
  pages, JSON pages on Wikipedias) — the class of bug a per-wiki assumption would ship.
- Wikidata patrol queues are unranked (chronological) until a Wikidata scoring signal
  exists.
- No write capability is added here.

## Non-Goals

- **No entity write / statement editing** — ADR-0017.
- **No Wikidata-specific scoring model** — follow-on.
- **No qualifier/rank rendering polish** beyond correct classification (a renderer
  detail, not a contract decision).
- **No Wikibase REST API adoption** for the read/diff path (Action API only).
- **No undo semantics for entities** — the PRD-0011 flagged follow-on.
