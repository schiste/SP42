# ADR-0028: Deterministic eligibility gate contract — capability-gated verdicts, open outcome vocabulary, ruleset composition over the intake engine, gate chaining

**Status:** Proposed
**Date:** 2026-08-24
**Author:** Christophe Henner (drafted by Claude Code)
**Summary:** The deterministic eligibility gate is a platform gate type — distinct from intake at a different altitude, and structurally separate from both the scoring gate (ADR-0021) and a future stochastic eligibility gate — that evaluates a `ReviewableItem` already admitted by intake against a named ruleset composed entirely from intake's own condition tree plus a prior-outcome reference, producing a reproducible, capability-gated, per-ruleset-keyed verdict recorded on `ContentLifecycle` (ADR-0027), with an explicit disallowed-reasons blocklist and the same fail-closed mechanical linting as intake.

## Context

NPP quickfail/notability, AfC accept/decline, CSD, PROD, AfD, and redirect
handling were sketched earlier as sharing one shape: a ruleset id in, a
verdict-plus-reasons out. Research against the primary policy pages (WP:CSD,
WP:PROD, WP:AFD, WP:GNG/SNG, WP:AFC, redirect CSD R2–R4/R#DELETE, WP:NPP)
confirmed that shape and sharpened four things the earlier essay-derived
sketch couldn't settle:

- **Outcome vocabularies are wide and ruleset-specific**, not a shared small
  set. AfD alone has 16+ named resolutions (Keep, Speedy keep, Delete, Merge,
  Reverse merge, Redirect, Delete-and-redirect, Disambiguate, Userfy,
  Draftify, Move-to-projectspace, Transwiki, Snowball-close, Withdraw, No
  consensus, Relist); PROD is near-binary; NPP's notability check is
  three-way. A single closed outcome enum, the shape `CitationVerdict`
  (ADR-0007/0008) uses, does not fit here.
- **A meaningful subset of rulesets are fully machine-checkable** against
  page-age, namespace, and actor/history facts — G13 (draft unedited 6
  months), PROD's eligibility preconditions (mainspace-only, never-PRODed,
  7-day objection window), redirect CSD R2–R4 (target namespace, page age,
  history shape), and G5 (no substantial edits since a ban-violating
  creation) — structurally identical to an intake condition tree, not a new
  rule shape. GNG/SNG's either-notability-path-clears-the-bar structure is a
  real `Any(...)` requirement, not just quickfail's. Others (G1–G3, GNG's
  "significant"/"reliable") are irreducibly judgment calls this gate does not
  attempt to make.
- **Gate chaining is a hard requirement**, not an edge case: G4's precondition
  is literally "a prior AfD gate resolved this page to Delete"; NPP's
  redirect-reopening rule and AfC-accept-suppresses-NPP-unreviewed are the
  same shape. This is why ADR-0027 (`ContentLifecycle`) had to be drafted and
  land first — this ADR's `PriorOutcome` condition and verdict recording both
  read and write through it.
- **AFCSTANDARDS explicitly forbids certain decline reasons** (e.g.
  citation-formatting complaints) — evidence that a ruleset schema needs to
  express what is *not* a valid reason, not only what is.

**Eligibility itself splits into two structurally separate gate types, not
one gate with an evaluator field:** a deterministic gate (pure rule-tree
evaluation over page/actor/history facts, reproducible and safe to resolve
`Terminal` unattended) and a stochastic gate (model/ML-judgment verdicts,
needing a grounding/anti-fabrication discipline this ADR does not attempt to
design). Keeping them as separate contracts — rather than one verdict shape
with an `evaluator` switch — makes it a type error to consume one gate's
verdict as if it were the other's; a caller cannot accidentally treat a
model-derived judgment as a reproducible rule match, or vice versa. The two
compose only by both writing through `ContentLifecycle` (ADR-0027) and
reading each other's recorded outcomes via `PriorOutcome`, never through a
shared Rust type — the same data-transfer boundary this ADR already needed
for chaining within one gate. **This ADR designs the deterministic gate
only**; the stochastic gate is a Non-goal.

