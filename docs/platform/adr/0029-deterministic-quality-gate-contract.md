# ADR-0029: Deterministic quality gate contract — promotion verdicts, shared resolution vocabulary with the eligibility gate, per-project ruleset optionality

**Status:** Proposed
**Date:** 2026-08-24
**Author:** Christophe Henner (drafted by Claude Code)
**Summary:** The deterministic quality gate is a platform gate type — at the same altitude as the eligibility gate (ADR-0028) but structurally separate from it, and from a future stochastic quality gate — that evaluates a `ReviewableItem`'s candidacy for a promotion tier (DYK/GA/FA-equivalent) against a named ruleset composed from intake's condition tree plus ADR-0028's `PriorOutcome` primitive, reusing ADR-0027's `ResolutionClass` verbatim rather than defining a parallel vocabulary, producing a reproducible, capability-gated, per-ruleset-keyed verdict recorded on `ContentLifecycle`, with every quality tier a per-project opt-in rather than an assumed universal.

## Context

The workflow diagram behind this design labels two visually distinct node
types: `[eligibility gate]` (Speedy deletion, PROD, AfD, NPP, AfC, AI cleanup
noticeboard, CCI) and `[quality gate]` (DYK, GA, FA review). Both shapes are
"a ruleset id in, a verdict-plus-reasons out" — the same evaluation shape
ADR-0028 already contracts — but they answer structurally different
questions: eligibility asks *should this exist, be removed, or be
reclassified* (a retention risk), quality asks *does this clear a bar worth
recognizing* (a promotion). Collapsing them into one gate type differentiated
only by ruleset content would make it possible to mistake a promotion
verdict for a retention verdict (or read gate-chaining logic that quietly
does), the same category of mistake the deterministic/eligibility-vs-
stochastic-eligibility split exists to make a type error rather than a
runtime one. Per that same reasoning, extended one level further: quality
gets its own contract, not a flag on eligibility's.

Unlike ADR-0028, this ADR is not backed by the same primary-policy research
pass (WP:CSD/PROD/AfD/GNG/AfC were each read against their actual policy
pages before that contract was drafted). WP:DYK/WP:GAN/WP:FAC's general
shape is well known — DYK gates on novelty/length/sourcing within a fixed
window, GA is a single-reviewer checklist assessment with a "hold" state,
FA is a coordinator-driven, discussion-heavy promotion — but that shape has
not been verified against primary policy text with the same rigor. The
ruleset examples below are illustrative of the *contract's* shape, not a
policy-verified ruleset ready to configure (see Non-goals).

**Multi-project fit is a first-class constraint here, more than it was for
eligibility.** Most Wikimedia content projects have some rough equivalent of
"should this be deleted" (CSD/PROD/AfD-shaped processes exist, in some form,
on nearly every sizeable wiki). Promotion tiers are far less universal: many
language editions have no FA-equivalent process at all, a lighter or
nonexistent GA-equivalent, and DYK-equivalents vary widely in criteria where
they exist. Non-article projects complicate this further — Commons has its
own Featured Picture/Quality Image processes, structurally similar but not
DYK/GA/FA-shaped; Wikidata and Wiktionary have no direct analogue at all. A
quality gate contract that assumed every project configures all three tiers,
or that "quality tiers" mean the same thing project to project, would fail
the same reuse-by-design test ADR-0026 §5's wiki-relative capability
resolution exists to pass. The mechanism must work correctly for a project
that configures zero quality rulesets, exactly as well as for one that
configures many.

## Decision

### 1. The quality gate is its own gate type, same altitude as eligibility, structurally separate from a future stochastic quality gate, homed in `sp42-platform` / `sp42-types`
Like the deterministic eligibility gate (ADR-0028 §1), this gate runs only on
items intake has already admitted, always requires a capability, and always
records a judgment derived mechanically from its condition tree — nothing
here calls a model or asks a human to fill in an `evaluator` slot. Its types
live under a name that makes the boundary explicit at compile time (e.g.
`quality::deterministic`), sibling to `eligibility::deterministic`, not
nested under it — a `QualityVerdict` and an `EligibilityVerdict` must never
be interchangeable at the type level, the same discipline ADR-0028 §1
established for deterministic vs. stochastic eligibility, now drawn a second
time between eligibility and quality.

