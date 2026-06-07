# PRD-0002: Patrol review workflow

**Author:** Luis Villa
**Date:** 2026-06-05
**State:** Implemented
**As-built:** retroactive characterization of an already-shipped feature (no forward "closing PR").
**Related ADRs:** ADR-0001 (foundational decisions), ADR-0002 (local dev-auth bridge contract — but operator identity/capabilities are owned by PRD-0005)
**Discussion:** (PR link added on filing)

## Scope boundary

This PRD characterizes **the operator's main review loop** — what a reviewer does, in what order, to work a wiki's recent changes one revision at a time: pull a ranked queue, walk it, inspect the diff in place, and choose a disposition without leaving the page. It is the workflow that ties the other surfaces together, and it deliberately *references* rather than restates them:

- **What a score, signal, or rank MEANS, and the score-gated action *recommendation*** — why an edit surfaced and which disposition scoring *suggests* — is scoring/ranking semantics owned by **PRD-0003**. This workflow consumes and displays that result; it does not define it.
- **What a chosen disposition DOES on the wiki, and how the operator knows it landed** — the on-wiki meaning of rollback / undo / patrol / tag / inline-fix and its reported outcome — is owned by **PRD-0004**. This PRD picks the disposition and ends the loop; PRD-0004 picks up at "the operator has decided."
- **Who the operator is acting as, and which dispositions their identity may perform** — the server-owned session, the per-wiki capability report, and login — is owned by **PRD-0005**. This workflow *relies on* that surface to know which actions to offer and which to mark available.
- **How a disposition is carried out** — the action contract, token acquisition, request building, and execution — is *implementation* (the content-edit editing mechanism is governed by ADR-0003). This PRD references that mechanism, it does not specify it.
- **How the live view and the per-selection action preflight are *assembled*** — the `GET /operator/live/{wiki_id}` payload shape and the preflight's construction — is *implementation*; that contract has no ADR of its own yet (see Known gaps).

The one split worth stating explicitly is inside the per-selection **action preflight**, because it straddles two PRDs:

- **Action *availability* + retry classification** — does the active session have the rights and tokens to perform this disposition, and if not, what is the retry path (session refresh / backoff / operator change)? This is an operator-workflow + auth concern (identity owned by PRD-0005) and is **owned by this PRD**. Its observable is `preflight_classifies_missing_tokens_as_session_refresh` (in the DoD below).
- **Action *recommendation* (the score gate)** — given the edit's score and signals, which single disposition does scoring *suggest*? Per the scoring constitution, "Scoring may recommend attention and actions" (`docs/scoring/SCORING_CONSTITUTION.md` §5.1) and recommendation thresholds are a reserved scoring question (§17 Q2), so this is scoring/action semantics **owned by PRD-0003**, whose DoD binds it (`preflight_recommends_rollback_for_high_score_edit`). This PRD only *displays* the recommended disposition the preflight returns.

## Problem

SP42's reason to exist is to give a patroller one place to watch a wiki's recent
changes and act on them, replacing the scatter of LiveRC, Huggle, SWViewer, RTRC,
and friends (ADR-0001 context). Before this workflow an operator had no SP42 surface
that did the core loop end to end: there was no way to pull a wiki's stream of recent
changes into a single ranked queue, walk it one revision at a time, see the diff and
the reasons the edit was ranked where it was, and then pick a disposition for that
revision without leaving the page.

The operators this is for are experienced reviewers working a single wiki at a time
(the live surface is keyed per `wiki_id`, e.g. `frwiki`). They need the unactioned,
highest-concern edits surfaced first, enough context next to each one to judge it
quickly, and a fast advance to the next item so a session is a steady cadence of
inspect-decide-advance rather than hunting across tools and tabs.

## Proposal

The patrol review workflow is the operator's main loop. It lets a reviewer:

- **Open one live operator view for a wiki.** A single live surface for `wiki_id`
  brings together, in one place, the ranked queue, the selected revision's diff, the
  scoring context, the dispositions prepared for the selection, stream/backlog status,
  and session/auth state — so the operator works the loop without leaving the page.
  (How that view is assembled and served is implementation; its contract is exercised
  by the end-to-end test in the DoD.)
- **Have recent changes ingested into one queue automatically**, from two sources —
  the live `EventStreams` feed and the `MediaWiki` `recentchanges` backlog poll — that
  the operator experiences as a single, source-agnostic queue. Bots and other
  out-of-scope changes are filtered out before they ever reach the queue, and ingestion
  resumes where the operator left off: a reconnect or a fresh page load picks up at a
  checkpoint rather than re-showing handled edits. (The normalization and checkpoint
  mechanism is implementation; the behavior is bound in the DoD.)
