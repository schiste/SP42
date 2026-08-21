# ADR-0010: Operator-confirmed content proposals (propose/confirm)

**Status:** Accepted
**Date:** 2026-06-09
**Author:** Luis Villa
**Summary:** Generated content edits follow a two-step propose/confirm pattern: a read-only proposal route computes the change and the operator explicitly confirms before it flows through the standard write path — nothing is written on proposal.

Spawned by PRD-0008 (bare-URL repair). PRD-0008 claimed no ADR number at draft time (its Resolved question 5) because the citation-verification series (ADR-0006–0009) was then unmerged; that series has since merged to main (PR #24), so this ADR takes the next free number, 0010.

## Context

PRD-0008 needs SP42 to *generate* article content (a filled citation
template) rather than only tag or revert it. Nothing generated may reach a
wiki without the operator seeing and confirming the exact bytes. ADR-0003
already gives us drift-guarded, node-anchored edits; ADR-0002 gives us a
bridge session that keeps tokens out of client processes. What was missing
is the contract for how a generated edit travels between "proposed" and
"applied".

## Decision

1. **Two-step propose/confirm.** A read-only proposal route computes
   `{proposals, declined}` for a revision; a separate authenticated apply
   route performs the write. Nothing is written during proposal generation.
2. **Proposals are replayable edit payloads.** Each proposal carries the
   ADR-0003 `WikitextNodeLocator` (kind, ordinal, `expected_text` anchor)
   plus the complete `replacement_wikitext`. Apply replays that payload
   verbatim — the server re-runs the anti-drift re-check and sends
   `baserevid`, so a changed article refuses (`node-drift` /
   `node-out-of-range`, HTTP 400 with `http_status: 409` in the body, zero
   wiki writes) rather than guessing.
3. **Declines are structured outcomes, not errors.** A reference that cannot
   get a usable proposal (`metadata-unavailable`, `no-usable-title`) stays a
   finding; one junk URL never fails a whole proposal response.
4. **Per-wiki presence gate.** A wiki opts in by naming the template in its
   config (`WikiTemplates.bare_url_citation`); the same check guards both
   routes (`bare-url-repair-not-enabled`). Production configs simply omit
   the key.
5. **Wire contracts live in `sp42-core`.** Request/response/proposal types
   are shared serde types (the `action_contracts` precedent), so the server
   and every shell speak one contract.

## Relation to ADR-0008 (the single write lane)

ADR-0008's Context asserts that SP42 has a single wiki-write lane: the
operator-confirmed action path (`SessionActionExecutionRequest` → `POST
/dev/actions/execute`). This ADR deliberately adds a second, sibling
operator-confirmed write surface (the bare-url apply route) rather than
introducing a new `SessionActionKind` verb:

- **Why not a new SessionActionKind:** Adding a variant ripples through
  exhaustive `match` statements across every shell, including wasm-gated code
  (`#[cfg(target_arch = "wasm32")]` in `sp42-app/src/pages/`) invisible to
  host builds. A missing case would ship undetected until runtime. This risk
  was deliberately avoided for the MVP; the decision is recorded in the
  implementation plan.
- **What is preserved from ADR-0008's principle:** Nothing reaches a wiki
  unreviewed. The apply route enforces the same gates as `post_execute_action`
  — session, CSRF, and the per-session edit-capability check — and the write
  goes through the same single writer (`execute_wiki_page_save`) with
  `baserevid` guard, making divergence impossible.
- **What is NOT carried over (MVP omissions):** Action-history recording
  (already in Non-Goals).
- **Supportive precedents from the merged series:** ADR-0008 Decision 7
  places contracts/logic in `sp42-core` (matches this ADR's Decision 5);
  ADR-0008's optional, config-driven, default-absent per-wiki field pattern
  (like `liftwing_url`) matches this ADR's Decision 4 presence gate; ADR-0007's
  structured abstention (`SourceUnavailable` is a verdict, not an error) is the
  precedent for this ADR's Decision 3 structured declines.
- **Future fold-into-the-action-lane:** If/when a future phase introduces a
  `SessionActionKind` for confirmed proposals, the shell wasm-visibility
  problem will have grown beyond this feature — that change can then fold
  both routes into a single `SessionActionKind` variant without regret.

## Consequences

- Any future "SP42 writes content" feature (citation repair from the
  verification pipeline, template fixes, typo repair) can reuse the same
  propose/confirm shape: locator + replacement + verbatim replay.
- The apply path inherits ADR-0003's guarantees wholesale; there is no new
  write machinery to audit.
- The proposal payload is self-describing, so a CLI, browser, or desktop
  shell can render a faithful "before/after" without server round-trips.
- Replaying stale proposals is safe by construction (refusal, not
  mis-targeting), at the cost of operators occasionally re-running preview.

## Non-Goals

- Batch or automatic application of proposals — every apply is one
  operator-confirmed payload.
- Production-wiki enablement (testwiki only in the MVP; frwiki enablement is
  a follow-on with per-wiki template/language mapping).
- Action-history logging for bare-URL applies (MVP omission, noted in the
  implementation plan).
