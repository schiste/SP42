# ADR-0028: Eligibility gate contract — capability-gated verdicts, open outcome vocabulary, ruleset composition over the intake engine, gate chaining

**Status:** Proposed
**Date:** 2026-08-24
**Author:** Christophe Henner (drafted by Claude Code)
**Summary:** Eligibility is a platform gate type — distinct from intake at a different altitude — that evaluates a `ReviewableItem` already admitted by intake against a named ruleset composed from intake's own condition tree plus a prior-outcome reference, producing a capability-gated, per-ruleset-keyed verdict recorded on `ContentLifecycle` (ADR-0027), with an explicit disallowed-reasons blocklist and the same fail-closed mechanical linting as intake.

## Context

NPP quickfail/notability, AfC accept/decline, CSD, PROD, AfD, and redirect
handling were sketched earlier as sharing one shape: a ruleset id in, a
verdict-plus-reasons out. Research against the primary policy pages (WP:CSD,
WP:PROD, WP:AFD, WP:GNG/SNG, WP:AFC, redirect CSD R2–R4/R#DELETE, WP:NPP)
confirmed that shape and sharpened three things the earlier essay-derived
sketch couldn't settle:

- **Outcome vocabularies are wide and ruleset-specific**, not a shared small
  set. AfD alone has 16+ named resolutions (Keep, Speedy keep, Delete, Merge,
  Reverse merge, Redirect, Delete-and-redirect, Disambiguate, Userfy,
  Draftify, Move-to-projectspace, Transwiki, Snowball-close, Withdraw, No
  consensus, Relist); PROD is near-binary; NPP's notability check is
  three-way. A single closed outcome enum, the shape `CitationVerdict`
  (ADR-0007/0008) uses, does not fit here.
- **Several rulesets are fully machine-checkable** against page-age,
  namespace, and actor/history facts — G13 (draft unedited 6 months), PROD's
  eligibility preconditions (mainspace-only, never-PRODed, 7-day objection
  window), redirect CSD R2–R4 (target namespace, page age, history shape),
  and G5 (no substantial edits since a ban-violating creation) — structurally
  identical to an intake condition tree, not a new rule shape. GNG/SNG's
  either-notability-path-clears-the-bar structure is a real `Any(...)`
  requirement, not just quickfail's. Others (G1–G3, GNG's "significant"/
  "reliable") are irreducibly human judgment. Eligibility has to carry both
  on the same contract.
- **Gate chaining is a hard requirement**, not an edge case: G4's precondition
  is literally "a prior AfD gate resolved this page to Delete"; NPP's
  redirect-reopening rule and AfC-accept-suppresses-NPP-unreviewed are the
  same shape. This is why ADR-0027 (`ContentLifecycle`) had to be drafted and
  land first — this ADR's `PriorOutcome` condition and verdict recording both
  read and write through it.
- **AFCSTANDARDS explicitly forbids certain decline reasons** (e.g.
  citation-formatting complaints) — evidence that a ruleset schema needs to
  express what is *not* a valid reason, not only what is.

## Decision

### 1. Eligibility is a gate type, at a different altitude than intake, homed with it in `sp42-platform` / `sp42-types`
Intake (ADR-0026) is unattended, requires no capability, and produces only a
scoping decision — is this item even in play. Eligibility runs only on items
intake has already admitted, always requires a capability, and always
records a judgment. Blurring the two would fill every workflow's audit trail
with meaningless "verdicts" for the items that were never real candidates —
the distinction ADR-0026 §8 already drew is restated here as binding: an
`EligibilityRuleset` with no `capability_required` is a misconfiguration
(§6), not a permissive default. The ruleset/verdict contracts and the
evaluation engine sit in `sp42-platform` / `sp42-types`, the same footing as
intake (ADR-0026 §1) and `ContentLifecycle` (ADR-0027 §5) — a second domain
(AfC, AfD, CCI) reusing the same gate shape is exactly the reuse-by-design
test that earns platform placement.

