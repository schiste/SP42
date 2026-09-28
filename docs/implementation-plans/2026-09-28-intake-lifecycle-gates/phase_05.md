# Intake / Lifecycle / Gates Implementation Plan — Phase 5: ADR-0027 content lifecycle

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** Implement ADR-0027 — the durable `ContentLifecycleRecord` with an
append-only transition log, `ResolutionClass` defined once, the
transition-triggered watch registry, and the recency-ordered `transitions_matching`
query that ADR-0028's `PriorOutcome` depends on.

**Architecture:** Pure types and transitions in
`crates/sp42-platform/src/content_lifecycle.rs`; the durable store in
`crates/sp42-platform/src/content_lifecycle_store.rs`, modelled directly on
`sp42-citation/src/citation/storage.rs` — a versioned envelope plus namespaced
keys over the injected `Storage` trait.

**Tech Stack:** `sp42_types::Storage` (async, injected), `serde`, `thiserror`,
`sha2` for the D2 digest, `proptest`.

**Scope:** Phase 5 of 8. Hard prerequisite for Phases 6 and 7.

**Codebase verified:** 2026-09-28.

- The storage model to copy is `sp42-citation/src/citation/storage.rs`: a
  `const SNAPSHOT_SCHEMA_VERSION: u32 = 1` (:31) plus a `{project, version, …}`
  envelope (:37), namespaced keys like `format!("citation:verdict:{hash}:{}", …)`
  (:128), and async store/load over an injected trait —
  `pub async fn store_verdict<S>(storage: &S, …) where S: Storage + ?Sized` (:210).
- The `Storage` trait is `crates/sp42-types/src/traits.rs:29-34`
  (`get`/`set`/`remove`, all async, `#[async_trait]`). Implementations:
  `MemoryStorage` (:122), `FileStorage` (:165), and in the browser
  `LocalStorageBrowserStorage` / `VolatileBrowserStorage`
  (`sp42-app/src/platform/runtime.rs:120,177`).
- `FileStorage::set` (:204-223) writes to a temp file and `fs::rename`s — atomic
  publish. That is the durability precedent, and it is inherited for free.
- Keys are hex-encoded to filenames (:182-188), so **any key charset works**,
  including `:`.
- There is **no database**: no `rusqlite`/`sled`/`rocksdb`/`redb` in `Cargo.lock`.
  There is also **no migration framework** — the whole precedent is
  versioned-envelope + tolerant decode + `#[serde(default)]` on everything added
  after v1 (`sp42-citation/src/citation/storage.rs:70,73`).
- `sp42-platform` has no async runtime: no `tokio` dep, only `futures`. Tests use
  `futures::executor::block_on` (`wiki_storage.rs:841`).
- ADR-0027 §2's `states: HashMap<String, LifecycleState>` collides with
  CONSTITUTION §1.4 ("explicit ordering of HashMap iterations when order matters").
  See D8.

---

## Task 1: Types and the single `ResolutionClass`

**Files:**
- Create: `crates/sp42-types/src/lifecycle.rs`

**Step 1: `ResolutionClass` here, once.** ADR-0027 §2 defines it and ADR-0029 §3
reuses it verbatim. CONSTITUTION §6.1 — "every type exists in one place. No
copies." Define it in `sp42-types`, not in the eligibility module, or ADR-0029
will grow a parallel vocabulary, which its own Alternatives explicitly rejects.

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionClass { Terminal, NeedsHuman, Relist, RouteElsewhere }
```

**Step 2: `LifecycleTransition` per ADR-0027 §2, plus the D8 tiebreak.**
`TransitionCause = GateVerdict { …complete envelope… } | ReviewerAction { actor,
action_kind } | SystemReentry { reason }`. `GateVerdict` carries the *full*
verdict envelope, not just a routing key — ADR-0027 §2 is explicit that without
`resolution_class` in the transition a `PriorOutcome` query could not filter on it,
and without `reasons`/`matched_rule_path` the log is not an audit trail.

Add the D8 field now, before anything writes:

```rust
pub struct LifecycleTransition {
    pub seq: u64,                    // monotonic, assigned at write time (D8)
    pub track: String,
    pub from: Option<LifecycleState>,
    pub to: LifecycleState,
    pub caused_by: TransitionCause,
    pub occurred_at: Timestamp,     // resolved against Clock::now(), never wall-clock
}
```

**Step 3: `ContentLifecycleRecord`** with `states` keyed by **track** (ADR-0027
§1), and the invariant `states[track] == to` of that track's most recent entry
stated as a documented, test-pinned invariant rather than a comment.

**Step 4: Commit** — `feat(types): content lifecycle record, transition, and the shared ResolutionClass`

---

## Task 2: The pure transition engine

**Files:**
- Create: `crates/sp42-platform/src/content_lifecycle.rs`

**Step 1: Recording is append + derive, never overwrite.** ADR-0027 §2: the log is
the source of truth and the per-track map is a cache. Implement
`record_transition(&mut record, transition) -> Result<(), LifecycleError>` that
appends and re-derives the one track's entry, and rejects an `occurred_at` earlier
than the track's current head unless the transition carries an explicit supersede
marker.

**Step 2: `transitions_matching` per ADR-0027 §4, with D8's tiebreak.**

```rust
pub fn transitions_matching(
    record: &ContentLifecycleRecord,
    predicate: &TransitionPredicate,
) -> Vec<&LifecycleTransition>
```

Sort by `(occurred_at desc, seq desc)`. The `seq` tiebreak is what makes
ADR-0027 §4's "load-bearing" ordering deterministic under CONSTITUTION §1.4 —
without it, two transitions in the same millisecond have no defined order, and
`FixedClock` makes that the normal case in tests rather than an edge case.

**Step 3: The pure/edges split, following `review_session.rs:10-14`.** This module
is the pure core — types and transitions, no I/O. The store is a separate module.
The doc comment convention is worth copying verbatim in spirit: *"This module is
the pure core… The imperative edges live elsewhere."*

**Step 4: Commit** — `feat(platform): append-only transition recording and the recency-ordered query`

---

## Task 3: The durable store

**Files:**
- Create: `crates/sp42-platform/src/content_lifecycle_store.rs`

**Step 1: Versioned envelope + namespaced keys, per the citation precedent.**

```rust
const LIFECYCLE_SCHEMA_VERSION: u32 = 1;

