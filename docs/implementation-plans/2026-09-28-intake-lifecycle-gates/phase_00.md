# Intake / Lifecycle / Gates Implementation Plan — Phase 0: decisions and structural unblockers

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** Settle the nine points where the six ADRs and the actual codebase
disagree, and remove the two structural blockers that would otherwise make later
phases either unbuildable or wrong.

**Architecture:** Two code changes and one new trait. Task 1 is a dependency
retarget inside `sp42-wiki`; Task 2 adds a `WikiRegistryView` trait to
`sp42-types` and implements it in `sp42-wiki`; Task 3 is documentation-only. No
new crate, no behavior change to any existing caller.

**Tech Stack:** Cargo workspace dependency graph, `serde`/`serde_yaml` (already in
`sp42-wiki`), and `docs/platform/architecture.md` regeneration.

**Scope:** Phase 0 of 8. Nothing here is intake. This phase exists because three
of the later phases cannot be written correctly until these are answered.

**Codebase verified:** 2026-09-28. Key facts:

- `crates/sp42-wiki/Cargo.toml:18` declares `sp42-core = { path = "../sp42-core" }`;
  `crates/sp42-core/Cargo.toml:24` declares `sp42-platform = { path = "../sp42-platform" }`.
  `sp42-wiki/src/registry.rs:7` does `use sp42_core::WikiConfig;`.
  `WikiConfig` itself is defined in `crates/sp42-platform/src/types.rs:405`.
  So the retarget is a one-line import change plus a manifest edit.
- `scripts/check-layering.sh:44-78` LAYER map: `sp42-wiki` and `sp42-platform`
  are both `platform` (rank 0). Line 101 tests `RANK[dst] > RANK[src]`, so
  platform→platform never trips. Lines 84 and 99: `EXEMPT_AS_TARGET = {"tooling",
  "hybrid"}`, and `sp42-core` is `hybrid` — so the `sp42-wiki → sp42-core` edge is
  skipped entirely rather than checked.
- `sp42-wiki/src/registry.rs:154-162` `resolve()` falls back to
  `crate::sites::derive_wiki_config(wiki_id)`, which reads
  `include_str!("../data/wikimedia-sites.json")` (`sites.rs:23`).
  `registry.rs:277-301` asserts `registry.resolve("dewiki")` succeeds while
  dewiki is unconfigured.
- `crates/sp42-platform/Cargo.toml` `[dependencies]`: `async-trait`, `base64`,
  `futures`, `regex`, `serde`, `serde_json`, `serde_yaml`, `sha2`, `similar`,
  `sp42-types`, `thiserror`, `tracing`, `unicode-normalization`, `url`.
  **No `tokio`, no `reqwest`** — this is CONSTITUTION §2.3's "no dependency on
  any I/O crate" made concrete, and it is the constraint that shapes Phase 3.
- No `struct Timestamp` / `type Timestamp` anywhere in `crates/`. Every existing
  timestamp is a bare `i64` epoch-ms field: `EditEvent.timestamp_ms`
  (`sp42-platform/src/types.rs:96`), `VerdictEnvelope.recorded_at_ms`, `fetched_at_ms`.
- `config_version`: 14 mentions across 0026/0027/0028/0029, **zero** in `crates/`.
  The nearest existing thing is `policy_version: String`, hand-authored as `v0.1`
  in `configs/scoring/active/*.yaml`, schema-pinned to `^v\d+\.\d+$`, validated
  only for non-blankness (`scoring_policy.rs:390`) and never hashed.
  `sha2` is already a `sp42-platform` dependency; `sha256_hex` exists at
  `sp42-citation/src/citation/verify.rs:391`.
- `scripts/ci-all.sh` is 12 lines and runs `sp42_run_xtask "$repo_root" ci-all`.
  `xtask/src/main.rs:345-439` `ci_all()` invokes only cargo/trunk/tauri — **no
  `scripts/*.sh` at all**. `scripts/check-scoring-governance.sh` appears nowhere
  in `.github/workflows/ci.yml` or `.husky/`; it is referenced only in ADR prose.
  The real gate list is `.github/workflows/ci.yml` lines 74-106.
- `sp42-live/src/stream_ingestor.rs:66-89` `ingest()` filters on: wiki match,
  change type, `raw.bot`, `raw.minor`, namespace allowlist. **No rights filter.**
  It derives *identity class* via `classify_editor` (line 150):
  `~…` → `Temporary`, IP-parseable → `Anonymous`, else `Registered`.
  `EditEvent` (`types.rs:89-113`) has no `page_id`, no `created_at`, no
  `last_revision_at`, no `account_created_at`.