### 2. Rulesets compose intake's condition tree, extended with one chaining primitive
```
EligibilityCondition = All(Vec<EligibilityCondition>) | Any(Vec<EligibilityCondition>) | Not(Box<EligibilityCondition>)
                      | Rule(IntakeRule)
                      | PriorOutcome { gate_id: String, ruleset_id: Option<String>, resolution_class: Option<ResolutionClass>, within: Option<Duration> }
```
`IntakeField` / `IntakeOp` / `IntakeValue` (ADR-0026 §4) are reused verbatim
— no parallel rule vocabulary. This is the ADR's central design commitment,
backed directly by two research findings: GNG/SNG's OR structure needs the
same `Any` combinator intake already has, and G13/PROD/R2–R4/G5 are page/
actor/history facts of exactly the kind `IntakeField` already models.
Duplicating the engine would duplicate the linter, the `CapabilityRef`
resolution, and the fail-closed discipline for no benefit. `PriorOutcome`
resolves against ADR-0027 §4's `transitions_matching` query — the concrete
dependency edge that ADR requires.

### 3. Outcomes are open per ruleset; only the resolution class is closed
```
EligibilityOutcome { key: String, resolution_class: ResolutionClass }
ResolutionClass = Terminal | NeedsHuman | Relist | RouteElsewhere
```
A ruleset owns its own key vocabulary (`clearly_fails` / `uncertain` /
`sufficient`, or `keep` / `delete` / `merge` / `no_consensus` / `relist`, …);
the engine branches generically only on `resolution_class`. This avoids the
closed-enum seam ADR-0021 §5 already flagged as a real cost (a new domain
can't add a signal without editing a platform type) — here a new ruleset
never needs a platform-type edit at all. `Relist` is its own resolution
class, not a special case of `NeedsHuman`: AfD relist means "re-run this
gate later," a distinct engine action from "hand this to a human now."

### 4. Verdicts are always recorded through `ContentLifecycle`, evaluator-attributed
```
EligibilityVerdict {
    ruleset_id, gate_id, item_id,
    outcome: { key, resolution_class },
    reasons: Vec<Reason>, matched_rule_path: Option<...>,
    evaluated_at, evaluator: Automated | Human { actor },
}
```
A verdict is written as a `ContentLifecycle` transition whose
`TransitionCause::GateVerdict` (ADR-0027 §2) carries `{gate_id, ruleset_id,
outcome_key}` — eligibility does not own a separate store; writing through
ADR-0027's log is exactly what makes §2's `PriorOutcome` queries resolve
against genuinely current data. The shape generalizes `CitationVerdict`'s
precedent (ADR-0007/0008: categorical, informational-first, no numeric
confidence) off a single closed enum onto the open key-plus-class shape of
§3.

### 5. A ruleset declares what it may *not* decide on
```
EligibilityRuleset { id, condition: EligibilityCondition, outcomes: Vec<{ key, resolution_class, effects }>, disallowed_reasons: Vec<String>, capability_required: String }
```
A recorded `Reason` matching `disallowed_reasons` is rejected at evaluation
time — the same fail-closed-and-loud discipline as ADR-0026 §6, extended
from a schema-resolution check to a policy-content check. This directly
encodes AFCSTANDARDS' real rule that certain decline reasons are simply not
valid, regardless of how plausible they read.

### 6. Configs are linted by extending ADR-0026's linter, not a second one
The xtask from ADR-0026 §7 gains: ruleset schema validation, `PriorOutcome`
`gate_id` existence, `disallowed_reasons` non-overlap with the ruleset's own
`outcomes`, and non-empty `capability_required`. Load-time rejection of an
unresolvable `PriorOutcome` `gate_id` or `CapabilityRef`; an eval-time
failure produces the same distinct `Misconfigured` state ADR-0026 §6 defines
— never silently collapsed into a real outcome.

### 7. Automated is a first-class evaluator, not a fiction
`evaluator: Automated` is legitimate wherever a ruleset's condition is fully
machine-checkable (G13, PROD's precondition set, R2–R4, G5) — no synthetic
"attributed to a human" placeholder. A ruleset that mixes a machine-checkable
precondition with a human-only judgment (PROD's mechanical eligibility
window feeding a human's "is this uncontroversial?" call) expresses that as
the condition gating entry to a human-judgment outcome branch; this ADR does
not mandate splitting such cases into two separate gates, only that both
shapes are representable on one contract.

## Alternatives

- **A closed global `EligibilityOutcome` enum, `CitationVerdict`-style.**
  Rejected — AfD alone needs 16+ outcomes; a platform enum edited per new
  ruleset repeats the exact closed-enum seam ADR-0021 §5 flags as a real
  cost, deliberately avoided here from the start.
- **A separate rule vocabulary from intake.** Rejected — GNG/SNG's OR
  structure and the fully machine-checkable rulesets are structurally
  identical to intake facts; a second engine duplicates the linter,
  `CapabilityRef` resolution, and fail-closed discipline for no benefit.
- **No gate-chaining primitive; require workflows to pass prior verdicts as
  explicit input.** Rejected — G4, redirect-reopening, and AfC-suppresses-NPP
  all need a fact about the *past* that a caller-supplied input can't
  guarantee is present or current; only a durable, queryable record makes
  chaining safe against a workflow author simply forgetting to wire it.
- **Fold `ContentLifecycle` recording into this ADR instead of a separate
  ADR-0027.** Rejected — other gate types (`content-lifecycle-transition`,
  `tag-lifecycle`) need the same record independent of eligibility, so it
  does not belong scoped to this contract.
- **Let a ruleset cite a disallowed reason silently (log-only).** Rejected —
  matches ADR-0026's "misconfiguration is never silent" precedent; a policy
  author citing a banned reason is a policy bug, not a warning-level event.

## Consequences

- Every policy-derived ruleset from the research (`npp-quickfail`,
  `gng-sng`, `afc-accept`, `redirect-csd`, `r-delete`, `prod-standard`,
  `prod-llm`) becomes expressible in one schema without a new outcome enum
  per ruleset.
- Fully machine-checkable rulesets (G13, PROD precondition, R2–R4, G5) can
  run as `Automated`-evaluator verdicts feeding a human's final call — a
  concrete near-term reviewer-workload payoff, distinct from the
  mechanism-ahead-of-adoption pattern ADR-0026/ADR-0027 otherwise share.
- **Hard-depends on ADR-0027**: `PriorOutcome` resolution and verdict
  recording both read and write through `ContentLifecycle`; this is not
  sequencing convenience.
- Nothing is implemented yet — ruleset types, the linter extension,
  `PriorOutcome` resolution, and the per-ruleset outcome registry all still
  need to be built, same Proposed-stage status as ADR-0026/ADR-0027.
- The linter must validate static `disallowed_reasons` at config-lint time,
  and re-check a templated `Reason` at eval time when it's built from
  runtime data — flagged as an implementation subtlety this ADR does not
  fully resolve.
- Forecloses a domain hand-rolling its own verdict shape for anything
  gate-shaped outside this contract — the reuse-by-design test a second
  domain (AfC, AfD, CCI) satisfies here is exactly what earns this a platform
  ADR rather than a patrol-domain one.

## Non-goals

- `ContentLifecycle`'s own state/transition/watch mechanism — ADR-0027.
- Intake's rule engine itself (`IntakeCondition`/`Field`/`Op`/`Value`,
  `CapabilityRef` resolution) — ADR-0026; this ADR only extends it with
  `PriorOutcome`.
- The workflow engine, gate marketplace, or any `npp.yaml`-equivalent format
  — a separate future ADR, the same non-goal ADR-0026 already carried
  forward.
- The discussion/consensus contract for AfD-shaped multi-party outcomes
  (participants, !votes, closing rationale, relist quorum) — a separate
  future platform capacity; `Relist` here is the hook such a gate would set,
  not the discussion mechanism itself.
- Concrete per-wiki or per-domain ruleset content (an actual
  `npp-quickfail.yaml` or `gng-sng.yaml`) — policy, not mechanism.
- The reviewer capability model's internals (what `capability_required`
  actually checks against) — ADR-0002/ADR-0014's existing identity/session
  model is referenced, not re-decided.