Scoring (ADR-0021) is a third, separate gate category and stays untouched
here: it produces a ranking/prioritization signal, not an admission
decision, and already mixes ORES, LLM, and deterministic signals internally
for that purpose. Formalizing that internal mix is flagged as scoring's own
future refactor, not something this ADR touches or depends on.

## Decision

### 1. The deterministic eligibility gate is a gate type, at a different altitude than intake, structurally separate from the (future) stochastic gate, homed in `sp42-platform` / `sp42-types`
Intake (ADR-0026) is unattended, requires no capability, and produces only a
scoping decision — is this item even in play. This gate runs only on items
intake has already admitted, always requires a capability, and always
records a judgment — but only a judgment its condition tree can derive
mechanically; nothing here calls a model or asks a human to fill in an
`evaluator` slot. Blurring intake and eligibility would fill every
workflow's audit trail with meaningless "verdicts" for items that were never
real candidates — the distinction ADR-0026 §8 already drew is restated here
as binding: an `EligibilityRuleset` with no `capability_required` is a
misconfiguration (§6), not a permissive default. The ruleset/verdict
contracts and the evaluation engine sit in `sp42-platform` / `sp42-types`,
the same footing as intake (ADR-0026 §1) and `ContentLifecycle` (ADR-0027
§5) — a second domain (AfC, AfD, CCI) reusing the same gate shape is exactly
the reuse-by-design test that earns platform placement. Types live under a
name that makes the boundary explicit at compile time (e.g.
`eligibility::deterministic`), so a future stochastic gate's types cannot be
substituted by accident.

### 2. Rulesets compose intake's condition tree, extended with one chaining primitive
```
EligibilityCondition = All(Vec<EligibilityCondition>) | Any(Vec<EligibilityCondition>) | Not(Box<EligibilityCondition>)
                      | Rule(IntakeRule)
                      | PriorOutcome { gate_id: String, ruleset_id: Option<String>, outcome_key: Option<String>, resolution_class: Option<ResolutionClass>, within: Option<Duration> }
                      | PriorReviewerAction { actor: Option<String>, action_kind: Option<String>, within: Option<Duration> }
```
`IntakeField` / `IntakeOp` / `IntakeValue` (ADR-0026 §4) are reused verbatim
— no parallel rule vocabulary. This is the ADR's central design commitment,
backed directly by two research findings: GNG/SNG's OR structure needs the
same `Any` combinator intake already has, and G13/PROD/R2–R4/G5 are page/
actor/history facts of exactly the kind `IntakeField` already models.
Duplicating the engine would duplicate the linter, the `CapabilityRef`
resolution, and the fail-closed discipline for no benefit. `PriorOutcome`
resolves against ADR-0027 §4's `transitions_matching` query — the concrete
dependency edge that ADR requires — and is not restricted to referencing
another deterministic gate: `gate_id` may equally name a future stochastic
gate, since the query reads a generic `LifecycleTransition`, never a
gate-specific verdict type. This is exactly the "separated but composed
through data transfer" boundary: this gate's condition tree can react to
what another gate decided without ever importing that gate's Rust types.
Per ADR-0027 §4, `transitions_matching` returns matches most-recent-first;
`PriorOutcome` examines only that first (i.e. current) matching transition
for the named `gate_id`/`ruleset_id` lineage, never any historical
occurrence — a page `promoted` to GA and later `delisted` must read as
delisted to a `PriorOutcome` check, not as still-promoted because a
`promoted` transition occurred at some point in its history.

`resolution_class` alone is not enough to express G4 ("a prior AfD gate
resolved this page to Delete"): AfD's Keep, No consensus, and Merge can all
share `Terminal` alongside Delete, so a resolution-class-only predicate
can't tell them apart. `outcome_key` is the exact-match predicate that closes this: when set, it's
compared against the referenced transition's own `outcome_key` (ADR-0027
§2) — data the transition already carried, but that this condition had no
way to compare against directly, only through the coarser
`resolution_class`. `ruleset_id` and
`outcome_key` compose independently — a condition can pin the ruleset
without pinning the outcome, or pin an outcome key that's only meaningful
given a specific ruleset's own vocabulary (§6 validates that combination).

