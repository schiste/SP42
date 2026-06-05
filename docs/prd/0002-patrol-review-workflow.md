# PRD-0002: Patrol review workflow

**Author:** Luis Villa
**Date:** 2026-06-05
**State:** Implemented
**As-built:** retroactive characterization of an already-shipped feature (no forward "closing PR").
**Related ADRs:** ADR-0001 (foundational decisions), ADR-0002 (local dev-auth bridge contract)
**Discussion:** (PR link added on filing)

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

- **Open a live operator view for one wiki** and receive a single assembled payload:
  the ranked queue, the selected revision's diff, the scoring context, a prepared
  set of actions for the selection, stream/backlog status, and session/auth state
  (`LiveOperatorView`, `crates/sp42-core/src/live_operator.rs:148`; assembled by the
  server route `GET /operator/live/{wiki_id}`,
  `crates/sp42-server/src/operator_live.rs:25`).
- **Have recent changes ingested into a queue automatically**, from two sources that
  normalize into the same `EditEvent`/`QueuedEdit` shape: the live `EventStreams`
  feed (`StreamIngestor::ingest`, `crates/sp42-core/src/stream_ingestor.rs:63`) and the
  `MediaWiki` `recentchanges` backlog poll (`build_recent_changes_request`,
  `crates/sp42-core/src/recent_changes.rs:82`). Bots and other out-of-scope changes are
  filtered out before they ever reach the queue, and ingestion checkpoints its cursor so
  a reconnect or a fresh page load resumes where the operator left off rather than
  re-showing handled edits (`StreamRuntime::next_actionable_event`,
  `crates/sp42-core/src/stream_runtime.rs:87`; backlog `rccontinue` checkpoint reuse,
  `crates/sp42-server/tests/operator_live.rs:443`).
- **Inspect the selected revision in place.** The patrol surface
  (`PatrolSurface`, `crates/sp42-app/src/pages/patrol.rs:27`) shows a queue pane, a diff
  pane, a session bar, a filter bar, and an action footer. Selecting a queue row (click,
  the `Up`/`Down` arrow keys, or a `rev=` URL hash) loads that revision's structured diff
  — served from a prefetched per-revision cache when available, fetched on demand on a
  miss (`install_selected_diff_effect`,
  `crates/sp42-app/src/pages/patrol/revision_artifacts.rs:177`). Selection is driven only
  by explicit operator action, never by a background stream insert — `selected_edit` is
  the "Single authoritative source for the selected edit ... Only updated by explicit
  human actions — never by EventStream inserts" (`crates/sp42-app/src/pages/patrol.rs:50`).
- **See why an edit ranks where it does.** Each `QueuedEdit` carries a `CompositeScore`
  (`crates/sp42-core/src/types.rs:286`) whose `total` and per-signal `contributions`
  (`SignalContribution`, `crates/sp42-core/src/types.rs:279`) explain the rank. The
  per-signal labels the operator sees — `Anonymous user`, `Obvious vandalism`,
  `LiftWing risk`, `Trusted user`, … — are the `Display` strings of the `ScoringSignal`
  enum (impl at `crates/sp42-core/src/types.rs:249`). The operator can open an inspector feed of
  labelled queue/stream/backlog/diff/review lines for the session (`InspectorFeed`,
  `crates/sp42-app/src/components/inspector_feed.rs:26`). (The score itself — which
  signals fire and the weights they carry — is scoring/ranking *semantics* owned by
  PRD-0003; this workflow consumes and displays the result, it does not define it.)
- **Filter the queue without re-fetching.** Bots, minor edits, new pages, registered /
  anonymous / temporary editors, a tag, and a minimum score can be toggled client-side;
  changing filters re-selects the first visible edit
  (`filter_edits`, `crates/sp42-core` filter params applied in
  `crates/sp42-app/src/pages/patrol/queue_controller.rs:91`).
- **Choose a disposition for the selected revision** from the action footer or a
  one-key shortcut — `r` rollback, `u` undo, `p` mark patrolled, `s` skip
  (`crates/sp42-app/src/pages/patrol/keyboard_controller.rs:18`). On an accepted
  action the edit (and its grouped siblings) is removed from the queue, the review note
  is cleared, and selection advances to the next item, giving the fast N→N+1 cadence
  (`install_action_effect`, `crates/sp42-app/src/pages/patrol/action_controller.rs:120`;
  the removal-and-advance itself is `remove_accepted_edit`,
  `crates/sp42-app/src/pages/patrol/action_controller.rs:240`).
