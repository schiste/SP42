# Intake / Lifecycle / Gates Implementation Plan — Phase 2: ADR-0031 verdict reason

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** Implement ADR-0031 — the static, per-ruleset `ReasonCode` plus a
per-project, per-language catalog — so that `disallowed_reasons` becomes a
language-independent blocklist decidable at config-lint time. This must land
before Phase 6, because ADR-0031 §2 is what makes the blocklist static.

**Architecture:** Reason types in `sp42-types`; catalog loading and validation in
`sp42-platform`, modelled on `sp42-citation/src/citation/storage.rs` (versioned
envelope + namespaced keys) and on the scoring policy's
parse → validate → compile pipeline.

**Tech Stack:** `serde`/`serde_yaml` (both already deps), `thiserror`,
`BTreeMap` for deterministic catalog ordering.

**Scope:** Phase 2 of 8. Independent of Phase 1 except that both land in
`sp42-types`; do them in either order but keep the commits separate.

**Codebase verified:** 2026-09-28.

- The parse → validate → compile precedent is
  `sp42-platform/src/scoring_policy.rs`: `parse_scoring_policy` (:254),
  `compile_scoring_policy` (:266), `validate_scoring_policy` (:383), with
  `ScoringPolicyError::InvalidField { field, message }` (:558 `signal_from_slug`)
  as the total-function-that-errors-on-unknown-slug shape. **Reuse that error
  variant shape verbatim** — it is exactly ADR-0031's "a code no version of the
  ruleset could ever produce is a config bug".
- Policy files are embedded at compile time with `include_str!`
  (`scoring_policy.rs:14-23`), so an uncompilable policy is a build failure. The
  same trick is how `configs/frwiki.yaml` reaches `sp42-wiki`
  (`registry.rs:5`). Use it for the shipped catalog so a malformed catalog cannot
  reach production.
- `PolicyLifecycle { Active | Candidate | Suggested }` (`scoring_policy.rs:62`) is
  the existing closed-enum-with-`snake_case`-serde convention.
- CONSTITUTION §1.4: use `BTreeMap`/`BTreeSet` wherever iteration order could
  reach an output. A catalog's iteration order is exactly that.

---

## Task 1: The reason type

**Files:**
- Create: `crates/sp42-types/src/reason.rs`
- Modify: `crates/sp42-types/src/lib.rs`

**Step 1: Declare per ADR-0031 §1.**

```rust
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Reason { pub code: ReasonCode, pub params: ReasonParams }

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReasonCode(String);   // opaque newtype, not a bare String
```

`ReasonCode` is a newtype rather than a `String` so that a code cannot be
accidentally produced from runtime string concatenation — that is the whole
mechanism by which ADR-0031 §2 makes codes statically declared and
composition-impossible, and it is what allows ADR-0031 §3 to claim the blocklist
is decidable at lint time. Keep the constructor crate-private or
`pub(crate)`, exposed to config-parsing code only.

**Step 2: `ReasonParams` as typed data, never sentence fragments.**
ADR-0031 §5 is explicit: a parameter is a page id, a count, a namespace name, a
threshold — never a fragment of prose. Model it as a small ordered multimap with
typed accessors rather than `HashMap<String, String>`, so template rendering can
be exhaustive and a missing parameter is a load error rather than a blank:

```rust
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReasonParams(BTreeMap<String, ReasonParam>);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReasonParam { Text(String), Count(i64), PageId(i64), TimestampMs(i64) }
```

**Step 3: `resolve` for display.** Pure function from `(code, params, catalog,
lang) -> String`. It must not read the clock, the filesystem, or the network —
CONSTITUTION §2.3, and the `iso_date_from_epoch_ms` precedent
(`sp42-citation/src/bare_url_repair.rs:295-313`, whose doc comment says "pure so
the shell can pass `clock.now_ms()` and the core stays clock-free").

**Step 4: Commit** — `feat(types): Reason, ReasonCode, and typed reason parameters`

---

## Task 2: The per-project, per-language catalog

**Files:**
- Create: `configs/reason-catalog/frwiki/en.yaml`
- Create: `crates/sp42-platform/src/reason_catalog.rs`
- Create: `schemas/reason-catalog.schema.json`

**Step 1: Follow the scoring policy's directory and schema discipline.**
`configs/scoring/{active,candidate,suggested}/`, one JSON Schema per artifact type
in `schemas/`, `additionalProperties: false` at every level. For the catalog use
`configs/reason-catalog/<wiki_id>/<lang>.yaml` per ADR-0031 §4, with
`schemas/reason-catalog.schema.json` pinning `template` (non-empty) and
`param_schema` (the `ReasonParam` variant names a template may use).

**Step 2: Load, validate, and make a missing entry a load-time error.**
ADR-0031 §4 is the load-bearing rule: "a catalog entry missing for a code a
configured ruleset can produce is a **load-time error for that project**, not a
runtime fallback to the bare identifier." So the loader takes the set of codes the
project's configured rulesets can produce and requires the catalog to cover them:

```rust
pub fn load_reason_catalog(yaml: &str, required_codes: &BTreeSet<ReasonCode>)
    -> Result<ReasonCatalog, ReasonCatalogError>
```

Iterate `required_codes` and reject any uncovered one. The error must name the
missing code, per the `InvalidField { field, message }` precedent.

**Step 3: Embed the shipped catalog with `include_str!`** so a malformed catalog
is a build failure, matching `scoring_policy.rs:14-23`.

**Step 4: Commit** — `feat(platform): per-project per-language reason catalog with load-time coverage`

---

## Task 3: Pin the properties

**Files:**
- Modify: `crates/sp42-platform/src/reason_catalog.rs` (inline tests)

**Step 1: The load-time-error test is the one that matters.** A ruleset that can
produce `insufficient_sources` with no `en` catalog entry must be *rejected at
load*, and the test must assert the error rather than a fallback string. That test
is the direct check on ADR-0031 §4.

**Step 2: Coverage across languages.** A code present in `en` but missing in `fr`
is rejected for the French profile — this is the property that makes the
blocklist language-independent rather than best-effort.

**Step 3: Rendering is total.** Every `param_schema` entry a template references
resolves against a supplied `ReasonParams`, and a template referencing an
undeclared parameter is a schema error.

**Step 4: Commit** — `test(reason): catalog coverage is a load error, not a fallback`

---

## Phase 2 exit criteria

- [ ] A ruleset code with no catalog entry fails at load, with the code named in the error
- [ ] Per-language coverage enforced independently
- [ ] `ReasonCode` cannot be constructed from runtime concatenation outside config parsing
- [ ] `resolve` is pure — no clock, no I/O
- [ ] Catalog iteration is `BTreeMap`-ordered (CONSTITUTION §1.4)
- [ ] `cargo clippy -- -D warnings` clean; `sp42-platform` coverage still ≥90%