- **Inspect the selected revision in place.** The patrol surface shows a queue pane, a
  diff pane, a session bar, a filter bar, and an action footer. Selecting a queue row
  (click, the `Up`/`Down` arrow keys, or a `rev=` URL hash) loads that revision's
  structured diff in place. Selection is driven only by explicit operator action, never
  by a background stream insert: the operator's place in the queue is theirs, and a new
  edit arriving on the stream never re-points the inspection out from under them.
- **See why an edit ranks where it does**, in the operator's own vocabulary. Each queued
  edit carries the per-signal reasons that explain its rank, and the per-signal labels
  the operator sees — `Anonymous user`, `Obvious vandalism`, `LiftWing risk`,
  `Trusted user`, … — are surfaced next to the edit. The operator can open an inspector
  feed of labelled queue/stream/backlog/diff/review lines for the session. (The score
  itself — which signals fire and the weights they carry — is scoring/ranking *semantics*
  owned by PRD-0003; this workflow consumes and displays the result, it does not define
  it.)
- **Filter the queue without re-fetching.** Bots, minor edits, new pages, registered /
  anonymous / temporary editors, a tag, and a minimum score can be toggled client-side
  over the already-fetched edits, so toggling a filter is instant and does not spend a
  round trip or disturb the checkpoint; changing filters re-selects the first visible
  edit. (Client-side filtering is implementation; the coarse server-side `rcshow`
  bot/unpatrolled/minor/anon cut is applied once at ingestion — see Alternatives.)
- **Choose a disposition for the selected revision** from the action footer or a
  one-key shortcut — `r` rollback, `u` undo, `p` mark patrolled, `s` skip. On an accepted
  action the edit (and its grouped siblings) is removed from the queue, the review note
  is cleared, and selection advances to the next item, giving the fast N→N+1 cadence.
