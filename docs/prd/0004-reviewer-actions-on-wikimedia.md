# PRD-0004: Reviewer actions on Wikimedia

**Author:** Luis Villa
**Date:** 2026-06-05
**State:** Implemented
**As-built:** retroactive characterization of an already-shipped feature (no forward "closing PR").
**Related ADRs:** ADR-0003 (node-anchored wikitext editing — governs the *mechanism* of the content-edit dispositions; the action contract itself has no ADR yet, see Known gaps)
**Discussion:** https://github.com/schiste/SP42/pull/4

## Scope boundary

This PRD characterizes **what each reviewer disposition means on the wiki** and **how the operator knows it landed** — the user-facing semantics of acting on a reviewed revision. It deliberately excludes two adjacent concerns:

- **Choosing a disposition** — the action toolbar, the one-key shortcuts, and the remove-from-queue-and-advance cadence — is the review *workflow*, characterized in **PRD-0002**. This PRD picks up at "the operator has decided" and describes what the decision does.
- **How a disposition is carried out** — the action contract (`SessionActionKind` and the execute-action route), token acquisition, the CSRF/`baserevid` enforcement, request building, and the content-edit text replacement — is *implementation*, not user-facing meaning. The content-edit *editing mechanism* is governed by **ADR-0003**; the broader action contract has **no ADR of its own yet** (see Known gaps). This PRD references that mechanism, it does not specify it.

## Problem

An SP42 operator works a ranked queue of recent-changes revisions and, for each, must dispose of it on the wiki — revert vandalism, accept a good edit, mark it reviewed, or flag a problem — under their own Wikimedia account and rights, without leaving the review surface. Before this feature SP42 could surface and rank revisions but offered no path from the review surface to an on-wiki disposition: the operator would leave SP42, repeat the action by hand on the wiki, and lose their queue position and context. This is for the **experienced reviewer/patroller** acting under their own account on a configured wiki (e.g. `frwiki`), one revision at a time.

## Proposal

The operator can dispose of the selected revision in five ways. Each has a distinct **on-wiki meaning** — the operator chooses among them by intent, and SP42 carries the chosen disposition out under the operator's authenticated session and reports the outcome.

- **Revert a user's whole run (rollback)** — reverts all consecutive most-recent edits by the same author in a single step, attributed to the operator. The on-wiki result is a one-step mass revert; it is the standard fast path for clear single-author vandalism. Requires the operator's rollback right.
- **Undo one revision** — reverses a single specific revision while preserving later edits, for a targeted bad change that is not the author's whole run. Requires the edit right.
- **Mark reviewed (patrol)** — records that the operator has reviewed the revision. It changes no article content; its only effect is to clear the revision from patrol queues. A single review can apply across a group of revisions the queue had merged. Requires the patrol right.
- **Flag for a citation (tag citation-needed)** — annotates an operator-selected claim with the wiki's citation-needed template (e.g. `{{refnec|…|date=…}}` on `frwiki`), changing what *readers and editors* see on the article, then marks the original revision reviewed. Requires the edit right.
- **Fix inline (inline edit)** — replaces an operator-selected span of article text with corrected text, then marks the original revision reviewed. Requires the edit right.

**What the operator confirms, and what they see.** The operator commits a disposition by invoking it on the review surface; their review note becomes the edit summary, and for the two content dispositions their selected span (and, for an inline fix, the replacement text) is what gets written. SP42 then shows an operator-readable outcome — *accepted*, *rejected*, a MediaWiki "no change" warning, or a *retryable* error the operator can re-try — and records the result in the session's action history. The operator never handles a token or a base revision by hand.

**The on-wiki result is the operator's, under their identity.** Every disposition lands under the operator's own authenticated Wikimedia session and rights, attributed to them, with their note as the summary — never an autonomous or anonymized edit. A disposition the operator's session is not entitled to perform is not offered, and is independently refused server-side. *(How identity, grants, and capabilities are established is **PRD-0005**; this PRD relies on that surface to gate which dispositions are offered.)*

**Relation to ADR-0003 (content-edit fidelity).** The two content dispositions (inline fix, citation flag) are carried out today by literal first-occurrence text replacement on the fetched wikitext. That mechanism is adequate for the shipped patrol mission but can target the wrong occurrence and cannot anchor an edit to a specific reference or template node. **ADR-0003** records the decision to move these dispositions onto node-anchored editing for fidelity. This PRD documents the *operator-visible behavior and its risks today* (below); ADR-0003 owns the mechanism's future. Rollback / undo / patrol author no article text and are unaffected.

## Definition of Done

Each item is an operator-observable behavior that is **already true**, bound to an existing test. (The underlying action-execution *contract* — request construction, token acquisition, payload serialization — is additionally unit-tested in `crates/sp42-core/src/action_executor.rs`, but is mechanism owned by the not-yet-written action-contract ADR; see Known gaps.)

