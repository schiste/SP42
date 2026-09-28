# Intake / Lifecycle / Gates Implementation Plan — Phase 7: ADR-0029 deterministic quality gate

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** Implement ADR-0029 — a platform gate type at the same altitude as
eligibility but structurally separate from it, reusing `PriorOutcome`,
`PriorReviewerAction`, and `ResolutionClass` verbatim, with every quality tier a
per-project opt-in.

**Architecture:** `crates/sp42-platform/src/quality/deterministic.rs`, a **sibling**
of `eligibility/deterministic.rs` — not nested under it (ADR-0029 §1).

**Tech Stack:** everything from Phases 1–6. No new dependencies.

**Scope:** Phase 7 of 8. Structurally the mirror of Phase 6 — the point of this
phase is that it is *mostly not new work*, and the plan should make that visible
rather than pretend otherwise.

**Codebase verified:** 2026-09-28. Everything Phase 6 established applies; the
specific facts are the sibling-module requirement (ADR-0029:59-63) and the
cross-gate `PriorOutcome` direction (ADR-0029 §2, Alternatives:249-255).

---

## Task 1: The sibling module and the compile-time boundary

**Files:**
- Create: `crates/sp42-platform/src/quality/mod.rs`
- Create: `crates/sp42-platform/src/quality/deterministic.rs`

**Step 1: `QualityVerdict` is a distinct Rust type**, structurally identical in
shape to `EligibilityVerdict` (both write through the same
`TransitionCause::GateVerdict`) but not interchangeable. The test that proves the
boundary is a compile-fail assertion — assert in the module docs, and add a test
that the two types are not mutually coercible, or a `trybuild`-style negative if
the repo ever adopts one (it does not today; do not add a dependency for it).

**Step 2: `QualityCondition` reuses Phase 6's `PriorOutcome`/
`PriorReviewerAction` verbatim** (ADR-0029 §2). Neither needs changes to serve a
second gate type — that is the payoff of having made them generic over `gate_id`
in Phase 6. Do not write a quality-flavoured variant; CONSTITUTION §6.1.

**Step 3: Reuse `ResolutionClass` verbatim — no fifth variant** (ADR-0029 §3).
DYK approval/rejection → `Terminal`; GA "on hold" → `Relist`; a contested GA/FA
promotion → `NeedsHuman`; FA's "not yet GA" precondition → `RouteElsewhere`. The
argument that this is sufficient is precisely what makes centralizing the
vocabulary in Phase 5 correct, and it should hold up on inspection.

**Step 4: Commit** — `feat(platform): quality conditions and verdict as a sibling gate type`

---

## Task 2: Rulesets, opt-in, and the shared linter

**Files:**
- Create: `configs/quality/*.yaml`
- Create: `schemas/quality-ruleset.schema.json`
- Modify: `crates/sp42-platform/src/intake_lint.rs`

**Step 1: `QualityRuleset`/`QualityOutcomeArm` mirror Phase 6** — ordered guarded
arms, first `True` wins, `Unknown` falls through, mandatory trailing catch-all.
Same rationale, same shape, no variant.

**Step 2: `disallowed_reasons` as `ReasonCode`s, lint-time** (ADR-0029 §5) — and
note the ADR's own point that this matters *more* here than for eligibility,
because quality rationales are the most free-text and most project-specific of the
three gate types. Matching rendered prose would have been the difference between a
blocklist that works and one that quietly does not.

**Step 3: Zero configured quality rulesets is a valid state** (ADR-0029 §8) — not
a misconfiguration, and not a degraded one. The linter must therefore treat an
empty quality ruleset set as passing, and there must be **no embedded default
quality ruleset**, unlike intake's always-on baseline. `gate_id`/`ruleset_id` stay
opaque project-chosen strings throughout; nothing assumes DYK/GA/FA.

**Step 4: Extend the shared linter a second time** (ADR-0029 §6), with one
change Phase 6 does not have: `PriorOutcome.gate_id` existence is now checked
against **both** eligibility and quality gate ids, because chaining crosses gate
types (ADR-0029 §2).

**Step 5: Commit** — `feat(platform): quality ruleset validation, cross-gate PriorOutcome, and the opt-in default`

---

## Task 3: The cross-gate chaining case

**Files:**
- Create: `evals/quality/fixtures/<wiki>/*.yaml`

**Step 1: The GA-delisting case that answers "why two gate types at all".**
A GA reassessment ruleset requiring "was this previously promoted to GA" must read
as *not promoted* once a `delisted` transition supersedes it. GA promotion,
delisting, and FA promotion all share `resolution_class: Terminal`, so this only
works because `PriorOutcome` carries an exact-match `outcome_key` alongside the
class (ADR-0029 §2). This is the same reasoning as G4, and it is the concrete
justification for the separation.

**Step 2: A cross-gate case**: a quality ruleset requiring "was kept, not merged,
at a prior AfD" — `gate_id` crossing from a quality ruleset into an eligibility
gate's recorded verdicts, with neither gate's types imported by the other.

**Step 3: Commit** — `test(quality): cross-gate PriorOutcome chaining and GA delisting recency`

---

## Task 4: Explicitly defer the policy research

**Files:**
- Modify: `docs/platform/adr/0029-deterministic-quality-gate-contract.md` (`**Amended:**` note only)

**Step 1: Do not ship a "verified" DYK/GA/FA ruleset.** ADR-0029's Context is
explicit that no primary-policy research pass was done against WP:DYK/WP:GAN/
WP:FAC, and it carries that as a Non-goal. The mechanism is the deliverable; the
tier definitions are policy. Any concrete `configs/quality/*.yaml` that looks
authoritative would misrepresent the evidence base.

**Step 2: If a demonstration ruleset is wanted**, mark it as illustrative in a
header comment and name the research pass as its prerequisite — do not let it
become a de-facto default that §8's "per-project opt-in" reasoning was built to
avoid.

**Step 3: Commit** — only if the `**Amended:**` note is needed; otherwise skip.

---

## Phase 7 exit criteria

- [ ] `quality::deterministic` is a sibling of `eligibility::deterministic`
- [ ] `QualityVerdict` and `EligibilityVerdict` cannot be interchanged
- [ ] `ResolutionClass` still has exactly four variants and one definition
- [ ] No quality-flavoured `PriorOutcome` exists
- [ ] An empty quality ruleset set passes the linter
- [ ] No embedded default quality ruleset ships
- [ ] The linter checks `PriorOutcome.gate_id` across **both** gate types
- [ ] The GA-delisting recency case passes
- [ ] No concrete DYK/GA/FA ruleset is presented as policy-verified