---

## Task 1: Retarget `sp42-wiki` off `sp42-core` (removes Trap 1)

**Files:**
- Modify: `crates/sp42-wiki/Cargo.toml`
- Modify: `crates/sp42-wiki/src/registry.rs:7` (and any other `sp42_core::` import)

**Why first:** every later phase that wants intake to consult the wiki registry
needs a `sp42-platform → sp42-wiki` edge, and that edge is a build-breaking cycle
until this lands. Doing it as its own commit also keeps the architecture-map
regeneration isolated to one reviewable change.

**Step 1: Confirm the edge is really only a facade use.** `WikiConfig` lives in
`sp42-platform/src/types.rs:405` and is only *imported* by `sp42-wiki`. Check for
every `sp42_core` reference in the crate:

```
grep -rn "sp42_core" crates/sp42-wiki/src/
```

If anything other than `WikiConfig` is reached for, it must move to
`sp42-platform`/`sp42-types` first or be re-derived — do not paper over it by
keeping the `sp42-core` dependency, which is the entire point of this task.

**Step 2: Swap the dependency.** In `crates/sp42-wiki/Cargo.toml`, replace

```toml
sp42-core = { path = "../sp42-core" }
```

with

```toml
sp42-platform = { path = "../sp42-platform" }
sp42-types = { path = "../sp42-types" }
```

and in `registry.rs:7` change `use sp42_core::WikiConfig;` to
`use sp42_platform::WikiConfig;`.

**Step 3: Verify the build and the cycle is genuinely gone.**

```
cargo check -p sp42-wiki
cargo tree -p sp42-wiki -e normal | grep -c sp42-core   # expect 0
```

**Step 4: Regenerate the architecture map.** The dependency edge moved, so
`./scripts/generate-architecture-map.sh --check` will now fail. Regenerate
(`./scripts/generate-architecture-map.sh`) and commit the result — the map is a
generated file, do not hand-edit it.

**Step 5: Commit** — `refactor(wiki): depend on sp42-platform directly, not the sp42-core facade`

---

## Task 2: A `registered` predicate that is not `resolve()` (removes Trap 2)

**Files:**
- Modify: `crates/sp42-types/src/traits.rs` (new trait + re-export from `lib.rs`)
- Modify: `crates/sp42-wiki/src/registry.rs` (impl)
- Modify: `crates/sp42-wiki/src/registry.rs:277-301` (the test that locks auto-derivation)

**Step 1: Declare the trait in `sp42-types`, next to the other platform edges.**
`sp42-types/src/traits.rs` is the home for every outbound dependency edge
(`HttpClient`, `Storage`, `Clock`, `EventSource`, `WebSocket`) and it is the only
crate both `sp42-platform` and `sp42-wiki` can share without a cycle:

```rust
/// The subset of the wiki registry that platform code may consult.
///
/// Deliberately narrower than resolution: `sp42-wiki` can derive a config for any
/// known Wikimedia dbname, but ADR-0026 §5 requires that a wiki be *registered*
/// before intake will filter on it, so auto-derivation must not be reachable
/// from a filter decision.
pub trait WikiRegistryView: Send + Sync {
    /// Wiki ids that have been explicitly configured/registered.
    fn registered_wiki_ids(&self) -> Vec<String>;
    /// True only for explicitly registered wikis. Never true for a merely
    /// derivable one.
    fn is_registered(&self, wiki_id: &str) -> bool;
}
```

Re-export it flat from `sp42-types/src/lib.rs`, matching how every other trait in
that file is exposed (`pub use traits::{Clock, ... }`).

**Step 2: Implement it for `WikiRegistry`.** This is a thin, honest wrapper —
`wiki_ids()` (`registry.rs:186`) already returns only the configured keys:

```rust
impl WikiRegistryView for WikiRegistry {
    fn registered_wiki_ids(&self) -> Vec<String> { self.wiki_ids() }
    fn is_registered(&self, wiki_id: &str) -> bool { self.wiki_ids().iter().any(|id| id == wiki_id) }
}
```

Prefer this over reaching into `inner.configs` — `wiki_ids()` is the public,
already-sorted accessor, and going through it keeps the two notions of "known"
from drifting.