A human's decision is **not** addressable through `PriorOutcome`:
`TransitionCause::ReviewerAction` (ADR-0027 §2) carries `{actor,
action_kind}`, not a `gate_id`/`ruleset_id`/`outcome_key`/`resolution_class`
tuple, so `PriorOutcome`'s filters have nothing on a `ReviewerAction`
transition to match against. `PriorReviewerAction` is the dedicated
predicate for that case — filtering on `ReviewerAction`'s own native fields
(an open `action_kind` string, e.g. `"confirmed-notable"`, is the human
counterpart to `outcome_key`) rather than forcing `ReviewerAction` to grow
gate-shaped fields it has no business carrying. This keeps the same
boundary §7 establishes: a human's decision and a machine's verdict are
different `TransitionCause` variants, and now also different condition
variants a ruleset author reaches for explicitly, never a single predicate
that blurs which kind of prior decision it's actually reading.

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
`NeedsHuman` is this gate's honest boundary marker, not a placeholder for
work this gate secretly does — see §7.

### 4. Verdicts are always recorded through `ContentLifecycle`; a verdict from this gate is always machine-produced
```
EligibilityVerdict {
    ruleset_id, gate_id, item_id,
    outcome: { key, resolution_class },
    reasons: Vec<Reason>, matched_rule_path: Option<Vec<String>>,
    config_version: String, revision_id,
    evaluated_at,
}
```
There is no `evaluator` field: every verdict this gate produces is, by
construction, a deterministic rule-tree match, reproducible from
`matched_rule_path` alone — carrying an evaluator tag here would imply the
gate sometimes produces verdicts some other way, which it structurally
cannot. "Reproducible from `matched_rule_path` alone" needs two more things
to actually be true, though: which *version* of the ruleset produced this
match (a ruleset can be edited; `matched_rule_path` names rule positions
within whatever version was live, meaningless against a different one), and
what *page state* the underlying facts were read against (a `Rule(IntakeRule)`
match depends on mutable page/actor data, not just the ruleset). `config_version`
mirrors ADR-0026's `IntakeDecision.config_version`; `revision_id` is the same
field `IntakeItem` (ADR-0026 §3) already carries for the item the facts were
read from. Without both, a verdict can be *audited* (what fired, why) but not
*replayed* (would the same input, against the same ruleset version, produce
the same result) — a materially weaker guarantee than the ADR's stated
reproducibility claim. A verdict is written as a `ContentLifecycle` transition whose
`TransitionCause::GateVerdict` (ADR-0027 §2) carries the complete verdict
envelope — `{gate_id, ruleset_id, outcome_key, resolution_class, reasons,
matched_rule_path, config_version, revision_id}` — not just its routing key; eligibility does not own a
separate store, so the transition itself has to be the durable record, or
§2's `PriorOutcome` queries (on `resolution_class` or `outcome_key`) and any
later audit read would have nothing to read. Writing through ADR-0027's log
is what lets a future stochastic gate or a human's `ReviewerAction` be
referenced by this gate's own `PriorOutcome` without either side depending
on the other's types. The shape generalizes `CitationVerdict`'s precedent
(ADR-0007/0008: categorical, informational-first, no numeric confidence)
off a single closed enum onto the open key-plus-class shape of §3.

### 5. A ruleset is an ordered list of guarded outcomes, not one condition plus a separate outcome catalogue
```
EligibilityRuleset { id, outcomes: Vec<EligibilityOutcomeArm>, disallowed_reasons: Vec<String>, capability_required: String }
EligibilityOutcomeArm { key: String, resolution_class: ResolutionClass, when: EligibilityCondition, effects }
```
An earlier draft of this shape paired one top-level `condition` with a
separate `outcomes` catalogue — but `EligibilityCondition` only ever
resolves to `True | False | Unknown` (§2's three-valued base, inherited from
ADR-0026 §4), while a real ruleset routinely declares three or more named
outcomes (`clearly_fails` / `uncertain` / `sufficient`). Nothing in that
shape said *which* outcome a `True` result should select. Ordered guarded
arms close the gap directly: evaluation walks `outcomes` in declared order,
and the first arm whose `when` evaluates `True` produces the verdict — the
same "first match wins" discipline a decision list gives, not a novel
mechanism. An arm whose `when` evaluates `Unknown` is treated like `False`
for selection purposes and falls through to the next arm, the identical
"unknown can never itself cause a match" discipline ADR-0026 §4 already
established for admission; it never manufactures a match a confirmed fact
didn't earn. Every ruleset must declare a final, unconditional catch-all arm
(`when: All([])`, vacuously `True` under Kleene semantics — §2's `All` with
no children) so every input produces some outcome; §6 rejects a ruleset
missing one at load time, the same "no branch is statically unreachable /
every input has a defined destination" discipline ADR-0026 §7 already
applies to condition trees. `resolution_class: NeedsHuman` on the catch-all
arm is the common case (PROD: mechanical preconditions first, judgment call
falls through to a human), but is the ruleset author's choice, not fixed by
the mechanism — a fully mechanical binary ruleset can catch-all to a
`Terminal` "not eligible" outcome instead.

