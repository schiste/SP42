# Intake / Lifecycle / Gates Implementation Plan — Phase 3: ADR-0026 contracts and the three-valued evaluator

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** Implement ADR-0026 §3–§6 — the `IntakeItem` envelope, the composable
condition tree, the typed field/op/value vocabulary, three-valued evaluation with
Kleene composition, and the routed decision with `on_unknown` kept distinct from
`on_fail`.

**Architecture:** Contracts in `sp42-types`; the pure evaluator and policy
compilation in `sp42-platform`, beside `scoring_engine.rs`. No I/O in the
evaluator — it is a pure function from `(item, compiled pipeline, now_ms)` to a
decision, which is what makes CONSTITUTION §1.2/§1.4 satisfiable.

**Tech Stack:** `serde`/`serde_yaml`, `thiserror`, `proptest` for the Kleene
properties. `regex` is already a `sp42-platform` dep if the `Custom(String)`
resolver registry needs pattern matching.

**Scope:** Phase 3 of 8. Phase 4 adds the linter over exactly the config surface
this phase defines.

**Codebase verified:** 2026-09-28.

- The pure-engine precedent is `sp42-platform/src/scoring_engine.rs`, whose entire
  public surface is two free functions (`score_edit` :15,
  `score_edit_with_context` :28) re-exported from `lib.rs:146`. Everything else is
  private. **Match that**: `evaluate_intake` and a config-compiled entry point,
  nothing else public.
- `sp42-platform` has **no** `Clock` usage — `grep` finds only the two re-export
  lines (`traits.rs:5-6`, `lib.rs:166-167`). Every platform function that needs
  time takes `now_ms: i64` as a plain parameter. **ADR-0026 §4's
  `Clock::now() ± Duration` therefore becomes a resolved `now_ms: i64` parameter**,
  computed by the shell. This is the `iso_date_from_epoch_ms` precedent
  (`sp42-citation/src/bare_url_repair.rs:295-313`) and it is what keeps §2.3 true.
  Do not put a `&dyn Clock` in the platform evaluator.
- Determinism style to copy from `scoring_engine.rs`: integer arithmetic with
  `saturating_add`/`saturating_sub` (:480), symmetric rounding in `scale_weight`
  (:395) with its own test (:902), and `BTreeMap`/`BTreeSet` for any map that
  reaches an output (`queue_builder.rs:7,102-120`).
- `proptest!` blocks already in `sp42-platform`: `scoring_engine.rs:1027` (six
  properties), `priority_queue.rs:107`, `liftwing.rs:323`, `user_analyzer.rs:287`,
  `context_builder.rs:91`. Same idioms.
- CONSTITUTION §6.4: ≤40 lines per function, ≤4 params. A condition-tree evaluator
  is naturally recursive; keep each combinator its own small function.
- `signal_from_slug` (`scoring_policy.rs:558`) is the precedent for "unknown
  vocabulary value is a load-time rejection, never a silent no-match" — the exact
  semantics ADR-0026 §7 wants.

---

## Task 1: Contracts in `sp42-types`

**Files:**
- Create: `crates/sp42-types/src/intake.rs`

**Step 1: `IntakeItem` per ADR-0026 §3, with the `Option` typing intact.**

```rust
pub struct IntakeItem {
    pub wiki_id: String,
    pub event_type: String,
    pub namespace: i32,
    pub page_id: Option<i64>,
    pub revision_id: Option<i64>,
    pub created_at: Option<Timestamp>,        // subject page's creation
    pub last_revision_at: Option<Timestamp>,
    pub actor: IntakeActor,
    pub observed_at: Timestamp,               // THIS event
    pub payload_ref: Option<String>,
}
pub struct IntakeActor {
    pub username: Option<String>,
    pub rights: Option<Vec<String>>,          // None = not fetched, Some(vec![]) = fetched & empty
    pub is_bot: bool,
    pub account_created_at: Option<Timestamp>,
}
```

`rights` being `Option` is the load-bearing detail from ADR-0026 §3:31 — a bare
`Vec::new()` cannot distinguish "not looked up" from "confirmed no rights", which
would make `ActorRights Has "sysop"` evaluate a confirmed `False` for an actor
nobody checked. Keep the distinction and pin it with a named test.

**Step 2: The vocabulary per ADR-0026 §4** — `IntakeField` (closed + `Custom(String)`),
`IntakeOp`, `IntakeValue`, `IntakeCondition` with `All`/`Any`/`Not`/`Rule`.
`All`/`Any`/`Not` are required from day one; ADR-0026 Alternatives:115 records
that an AND-only list would force a breaking config migration on the first real
policy.

**Step 3: `IntakePipeline` and `IntakeDecision` per ADR-0026 §6**, including
`on_unknown: Option<IntakeOutcome>` and the `Misconfigured` variant. `Misconfigured`
is deliberately *not* a `Drop`: ADR-0026 §6 requires a broken filter to be
distinguishable from an intentional one.

**Step 4: `config_version`** per D2 from Phase 0 — attach the compile-time digest,
not a hand-written string.

**Step 5: Commit** — `feat(types): intake envelope, condition tree, and pipeline contracts`

---

## Task 2: The three-valued evaluator

