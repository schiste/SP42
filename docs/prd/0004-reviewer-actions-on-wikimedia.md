# PRD-0004: Reviewer actions on Wikimedia

**Author:** Luis Villa
**Date:** 2026-06-05
**State:** Implemented
**As-built:** retroactive characterization of an already-shipped feature (no forward "closing PR").
**Related ADRs:** ADR-0003 (node-anchored wikitext editing — governs the *future* shape of the content-edit verbs; this PRD documents the *current* string-replace behavior and relates it to ADR-0003)
**Discussion:** (PR link added on filing)

## Problem

An SP42 operator works a ranked queue of recent-changes revisions and, for each
one, needs to dispose of it on-wiki without leaving the review surface. Reading
and scoring a revision is only half the job; the operator also has to *act* —
revert vandalism, accept a good edit, or flag a problem — using their own
Wikimedia account and rights, and have that action land on the wiki with a
sensible edit summary and base revision.

Before this feature, SP42 could surface and rank revisions but had no path from
the review surface to an on-wiki disposition. The operator would have had to
leave SP42, open the wiki, and repeat the action manually — losing the queue
position, the score context, and the review note, and doing token handling by
hand. This is for the **experienced reviewer/patroller** acting under their own
account on a configured wiki (e.g. `frwiki`), one revision at a time.

## Proposal

SP42 lets the operator dispose of the selected revision with five named
Wikimedia actions, each carrying its own on-wiki meaning. The operator picks an
action; SP42 fetches the right MediaWiki token, builds the API request, and
applies it under the operator's authenticated session, then records the outcome
in the session's action history with operator-readable feedback.

The five verbs (`SessionActionKind`, `action_executor.rs:80`):

- **Rollback** — one-click revert of all consecutive top edits by the same
  user, via the MediaWiki `action=rollback` API, keyed by article title +
  target user (`build_rollback_request`, `action_executor.rs:152`). Requires a
  rollback token and the rollback right.
- **Undo** — revert a specific revision via `action=edit` with
  `undo`/`undoafter` (`build_undo_request`, `action_executor.rs:240`). The
  operator's selected revision is `undo`, and `undoafter` is the revision it
  followed; SP42 refuses an undo where the target is not strictly newer than the
  prior revision (`action_executor.rs:256`).
- **Patrol** — mark the revision as patrolled via `action=patrol`
  (`build_patrol_request`, `action_executor.rs:189`), keyed by `revid`. A single
  patrol token can fan out across a group of revision ids when the queued edit
  carries `batch_rev_ids` (`action_routes.rs:260`).
- **Tag citation-needed** — insert the wiki's citation-needed template (e.g.
  `{{refnec|…|date=…}}` on `frwiki`) around an operator-selected phrase and save
  the whole page via `action=edit` (`execute_tag_citation_needed_action`,
  `action_routes.rs:316`), then best-effort patrol the original revision.
- **Inline edit** — replace an operator-selected span of article text with
  replacement text and save the whole page (`execute_inline_edit_action`,
  `action_routes.rs:360`), then best-effort patrol the original revision.

**What the operator confirms.** The operator confirms an action by invoking it
on the review surface — the Rollback / Undo / Patrol buttons on the patrol
action toolbar (`action_bar.rs:56`), or the inline-edit / citation-needed
affordances reached from the diff context menu
(`revision_artifacts.rs:311`, `:358`). The request SP42 sends carries the
revision id, article title, the operator's review note as the edit summary, and,
for the two content-edit verbs, the operator's selected text (and, for inline
edit, the replacement text). The operator sees per-action feedback —
"accepted", "rejected", or an error — and the queue advances.

**Base revision and tokens, as the operator experiences them.** SP42 fetches the
needed token (`rollback`, `patrol`, or `csrf`) immediately before each action
under the operator's bearer session — the operator never handles a token. For
the two content-edit verbs SP42 reads the page's current wikitext
(`fetch_page_wikitext`, `main.rs:1024`), applies the change, and saves with
`baserevid` set to the patrolled revision id (`action_routes.rs:348`, `:399`;
`build_wiki_page_save_request`, `action_executor.rs:288`) so MediaWiki can detect
an intervening edit. Every action POST must carry an authenticated bridge
session (`action_routes.rs:45`) and a valid same-session CSRF header
(`x-sp42-csrf-token`, `main.rs:111`; `validate_csrf_header`, `main.rs:486`,
called at `action_routes.rs:50`), or it is refused.