- **See, next to the selection, which dispositions the current session can actually
  perform.** The live view attaches a per-selection action preflight so the operator is
  not offered a doomed action blind. Whether an action is *available* is an
  operator-rights/token question (this workflow's concern; identity owned by PRD-0005): a
  disposition blocked by a missing token is marked `available=false` with a retry
  classification rather than silently failing. Whether an action is *recommended* — the
  score-gated suggestion — is scoring/action-recommendation *semantics* owned by PRD-0003;
  this workflow surfaces the recommendation the preflight produces, it does not define the
  gating (see Scope boundary).

A disposition is the operator's decision about a revision; what that decision *means on
the wiki* and how its outcome is reported is owned by **PRD-0004**, and the actual
MediaWiki call behind it (tokens, request building, execution, audit) is implementation,
out of scope for this PRD.

## Definition of Done

Each item is a behavior that already holds, bound to an existing test or observable.
Items the workflow merely *consumes* but does not *own* (the score itself; the
score-gated recommendation, bound in PRD-0003) are deliberately not listed. (Internal
assembly/classification helpers behind the loop — the operator summary, the per-edit
prepared-action previews, inspector-lane classification, and the wikitext article
inventory — are additionally unit-tested across `crates/sp42-reporting`,
`crates/sp42-core`, and `crates/sp42-app`
(`operator_summary.rs::builds_operator_summary_from_report` in sp42-reporting;
`review_workbench.rs::builds_request_and_training_previews` and
`article_inventory.rs::inventories_article_references_and_categories` in sp42-core;
`inspector_feed.rs::classifies_known_prefixes` in sp42-app), but they are
assembly mechanism, not operator-observable loop outcomes.)

- [x] A single `recentchanges`-feed event is ingested and normalized into a queued
  edit (rev id, editor classification) — verified by
  `crates/sp42-live/src/stream_ingestor.rs::ingests_supported_recentchange_event`.
- [x] A newline-delimited batch of stream events is ingested into an ordered list of
  edits — verified by
  `crates/sp42-live/src/stream_ingestor.rs::ingests_json_lines_batch`.
- [x] Out-of-scope changes are filtered before reaching the queue (unrelated wiki, bot
  edits) — verified by `crates/sp42-live/src/stream_ingestor.rs::filters_out_unrelated_wikis`
  and `::filters_out_bot_edits`.
- [x] A `MediaWiki` `list=recentchanges` backlog request is built with the expected
  params and an unpatrolled/bot `rcshow` filter, and its response parses into edit
  events plus a continue token — verified by
  `crates/sp42-live/src/recent_changes.rs::builds_recentchanges_request` and
  `::parses_recentchanges_response`.
- [x] The stream runtime walks events until an actionable edit is produced and persists
  the stream cursor, so ingestion resumes at a checkpoint — verified by
  `crates/sp42-live/src/stream_runtime.rs::streams_until_actionable_event_and_persists_cursor`
  and `::drains_actionable_events_up_to_limit`.
- [x] A malformed event does not advance the checkpoint (handled edits are not skipped
  on the next read) — verified by
  `crates/sp42-live/src/stream_runtime.rs::invalid_payload_does_not_advance_checkpoint`.
- [x] The live operator view assembles a per-wiki payload (ranked queue, selected diff,
  prepared review actions, backlog status, capabilities) and reuses the persisted
  backlog checkpoint across repeated and concurrent requests — verified by the
  end-to-end server test
  `crates/sp42-server/tests/operator_live.rs::operator_live_contract_reuses_checkpoints_and_handles_concurrent_requests`
  (asserts `queue`, `diff`, `backlog_status`, and checkpoint reuse on the assembled view).
- [x] For the selected edit, the preflight reports which dispositions the active session
  can *perform*, and a disposition blocked by a missing token is reported as
  `available=false` with a retry classification rather than silently failing — verified
  by `crates/sp42-live/src/live_operator.rs::preflight_classifies_missing_tokens_as_session_refresh`.
  (The *recommendation* half of the preflight — the score gate — is bound in PRD-0003's
  DoD, not here.)

## Alternatives

- **Background auto-advancing selection.** The queue could re-point the operator to a
  newer/higher-scoring edit as the stream delivers it. The shipped design deliberately
  rejects this: the selected revision is updated only by explicit human action, never by
  a stream insert. The operator's place in the queue is theirs; new edits accumulate
  without yanking the current inspection.
- **Server-side filtering of the queue.** Bot/minor/editor-class/tag/min-score filters
  could be a server query. They are applied client-side over the already-fetched edits
  so toggling a filter is instant and does not spend a round trip or disturb the
  checkpoint; the server-side `rcshow` filter is reserved for the coarse
  bot/unpatrolled/minor/anon cut at ingestion.
- **Two separate ingestion features (stream vs. backlog).** Instead, both the live
  `EventStreams` feed and the `recentchanges` poll normalize into the same edit shape
  and share the same checkpoint discipline, so the queue and the rest of the loop are
  source-agnostic.
- **Numeric-only ranking.** The queue could surface only a score. Instead each edit
  carries its per-signal reasons so the rank is explainable in the operator's own
  vocabulary, not an opaque number. (The score and its decomposition are PRD-0003.)

## Risks

- **Stale or duplicated queue items on reconnect.** A reconnect that didn't checkpoint
  correctly could re-show handled edits. Mitigation: the cursor is only advanced when an
  event is actually processed, a malformed payload leaves it untouched, and the backlog
  continue token is persisted and reused across requests.
- **Acting on the wrong revision after a fast advance.** Because dispositions are
  one-keypress, an operator could act on a revision that changed under them. Mitigation:
  the selection is decoupled from stream inserts, so a newer edit never re-points the
  inspection; keyboard handling is suppressed while typing in an input/textarea so a note
  keystroke never fires an action; and the action request is built from the explicitly
  selected edit's `rev_id`.
- **Choosing a disposition the account can't perform.** Rights or tokens may be missing.
  Mitigation: the preflight marks unavailable actions and classifies the retry path
  (session refresh / backoff / operator change) rather than letting the operator fire a
  doomed action blind, and an auth failure at action time triggers a
  re-authenticate-and-retry. (How identity and capabilities are established and gated is
  PRD-0005; what the action does once chosen is PRD-0004.)
- **Inspection context can mislead.** The article inventory is wikitext-derived: it does
  not yet validate external URLs, Wikidata claims, or Commons metadata, so it is context
  for the operator's judgment rather than a verified fact. The diff and the score are a
  decision aid, not a verdict — the human still confirms every disposition.

## Known gaps / drift

- **The assembled live-operator-view contract has no ADR.** The `GET /operator/live/{wiki_id}`
  payload that bundles the ranked queue, the selected diff, the scoring context, the
  per-selection action preflight, and backlog/session state into one response is a
  public-contract concern that, per `docs/prd/README.md`, warrants its own ADR, but none
  exists; its structure lives only in code and is pinned only by the end-to-end
  `operator_live` test. *(This PRD owns the loop's user-facing intent; the assembled
  view's structure should become an ADR this PRD links. The disposition-execution
  contract gap is owned by PRD-0004.)*
- **No standalone test for the `patrol.rs` surface wiring.** The page-level component
  (queue/diff/action panes, the keyboard handler, the selection-vs-stream invariant in
  `crates/sp42-app/src/pages/patrol.rs`) is exercised only indirectly; the app-layer
  Leptos controllers (`revision_artifacts`, `action_controller`, `queue_controller`,
  `keyboard_controller`, `eventstream_controller`, `load_controller`, `view_components`)
  carry no `#[cfg(test)]` module. The end-to-end coverage of the
  loop lives in the server test (`operator_live.rs`) and the core-crate unit tests, not
  in the WASM UI layer. The DoD above is bound only to surfaces that do have tests.
- **`Action` vs `SessionActionKind` are two disposition vocabularies.** The review
  workbench's training labels use `core::Action` (Rollback / Revert / Warn / Report /
  MarkPatrolled, `crates/sp42-core/src/types.rs`), while the live loop and preflight
  use `SessionActionKind` (Rollback / Patrol / Undo / TagCitationNeeded / InlineEdit,
  `crates/sp42-core/src/action_contracts.rs`). The sets overlap but are not identical
  (e.g. `Warn`/`Report` exist only in `Action`; `Undo`/`Patrol`/tagging only in
  `SessionActionKind`). This is undocumented and a likely source of confusion for a
  maintainer.