**Step 3: Add the negative test, which is the one that matters.** The existing
test at `registry.rs:277-301` documents that an unconfigured-but-real project
derives dynamically. That behavior is *fine* and must be kept — it just must not
be what intake gates on. Add a test asserting the pair:

```rust
// resolution may derive an unconfigured project, but registration must not
let reg = registry_with_only_frwiki();
assert!(reg.resolve("dewiki").is_ok(), "dynamic derivation is still supported");
assert!(!reg.is_registered("dewiki"), "…but an unconfigured wiki is not registered");
assert!(reg.is_registered("frwiki"));
```

**Step 4: Re-export from `sp42-platform`** (`src/traits.rs` is now a pure
compat re-export of `sp42-types`; add the name there and to the root
`pub use` list in `lib.rs`).

**Step 5: Commit** — `feat(wiki): add a registered-wiki predicate distinct from resolution`

---

## Task 3: Record the nine decisions

The seven remaining decisions (D1, D2, D4, D6, D7, D8, D9) are recorded here with
a recommendation. Each needs an answer from the project owner; where the answer
differs from the recommendation, the phase file that depends on it must be edited
before that phase starts.

### D1 — `Timestamp` does not exist. The ADRs use it in ~10 signatures.

Every existing timestamp in the codebase is a bare `i64` epoch-ms field. The ADRs
write `Timestamp` in `IntakeItem.created_at/observed_at/last_revision_at`,
`LifecycleTransition.occurred_at`, `ContentLifecycleRecord.updated_at`,
`EligibilityVerdict.evaluated_at`, `QualityVerdict.evaluated_at`, and more.

**Recommendation: introduce a `Timestamp` newtype in `sp42-types`.** Six ADRs and
roughly a dozen new fields all named `Timestamp` is exactly the situation
CONSTITUTION §6.1 ("every type exists in one place") is written for, and a newtype
is where the unit is defined once. Cost: it must serialize transparently
(`#[serde(transparent)]` over `i64`) so nothing else in the tree changes. Do
**not** retrofit it onto `EditEvent.timestamp_ms` — that would rewrite shipped
scoring behavior's wire format for no gain. If the owner prefers house style over
the ADRs' vocabulary, then use `i64` ms consistently and note in the plan that
ADR `Timestamp` == `i64` epoch-ms; that is a defensible call, but it must be a
deliberate one, and it should be recorded as an `**Amended:**` note on 0026/0027
so the ADRs and the code do not silently disagree.

### D2 — `config_version` has no implementation anywhere.

This is load-bearing: ADR-0028 §4's entire replay argument ("a verdict can be
audited but not replayed" without it) and ADR-0026 §6's `IntakeDecision` both
depend on it. The existing `policy_version` is a hand-written `v0.1` string that
nothing hashes.

**Recommendation: a per-pipeline / per-ruleset content hash, computed at compile
time, carried as a short hex digest.** A single global version is wrong — any
unrelated config edit would invalidate the replayability of all recorded
history. Compute the digest over the *canonical serialization of the one compiled
pipeline or ruleset that produced the decision*, not over the config directory.
`sha2` is already a `sp42-platform` dependency and `sha256_hex` already exists in
`sp42-citation`. Keep a human-readable label alongside the digest so an operator
can tell `npp-quickfail@3f9a…` from `npp-quickfail@a1c2…`.

### D4 — `sp42-app` has no durable store, but ADR-0026 §2 says intake is always-on there and ADR-0027 wants a durable log.

`sp42-app`'s only `Storage` impls are `LocalStorageBrowserStorage` and
`VolatileBrowserStorage` (`crates/sp42-app/src/platform/runtime.rs:120,177`); the
latter is explicitly non-durable. CONSTITUTION §10.1 already restricts
LocalStorage for tokens, and §5.2 caps the wasm bundle at <400KB gzipped, so a new
IndexedDB store is not free.

**Recommendation: the lifecycle log is server-side; the browser shell evaluates
intake but does not record transitions.** This keeps the append-only log's
durability and audit-trail claims honest, and avoids spending wasm budget on a
store the browser shell cannot make authoritative. It does mean the browser
cannot answer `PriorOutcome` offline — which is acceptable because it cannot
*write* the answers either. This is a product-behavior call; if the browser must
record, it needs an IndexedDB `Storage` impl first, as its own phase.

### D6 — The gate wiring ADR-0026 §7 describes does not exist.

ADR-0026 §7 says the linter is "an `xtask` subcommand run from `ci-all.sh`,
alongside `scripts/check-layering.sh` and `scripts/check-scoring-governance.sh`".
In reality `ci-all.sh` runs no `check-*.sh`, and `check-scoring-governance.sh` is
not in `ci.yml` either — it is an orphaned script referenced only in prose.

