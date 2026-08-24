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

### 1. `ContentLifecycleRecord`, keyed by item identity and scoped by track
```
ContentLifecycleRecord { item_id: ReviewableItemId, wiki_id, states: HashMap<String, LifecycleState>, history: Vec<LifecycleTransition>, updated_at: Timestamp }
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

`states` is keyed by **track** — a domain-owned string identifying the
workflow lane the state belongs to (`"npp"`, `"afd"`, `"cci"`, …) — rather
than the record holding one global `state`. NPP, AfD, and CCI can all be
concurrently active on the same page (a page under AfD discussion can
simultaneously be mid-CCI-investigation), and each track's transitions are
its own domain's business: AfD resolving to `nominated-for-deletion` must
not silently overwrite what NPP's own track last recorded, or vice versa. A
single global `state` field cannot be canonical across concurrent domains
without one of them winning arbitrarily; a per-track map is. A domain reads
and writes only its own track's entry; cross-track questions ("is this page
also under CCI investigation right now") are answered by looking up that
other track's entry, not by a domain having ever owned the global field.
Each domain registers the `LifecycleState.key` vocabulary valid for the
track(s) it owns — the same per-domain registry pattern ADR-0026 §4 uses
for `Custom(String)` fields — which is what §3's watch validation checks
against.

### 2. History is append-only; current per-track state is a derived cache, not a separate write
```
LifecycleTransition {
    track: String,
    from: Option<LifecycleState>, to: LifecycleState,
    caused_by: TransitionCause,
    occurred_at: Timestamp,   // resolved against Clock::now(), never wall-clock — same discipline as ADR-0026 §4
}
TransitionCause = GateVerdict { gate_id: String, ruleset_id: String, outcome_key: String, resolution_class: ResolutionClass, reasons: Vec<Reason>, matched_rule_path: Option<Vec<String>>, config_version: String, revision_id }
                 | ReviewerAction { actor: String, action_kind: String }
                 | SystemReentry { reason: String }
ResolutionClass = Terminal | NeedsHuman | Relist | RouteElsewhere
```
`states[track]` always equals the `to` of that track's most recent entry in
`history`; the log is the source of truth, and the per-track cache exists
only so a hot-path read doesn't need to replay history filtered by track. A
transition is recorded, never overwritten — this is what makes §4's queries
meaningful and is the direct fix for `LiveOperatorView`'s no-history gap.
`GateVerdict` carries the complete verdict envelope, not just its routing
key: without `resolution_class` in the transition itself, a `PriorOutcome`-
style query (§4) could never filter on resolution class without a second
store, and without `reasons`/`matched_rule_path` the transition log stops
being the audit trail it otherwise claims to be. `config_version` and
`revision_id` are what let a gate-type contract's verdict (ADR-0028 §4,
ADR-0029 §4) actually be replayed later: `matched_rule_path` alone names
*which* rules fired, not *which version of the ruleset* was live or *what
page state* the facts were read against, and without both a verdict that
depended on a since-changed ruleset or a since-edited page cannot be
reproduced from the transition log alone — the log would assert
reproducibility it can't deliver. `ResolutionClass` is
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
LifecycleWatch { track: String, from: LifecycleState, to: LifecycleState, re_trigger: WorkflowId }
```
When a recorded transition matches a registered watch (same `track`, same
`from`/`to`), the engine enqueues
the item into the target workflow's **intake** (ADR-0026) rather than each
domain reimplementing its own polling or callback. Watches are config, not
code — the same "policy in files, mechanics in code" discipline as ADR-0021
§2 and ADR-0026 §2. This is the literal mechanism the WP:NPP
redirect-reopening rule confirmed in research needs: `{ track: npp, from:
redirect, to: article, re_trigger: npp }`.

`LifecycleState.key` is deliberately open (§1) so a domain never needs a
platform-type edit to add a state, but that openness means a typo'd `from`
or `to` in a watch config loads successfully and simply never fires — no
transition will ever match a key nothing produces, and nothing signals that
the watch is dead. The linter extending ADR-0026 §7 (already invoked below
for `re_trigger`) closes this the same way it closes an unresolvable
`Custom(String)` field: every watch's `track` must be a registered domain
track, and every `from`/`to` key must be a member of that track's own
registered `LifecycleState.key` vocabulary (§1) — a watch referencing an
unregistered key is rejected at load/CI time, not left to silently never
trigger.

### 4. A query surface, not just a write path — the read side ADR-0028 needs
```
fn transitions_matching(item_id: ReviewableItemId, predicate: TransitionPredicate) -> Vec<LifecycleTransition>
```
Results are ordered **most-recent-first** (`occurred_at` descending) — this
is load-bearing, not incidental: a caller asking "does this item's most
recent `GateVerdict` from a deletion-discussion gate have `outcome_key ==
delete`?" (G4) means the *current* standing of that gate/ruleset lineage,
not "has this outcome_key ever occurred." Without a defined order, a caller
could match against a stale entry — e.g. a page `promoted` to GA at some
point in the past and later `delisted` still reads as "promoted" if the
query doesn't distinguish "occurred" from "occurred most recently and
hasn't been superseded." This is the concrete dependency edge: ADR-0028's
`PriorOutcome` condition (§2 there) resolves by calling this query and,
per that section, examines only the first (most recent) matching entry —
not any historical occurrence. Without this surface existing, gate chaining
cannot be expressed at all.

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
- **One global `state` field, domains coordinate ordering out of band.**
  Rejected — NPP, AfD, and CCI can be concurrently active on the same item;
  a single field forces one domain's transition to silently clobber
  another's, and "coordinate out of band" is exactly the kind of bespoke,
  hardcoded coupling ADR-0013's platform/domain split exists to prevent.
  Scoping `states` by `track` (§1) lets each domain own its own reading
  without a coordination protocol.
- **Leave `LifecycleState.key` unvalidated in watch configs, matching
  intake's openness.** Rejected — intake's `Custom(String)` fields are
  validated against a registry (ADR-0026 §7); leaving watch `from`/`to`
  unvalidated would be the one place this design lets a typo load
  successfully and fail silently forever, exactly the misconfiguration
  class this whole ADR chain exists to make loud instead.
- **Leave `transitions_matching`'s result order unspecified.** Rejected — an
  unordered result makes "the current standing of a gate/ruleset lineage"
  inexpressible without every caller re-deriving recency itself; specifying
  most-recent-first once, here, is cheaper than every consumer (ADR-0028,
  ADR-0029) getting it right independently or getting it wrong silently.

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
- `GateVerdict`'s widened payload (`reasons`, `matched_rule_path`,
  `config_version`, `revision_id`) makes
  transitions larger than the minimal routing-key shape first drafted —
  acceptable at eligibility's scale (one verdict per ruleset evaluation, not
  per edit event), but a size/storage consideration the eventual engine
  should keep in view as more gate types write through this log.
- Watch configs get the same fail-closed misconfiguration discipline as
  intake (ADR-0026 §6/§7) — an unresolvable `re_trigger` workflow id, an
  unregistered `track`, or a `from`/`to` key outside that track's registered
  state vocabulary is rejected at load time, extending that linter rather
  than inventing a second one.
- Per-track state scoping (§1) means a consumer reading "the" state of an
  item must now name a track — there is no longer a single answer to "what
  state is this item in" independent of which domain's lens is asking. This
  is a deliberate consequence of concurrent-domain correctness, not an
  oversight, but it does mean any future cross-track summary view (e.g. "is
  this item under review by *any* domain") is a read that aggregates across
  `states`, not a single field lookup — flagged as an implementation
  consideration, not solved by this contract.

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
