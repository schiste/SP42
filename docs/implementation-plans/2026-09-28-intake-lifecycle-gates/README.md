# Intake / Lifecycle / Gates Implementation Plan

Plan for bringing six Accepted ADRs to life:

| ADR | What it fixes |
|---|---|
| [ADR-0030](../../platform/adr/0030-reviewable-item-identity.md) | `ReviewableItemId` — the identity 0027 records and 0028/0029 key on |
| [ADR-0031](../../platform/adr/0031-verdict-reason.md) | `Reason` — the reason payload 0027/0028/0029 all carry |
| [ADR-0026](../../platform/adr/0026-intake-contract.md) | Intake — condition-tree filter engine, capability resolution, config linter |
| [ADR-0027](../../platform/adr/0027-content-lifecycle-contract.md) | Content lifecycle — append-only transition log, watches, query surface |
| [ADR-0028](../../platform/adr/0028-deterministic-eligibility-gate-contract.md) | Deterministic eligibility gate |
| [ADR-0029](../../platform/adr/0029-deterministic-quality-gate-contract.md) | Deterministic quality gate |

All six are `Accepted` dated 2026-09-28. None of it is implemented — every
type in the chain is greenfield (zero hits across `crates/**/*.rs` for
`ReviewableItemId`, `IntakeCondition`, `ResolutionClass`, `ContentLifecycleRecord`,
`ReasonCode`, …). There is nothing to migrate or preserve.

## Read this first: the two traps

**Trap 1 — `sp42-platform → sp42-wiki` is a dependency cycle, and the layering
gate will not catch it.**

```
crates/sp42-wiki/Cargo.toml:18   sp42-core  = { path = "../sp42-core" }
crates/sp42-core/Cargo.toml:24   sp42-platform = { path = "../sp42-platform" }
```

Adding `sp42-wiki` to `sp42-platform` closes `platform → wiki → core → platform`.
Cargo rejects cyclic package dependencies outright, so the build breaks — but
`check-layering.sh` reports green, because `sp42-wiki` is `platform` (rank 0 vs 0,
line 101) and `sp42-core` is `hybrid`, which is in `EXEMPT_AS_TARGET` (line 99)
and therefore skipped. **Both edges are silently ignored.** Phase 0 Task 1 removes
the cause; do not add that dependency before it lands.

**Trap 2 — `WikiRegistry::resolve()` auto-onboards every Wikimedia project, which
ADR-0026 §5 forbids.**

```rust
// crates/sp42-wiki/src/registry.rs:154
pub fn resolve(&self, wiki_id: &str) -> Result<WikiConfig, WikiRegistryError> {
    if let Some(cfg) = self.inner.configs.get(wiki_id) { return Ok(cfg.clone()); }
    crate::sites::derive_wiki_config(wiki_id)   // ← embedded sites.json fallback
        .ok_or_else(|| WikiRegistryError::UnknownWikiId { .. })
}
```

A `(wiki_id, page_id, revision_id)` event naming `dewiki` resolves today, and a
test at `registry.rs:277-301` locks that behavior in. ADR-0026 §5 requires
registration to be an explicit administrative action and rejects auto-onboard
outright. Intake must gate on a *registered* predicate, not on `resolve()`.
`wiki_ids()` (`registry.rs:186`) is already the "configured only" set. Phase 0
Task 2 introduces the predicate.

## Sequencing

The dependency order is forced and is **not** the ADR numbering order.

```
        ┌─────────────────────────────────────────────┐
        │ Phase 0 — decisions + structural unblockers │
        └─────────────────────────────────────────────┘
                              │
      ┌───────────────────────┼───────────────────────┐
      ▼                       ▼                       │
┌───────────┐          ┌────────────┐                 │
│ Phase 1   │          │  Phase 2   │                 │
│ ADR-0030  │          │  ADR-0031  │                 │
│ item id   │          │  Reason    │                 │
└─────┬─────┘          └─────┬──────┘                 │
      └───────────┬──────────┘                        │
                  ▼                                   │
          ┌───────────────┐                           │
          │   Phase 3     │  ADR-0026 contracts +    │
          │               │  three-valued evaluator   │
          └───────┬───────┘                           │
                  │                                   │
      ┌───────────┴───────────────┐                   │
      ▼                           ▼                   │
┌───────────┐             ┌────────────┐              │
│  Phase 4  │             │   Phase 5  │              │
│  ADR-0026 │             │  ADR-0027  │              │
│  linter + │             │  lifecycle │              │
│  wiring   │             │  store     │              │
└───────────┘             └─────┬──────┘              │
                                │                     │
                  ┌─────────────┴──────────┐          │
                  ▼                        ▼          │
            ┌───────────┐            ┌───────────┐      │
            │  Phase 6  │            │  Phase 7  │      │
            │  ADR-0028 │            │  ADR-0029 │      │
            │ eligibility│            │  quality  │      │
            └─────┬─────┘            └─────┬─────┘      │
                  └──────────┬────────────┘            │
                             ▼                         │
                      ┌────────────┐                    │
                      │  Phase 8   │  sp42-live →     │
                      │            │  Stream adapter,  │
                      └────────────┘  shell wiring     │
                             │                         │
                             ▼                         │
                    ADR-0026 §7's own dependency ──────┘
                    (linter extended once, three times)
```

Why this order and not the ADR numbers:

- **0030 and 0031 first** despite being numbered last (numbers are sequential, so
  they had to land last). They are the base types — the lifecycle record is not
  constructible without an identity, and neither gate is constructible without
  reasons.