- **`skip` (`s`) has two code paths.** The keyboard handler sets a skip trigger
  (`crates/sp42-app/src/pages/patrol/keyboard_controller.rs`) that `install_skip_effect`
  (defined entirely in `crates/sp42-app/src/pages/patrol/action_controller.rs`) reacts to,
  advancing selection to `idx + 1`, while `ArrowDown` advances via a separate branch in the
  keyboard handler (`crates/sp42-app/src/pages/patrol/keyboard_controller.rs`). Both move to
  `idx + 1` but are not unified.
- **Two `recentchanges` timestamp parsers coexist.** `stream_ingestor.rs` and
  `recent_changes.rs` each carry a near-identical hand-rolled RFC-3339/`days_from_civil`
  implementation (`parse_rfc3339_utc` returning seconds+nanos,
  `crates/sp42-live/src/stream_ingestor.rs`; `parse_rfc3339_utc_to_ms` returning ms,
  `crates/sp42-live/src/recent_changes.rs`; each with its own `days_from_civil` at
  `stream_ingestor.rs` / `recent_changes.rs`). They are independently tested but
  duplicated; drift between them would silently change ingestion behavior.
- **Inspector lane classification is prefix-fragile.** `classify_inspector_line` keys off
  the string prefix before `=` (`crates/sp42-app/src/components/inspector_feed.rs`); a
  renamed summary line would fall through to the `General` lane without any test catching
  it.
- **`is_recommended` thresholds are magic numbers (recommendation semantics, owned by
  PRD-0003).** Disposition recommendation uses bare score cutoffs (rollback `>= 70`,
  `crates/sp42-live/src/live_operator.rs`; patrol `< 60`; undo `>= 40`)
  inside `is_recommended`, with no doc tying them to the scoring constitution
  (`docs/scoring/SCORING_CONSTITUTION.md`, which §17 Q2 leaves "Should action recommendation
  thresholds be constitutionalized separately from queue-priority thresholds?" open). A
  change to scoring weights could quietly shift recommendations. The recommendation
  capability and its test (`preflight_recommends_rollback_for_high_score_edit`) live in
  PRD-0003; this gap is noted here only because the threshold constants sit in the same
  `live_operator.rs` module this workflow assembles.
- **Edit-grouping merge is lossy by design but undocumented.** When `group_edits` collapses
  same-title/same-performer edits it keeps the newest revision but overwrites its score
  with the group max and its byte-delta with the group sum
  (`crates/sp42-app/src/pages/patrol/queue_controller.rs`); the operator sees a
  synthesized row whose displayed score no longer matches that single revision. There is
  no UI-layer test for this.
- **Anonymous-vs-temporary editor classification differs subtly across ingestors.** Both
  `classify_editor` implementations (`crates/sp42-live/src/stream_ingestor.rs` and
  `crates/sp42-live/src/recent_changes.rs`) classify a `~`-prefixed user as Temporary
  and an IP as Anonymous, but they are separate implementations; the live filter — now
  centralized in `crates/sp42-live/src/live_operator.rs` (`live_operator_query_matches`)
  and shared by the server and the `queue_controller.rs` client path — trusts whichever
  classification produced the event.