- [x] A chosen disposition (rollback, undo, patrol, page-save) actually executes against the wiki and a success body is accepted — verified by `crates/sp42-core/src/action_executor.rs::executes_rollback_through_http_trait`, `::executes_patrol_through_http_trait`, `::executes_undo_through_http_trait`, `::executes_page_save_through_http_trait`.
- [x] A non-success HTTP status is surfaced to the operator as a failure, not a silent accept — verified by `…action_executor.rs::rejects_non_success_http_status`.
- [x] A MediaWiki error returned inside a 2xx body (e.g. `badtoken`) is surfaced as a failure with its code/info — verified by `…action_executor.rs::rejects_action_response_with_api_error_even_on_2xx`.
- [x] A retryable API error (e.g. `maxlag`, `ratelimited`) is marked retryable so the operator can re-try — verified by `…action_executor.rs::marks_retryable_api_errors`.
- [x] A MediaWiki "no change" edit is reported as *not accepted*, not a false success — verified by `…action_executor.rs::parse_action_response_detects_nochange` and `::parse_action_response_normal_edit_is_not_nochange`.
- [x] The operator's review note flows into the recorded disposition's feedback — verified by `crates/sp42-server/src/action_routes.rs::action_feedback_includes_rationale_summary`.
- [x] Each disposition's outcome is recorded in, and read back from, the session's action history — verified by `crates/sp42-server/src/tests.rs::action_history_route_returns_recorded_entries`.
- [x] The session's action status aggregates outcomes and surfaces the latest result to the operator — verified by `…tests.rs::action_status_route_returns_shell_feedback`.
- [x] A disposition the session cannot perform is marked unavailable (rather than silently failing), and a missing token is classified as "available after a session refresh" — verified by `crates/sp42-core/src/live_operator.rs::preflight_classifies_missing_tokens_as_session_refresh` and `::retry_classifier_maps_codes_to_classes`.

*(The score-gated **recommendation** of a disposition — e.g. suggesting rollback for a high-score edit — is scoring semantics owned by **PRD-0003**; the toolbar and keys that **choose** a disposition are workflow owned by **PRD-0002**. This PRD owns only what the chosen disposition does and how its outcome is reported.)*

## Alternatives

- **One generic "edit" verb instead of five dispositions.** The shipped shape gives each disposition its own on-wiki meaning and its own entitlement gate, so the operator's intent maps onto the right outcome — mass-revert vs single-undo vs mark-reviewed vs annotate vs fix — rather than collapsing onto a generic edit. The cost is a richer action contract, whose *structure* is implementation (and wants an ADR it does not yet have — see Known gaps).
- **Node-anchored content editing from day one.** A parser-anchored editor was the right end state for the content dispositions, but the shipped patrol mission only needed literal replacement; the design deferred the fidelity question to **ADR-0003** (Parsoid vs WASM `wikiparser-node` vs pure-Rust, plus the licensing decision). The operator-visible cost of the interim is in Risks.

## Risks

(User-facing consequences as shipped; the code mechanisms behind them are ADR-0003 / implementation.)

- **Wrong-target content edit on recurrence.** A content disposition changes the *first* literal match of the selected span; if that span recurs earlier in the article, the wrong occurrence is changed. *Mitigation today:* none — there is no occurrence guard. ADR-0003's interim ambiguity guard is not yet implemented (Known gaps).
- **Edit-conflict window on content edits.** SP42 locates the span in the *current* page text but saves against the *patrolled* revision as the base, so a concurrent edit between review and save is possible. *Mitigation:* MediaWiki's own base-revision conflict detection on save, which SP42 surfaces as a failure; the window itself is not closed in SP42.
- **A content edit fails safe on a non-exact match.** If the selected span is not byte-identical to current page text, the disposition is refused with "text not found" and the page is left unchanged — a safe failure, but a usability cost.
- **Acting beyond the session's rights.** A disposition the session cannot perform is not offered, and is independently refused server-side. *Mitigation:* exists; its unit-test coverage is a Known gap.
- **Silent best-effort review after a content edit.** After a successful citation flag or inline fix, SP42 marks the original revision reviewed and ignores any failure of *that* step; the primary edit still succeeds, but the review side-effect can silently no-op.

## Known gaps / drift

- **The action contract has no ADR.** ADR-0003 governs only the content-edit *editing mechanism*. The broader action contract — the set of dispositions (`SessionActionKind`), the execute-action route, token acquisition, and the CSRF/`baserevid` enforcement — is a public-contract concern that, per `docs/prd/README.md`, warrants its own ADR, but none exists; its structural decisions live only in code. *(This PRD owns the dispositions' user-facing meaning; the contract's structure should become an ADR this PRD links.)*
- **The per-disposition entitlement re-check is untested at the unit level.** The server's per-verb required-field and capability gate (`validate_action_request`, `action_routes.rs:762`) is exercised only through the live route; no unit test asserts that a missing field or unmet entitlement yields `400`/`403`.
- **No forged-request rejection test on the execute-action route.** The route requires an authenticated session and a same-session CSRF header, but no test POSTs a missing/forged header to *this* endpoint (the only such test covers the session-delete route). *(The CSRF mechanism itself is PRD-0005 / implementation.)*
- **The two content dispositions have no tests.** The inline-fix and citation-flag paths (`action_routes.rs:360`, `:411`), including the "text not found" rejection, are entirely uncovered.
- **The undo ordering guard is untested.** SP42 refuses an undo whose target is not strictly newer than the prior revision (`action_executor.rs:256`), but only the success path is tested.
- **ADR-0003 interim hardening not yet present.** The content dispositions still do unguarded first-occurrence replacement; ADR-0003's exactly-one-occurrence guard is described but not implemented.
- **Hand-rolled date for the citation flag.** The citation-needed template's date is derived from epoch days by hand (`current_french_date`, `action_routes.rs:489`) and is untested.
- **Content dispositions are not on the main action toolbar.** The toolbar exposes Rollback / Undo / Patrol / Skip; the content dispositions are reached from the diff-viewer context menu — a less discoverable surface.
- **Multi-revision patrol fan-out is untested.** The loop issuing one patrol call per merged revision id (`action_routes.rs:260`) has no test; the patrol execution test covers a single id only.