**Safety surface, as shipped.** Each verb is gated on a capability derived from
the operator's OAuth grants, wiki rights, and token availability
(`wikimedia_capabilities.rs:278`): rollback needs the rollback grant + right +
token; patrol needs the patrol grant + right + token; undo and inline/tag edits
need the edit grant + right + a CSRF token. The action toolbar disables a button
the session cannot perform (`action_bar.rs:62`), and the server independently
re-checks, per verb, the required fields and the matching capability before
executing (`validate_action_request`, `action_routes.rs:762`). MediaWiki
"no change" responses are surfaced as a warning and recorded as *not accepted*
(`action_routes.rs:106`, `:173`), and retryable API errors (e.g. `maxlag`,
`ratelimited`) are flagged so the operator can retry
(`is_retryable_api_error`, `action_executor.rs:649`).

**Ownership note (action-execution path).** This PRD owns the user-facing
intent and definition of done for the **execute-action route** and the **five
action verbs**: the per-verb capability/field re-check that decides whether an
action verb runs (`validate_action_request`, `action_routes.rs:762`) and the
CSRF requirement *on the execute-action POST* (`action_routes.rs:50`). The
underlying shared mechanisms it relies on — how the per-session capability report
is computed (`capability_report_for_session`, called at `action_routes.rs:52`)
and the shared `validate_csrf_header` primitive (`main.rs:486`) — are SP42's
session/auth-bridge plumbing, documented by the sibling session-capability PRD;
this PRD characterizes only their use on the action route and does not restate
their general behavior or test coverage.

**Relation to ADR-0003.** The two *content-edit* verbs (inline edit,
tag-citation-needed) are implemented today as **first-occurrence literal
substring replacement** on the fetched wikitext (`page_text.replacen(…, 1)`,
`action_routes.rs:378` and `:423`). ADR-0003 records the decision to move these
verbs onto a node-anchored `WikitextEditor` (document-order ordinals, anti-drift
re-grounding on the expected node text, lossless re-serialization) and, as an
interim hardening, to add an exactly-one-occurrence guard. **This PRD documents
the current string-replace behavior**; the node-anchored design is future work
governed by ADR-0003. The `Rollback` / `Undo` / `Patrol` verbs touch no wikitext
and are unaffected by ADR-0003.

## Definition of Done

Each item is a behavior that is **already true**, bound to an existing test.

- [x] SP42 builds a well-formed `action=rollback` request keyed by title + user — verified by `crates/sp42-core/src/action_executor.rs::builds_rollback_request_body`.
- [x] SP42 builds a well-formed `action=patrol` request keyed by `revid` — verified by `crates/sp42-core/src/action_executor.rs::builds_patrol_request_body`.
- [x] SP42 builds a well-formed `action=edit` undo request carrying `undo`/`undoafter` — verified by `crates/sp42-core/src/action_executor.rs::builds_undo_request_body`.
- [x] SP42 builds a full-page save request carrying the operator's `baserevid` (and tags/minor flags) — verified by `crates/sp42-core/src/action_executor.rs::builds_wiki_page_save_request_body`.
- [x] SP42 builds a token-query request for the correct token type — verified by `crates/sp42-core/src/action_executor.rs::builds_token_query_request`.
- [x] Rollback, patrol, undo, and page-save each execute through the injected HTTP client and accept a 2xx success body — verified by `crates/sp42-core/src/action_executor.rs::executes_rollback_through_http_trait`, `::executes_patrol_through_http_trait`, `::executes_undo_through_http_trait`, and `::executes_page_save_through_http_trait`.
- [x] SP42 fetches and extracts the action token from a token-query response — verified by `crates/sp42-core/src/action_executor.rs::fetches_token_through_http_trait` and `::parses_patrol_token_response`.
- [x] A non-success HTTP status from MediaWiki is surfaced as a failure (not a silent accept) — verified by `crates/sp42-core/src/action_executor.rs::rejects_non_success_http_status`.
- [x] An API-level error in a 2xx body (e.g. `badtoken`) is surfaced as a failure with its code/info — verified by `crates/sp42-core/src/action_executor.rs::rejects_action_response_with_api_error_even_on_2xx` and `::parses_action_response_summary_for_api_error_payload`.
- [x] Retryable API errors (e.g. `maxlag`) are marked retryable so the operator can retry — verified by `crates/sp42-core/src/action_executor.rs::marks_retryable_api_errors`.
- [x] A MediaWiki "no change" edit is detected and not treated as an accepted change — verified by `crates/sp42-core/src/action_executor.rs::parse_action_response_detects_nochange` and `::parse_action_response_normal_edit_is_not_nochange`.
- [x] The action request contract serializes without leaking token material — verified by `crates/sp42-core/src/action_executor.rs::session_action_contract_serializes_without_token_material`.
- [x] The operator's review note flows into the recorded action's rationale/feedback — verified by `crates/sp42-server/src/action_routes.rs::action_feedback_includes_rationale_summary`.
- [x] The session's action history is recorded and read back per session with a limit — verified by `crates/sp42-server/src/tests.rs::action_history_route_returns_recorded_entries`.
- [x] The session's action status report aggregates totals and emits operator shell feedback (including the latest result excerpt) — verified by `crates/sp42-server/src/tests.rs::action_status_route_returns_shell_feedback`.
- [x] SP42 recommends a disposition (e.g. Rollback for a high-score edit) and marks each verb available/unavailable from the session's capabilities — verified by `crates/sp42-core/src/live_operator.rs::preflight_recommends_rollback_for_high_score_edit`.
- [x] A missing action token is classified as "available after session refresh" rather than a hard failure — verified by `crates/sp42-core/src/live_operator.rs::preflight_classifies_missing_tokens_as_session_refresh` and `::retry_classifier_maps_codes_to_classes`.
- [x] The action toolbar maps a verb's preflight reasons into a tooltip and resolves the matching recommendation — verified by `crates/sp42-app/src/components/action_bar.rs::find_recommendation_returns_matching_kind`, `::find_recommendation_returns_none_for_missing_kind`, and `::tooltip_from_reasons_joins_reasons`.