- **See, next to the selection, which dispositions the current session can actually
  perform.** As one section of the assembled view, the live route attaches a per-selection
  action preflight (`LiveOperatorView.action_preflight`,
  `crates/sp42-core/src/live_operator.rs:169`; built by
  `build_live_operator_action_preflight`, `crates/sp42-core/src/live_operator.rs:187`),
  so the operator is not offered a doomed action blind. Whether an action is *available*
  is an operator-rights/token question (this workflow's concern; see ADR-0002): a
  disposition blocked by a missing token is marked `available=false` with a retry
  classification rather than silently failing
  (`action_availability`, `crates/sp42-core/src/live_operator.rs:283`). Whether an action
  is *recommended* — the score-gated suggestion (`is_recommended` and its score cutoffs) —
  is scoring/action-recommendation *semantics* and is owned by PRD-0003; this workflow
  surfaces the recommendation the preflight produces, it does not define the gating.

A disposition (`SessionActionKind`: rollback, patrol, undo, tag-citation-needed,
inline-edit — `crates/sp42-core/src/action_executor.rs:80`) is the operator's decision
about a revision; the actual MediaWiki call behind it (tokens, request building,
execution, audit) is out of scope for this PRD and is governed elsewhere.

## Scope boundary with PRD-0003 (score-gated action recommendation)

The action preflight (`build_live_operator_action_preflight`) does two structurally
different things, split here so each is owned by exactly one PRD:

- **Action *availability* + retry classification** — does the active session have the
  rights and tokens to perform this disposition, and if not, what is the retry path
  (session refresh / backoff / operator change)? This is an operator-workflow + auth
  concern (ADR-0002 dev-auth bridge) and is **owned by this PRD (PRD-0002)**. Its
  observable is `preflight_classifies_missing_tokens_as_session_refresh` (in the DoD
  below).
- **Action *recommendation* (the score gate)** — given the edit's `CompositeScore` and
  signals, which single disposition does scoring *suggest*? This is the score-gated
  suggestion produced by `is_recommended` (`crates/sp42-core/src/live_operator.rs:373`,
  cutoffs at `:392`/`:398`/`:401`). Per the scoring constitution, "Scoring may recommend
  attention and actions" (`docs/scoring/SCORING_CONSTITUTION.md` §5.1) and recommendation
  thresholds are a reserved scoring question (§17 Q2), so this is scoring/action-semantics
  and is **owned by PRD-0003**, whose DoD binds the recommendation behavior
  (`live_operator.rs::preflight_recommends_rollback_for_high_score_edit`). This PRD only
  *displays* the recommended disposition the preflight returns.

This split removes the prior double-attribution: this PRD no longer claims the
recommendation semantics or binds the `preflight_recommends_rollback_for_high_score_edit`
test in its own Definition of Done.

## Definition of Done

Each item is a behavior that already holds, bound to an existing test or observable.
Items the workflow merely *consumes* but does not *own* (the score itself; the score-gated
recommendation) are deliberately not listed — they are bound in PRD-0003.

- [x] A single `recentchanges`-feed event is ingested and normalized into a queued
  edit (rev id, editor classification) — verified by
  `crates/sp42-core/src/stream_ingestor.rs::ingests_supported_recentchange_event`.
- [x] A newline-delimited batch of stream events is ingested into an ordered list of
  edits — verified by
  `crates/sp42-core/src/stream_ingestor.rs::ingests_json_lines_batch`.
- [x] Out-of-scope changes are filtered before reaching the queue (unrelated wiki, bot
  edits) — verified by `crates/sp42-core/src/stream_ingestor.rs::filters_out_unrelated_wikis`
  and `::filters_out_bot_edits`.
- [x] A `MediaWiki` `list=recentchanges` backlog request is built with the expected
  params and an unpatrolled/bot `rcshow` filter, and its response parses into edit
  events plus a continue token — verified by
  `crates/sp42-core/src/recent_changes.rs::builds_recentchanges_request` and
  `::parses_recentchanges_response`.
- [x] The stream runtime walks events until an actionable edit is produced and persists
  the stream cursor, so ingestion resumes at a checkpoint — verified by
  `crates/sp42-core/src/stream_runtime.rs::streams_until_actionable_event_and_persists_cursor`
  and `::drains_actionable_events_up_to_limit`.
- [x] A malformed event does not advance the checkpoint (handled edits are not skipped
  on the next read) — verified by
  `crates/sp42-core/src/stream_runtime.rs::invalid_payload_does_not_advance_checkpoint`.
- [x] The live operator view assembles a per-wiki payload (ranked queue, selected diff,
  prepared review actions, backlog status, capabilities) and reuses the persisted
  backlog checkpoint across repeated and concurrent requests — verified by the
  end-to-end server test
  `crates/sp42-server/tests/operator_live.rs::operator_live_contract_reuses_checkpoints_and_handles_concurrent_requests`
  (asserts `queue`, `diff`, `backlog_status`, and checkpoint reuse on the assembled view).
- [x] For the selected edit, the preflight reports which dispositions the active session
  can *perform*, and a disposition blocked by a missing token is reported as
  `available=false` with a retry classification rather than silently failing — verified
  by `crates/sp42-core/src/live_operator.rs::preflight_classifies_missing_tokens_as_session_refresh`.
  (The *recommendation* half of the preflight — the score gate — is bound in PRD-0003's
  DoD, not here.)
- [x] An operator-facing session summary is built from the patrol report — queue depth,
  the selected revision, severity counts, section availability — verified by
  `crates/sp42-core/src/operator_summary.rs::builds_operator_summary_from_report`.
- [x] Prepared per-edit review actions (rollback / patrol / undo previews) are built for
  a ranked edit — verified by
  `crates/sp42-core/src/review_workbench.rs::builds_request_and_training_previews`.
- [x] Inspector lines are classified into the operator's labelled lanes (queue, stream,
  backlog, coordination, diff, …) for the live session feed — verified by
  `crates/sp42-app/src/components/inspector_feed.rs::classifies_known_prefixes`.