**Files:**
- Create: `crates/sp42-platform/src/intake_engine.rs`

**Step 1: Three-valued result, then Kleene composition per ADR-0026 §4.**

```rust
pub enum Tri { True, False, Unknown }
```

`All` is `False` if any child is `False`, else `Unknown` if any is `Unknown`, else
`True`. `Any` is `True` if any child is `True`, else `Unknown` if any is
`Unknown`, else `False`. `Not` maps `True↔False` and leaves `Unknown`. Write each
as its own small function (CONSTITUTION §6.4).

The property that matters, and the reason for the whole design: **`Not` never
manufactures a `True` from an `Unknown`.** `Not(CreatedAt YoungerThan 1h)` against
a missing `created_at` must read `Unknown`, not "confirmed old enough"
(ADR-0026 §4 and Alternatives:120).

**Step 2: Field/op/value type checking at evaluation time too, not only in the
linter.** ADR-0026 §4 says a temporal field does not accept `Has`, a list field
does not accept `Before`. The linter (Phase 4) catches config mistakes, but the
engine must still refuse an incompatible triple it is handed — a
`Result<Tri, IntakeError>`, not a silent `False`.

**Step 3: The public entry point is two functions, mirroring `scoring_engine`.**

```rust
pub fn evaluate_intake(item: &IntakeItem, pipeline: &CompiledPipeline, now_ms: i64)
    -> Result<IntakeDecision, IntakeError>
```

**Step 4: Routing per ADR-0026 §6.** `True → on_pass`, `False → on_fail`,
`Unknown → on_unknown` defaulting to `Drop`. The default must be applied at
compile time and must **never** be inherited from `on_fail` — that inheritance is
precisely the fail-open gap the ADR exists to close, and it is reachable whenever
`on_fail` is `Admit`.

**Step 5: Commit** — `feat(platform): three-valued intake condition evaluator with Kleene composition`

---

## Task 3: Per-field compatibility table

**Files:**
- Modify: `crates/sp42-platform/src/intake_engine.rs`

**Step 1: One table, used by both the engine and the linter.**
ADR-0026 §4 requires each field to have a fixed value type and a fixed operator
set; ADR-0026 §7 requires the linter to check every `field`/`op`/`value` triple
against it. **Define it once** and have the linter consume it — CONSTITUTION §6.1
forbids the copy, and §14.4 makes a second table a constitutional violation.

```rust
fn field_spec(field: &IntakeField) -> Result<FieldSpec, IntakeError>  // value type + allowed ops
```

**Step 2: `Custom(String)` resolution goes through a registry, not a match arm.**
ADR-0026 §4 requires a per-domain field-resolver registry so a new filterable fact
does not need a platform edit. Declare the registry trait in `sp42-types`
alongside the other platform edges, implement it per domain, and let the engine
hold it by reference. An unregistered `Custom` field is an error (Phase 4 rejects
it at lint time; the engine errors too).

**Step 3: Commit** — `feat(platform): shared per-field type/operator table and Custom field registry`

---

## Task 4: Pin the Kleene properties

**Files:**
- Modify: `crates/sp42-platform/src/intake_engine.rs` (inline tests + `proptest!`)

**Step 1: Exhaustive table test over the 27 three-valued cases** for
`All`/`Any`/`Not` combinations — small enough to enumerate completely, and
exhaustive is better than sampled for a three-valued lattice.

**Step 2: `proptest` for the algebraic laws**, which is where a Kleene
implementation actually breaks:

- `Not(Not(x)) == x` for all three values
- `All([])` is `True` and `Any([])` is `Unknown` (vacuous truth; ADR-0028 §5
  depends on `All([])` being the unconditional catch-all)
- `All([x, …])` containing a `False` is `False` regardless of the rest
- **no expression built from `Not` over an `Unknown` ever yields `True`**

**Step 3: The missing-data tests that mirror D9.** Because the first real adapter
will produce `created_at: None` and `rights: None` (see Phase 8 and D9), test
directly that a temporal rule and a rights rule each yield `Unknown` — not `False`
— on such an item, and that routing sends them to `on_unknown`.

**Step 4: Commit** — `test(intake): Kleene laws and unknown-propagation properties`

---

## Phase 5 exit criteria (cross-phase note)

The `on_unknown` default and the `Misconfigured` distinction are the two properties
Phase 5's store depends on: ADR-0027 records `GateVerdict` transitions, and
ADR-0028 §6 reuses ADR-0026's `Misconfigured` state for eval-time resolution
failures rather than inventing a second one. Keep the name and the shape stable.

## Phase 3 exit criteria

- [ ] Exhaustive 3×3×3 combinator table green; all four `proptest` laws green
- [ ] `Not(Unknown) == Unknown`; no `Not` path yields `True` from `Unknown`
- [ ] An incompatible `field`/`op`/`value` triple is an `Err`, not a silent `False`
- [ ] `on_unknown` defaults to `Drop` and never inherits `on_fail`
- [ ] `Misconfigured` is distinct from `Drop`
- [ ] Evaluator takes `now_ms: i64`; `grep -n "Clock" crates/sp42-platform/src/intake_engine.rs` is empty
- [ ] The per-field table is defined once and shared with Phase 4
- [ ] `sp42-platform` coverage still ≥90%