pub async fn store_lifecycle<S>(storage: &S, record: &ContentLifecycleRecord)
    -> Result<String, LifecycleStoreError> where S: Storage + ?Sized
```

Key scheme: `content-lifecycle:v1:<wiki_id>:<item_id_encoded>`. The item id's
encoded form is already delimiter-safe by Phase 1's decision.

**Step 2: Append-only means append, not rewrite.** The performance note in
ADR-0027 §Consequences is real — a full rewrite per transition does not scale, and
`transitions_matching` must be a bounded lookup, not a history scan. Prefer a
per-transition key (`…:<seq>`) plus a small head pointer, or an append-only
segment format, over rewriting the whole record. State the choice in a comment
because it is the one place where "append-only" and "a single record" pull apart.

**Step 3: Tolerant decode, per the house rule.** `#[serde(default)]` on every field
added after v1, `skip_serializing_if = "Option::is_none"` on optionals
(`types.rs:105-112`). An unknown future `v` must be a clean error, never a
misparse — same discipline as Phase 1's codec.

**Step 4: Commit** — `feat(platform): durable content lifecycle store over the injected Storage trait`

---

## Task 4: The watch registry

**Files:**
- Create: `crates/sp42-platform/src/lifecycle_watch.rs`

**Step 1: `LifecycleWatch { track, from, to, re_trigger: WorkflowId }` per
ADR-0027 §3, matched on exact `track`/`from`/`to`. On match, enqueue into the
target workflow's **intake** (Phase 3) rather than into the workflow directly —
the ADR is explicit that the engine does not reimplement per-domain callbacks.

**Step 2: The closed-vocabulary validation, extending Phase 4's linter.**
`LifecycleState.key` is deliberately open (ADR-0027 §1) so a domain never edits a
platform type — which means a typo'd `from`/`to` in a watch config loads fine and
never fires, silently, forever. ADR-0027 §3 requires the linter to reject a watch
whose `track` is unregistered or whose `from`/`to` is outside that track's
registered vocabulary. This is a **rule added to the existing linter**, per
CONSTITUTION §6.1 and ADR-0027 §3's own "extending that linter rather than
inventing a second one".

**Step 3: Commit** — `feat(platform): transition-triggered re-entry watches, linted by the shared linter`

---

## Task 5: Pin the ordering and the invariant

**Files:**
- Modify: `crates/sp42-platform/src/content_lifecycle.rs` (inline tests + `proptest!`)

**Step 1: The recency test that ADR-0028 §2 depends on.** A record with
`promoted` then later `delisted` must read as *delisted* to a `PriorOutcome`-shaped
query, never as still-promoted. This is the G4/GA-delisting case and it is the
single most important test in this phase.

**Step 2: The same-millisecond tiebreak.** Two transitions with identical
`occurred_at` order by `seq` descending, deterministically. Without D8 this test
cannot be written.

**Step 3: `proptest` round-trip** of the store envelope, mirroring
`sp42-coordination/src/codec.rs:142`.

**Step 4: Invariant test**: after any sequence of `record_transition` calls,
`states[track] == to` of that track's most recent entry, for every track.

**Step 5: Commit** — `test(lifecycle): recency ordering, seq tiebreak, and the states/history invariant`

---

## Phase 5 exit criteria

- [ ] A `promoted`-then-`delisted` record reads as delisted (the G4 case)
- [ ] Equal `occurred_at` orders by `seq` — deterministic, tested
- [ ] `states[track] == to(most recent)` holds after arbitrary transition sequences
- [ ] `ResolutionClass` is defined once and shared; `grep` finds no second definition
- [ ] The store is append-only and does not rewrite history per transition
- [ ] An unknown future envelope `v` is a clean error, not a misparse
- [ ] `block_on` used in tests (no `tokio` in `sp42-platform`)
- [ ] The watch vocabulary rule is in the shared linter, not a new checker