## Alternatives

- **One generic "edit" verb instead of five named actions.** The shipped shape
  gives each disposition its own MediaWiki primitive (rollback, undo, patrol,
  template-tag, inline edit) so the operator's intent maps onto the right API
  call, the right token, and the right capability gate, rather than collapsing
  everything onto `action=edit`. The verb is carried explicitly in
  `SessionActionExecutionRequest.kind` and dispatched in
  `execute_session_action` (`action_routes.rs:238`).
- **Node-anchored content editing from day one.** A parser-anchored editor was
  the obviously-correct end state for the content-edit verbs, but the shipped
  patrol/revert mission only needed literal replacement; the design deferred the
  parser to ADR-0003, where the cost/benefit (Parsoid vs. WASM `wikiparser-node`
  vs. pure-Rust, plus a GPL licensing decision) is worked out. The shipped verbs
  use `replacen(…, 1)` (`action_routes.rs:378`, `:423`) as the interim.
- **A hash-bound node locator on the edit request.** ADR-0003 describes the
  end-state binding for content edits — a `WikitextEditor` locator (node-kind +
  document-order ordinal + the *expected node text* used as the anti-drift
  anchor), so a save re-grounds on the exact node it targeted. SP42 has **not**
  adopted that locator for the shipped content-edit verbs. Today's binding is
  weaker and is *not* a node locator: the operator's button-click is the
  human confirmation, a same-session CSRF header authorizes the POST
  (`validate_csrf_header`, `main.rs:486`), and the only on-wiki anchor is
  `baserevid` (the patrolled rev). The hash-bound locator is future work
  governed by ADR-0003.

## Risks

- **Wrong-target content edit on recurrence.** First-occurrence `replacen`
  edits the *first* literal match; if the operator's selected phrase or URL
  recurs earlier in the article, the wrong span is changed
  (`action_routes.rs:378`, `:423`). *Mitigation today:* none in code — there is
  no occurrence-count guard. ADR-0003 Decision 4 proposes an exactly-one guard;
  it is not yet implemented. (See Known gaps.)
- **TOCTOU between fetched text and saved base revision.** SP42 locates the
  needle in the *current* page text but saves with `baserevid` = the *patrolled*
  revision (`fetch_page_wikitext`, `main.rs:1024`; `action_routes.rs:348`,
  `:399`). *Mitigation today:* MediaWiki's own `baserevid` conflict detection on
  save; SP42 surfaces an edit-conflict API error as a failure
  (`parse_action_response_summary`). The window itself is not closed in SP42.
- **Brittle verbatim match fails the edit.** If the selected text is not
  byte-identical to current page text, the content-edit verbs refuse with
  `text-not-found` (`action_routes.rs:382`, `:427`). *Mitigation:* the operator
  sees a "rejected"/error status and the page is unchanged — a safe failure, but
  a usability cost.