- [x] The current article's references, sections, categories, and citation templates are
  inventoried from wikitext as inspection context — verified by
  `crates/sp42-core/src/article_inventory.rs::inventories_article_references_and_categories`.

## Alternatives

- **Background auto-advancing selection.** The queue could re-point the operator to a
  newer/higher-scoring edit as the stream delivers it. The shipped design deliberately
  rejects this: `selected_edit` is the "Single authoritative source for the selected
  edit ... Only updated by explicit human actions — never by EventStream inserts"
  (`crates/sp42-app/src/pages/patrol.rs:50`). The operator's place in the queue is theirs;
  new edits accumulate without yanking the current inspection.
- **Server-side filtering of the queue.** Bot/minor/editor-class/tag/min-score filters
  could be a server query. They are applied client-side over the already-fetched edits
  (`filter_edits`, `crates/sp42-app/src/pages/patrol/queue_controller.rs:91`) so toggling
  a filter is instant and does not spend a round trip or disturb the checkpoint; the
  server-side `rcshow` filter is reserved for the coarse bot/unpatrolled/minor/anon cut
  at ingestion (`crates/sp42-core/src/recent_changes.rs:116`).
- **Two separate ingestion features (stream vs. backlog).** Instead, both the live
  `EventStreams` feed and the `recentchanges` poll normalize into the same `EditEvent`
  and share the same checkpoint discipline (`stream_runtime.rs`, `recent_changes.rs`),
  so the queue and the rest of the loop are source-agnostic.
- **Numeric-only ranking.** The queue could surface only a score. Instead each edit
  carries its per-signal `contributions` (`SignalContribution`,
  `crates/sp42-core/src/types.rs:279`) so the rank is explainable in the operator's own
  vocabulary, not an opaque number.

## Risks

- **Stale or duplicated queue items on reconnect.** A reconnect that didn't checkpoint
  correctly could re-show handled edits. Mitigation: the cursor is only advanced when an
  event is actually processed, a malformed payload leaves it untouched
  (`crates/sp42-core/src/stream_runtime.rs::invalid_payload_does_not_advance_checkpoint`,
  `:359`), and the backlog `rccontinue` is persisted and reused across requests
  (`crates/sp42-server/tests/operator_live.rs:443`).
- **Acting on the wrong revision after a fast advance.** Because dispositions are
  one-keypress, an operator could act on a revision that changed under them. Mitigation:
  the selection is decoupled from stream inserts (`crates/sp42-app/src/pages/patrol.rs:50`);
  keyboard handling is suppressed while typing in an input/textarea so a note keystroke
  never fires an action (`is_text_entry_event` guard,
  `crates/sp42-app/src/pages/patrol/keyboard_controller.rs:14` / `:54`); and the action
  request is built from the explicitly selected edit's `rev_id`
  (`build_action_request`, `crates/sp42-app/src/pages/patrol/action_controller.rs:188`).
- **Choosing a disposition the account can't perform.** Rights or tokens may be missing.
  Mitigation: the preflight marks unavailable actions and classifies the retry path
  (session refresh / backoff / operator change) rather than letting the operator fire a
  doomed action blind (`action_availability`,
  `crates/sp42-core/src/live_operator.rs:283`); an auth failure at action time triggers a
  re-authenticate-and-retry (`retry_after_reauthentication`,
  `crates/sp42-app/src/pages/patrol/action_controller.rs:277`).