### 2. Rulesets compose intake's condition tree and reuse `PriorOutcome`/`PriorReviewerAction` verbatim — including across gate types
```
QualityCondition = All(Vec<QualityCondition>) | Any(Vec<QualityCondition>) | Not(Box<QualityCondition>)
                  | Rule(IntakeRule)
                  | PriorOutcome { gate_id: String, ruleset_id: Option<String>, outcome_key: Option<String>, resolution_class: Option<ResolutionClass>, within: Option<Duration> }
                  | PriorReviewerAction { actor: Option<String>, action_kind: Option<String>, within: Option<Duration> }
```
This is the same `PriorOutcome`/`PriorReviewerAction` pair ADR-0028 §2
defines, not a quality-flavored reimplementation — both were already generic
over `gate_id`/`ReviewerAction`'s own fields and read a `LifecycleTransition`,
never a gate-specific verdict type (ADR-0028 §2), so neither needs changes
to serve a second gate type; a GA/FA coordinator's contested-promotion call
(§7) is exactly as addressable via `PriorReviewerAction` as a human
eligibility decision is. Two
directions of chaining follow directly from that genericity, both real:
a GA reassessment ruleset can require "was this article previously promoted
to GA" (`PriorOutcome` against this gate's own prior verdicts — GA
promotion, delisting, and FA promotion can all share `Terminal`, so an
exact-match `outcome_key` is what distinguishes "still holds GA status" from
"already delisted," the identical reasoning ADR-0028 §2 gives for G4); and
an eligibility ruleset can require "is this a Featured Article" before
allowing a lighter deletion path, or a quality ruleset can require "was this
kept, not merged, at a prior AfD" before considering DYK/GA candidacy —
`gate_id` crossing from a quality ruleset to an eligibility gate's recorded
verdicts, or vice versa, with neither gate's Rust types imported by the
other. Per ADR-0027 §4/ADR-0028 §2, `PriorOutcome` reads only the most
recent matching transition for a `gate_id`/`ruleset_id` lineage, never any
historical occurrence — a GA `promoted` transition later superseded by a
`delisted` one must read as delisted, not as still-promoted because
`promoted` occurred at some point in the item's history. This is the
concrete case that answers why the two stayed separated-
but-composable rather than merged: the composition the platform vision
requires is exactly this cross-gate-type `PriorOutcome` reference, and it
only works cleanly because both write through the same `ContentLifecycle`
log in the same shape.

### 3. Outcomes are open per ruleset; `resolution_class` is ADR-0027's vocabulary, reused verbatim — not redefined
```
QualityOutcome { key: String, resolution_class: ResolutionClass }
```
No new `ResolutionClass` is introduced. `Terminal | NeedsHuman | Relist |
RouteElsewhere` (ADR-0027 §2) covers the quality gate's actual outcome
shapes without a fifth value: a DYK approval or rejection is `Terminal`; a
GA review's "on hold" (revise and re-check) is `Relist` — the same
re-run-later semantics as AfD's relist, not a new concept; a GA/FA promotion
contested enough to need a coordinator's judgment is `NeedsHuman`; and FA's
common precondition — "not yet promoted GA, or recommended through peer
review first" — is a genuine `RouteElsewhere`, the same class eligibility
uses for e.g. AfD routing to a merge discussion. Centralizing
`ResolutionClass` in ADR-0027 rather than letting each gate type define its
own is precisely what makes this reuse possible without drift: a second gate
type needing a class eligibility's four don't cover would be a real signal
to revisit the vocabulary, and none of DYK/GA/FA's shapes are that signal.
The ruleset still owns its own `key` vocabulary (`"approved"` /
`"failed-hook"` / `"failed-length"` for DYK; `"promoted"` / `"delisted"` /
`"on-hold"` for GA/FA) — open per ruleset, never a platform-type edit, the
same closed-enum seam ADR-0021 §5 and ADR-0028 §3 already reject.

