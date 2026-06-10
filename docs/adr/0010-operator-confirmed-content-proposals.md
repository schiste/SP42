# ADR-0010: Operator-confirmed content proposals (propose/confirm)

**Status:** Proposed
**Date:** 2026-06-09
**Author:** Luis Villa

Spawned by PRD-0008 (bare-URL repair), which reserved this number at draft
time because the citation-verification series holds unmerged drafts through
ADR-0009.

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
