# ADR-0027: Content lifecycle contract — canonical state, append-only transition log, transition-triggered re-entry

**Status:** Proposed
**Date:** 2026-08-24
**Author:** Christophe Henner (drafted by Claude Code)
**Summary:** Every reviewable item gets one durable, cross-domain lifecycle record — a current state plus an append-only transition log — that any gate can query for a prior verdict and any workflow can watch for a specific transition to automatically re-trigger, replacing the hardcoded, patrol-only state that `LiveOperatorView` improvises today.

## Context

The eligibility-gate design (see ADR-0028, drafted alongside this ADR) needs two
things no existing SP42 mechanism provides: a durable record of what a
`ReviewableItem` *is* right now, and a way for a rule to reference a *prior*
gate's outcome. Both requirements come directly from Wikipedia policy, not
speculation:

- WP:CSD's **G4** ("recreation after deletion") is only meaningful if
  something can ask "did a deletion-discussion gate previously resolve this
  page to Delete?" — a query against recorded history, not the current item.
- WP:NPP documents that converting a reviewed redirect into an article
  **force-reopens it as unreviewed**, specifically to stop reviewers from
  hijacking a reviewed redirect to sneak in an unreviewed article. That is an
  engine reacting to a state *transition*, not a workflow step deciding
  something about the current item.
- AfC's "Approved" outcome should enter NPP pre-reviewed, not re-enter as a
  fresh unreviewed candidate — another case of one domain's transition
  needing to be visible to another domain's queue.

Today the closest thing to a lifecycle record is `LiveOperatorView`
(`sp42-patrol`) — a single hand-assembled struct with one implicit state,
decided entirely by patrol's own code, with no queryable history and no
re-trigger mechanism. That is exactly the pattern ADR-0013's platform/domain
split exists to correct: a second domain (AfC, AfD, CCI) would clearly reuse
"what state is this item in, and how did it get there" by design, so this is
a platform contract, and — because ADR-0028's `PriorOutcome` condition and
verdict recording both read and write through it — a **hard dependency** of
that ADR, not a parallel concern.

## Decision

### 1. `ContentLifecycleRecord`, keyed by item identity
```
ContentLifecycleRecord { item_id: ReviewableItemId, wiki_id, state: LifecycleState, history: Vec<LifecycleTransition>, updated_at: Timestamp }
LifecycleState { key: String, disposition: StateDisposition }
StateDisposition = Active | Terminal
```
`ReviewableItemId` is referenced, not designed, here — it is the generic
queueable-unit identity sketched informally in earlier design discussion
(page \| draft \| nomination \| investigation case) and belongs to its own
future ADR (Non-goals). `LifecycleState` follows ADR-0026's typed-but-open
pattern: `key` is domain-owned vocabulary (`"reviewed"`, `"draft"`,
`"merged"`, `"redirected"`, `"deleted"`, `"stub"`, …), never a platform enum
edited per new domain lane; `disposition` is the one thing the engine needs
to reason about generically — whether the item is still under active review
or has reached a terminal state.

### 2. History is append-only; current state is a derived cache, not a separate write
```
LifecycleTransition {
    from: Option<LifecycleState>, to: LifecycleState,
    caused_by: TransitionCause,
    occurred_at: Timestamp,   // resolved against Clock::now(), never wall-clock — same discipline as ADR-0026 §4
}
TransitionCause = GateVerdict { gate_id: String, ruleset_id: String, outcome_key: String, resolution_class: ResolutionClass, reasons: Vec<Reason>, matched_rule_path: Option<Vec<String>> }
                 | ReviewerAction { actor: String, action_kind: String }
                 | SystemReentry { reason: String }
ResolutionClass = Terminal | NeedsHuman | Relist | RouteElsewhere
```
`state` always equals `history.last().to`; the log is the source of truth,
and `state` exists only so a hot-path read doesn't need to replay history. A
transition is recorded, never overwritten — this is what makes §4's queries
meaningful and is the direct fix for `LiveOperatorView`'s no-history gap.
`GateVerdict` carries the complete verdict envelope, not just its routing
key: without `resolution_class` in the transition itself, a `PriorOutcome`-
style query (§4) could never filter on resolution class without a second
store, and without `reasons`/`matched_rule_path` the transition log stops
being the audit trail it otherwise claims to be. `ResolutionClass` is
defined here, once, rather than per gate type — it's the one thing the
engine needs to branch on generically across every verdict-producing gate,
the same reasoning that keeps `LifecycleState.key` open but `disposition`
closed (§1). A gate-type contract (e.g. a deterministic eligibility gate)
reuses it verbatim, exactly as it would reuse `IntakeField`/`Op`/`Value`
from ADR-0026 — one vocabulary, not a parallel one per gate type. `Reason`
is referenced, not defined, here — its shape belongs to whichever gate-type
contract produces it.

