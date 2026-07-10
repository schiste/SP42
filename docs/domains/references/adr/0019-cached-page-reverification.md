# ADR-0019: Cached page re-verification

**Status:** Proposed
**Date:** 2026-07-09
**Author:** Luis Villa

## Context

PRD-0014 gives the operator a **Re-verify** control on a finding: re-check this
citation against the *current* article state. The first implementation re-runs
one use-site and folds the fresh verdict back into the loaded report. Review
(Codex, on #109) surfaced a seam: the report was loaded for revision *N*, but
Re-verify checks the current revision *N+1* (after the operator's edit), so the
card now carries an *N+1* verdict while the report header, the raw-text report,
and the card's "show citation in article" link still read *N* — a
**mixed-revision** display. Marking one card as "re-verified at *N+1*" is
possible but treats findings as individually multi-revision, which is fiddly and
still leaves the page header/counts describing a revision that no longer matches
every card.

The clean contract (PRD-0014 amendment, 2026-07-09) is for Re-verify to
**refresh the whole page to the current revision** so *everything* is consistently
*as of N+1*. The obstacle is cost: re-running the full page runs a model panel
per citation, and Re-verify is meant to be a cheap "did my edit help?" check, not
a full rescan.

Two facts make a cheap refresh possible:

- **A verdict is content-addressable.** It is a function of the **source content**
  and the **claim sentence** (and the model panel). **ADR-0009** already defines a
  content-addressed verdict record keyed by `(snapshot_hash, claim)`
  (`verdict_storage_key`, `store_verdict`, `load_verdict` over the `Storage`
  trait) — but built it for *reproducibility* and left it **dormant**: nothing in
  the verify pipeline (**ADR-0011**) reads or writes it.
- **Within one session almost nothing an external source says changes.** The
  thing the operator changes is the **article** (their own edit); the external
  sources are stable across a few-minute session.

This ADR decides how to activate ADR-0009's dormant cache in ADR-0011's pipeline,
and what Re-verify becomes as a result.

## Decision

### 1. Activate the content-addressed verdict cache as a verify read-path

The verify pipeline consults the cache before spending inference. Per use-site,
after the source body is in hand:

1. Compute `snapshot_hash` from the fetched source content (ADR-0009's snapshot).
2. `load_verdict(snapshot_hash, claim)` — on a hit, reuse the stored verdict; no
   panel is called.
3. On a miss, run the model panel as today, then `store_verdict`.

Invalidation is **automatic and correct by construction**: a changed source
hashes differently and a reworded claim keys differently, so either naturally
misses. There is no separate invalidation path to get wrong.

### 2. The cache is in-memory, process-scoped, for now

The `Storage` backend behind the cache is an **in-memory** map for this pass — no
persistence, no external store. It is content-addressed, so sharing it across
sessions in one process is safe (a `(snapshot_hash, claim)` entry means the same
thing to everyone), but this ADR promises only *within-session* reuse. A
persistent or shared backend is a later decision (Non-Goals).

### 3. The cache key does **not** include a panel fingerprint

A verdict also depends on which models formed the panel. Because the panel is
**fixed for the life of a session** (it is read once from the `SP42_INFERENCE_*`
seam and does not change under the operator), `(snapshot_hash, claim)` is a
sufficient key within the in-memory scope this ADR covers. If a persistent or
cross-panel cache is introduced later, it MUST fold the panel fingerprint (e.g.
ADR-0006's `prompt_hash` / `ModelRef` set) into the key; that obligation is
recorded here so the in-memory shortcut is not silently promoted.

### 4. Re-verify is *cached page re-verification*

Re-verify stops being "re-check one use-site" and becomes "re-run the page against
the current revision, cheaply":

- **Re-fetch the article** at the latest revision and re-extract its use-sites.
  This is required — picking up the operator's edit is the entire point — and it
  is one Parsoid fetch, cheap next to inference.
- **Reuse session-cached source bodies.** External sources are treated as stable
  within a session, so there is **no per-re-verify source re-fetch**; the existing
  per-run source-body cache (`verify_page`'s `bodies` map, bounded by
  `MAX_PREFETCH_CACHE_BYTES`) is promoted to session lifetime. A source not yet in
  the body cache (e.g. one newly cited by the operator's edit) is fetched once.
- **Reuse cached verdicts** via Decision 1. Only citations whose source content or
  claim actually changed — in practice, the one the operator just edited — miss
  and spend inference.

The result is a page that is wholly *as of the current revision*: header, links,
per-verdict sections, and every card describe one revision. The mixed-revision
seam cannot arise because there is no longer a stale-revision report to fold into.

### 5. No force-refresh control

Re-verify does **not** offer a "re-run this one regardless" bypass. Every case
that warrants fresh inference busts the content-addressed cache on its own:

- operator edited the claim → new `claim` → miss → fresh;
- source content changed → new `snapshot_hash` → miss → fresh;
- operator repaired a bare URL into a cite template → the **source** is unchanged
  (same URL, same content), only the citation wikitext changed → **hit**, and
  reusing the verdict is *correct* (formatting does not change whether the source
  supports the claim);
- nothing changed → **hit** → the same verdict, which is the honest answer.

The only thing a bypass would add is re-running inference on byte-identical inputs
to roll the panel's nondeterminism again — a **second opinion**. That is a
deliberately separate, explicit future control ("re-run panel"), not folded into
Re-verify (Non-Goals).

## Alternatives Considered

- **Per-finding revision tags (mark the card "re-verified at *N+1*").** Point the
  re-verified card's link at *N+1* and badge it. Rejected as the primary fix: it
  models findings as individually multi-revision, complicates every consumer of
  `report.rev_id`, and still leaves the page header/counts describing a revision
  that not every card matches. Acceptable only as the *interim* #109 behavior with
  the mismatch documented as a known limitation.
- **Refresh to latest with no cache.** Correct labeling, but re-runs a full model
  panel per citation on every Re-verify — exactly the cost Re-verify exists to
  avoid. Rejected without the cache; adopted *with* it (this ADR).
- **Persist the cache now.** More reuse (across sessions/restarts), but pulls in a
  storage backend, a staleness/eviction policy, and the panel-fingerprint
  obligation (Decision 3) before any of it is needed. Deferred.

## Consequences

- The mixed-revision display PRD-0014 flagged is **resolved by construction**;
  no per-finding revision bookkeeping is needed.
- Re-verify cost drops to ≈ the citations that actually changed (usually one),
  plus one article fetch — restoring its "cheap, frequent check" intent while
  making it a whole-page refresh.
- ADR-0009's verdict cache stops being dormant; ADR-0011's pipeline gains a
  read-through cache. Neither is superseded — this **builds on ADR-0009's
  primitive** and **extends ADR-0011's pipeline**.
- **Session-scoped source staleness:** if an external source's content genuinely
  changes mid-session, the session keeps serving the cached body (and its cached
  verdict) until the caches drop — i.e. a fresh page load or a new session, which
  re-fetches. This is the deliberate "sources are stable in-session" trade; it is
  safe because it is in-memory and self-healing on reload, and it never produces a
  *fabricated* verdict (a cached verdict is always the real verdict for the cached
  content).
- Because the cache is in-memory and content-addressed, there is no persistence,
  no cross-process coherence question, and no invalidation code path to maintain.

## Non-Goals

- **Persistence or cross-session/-process sharing** of the cache (Decision 2).
- **Panel-change invalidation** — out of scope while the panel is session-fixed
  and the cache is in-memory; the obligation for any future persistent cache is
  recorded in Decision 3.
- **A second-opinion / re-roll control** that re-runs inference on unchanged
  inputs (Decision 5).
- **Detecting external-source drift within a session** (the session-staleness
  trade in Consequences).