### 4. Verdicts are always recorded through `ContentLifecycle`; a verdict from this gate is always machine-produced
```
QualityVerdict {
    ruleset_id, gate_id, item_id,
    outcome: { key, resolution_class },
    reasons: Vec<Reason>, matched_rule_path: Option<Vec<String>>,
    config_version: String, revision_id,
    evaluated_at,
}
```
Structurally identical in shape to `EligibilityVerdict` (ADR-0028 §4) —
deliberately so, since both write through the same widened
`TransitionCause::GateVerdict` (ADR-0027 §2) — but a distinct Rust type for
the same reason `EligibilityVerdict` and a future stochastic verdict must
stay distinct: nothing here carries an `evaluator` field, since this gate,
like ADR-0028's, cannot structurally produce a verdict any way other than a
reproducible rule-tree match. `config_version` and `revision_id` carry the
same replay-vs-audit distinction ADR-0028 §4 draws: `matched_rule_path`
alone names which rules fired, not which ruleset version was live or what
page state the facts were read against, so both are needed for a verdict to
actually be reproducible rather than merely traceable. Recording through `ContentLifecycle` rather
than a separate store is what makes §2's cross-gate `PriorOutcome` reads
correct in the first place — the same "eligibility does not own a separate
store" argument (ADR-0028 §4) applies here without modification.

### 5. A ruleset is an ordered list of guarded outcomes, not one condition plus a separate outcome catalogue
```
QualityRuleset { id, outcomes: Vec<QualityOutcomeArm>, disallowed_reasons: Vec<String>, capability_required: String }
QualityOutcomeArm { key: String, resolution_class: ResolutionClass, when: QualityCondition, effects }
```
Same shape and rationale as `EligibilityRuleset`/`EligibilityOutcomeArm`
(ADR-0028 §5): a single top-level `condition` only ever resolves to `True |
False | Unknown`, which cannot itself select among three or more named
outcomes (DYK's `approved`/`failed-hook`/`failed-length`; GA's
`promoted`/`delisted`/`on-hold`). Evaluation walks `outcomes` in order; the
first arm whose `when` evaluates `True` wins, `Unknown` falls through like
`False` (never manufactures a match), and a mandatory trailing catch-all arm
(`when: All([])`) guarantees every input produces some outcome — §6 rejects
a ruleset missing one, mirroring ADR-0028 §6's identical check. A GA/FA
ruleset's catch-all commonly resolves `NeedsHuman` (§7's "more of these
terminate at NeedsHuman than eligibility's" observation is exactly what a
judgment-heavy catch-all arm encodes), but is again the ruleset author's
choice, not fixed by the mechanism.