**Recommendation: Phase 4 adds the linter as an `xtask` subcommand *and* an
explicit step in `.github/workflows/ci.yml`'s `checks` job.** The CI step is not
optional; without it the fail-closed guarantee the ADRs are built on is
decorative. While there, adopt `check-scoring-governance.sh` into `ci.yml` as
well — it is a real gate that has silently not been running, and wiring it is a
two-line change that pays for itself.

### D7 — `IntakeItem` overlaps `EditEvent`; Constitution §6.1 forbids duplicates.

§6.1: "Every type, constant, and rule exists in one place… No copies. No
conversion layers." `IntakeItem` and `EditEvent` carry overlapping facts, and a
conversion function between them is the thing §6.1 warns about.

**Recommendation: document the relationship as two envelopes over one fact set —
`IntakeItem` is the pre-admission envelope, `EditEvent` is the post-scoring one —
with an explicit rule that a new *filterable* fact is added to `IntakeItem`, and
`EditEvent` gains it only if scoring needs it independently.** This mirrors how
`EditEvent.content_model` was added additively and back-compatibly
(`types.rs:105-112`). Because these ADRs are `Accepted` and therefore immutable in
substance, record this as an `**Amended:**` note on ADR-0026 rather than a silent
edit.

### D8 — ADR-0027 §4's "most-recent-first" has an ambiguous tiebreak.

`history` is a `Vec`, so ordering is a sort — but CONSTITUTION §1.4 requires
explicit ordering discipline, and two transitions can share an `occurred_at`
(same millisecond, and `FixedClock` makes this the norm in tests, not an edge
case). Without a tiebreak, ADR-0027 §4's "load-bearing" guarantee is untestable.

**Recommendation: add a monotonic `seq: u64` to `LifecycleTransition`, assigned at
write time, and order by `(occurred_at desc, seq desc)`.** This is additive, it
pins determinism per §1.4, and it costs nothing. It is also a small addition to
what ADR-0027 §2 specifies, so it wants the same `**Amended:**` treatment as D7.

### D9 — `sp42-live` does not filter on actor rights, and its events lack the temporal facts.

ADR-0026 §Context says `sp42-live` narrows by "which namespace, which event type,
which actor rights bypass review". In fact `ingest()`
(`stream_ingestor.rs:66-89`) filters on wiki, change type, bot, minor, and
namespace — there is no rights filter, and `EditEvent` has no `page_id`,
`created_at`, `last_revision_at`, or `account_created_at`.

**Consequence for the plan, and it is significant:** the first Stream adapter
yields `ActorRights: None` and all three temporal fields `None`, so **the
three-valued `Unknown` path is the normal path, not an edge case.** This is
correct — it is precisely the situation ADR-0026 §3's `Option` typing and §4's
`Unknown` routing were designed for, and §2's baseline pipeline already restricts
itself to "intrinsic facts (event type, bot flag)" for exactly this reason. The
plan must hold that line: **the shipped baseline pipeline must not reference any
temporal field**, or `on_unknown`'s fail-closed `Drop` default will silently drop
real traffic on day one. Phase 8 carries this as an explicit acceptance test.

Also worth recording: the hardcoded `ignore_bots: true` / `ignore_minor: false` /
`allowed_change_types: ["edit","new"]` defaults in
`stream_ingestor.rs:34-46` are a live queue cutoff living in Rust, which
SCORING_CONSTITUTION §14.3.2 explicitly forbids. Moving them into the baseline
pipeline config is constitution-mandated, not optional tidying.

**Step 5: Commit** — `docs: record the intake-stack implementation decisions as an ADR amendment note`

---

## Phase 0 exit criteria

- [ ] `cargo tree -p sp42-wiki` shows no `sp42-core`; `cargo build --workspace` is green
- [ ] `./scripts/check-layering.sh` green
- [ ] `./scripts/generate-architecture-map.sh --check` green, with the regenerated map committed
- [ ] A test asserts `resolve("dewiki")` succeeds while `is_registered("dewiki")` is false
- [ ] D1–D9 each have an owner decision recorded, and any divergence from the
      recommendation has been written back into the dependent phase file
- [ ] The `**Amended:**` notes for D7 and D8 are added to ADR-0026 / ADR-0027 and
      pushed, so the immutable ADRs and the code do not silently disagree
