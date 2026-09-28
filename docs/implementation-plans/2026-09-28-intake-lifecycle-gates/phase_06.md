# Intake / Lifecycle / Gates Implementation Plan — Phase 6: ADR-0028 deterministic eligibility gate

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** Implement ADR-0028 — a platform gate type, structurally separate from
intake, scoring, and any future stochastic gate, evaluating an admitted item
against a named ruleset composed from intake's own condition tree plus
`PriorOutcome`, producing a reproducible verdict recorded on `ContentLifecycle`.

**Architecture:** New module tree `crates/sp42-platform/src/eligibility/deterministic.rs`,
sibling to (not nested under) any quality module, so the two verdict types can
never be interchanged — ADR-0028 §1 requires the boundary be a compile-time
guarantee, and ADR-0029 §1 repeats the requirement for the sibling.

**Tech Stack:** everything from Phases 1–5. No new dependencies.

**Scope:** Phase 6 of 8.

**Codebase verified:** 2026-09-28.

- The "separate module path so types cannot be substituted" requirement is
  ADR-0028:82-84 (`eligibility::deterministic`) and ADR-0029:59-63
  (`quality::deterministic`, explicitly "sibling to `eligibility::deterministic`,
  not nested under it").
- `EligibilityVerdict`/`EligibilityRuleset` have no precedent; model the envelope
  on `CompositeScore`/`ScoringConfig` in `sp42-platform/src/types.rs:213,290,297`
  and the config side on `scoring_policy.rs`.
- `ScoringSignal` (types.rs:235) is a **closed 22-variant enum with no
  `Custom(String)` arm** — ADR-0021 §Consequences names that closedness as the
  known domain seam, and ADR-0031:113 cites it as the cost to avoid. The outcome
  `key` vocabulary here is the opposite choice (open, per-ruleset); do not
  reintroduce a closed platform enum for it.
- `x → enum` mapping convention, total and erroring on unknown:
  `signal_from_slug` (`scoring_policy.rs:558`) returns
  `ScoringPolicyError::InvalidField { field, message }`. Reuse that shape for
  `outcome_key` resolution and for `PriorOutcome`'s `outcome_key` membership check.

---

## Task 1: Conditions — reuse, do not rebuild

**Files:**
- Create: `crates/sp42-platform/src/eligibility/mod.rs`
- Create: `crates/sp42-platform/src/eligibility/deterministic.rs`

**Step 1: `EligibilityCondition` per ADR-0028 §2** — `All`/`Any`/`Not`/`Rule`/
`PriorOutcome`/`PriorReviewerAction`, where `Rule(IntakeRule)` **reuses Phase 3's
type verbatim**. No parallel rule vocabulary: ADR-0028 §2's central design
commitment, backed by CONSTITUTION §14.4 ("one implementation path for feature
extraction/rule application/policy loading/explanation"). A second vocabulary
would duplicate the linter, the `CapabilityRef` resolution, and the fail-closed
discipline for no benefit.

**Step 2: `PriorOutcome` resolves against Phase 5's `transitions_matching`**, and
per ADR-0028 §2 examines **only the first (most-recent) matching transition** for
the named `gate_id`/`ruleset_id` lineage. This is where the D8 tiebreak earns its
keep. It is generic over `gate_id`, so it can name a future stochastic gate
without either side importing the other's types.

**Step 3: A human's decision is not addressable through `PriorOutcome`.**
`TransitionCause::ReviewerAction` carries `{actor, action_kind}`, not gate-shaped
fields, so `PriorOutcome`'s filters have nothing to match against.
`PriorReviewerAction` is the dedicated predicate, filtering on `ReviewerAction`'s
own native fields (ADR-0028 §2, Alternatives:313-322).

**Step 4: Commit** — `feat(platform): eligibility conditions reusing the intake condition tree`

---

## Task 2: Rulesets as ordered guarded arms

**Files:**
- Create: `crates/sp42-platform/src/eligibility/deterministic.rs`

**Step 1: `EligibilityRuleset { id, outcomes: Vec<EligibilityOutcomeArm>,
disallowed_reasons, capability_required }` per ADR-0028 §5.** Walk `outcomes` in
declared order; the first arm whose `when` is `True` produces the verdict; an arm
evaluating `Unknown` falls through exactly like `False`, so unknown can never
manufacture a match.

**Step 2: The mandatory trailing catch-all** (`when: All([])`, vacuously `True`
under Kleene — Phase 3's property test covers this), so every input produces some
outcome. `resolution_class: NeedsHuman` is the common catch-all but is the ruleset
author's choice, not a fixed default.

**Step 3: `disallowed_reasons` is a set of `ReasonCode`s, checked at
config-lint time** (ADR-0031 §2/§3), not against a recorded reason at evaluation
time. Phase 2's catalog already requires coverage of every code a ruleset can
produce, so the two checks compose: a blocked code is a lint failure; an uncovered
code is a catalog load failure.

**Step 4: `EligibilityVerdict` per ADR-0028 §4** with the *complete* envelope —
`config_version` (the D2 digest) and `revision_id` are what make the verdict
replayable rather than merely auditable, and ADR-0028 §4 is explicit that without
both the claim is weaker than stated. No `evaluator` field: this gate structurally
cannot produce a verdict any other way.

**Step 5: Commit** — `feat(platform): eligibility rulesets as ordered guarded arms with a static blocklist`

---

## Task 3: Record through `ContentLifecycle`

**Files:**
- Modify: `crates/sp42-platform/src/eligibility/deterministic.rs`

**Step 1: A verdict is a `TransitionCause::GateVerdict`** carrying the full
envelope, written to Phase 5's log. Eligibility owns **no separate store** — that
is what makes §2's `PriorOutcome` queries on `resolution_class` or `outcome_key`
answerable, and what lets a future stochastic gate or a human `ReviewerAction` be
referenced without either side depending on the other's types.

**Step 2: `NeedsHuman` hands off to `ReviewerAction`, never to a human-flavored
verdict** (ADR-0028 §7). Keep the two `TransitionCause` variants distinct; that is
the whole point of the split.

**Step 3: Eval-time resolution failure reuses ADR-0026's `Misconfigured`** state
(ADR-0028 §6) — do not invent a second one, and never silently collapse it into a
real outcome.

**Step 4: Commit** — `feat(platform): eligibility verdicts recorded on the content lifecycle log`

---

## Task 4: Extend the one linter

**Files:**
- Modify: `crates/sp42-platform/src/intake_lint.rs`
- Modify: `configs/eligibility/*.yaml`
- Create: `schemas/eligibility-ruleset.schema.json`

**Step 1: Add rules to the existing pass** (ADR-0028 §6): last arm is the
unconditional catch-all; no earlier arm is statically `All([])`; `PriorOutcome`
`gate_id` exists; `disallowed_reasons` does not overlap the ruleset's own
`outcomes`; `capability_required` is non-empty; and — the one worth care — when
`PriorOutcome` names **both** `ruleset_id` and `outcome_key`, that `outcome_key` is
a member of that specific ruleset's declared outcomes. An `outcome_key` no version
of the referenced ruleset could ever produce is a config bug, not a rule that
quietly never matches.

**Step 2: Extend the fixture layout** as `evals/eligibility/fixtures/<wiki>/*.yaml`,
mirroring `evals/scoring/fixtures/vandalism_patrol/frwiki/`. Note the existing
fixtures are declarative metadata (slugs and prose), not executable scenarios — the
parse-and-assert harness lives in `sp42-patrol/src/scoring_evaluation.rs` (a
*domain* crate), because the engine is platform but the evaluation harness is not.
Decide deliberately whether the eligibility harness follows that split.

**Step 3: Commit** — `feat(platform): eligibility ruleset validation in the shared linter`

---

## Task 5: The ruleset that proves the mechanism

**Files:**
- Create: `configs/eligibility/npp-quickfail.yaml`
- Create: `evals/eligibility/fixtures/enwiki/*.yaml`

**Step 1: `npp-quickfail` using only machine-checkable facts** — page age,
namespace, actor/history — per ADR-0028 §Context (G13, G5, and GNG/SNG's
`Any(...)` disjunction are structurally identical to intake conditions). This is
the concrete payoff the ADR claims: a real policy ruleset expressible in the
schema with no new outcome enum.

**Step 2: An eval case for the G4 chaining shape** — a `PriorOutcome` naming an
AfD `gate_id` with `outcome_key: delete`, plus a negative case where a
`no_consensus` AfD outcome shares `resolution_class: Terminal` and must **not**
satisfy it. That negative case is what proves `outcome_key` earns its place over
`resolution_class` alone (ADR-0028 §2, Alternatives:298-303).

**Step 3: Commit** — `feat(eligibility): npp-quickfail ruleset and chaining eval cases`

---

## Phase 6 exit criteria

- [ ] `Rule` reuses `IntakeRule` verbatim — `grep` finds no second rule vocabulary
- [ ] First `True` arm wins; `Unknown` falls through; trailing catch-all required and enforced
- [ ] `PriorOutcome` reads only the most-recent matching transition
- [ ] `PriorReviewerAction` is required to address a `ReviewerAction` transition
- [ ] `EligibilityVerdict` and `QualityVerdict` are not interchangeable (once Phase 7 lands)
- [ ] `disallowed_reasons` is a static `ReasonCode` set, lint-time checked
- [ ] Verdict carries `config_version` + `revision_id`; no `evaluator` field
- [ ] Eval-time resolution failure yields `Misconfigured`, never a real outcome
- [ ] The linter is extended, not duplicated; one traversal still reports all types