- **0031 before 0028/0029** because ADR-0031 §2 makes reason *codes* statically
  declared per outcome arm. That is what makes `disallowed_reasons` decidable at
  config-lint time, so the linter checks must be written against it, not bolted on.
- **0026's evaluator before 0028/0029** because both reuse `IntakeField`/`Op`/
  `Value` verbatim. Building a second rule vocabulary is the exact thing ADR-0028
  §2/Alternatives rejects, and Constitution §14.4 ("No Unnecessary Repetition")
  makes it constitutional too.
- **0027 before 0028/0029** — both gates hard-depend on it for verdict recording
  and `PriorOutcome` resolution. ADR-0028 says so in its own Consequences.
- **The linter is one thing, built once and extended three times** (ADR-0026 §7 →
  ADR-0028 §6 → ADR-0029 §6). Three near-identical checkers is the outcome those
  sections exist to prevent.

## The decision register

These are the points where the ADRs and the codebase genuinely disagree, or where
a type the ADRs assume does not exist. Each needs an answer from the project
owner before the phase that depends on it starts. Detail and a recommendation for
each is in [`phase_00.md`](phase_00.md).

| # | Decision | Blocks | Recommendation |
|---|---|---|---|
| D1 | `Timestamp` does not exist anywhere in the codebase; the ADRs use it in ~10 signatures | All | Introduce a `Timestamp` newtype in `sp42-types` |
| D2 | `config_version` has no implementation anywhere — all 14 mentions are ADR prose | 4, 5, 6, 7 | Per-pipeline/per-ruleset content hash, computed at compile time |
| D3 | `sp42-wiki` → `sp42-core` → `sp42-platform` cycle (Trap 1) | 3, 8 | Retarget `sp42-wiki` to `sp42-platform` + `sp42-types` |
| D4 | `sp42-app` (wasm) has **no durable store**; ADR-0026 §2 says intake is always-on there and 0027 wants a durable log | 8 | Lifecycle log is server-side; browser evaluates but does not record |
| D5 | `resolve()` auto-onboards (Trap 2) | 3, 8 | Intake gates on a registered-predicate |
| D6 | ADR-0026 §7 says the linter runs "from `ci-all.sh` alongside `check-layering.sh`" — but `ci-all.sh` runs no `check-*.sh`, and `check-scoring-governance.sh` is orphaned (not in `ci.yml` either) | 4 | Add the `ci.yml` step explicitly; adopt the orphaned script as a bonus |
| D7 | `IntakeItem` overlaps `EditEvent` heavily; Constitution §6.1 forbids duplicate types and conversion layers | 3, 8 | Document `IntakeItem` as the pre-admission envelope, `EditEvent` as the scored one |
| D8 | ADR-0027 §4's "most-recent-first" is ambiguous when two transitions share an `occurred_at` | 5 | Add a monotonic `seq: u64`, order by `(occurred_at desc, seq desc)` |
| D9 | `sp42-live` does **not** filter on actor rights, contrary to ADR-0026 §Context; and `EditEvent` has no `page_id`/`created_at`/`last_revision_at`/`account_created_at` | 8 | Baseline pipeline must not reference temporal fields (ADR-0026 §2 already says this) |

## Explicitly out of scope

So these do not creep in:

- **`LiveOperatorQuery` / `filter_live_operator_queue`** (`sp42-live/src/live_operator.rs:209-254`)
  is a flat, hand-rolled AND-only boolean filter over already-scored `QueuedEdit`s.
  Structurally it is the thing ADR-0026's Alternatives rejects, and it overlaps
  intake fields — but it is a *display* query, not a candidacy filter. Not migrated.
- **The stochastic eligibility gate** and the **stochastic quality gate** — both
  explicit Non-goals in 0028 and 0029.
- **A primary-policy research pass on WP:DYK/WP:GAN/WP:FAC** — a Non-goal in
  0029, and a prerequisite for any concrete quality ruleset config, not for the
  mechanism.
- **A user-facing surface for `IntakeQuerySpec`** — a Non-goal in 0026. The
  mechanism supports it; nobody builds a UI for it yet.
- **The reviewer-facing Tier-B confirmation flow** — 0026 says it needs its own PRD.
- **New crates.** Everything lives in `sp42-platform` + `sp42-types`, which avoids
  the `LAYER` map edit in `check-layering.sh`, the `-p` edit in
  `scripts/check-coverage.sh`, and a new crate in the architecture map.

## Cross-cutting gates every phase must satisfy

- `scripts/check-coverage.sh` enforces **≥90% lines** on `sp42-platform`. This is
  the gate most likely to bite: all of the new code lands in that one crate.
- `scripts/check-forbidden-patterns.sh` scans **added lines only**, so new code
  must have no `unwrap()` in non-test code, no bare `TODO`, and no `#[allow]`
  without an issue link.
- `./scripts/generate-architecture-map.sh --check` must pass. It changes when a
  dependency edge moves — which Phase 0 Task 1 will cause, since retargeting
  `sp42-wiki` off `sp42-core` is an edge change.
- `cargo clippy -- -D warnings`, `cargo fmt --check`, `cargo ci-test` all run in
  CI. Inline `#[cfg(test)] mod tests` is the house style (~76 modules workspace-wide
  against exactly **one** `tests/` file).
- `proptest` is already a dev-dependency of `sp42-platform` and is the right tool
  for ADR-0030's round-trip requirement and for the Kleene `Unknown` properties.