A recorded `Reason` matching `disallowed_reasons` is rejected at evaluation
time — the same fail-closed-and-loud discipline as ADR-0026 §6, extended
from a schema-resolution check to a policy-content check. This directly
encodes AFCSTANDARDS' real rule that certain decline reasons are simply not
valid, regardless of how plausible they read.

### 6. Configs are linted by extending ADR-0026's linter, not a second one
The xtask from ADR-0026 §7 gains: ruleset schema validation, a check that a
ruleset's last `outcomes` arm is the unconditional catch-all required by §5
(and that no earlier arm is itself statically `All([])`, which would make
every arm after it unreachable — the same "no branch is statically
unreachable" check ADR-0026 §7 already runs on a condition tree, extended to
an ordered arm list), `PriorOutcome` `gate_id` existence, `disallowed_reasons`
non-overlap with the ruleset's own `outcomes`, and non-empty
`capability_required`. When a `PriorOutcome`
specifies both `ruleset_id` and `outcome_key`, the linter additionally
checks that `outcome_key` is a member of that specific ruleset's declared
`outcomes` — an `outcome_key` that no version of the referenced ruleset
could ever produce is a config bug, not a rule that will just never match.
Load-time rejection of an unresolvable `PriorOutcome` `gate_id` or
`CapabilityRef`; an eval-time failure produces the same distinct
`Misconfigured` state ADR-0026 §6 defines — never silently collapsed into a
real outcome.

### 7. `NeedsHuman` hands off to `ReviewerAction`, never to a human-flavored verdict
A ruleset that mixes a machine-checkable precondition with a judgment call
that isn't (PROD's mechanical eligibility window feeding "is this
uncontroversial?") expresses the machine-checkable part as its condition and
resolves the rest to `resolution_class: NeedsHuman` — a verdict this gate
records like any other. The human's subsequent decision is **not** captured
as another shape of `EligibilityVerdict`; it is recorded on
`ContentLifecycle` as `TransitionCause::ReviewerAction` (ADR-0027 §2), a
distinct cause from `GateVerdict`. This keeps the boundary this whole
redesign exists to enforce: this gate's verdict type asserts "produced by
mechanical rule evaluation" unconditionally, so nothing downstream can
mistake a human's call, or a future stochastic gate's model-derived call,
for this gate's kind of verdict — they are different `TransitionCause`
variants on the same log, not different values of one evaluator field.
`PriorReviewerAction` (§2) is what a later ruleset reaches for to read that
human decision back — e.g. requiring "a human already confirmed notability
for this page" before skipping straight to a lighter follow-up check — the
same read-back `PriorOutcome` gives a machine verdict, on the matching
condition variant for a human one.

## Alternatives

- **One eligibility gate with an `evaluator: Automated | Human | Stochastic`
  field, rather than separate gate types.** Rejected — this was the
  original shape drafted before this revision. It lets a caller pattern-match
  incompletely and treat a model-derived judgment as if it carried the same
  reproducibility guarantee as a rule match, exactly the mistake the
  deterministic/stochastic split exists to make a type error instead of a
  runtime one. Separate contracts, composing only through `ContentLifecycle`
  data, close that gap.
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
  `tag-lifecycle`, a future stochastic eligibility gate) need the same
  record independent of this one, so it does not belong scoped to this
  contract.
- **Let a ruleset cite a disallowed reason silently (log-only).** Rejected —
  matches ADR-0026's "misconfiguration is never silent" precedent; a policy
  author citing a banned reason is a policy bug, not a warning-level event.