- **Acting beyond the session's rights.** A verb the session cannot perform is
  disabled in the toolbar (`action_bar.rs:62`) and re-checked server-side per
  verb (`validate_action_request`, `action_routes.rs:762`), so an over-reach is
  refused with `403`. *Mitigation:* exists; the action-verb gate's coverage is a
  Known gap (below).
- **Forged/cross-site action on the execute-action route.** Every action POST
  requires an authenticated bridge session and the same-session CSRF header
  (`action_routes.rs:45`, `:50`). *Mitigation:* exists in code; the
  rejection path is not directly tested on *this* route (see Known gaps).
- **Silent best-effort patrol after a content edit.** After a successful tag or
  inline edit, SP42 patrols the original revision and ignores any failure
  (`patrol_original_edit_if_possible`, `action_routes.rs:435`). *Mitigation:*
  the primary edit still succeeds; the patrol side effect can silently no-op.

## Known gaps / drift

- **The action-verb capability/field re-check is untested at the unit level.**
  `validate_action_request` (`action_routes.rs:762`) — the per-verb required-field
  checks (title/target_user/selected_text) and the `can_rollback` / `can_patrol`
  / `can_undo` / `can_edit` gates that decide whether a *verb* runs — is exercised
  only through the live route handler. No unit test asserts that a missing field
  or an unmet capability for a verb yields `400`/`403`. (Scope note: this gap is
  the *action-verb* gate only; how the capability report itself is produced is
  owned by the sibling session-capability PRD and is not re-litigated here.)
- **No CSRF-rejection test on the execute-action route.** `validate_csrf_header`
  (`main.rs:486`) is a shared primitive, and the only CSRF-rejection test
  (`tests.rs::dev_session_delete_requires_csrf_for_cookie_session`, `tests.rs:544`)
  covers the `/dev/auth/session` DELETE route, not this one. No test POSTs a
  missing/forged CSRF header to the execute-action endpoint, so the CSRF gate
  *on the action route specifically* is uncharacterized. (The shared
  `validate_csrf_header` mechanism is owned and documented by the sibling
  session-capability PRD; this gap is scoped to its use on the action route.)
- **The two content-edit verbs have no tests.**
  `execute_inline_edit_action` (`action_routes.rs:360`, incl. the
  `replacen(…, 1)` at `:378`) and `apply_citation_template`
  (`action_routes.rs:411`, incl. the `text-not-found` rejection when
  `updated_text == page_text` at `:423`–`:431`) are entirely uncovered.
- **The undo ordering guard is untested.** `build_undo_request` refuses
  `undo_rev_id <= undo_after_rev_id` (`action_executor.rs:256`), but
  `builds_undo_request_body` only covers the success path; the rejection is
  uncharacterized.
- **ADR-0003 interim hardening not yet present.** Both content-edit sites still
  call `replacen(…, 1)` with no exactly-one-occurrence guard; ADR-0003
  Decision 4 (the ambiguity guard) is described but not implemented in the code
  read.
- **Base-revision drift is real, not just theoretical.** `baserevid` is the
  *patrolled* rev (`action_routes.rs:348`, `:399`) while the fetched wikitext is
  the *current* top rev (`fetch_page_wikitext`, `main.rs:1024`) — ADR-0003
  failure mode 2. The conflict behavior is untested.
- **Hand-rolled date for the citation-needed template.**
  `current_french_date` (`action_routes.rs:489`) derives month/year from epoch
  days by hand for `{{…|date=…}}` and is untested.
- **Citation-needed / inline-edit are not on the main action toolbar.** The
  toolbar (`action_bar.rs`) exposes only Rollback / Undo / Patrol / Skip; the
  content-edit verbs are reached from the diff-viewer context menu via
  `revision_artifacts.rs` effects (`:311`, `:358`), a less discoverable surface.
- **Patrol batch fan-out is untested.** The multi-id patrol loop
  (`action_routes.rs:260`–`:283`) issuing one call per `batch_rev_ids` entry has
  no test; `executes_patrol_through_http_trait` covers a single id only.
- **Content-edit binding is weak (no node anchor).** SP42 binds a shipped
  content edit only by the operator's button-click, the same-session CSRF header,
  and `baserevid`; there is no hash-bound node locator anchoring the edit to a
  specific `<ref>`/template node. The ADR-0003 `WikitextEditor` locator is the
  intended successor and is not yet implemented.