A recorded `Reason` matching `disallowed_reasons` is rejected at evaluation
time, the same as ADR-0028 §5: a GA/FA
review process that forbids certain rejection rationales (e.g. a
formatting-only complaint blocking an otherwise-passing review, mirroring
AFCSTANDARDS' disallowed-reasons precedent) expresses that as
`disallowed_reasons`, rejected at evaluation time — fail-closed, not
log-only, matching ADR-0026 §6 and ADR-0028 §5's precedent.

### 6. Configs are linted by extending the same linter — a third gate type, not a third linter
The xtask from ADR-0026 §7, already extended by ADR-0028 §6, gains the
identical validations for quality rulesets: schema conformance, a check that
a ruleset's last `outcomes` arm is the unconditional catch-all required by §5
(and that no earlier arm is itself statically `All([])`, which would make
every arm after it unreachable — the same check ADR-0028 §6 runs on
`EligibilityOutcomeArm`, extended here to `QualityOutcomeArm`),
`PriorOutcome` `gate_id` existence (now checked against *both* eligibility
and quality gate ids, since chaining crosses gate types per §2), `outcome_key`
membership validated against the specific referenced ruleset when both
`ruleset_id` and `outcome_key` are set, `disallowed_reasons` non-overlap with
the ruleset's own `outcomes`, and non-empty `capability_required`. One linter,
now serving two gate-type contracts plus intake, not a proliferating set of
near-identical checkers.

### 7. `NeedsHuman` hands off to `ReviewerAction`, never to a human-flavored verdict
Identical to ADR-0028 §7: a GA review's coordinator call on a contested
promotion, or an FA close, is recorded as `TransitionCause::ReviewerAction`
(ADR-0027 §2), never as another shape of `QualityVerdict`. This boundary
likely matters *more* here than for eligibility — GA/FA promotion is more
consistently discussion- and judgment-driven than eligibility's more
mechanical CSD/PROD criteria — so a larger fraction of real quality-gate
evaluations are expected to terminate at `NeedsHuman` and hand off, not
resolve `Terminal` unattended. That is the gate working as designed, not an
implementation gap.

### 8. A quality ruleset is a per-project opt-in; its absence is not a misconfiguration
A project registering zero quality rulesets — no DYK-equivalent, no
GA-equivalent, no FA-equivalent — is a fully valid, expected state, not a
degraded one. This gate type ships as a platform mechanism the same way
intake does (ADR-0026 §1/§2), but unlike intake's always-on baseline, it has
no embedded default ruleset and runs nothing until a project explicitly
configures one: there is no "safe broad" quality bar to ship in code, since
what counts as a promotion tier is entirely project-defined policy. This is
the direct answer to this ADR's multi-project constraint from Context: the
mechanism must be indifferent to whether a project configures one tier,
three tiers structured like DYK/GA/FA, or an entirely different tier
structure (Commons' Featured Picture/Quality Image shape). Nothing in §1–§7
assumes English Wikipedia's specific three-tier structure — `gate_id` and
`ruleset_id` are opaque, project-chosen strings throughout, following
ADR-0026 §5's wiki-relative discipline.

## Alternatives

- **One gate type for both eligibility and quality, differentiated only by
  ruleset content.** Rejected — reproduces the exact mistake the
  deterministic/stochastic eligibility split (ADR-0028 §1) exists to
  prevent, one level up: a caller could consume a promotion verdict as if it
  carried retention-risk meaning, or chain `PriorOutcome` across the two
  without the type system flagging that the semantics differ. Separate
  contracts, composed only through `ContentLifecycle` `PriorOutcome` reads
  (§2), keep that a compile-time distinction.
- **Define a quality-specific `ResolutionClass` rather than reusing
  ADR-0027's.** Rejected — directly the vocabulary-drift risk raised in this
  ADR's design discussion; DYK/GA/FA's actual outcome shapes (approve/reject,
  on-hold, contested-needs-human, precondition-routing) map cleanly onto the
  existing four values, and a second vocabulary for the same engine-branching
  purpose defeats why `ResolutionClass` was centralized in ADR-0027 in the
  first place.
- **Hardcode DYK/GA/FA as platform-known ruleset ids or an enum.** Rejected —
  ties the mechanism to English Wikipedia's specific tier names; a project
  with no FA-equivalent, a differently-structured tier system, or (Commons,
  Wikidata) no article-quality concept at all would have no way to express
  that without a platform-type edit — the same closed-enum seam ADR-0021 §5
  and ADR-0028 §3 already reject, now for tier identity rather than outcome
  vocabulary.
- **Require every project to register all quality tiers before the gate is
  usable.** Rejected — most language editions lack a meaningful FA-equivalent
  process, some lack GA or DYK equivalents too; forcing registration would
  either block smaller projects from any quality-gate use or push them toward
  authoring hollow rulesets purely to satisfy the mechanism. Zero-or-more
  registration (§8) is the correct default, extending ADR-0026 §2's "safe
  fallback in code, tuning in policy, no forced adoption" posture to "no
  forced tier structure" as well.
- **A quality gate that reads eligibility's `PriorOutcome` but is not itself
  readable the same way (one-directional composition).** Rejected — nothing
  in ADR-0027's `transitions_matching` query or ADR-0028's `PriorOutcome`
  design privileges a direction; an artificial one-way restriction would
  block real cases (eligibility wanting to know "is this a Featured Article"
  before applying a lighter deletion path) for no benefit, and would need its
  own justification this ADR has no grounds to supply.
- **One top-level `condition` plus a separate `outcomes` catalogue for
  `QualityRuleset`, the shape this ADR originally drafted.** Rejected for the
  identical reason ADR-0028 §5/Alternatives rejected it for
  `EligibilityRuleset`: `QualityCondition` only ever resolves to a single
  three-valued result, while a real quality ruleset routinely needs three or
  more named outcomes (DYK's approve/failed-hook/failed-length; GA's
  promoted/delisted/on-hold), and nothing in the old shape specified which
  outcome a `True` result should select. The ordered `QualityOutcomeArm` list
  (§5) is the same fix, not a quality-specific variant of it.
- **Widen `ReviewerAction` with gate-shaped fields instead of a separate
  `PriorReviewerAction` condition.** Rejected — reuses ADR-0028
  §2/Alternatives' reasoning without modification: a human's action isn't
  naturally shaped like a gate's verdict, and this gate composes with
  `PriorReviewerAction` as-is (§2) rather than needing its own variant, so
  there is even less reason here to blur the `GateVerdict`/`ReviewerAction`
  boundary than there was for eligibility.
- **Leave `QualityVerdict` at `{..., matched_rule_path}` with no
  `config_version`/`revision_id`.** Rejected — same gap ADR-0028
  §4/Alternatives identifies for `EligibilityVerdict`: `matched_rule_path`
  alone names which rules fired, not which ruleset version or page state
  produced the match, so a verdict without both fields could be audited but
  not replayed.

## Consequences

- Establishes a second, structurally distinct gate-type family on the same
  foundation (ADR-0026's condition tree, ADR-0027's `ContentLifecycle` and
  `ResolutionClass`, ADR-0028's `PriorOutcome` primitive) without touching or
  forking any of them — a concrete proof that the platform layer generalizes
  across "kinds of admission decision," not just across wikis running the
  same kind.
- **Hard-depends on ADR-0027** exactly as ADR-0028 does (verdict recording
  and `PriorOutcome` resolution both read/write through `ContentLifecycle`),
  and **depends on ADR-0028 only for the shape of `PriorOutcome` it reuses**,
  not for any of ADR-0028's eligibility-specific types — the two gate types
  are siblings composed through `ContentLifecycle` data, never through each
  other's Rust types.
- A project configuring no quality rulesets pays no cost and needs no
  workaround (§8) — this is the concrete test of the multi-project
  constraint this ADR was drafted under, and it is a design property, not an
  incidental one.
- `Reason`'s shape remains undefined here, same as ADR-0027/ADR-0028 leave
  it — this ADR inherits, rather than resolves, the open question of how a
  free-text (and therefore language- and project-specific) reason payload
  should be structured so it stays meaningful across projects using
  different languages for the same `resolution_class`. Worth flagging
  explicitly given quality/eligibility reasons will be authored in many
  languages across many projects: whichever future ADR finally shapes
  `Reason` should treat localization as a starting constraint, not an
  afterthought.
- Establishes the composition pattern a future stochastic quality gate
  (LLM-assessed prose quality, comprehensiveness judgment for GA/FA-shaped
  review) must also follow: its own contract, its own verdict type, crossing
  into this gate's or eligibility's condition trees only via `PriorOutcome`
  against the shared `ContentLifecycle` log.
- Nothing is implemented yet — ruleset types, the linter extension, and the
  per-ruleset outcome registry all still need to be built, same
  Proposed-stage status as ADR-0026/ADR-0027/ADR-0028.

## Non-goals

- `ContentLifecycle`'s own state/transition/watch mechanism, and
  `ResolutionClass`'s definition — ADR-0027; reused verbatim here.
- Intake's rule engine (`IntakeCondition`/`Field`/`Op`/`Value`, `CapabilityRef`
  resolution) — ADR-0026; reused verbatim here.
- The deterministic eligibility gate's own contract and its
  `EligibilityVerdict`/`EligibilityRuleset` types — ADR-0028; a sibling this
  ADR composes with through `ContentLifecycle`, not a dependency of this
  ADR's own types.
- **A stochastic quality gate** (model-assessed prose quality,
  comprehensiveness, or "significant coverage"-style judgment for GA/FA-shaped
  review) — a separate future ADR, the same deferral ADR-0028 already made
  for a stochastic eligibility gate, and needing the same
  grounding/anti-fabrication discipline before it can be designed.
- **A primary-policy research pass on WP:DYK/WP:GAN/WP:FAC** (and their
  equivalents, where they exist, on other projects) with the rigor ADR-0028
  §Context gave WP:CSD/PROD/AfD/GNG/AfC — this ADR's examples are
  illustrative of the contract's shape, not a policy-verified ruleset; that
  research is future work before any concrete quality ruleset config ships.
- Concrete per-project quality-tier definitions, including whether a given
  project has any promotion tiers at all, how many, and what they're called
  — policy/config, not architecture, per §8.
- Non-article-project quality concepts (Commons' Featured
  Picture/Quality Image, or any Wikidata/Wiktionary equivalent) — whether and
  how they map onto this contract is a future design question, not resolved
  here; §8 only establishes that the mechanism must not assume they don't
  exist or must look like DYK/GA/FA.
- The reviewer capability model's internals (what `capability_required`
  actually checks against) — ADR-0002/ADR-0014's existing identity/session
  model is referenced, not re-decided.