- **Inspection context can mislead.** The article inventory is wikitext-derived and, by
  its own note, "Inventory is wikitext-derived and does not yet validate external URLs,
  Wikidata claims, or Commons metadata" (`crates/sp42-core/src/article_inventory.rs:110`);
  diff/score are a decision aid, not a verdict. The human still confirms every disposition.

## Known gaps / drift

- **No standalone test for the `patrol.rs` surface wiring.** The page-level component
  (queue/diff/action panes, the keyboard handler, the selection-vs-stream invariant in
  `crates/sp42-app/src/pages/patrol.rs`) is exercised only indirectly; the app-layer
  Leptos controllers (`revision_artifacts`, `action_controller`, `queue_controller`,
  `keyboard_controller`) carry no `#[cfg(test)]` module. The end-to-end coverage of the
  loop lives in the server test (`operator_live.rs`) and the core-crate unit tests, not
  in the WASM UI layer. The DoD above is bound only to surfaces that do have tests.
- **`Action` vs `SessionActionKind` are two disposition vocabularies.** The review
  workbench's training labels use `core::Action` (Rollback / Revert / Warn / Report /
  MarkPatrolled, `crates/sp42-core/src/types.rs:385`), while the live loop and preflight
  use `SessionActionKind` (Rollback / Patrol / Undo / TagCitationNeeded / InlineEdit,
  `crates/sp42-core/src/action_executor.rs:80`). The sets overlap but are not identical
  (e.g. `Warn`/`Report` exist only in `Action`; `Undo`/`Patrol`/tagging only in
  `SessionActionKind`). This is undocumented and a likely source of confusion for a
  maintainer.
- **`skip` (`s`) has two code paths.** The keyboard handler raises a skip trigger
  (`crates/sp42-app/src/pages/patrol/keyboard_controller.rs:22` → `install_skip_effect`,
  `crates/sp42-app/src/pages/patrol/action_controller.rs:169`), which advances selection
  to `idx + 1`, while `ArrowDown` advances via a separate branch in the keyboard handler
  (`crates/sp42-app/src/pages/patrol/keyboard_controller.rs:33`). Both move to `idx + 1`
  but are not unified.
- **Two `recentchanges` timestamp parsers coexist.** `stream_ingestor.rs` and
  `recent_changes.rs` each carry a near-identical hand-rolled RFC-3339/`days_from_civil`
  implementation (`parse_rfc3339_utc` returning seconds+nanos,
  `crates/sp42-core/src/stream_ingestor.rs:215`; `parse_rfc3339_utc_to_ms` returning ms,
  `crates/sp42-core/src/recent_changes.rs:272`; each with its own `days_from_civil` at
  `stream_ingestor.rs:321` / `recent_changes.rs:313`). They are independently tested but
  duplicated; drift between them would silently change ingestion behavior.
- **Inspector lane classification is prefix-fragile.** `classify_inspector_line` keys off
  the string prefix before `=` (`crates/sp42-app/src/components/inspector_feed.rs:71`); a
  renamed summary line would fall through to the `General` lane without any test catching
  it.
- **`is_recommended` thresholds are magic numbers (recommendation semantics, owned by
  PRD-0003).** Disposition recommendation uses bare score cutoffs (rollback `>= 70`,
  `crates/sp42-core/src/live_operator.rs:392`; patrol `< 60`, `:398`; undo `>= 40`, `:401`)
  inside `is_recommended` (`:373`), with no doc tying them to the scoring constitution
  (`docs/scoring/SCORING_CONSTITUTION.md`, which §17 Q2 leaves "Should action recommendation
  thresholds be constitutionalized separately from queue-priority thresholds?" open). A
  change to scoring weights could quietly shift recommendations. The recommendation
  capability and its test (`preflight_recommends_rollback_for_high_score_edit`) live in
  PRD-0003; this gap is noted here only because the threshold constants sit in the same
  `live_operator.rs` module this workflow assembles.
- **Edit-grouping merge is lossy by design but undocumented.** When `group_edits` collapses
  same-title/same-performer edits it keeps the newest revision but overwrites its score
  with the group max and its byte-delta with the group sum
  (`crates/sp42-app/src/pages/patrol/queue_controller.rs:139`); the operator sees a
  synthesized row whose displayed score no longer matches that single revision. There is
  no UI-layer test for this.
- **Anonymous-vs-temporary editor classification differs subtly across ingestors.** Both
  `classify_editor` implementations (`crates/sp42-core/src/stream_ingestor.rs:145` and
  `crates/sp42-core/src/recent_changes.rs:254`) classify a `~`-prefixed user as Temporary
  and an IP as Anonymous, but they are separate implementations; the live filter
  (`crates/sp42-app/src/pages/patrol/queue_controller.rs:107`) trusts whichever produced
  the event.