### 3. Transition-triggered re-entry is a registered watch, not a bespoke callback
```
LifecycleWatch { from: LifecycleState, to: LifecycleState, re_trigger: WorkflowId }
```
When a recorded transition matches a registered watch, the engine enqueues
the item into the target workflow's **intake** (ADR-0026) rather than each
domain reimplementing its own polling or callback. Watches are config, not
code — the same "policy in files, mechanics in code" discipline as ADR-0021
§2 and ADR-0026 §2. This is the literal mechanism the WP:NPP
redirect-reopening rule confirmed in research needs: `{ from: redirect, to:
article, re_trigger: npp }`.

### 4. A query surface, not just a write path — the read side ADR-0028 needs
```
fn transitions_matching(item_id: ReviewableItemId, predicate: TransitionPredicate) -> Vec<LifecycleTransition>
```
This is the concrete dependency edge: ADR-0028's `PriorOutcome` condition
resolves by calling this query (e.g., G4: "does this item's most recent
`GateVerdict` from a deletion-discussion gate have `outcome_key == delete`?").
Without this surface existing, gate chaining cannot be expressed at all.

### 5. Platform-homed, alongside intake, scoring_engine, queue_builder
Lives in `sp42-platform` (engine) / `sp42-types` (contracts), the same
footing as `scoring_engine`, `queue_builder`, and intake (ADR-0026). As with
those, today's only concrete consumer is forward-looking (ADR-0028) — the
mechanism earns platform placement under the reuse-by-design test regardless
(`docs/README.md`), the same "mechanism ahead of adoption" pattern ADR-0021
and ADR-0026 already established.

## Alternatives

- **Bespoke per-domain state field, `LiveOperatorView`-style.** Rejected —
  doesn't generalize, no cross-domain query, no re-trigger mechanism; exactly
  the pattern ADR-0013's platform/domain extraction exists to correct.
- **General-purpose event sourcing of every action, score, and diff.**
  Rejected as premature and over-broad — scope is deliberately narrowed to
  state *transitions* relevant to gate outcomes and reviewer actions, not a
  general audit/event store. (Recorded as a Non-goal, not silently dropped.)
- **A closed global `LifecycleState` enum.** Rejected for the same reason
  ADR-0026 kept `IntakeField` open via `Custom(String)` — NPP/AfC/AfD/CCI
  states shouldn't require editing a platform type per new domain lane.
- **Each gate polls for its own reopening condition instead of a registered
  watch.** Rejected — this reproduces the fragile, hardcoded-in-one-place
  pattern this ADR exists to leave, and doesn't scale to N gates watching M
  transitions.
- **Store current state only, no history.** Rejected — gate chaining (G4)
  requires querying *past* recorded outcomes, not just the current state;
  this would also foreclose audit-trail needs later domains will have.
- **Keep `GateVerdict` to just `{gate_id, ruleset_id, outcome_key}` and
  require a separate verdict store for resolution-class and reasons
  detail.** Rejected — reproduces exactly the "separate store" a
  gate-type contract explicitly avoids by writing through this log, and
  leaves resolution-class-filtered `PriorOutcome` queries unimplementable
  against the log alone, since the data they'd need to filter on
  wouldn't exist there.

## Consequences

- Enables ADR-0028 (deterministic eligibility gate contract) to exist at all: verdict
  recording and gate-chaining reads both depend on this contract, not merely
  benefit from it.
- Resolves the redirect-reopening case (and its general shape,
  transition-triggered re-entry into any workflow) as a config-driven
  platform mechanism instead of a bespoke patrol-only rule.
- Nothing is implemented yet — types, the transition-recording engine, the
  watch registry, and its query surface all still need to be built, same
  Proposed-stage status as ADR-0026.
- The query surface (§4) must be fast enough for hot-path gate evaluation —
  a bounded lookup per item, not a full history scan on every eligibility
  call. This is flagged as an implementation constraint the eventual engine
  must satisfy, not solved by this contract.
- `GateVerdict`'s widened payload (`reasons`, `matched_rule_path`) makes
  transitions larger than the minimal routing-key shape first drafted —
  acceptable at eligibility's scale (one verdict per ruleset evaluation, not
  per edit event), but a size/storage consideration the eventual engine
  should keep in view as more gate types write through this log.
- Watch configs get the same fail-closed misconfiguration discipline as
  intake (ADR-0026 §6/§7) — an unresolvable `re_trigger` workflow id is
  rejected at load time, extending that linter rather than inventing a
  second one.

## Non-goals

- `ReviewableItem` itself (the generic queueable/rankable unit) — referenced
  here by name only; its own contract is a separate future ADR.
- The specific state vocabulary any one domain uses (what NPP/AfC/AfD/CCI
  actually call their states) — domain policy, not this contract.
- Eligibility ruleset and verdict shape — ADR-0028.
- General-purpose event sourcing or a full action/audit log — deliberately
  out of scope; see Alternatives.
- The discussion/consensus contract (AfD participants, !votes, closing
  rationale, relist quorum) — a separate future platform capacity.