- **Filter `PriorOutcome` only by `resolution_class`, not a specific outcome
  key.** Rejected — multiple named outcomes commonly share one
  `resolution_class` (AfD's Keep, No consensus, and Merge can all be
  `Terminal` alongside Delete), so the ADR's own headline chaining example
  (G4: "a prior AfD gate resolved this page to *Delete* specifically") isn't
  expressible without an exact-match `outcome_key` predicate alongside it.
- **One top-level `condition` plus a separate `outcomes` catalogue,
  selecting an outcome by some other means (first-declared-wins ordering
  over the catalogue, a match-arm keyed by the condition's own reasons).**
  Rejected — this was the shape first drafted; nothing in it actually
  specified the selection mechanism, since `EligibilityCondition` resolves
  to a single three-valued result while `outcomes` needs to name several.
  Ordered guarded arms (§5) fold "does this apply" and "which outcome" into
  one declaration per arm, removing the undefined step entirely rather than
  bolting a selection rule onto the old shape.
- **Widen `ReviewerAction` (ADR-0027 §2) with gate-shaped fields
  (`gate_id`/`outcome_key`/`resolution_class`) so `PriorOutcome` can address
  it directly, instead of a separate `PriorReviewerAction` condition.**
  Rejected — a human's action isn't naturally shaped like a gate's verdict
  (it doesn't have a `ruleset_id` or a machine-legible `outcome_key`
  vocabulary), and forcing it to carry those fields blurs exactly the
  `GateVerdict`-vs-`ReviewerAction` boundary §7 exists to keep sharp.
  `PriorReviewerAction` filtering on `ReviewerAction`'s own native
  `action_kind` keeps both `TransitionCause` variants minimal and honest
  about what they actually record.
- **Leave `EligibilityVerdict` at `{..., matched_rule_path}` with no
  `config_version`/`revision_id`.** Rejected — `matched_rule_path` alone
  names which rules fired against whatever ruleset version and page state
  happened to be live at evaluation time; without recording which version
  and which revision, a verdict can be audited but not replayed, a
  materially weaker guarantee than this ADR claims for it.

## Consequences

- Every fully machine-checkable policy ruleset from the research
  (`npp-quickfail`'s mechanical preconditions, `gng-sng`'s structural facts,
  `redirect-csd`, `r-delete`, `prod-standard`, `prod-llm`, G13, G5) becomes
  expressible in one schema without a new outcome enum per ruleset, and every
  verdict this gate produces is replayable from its `matched_rule_path`,
  `config_version`, and `revision_id` together — a concrete near-term
  reviewer-workload payoff distinct from the mechanism-ahead-of-adoption
  pattern ADR-0026/ADR-0027 otherwise share.
- Rulesets needing genuine judgment (G1–G3, GNG's "significant"/"reliable",
  AfC's substantive review) resolve to `NeedsHuman` and stop here — this gate
  does not attempt them, and does not grow an escape hatch to attempt them
  later; a stochastic gate or a human reviewer picks up from the recorded
  `ReviewerAction`/future `GateVerdict`, read back in only through
  `ContentLifecycle`, never through this gate's types.
- **Hard-depends on ADR-0027**: `PriorOutcome` resolution and verdict
  recording both read and write through `ContentLifecycle`; this is not
  sequencing convenience.
- Establishes the composition pattern a future stochastic eligibility gate
  must also follow: its own contract, its own verdict type, crossing into
  this gate's condition tree (or vice versa) only via `PriorOutcome` against
  the shared `ContentLifecycle` log.
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
- **A stochastic eligibility gate** (model/ML-derived admission judgments,
  e.g. an LLM-assisted GNG "significant coverage" call) — a separate future
  ADR. It needs its own grounding/anti-fabrication discipline, in the
  lineage of ADR-0007's citation-verification work, before it can be
  designed; it is deliberately not an `evaluator` variant bolted onto this
  contract.
- **Scoring's internal deterministic/ORES/LLM signal mix** (ADR-0021) — a
  separate future refactor of that ADR. Scoring produces a ranking signal,
  not an admission decision, and is untouched by this one.
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
