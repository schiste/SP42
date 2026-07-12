# ADR-0022: Reviewer-action contract — dispositions, execute route, tokens, CSRF/baserevid

**Status:** Accepted
**Date:** 2026-07-11
**Author:** Luis Villa (drafted by Claude Code)

**As-built:** retroactive characterization of a shipped contract (PRD-0004).
Records the action envelope — the disposition set, the execute route, token
acquisition, and CSRF/`baserevid` enforcement — as it exists, pinned by the
contract, executor, and write-path tests. Spawned from issue #20.

## Context

PRD-0004 (reviewer actions on Wikimedia) owns the dispositions' user-facing
meaning. ADR-0003 governs only how a content edit *mutates wikitext*, and
ADR-0010 governs the propose/confirm *generation* pattern — neither records the
**action envelope** around them: which dispositions exist, the route that runs
them, how tokens are acquired, and how a write is authorized and bound to the
reviewed revision. That contract is a reusable platform mechanism and warrants
its own ADR.

## Decision

### 1. The disposition set: `SessionActionKind`
Six variants (`sp42-platform::action_contracts`): `Rollback`, `Patrol`, `Undo`
(no wikitext mutation — `action=rollback`/`patrol`/`edit&undo`), and the content
writes `TagCitationNeeded`, `InlineEdit`, `FlagCitation` (PRD-0014). Each has a
kebab wire `label()`. Companion contract types live in the same module:
`TokenKind {Rollback, Patrol, Csrf}`, the per-verb request structs (each content
write carries `baserevid: Option<u64>`), `SessionActionExecutionRequest` /
`Response`, and the `is_retryable_action_api_error()` retryability predicate
(`maxlag`/`readonly`/`ratelimited`/DB/`failed-save` retryable; `badtoken`/
`permissiondenied` not).

### 2. One execute route, capability-gated before any wiki call
`POST /dev/actions/execute` takes a `SessionActionExecutionRequest` and, in the
`sp42-server` shell (`action_routes.rs`): resolves the bridge session (401 if
none) → validates the app-layer CSRF header → checks the disposition against the
session's `DevAuthCapabilityReport` (a right the session lacks is refused
403/400 **before** any MediaWiki request) → dispatches by kind.

### 3. Tokens are acquired server-side, per verb, just-in-time
Each dispatch arm fetches its own MediaWiki token (`Rollback`/`Patrol`/`Csrf`)
via `action_executor::execute_fetch_token` immediately before the call. The wire
request carries **no token material** — a pinned invariant
(`session_action_contract_serializes_without_token_material`) — so tokens never
round-trip through the client. The bearer client is built from the bridge
session's access token; the auth substrate is the dev-auth bridge (ADR-0002)
locally and Wikimedia OAuth (ADR-0014) for real identity.

### 4. Two distinct CSRF points, plus `baserevid` binding
- **App-layer CSRF:** `validate_csrf_header` compares an `X-CSRF` header against
  the bridge session's stored token (403 on missing/mismatch). This binds the
  request to the session and is separate from the MediaWiki edit token of §3.
- **`baserevid`:** every content write binds the save to the reviewed revision
  (`baserevid: Some(payload.rev_id)`), so MediaWiki rejects it on an edit
  conflict against a newer revision.
- **Zero-write-on-refusal:** node-anchored edits go through
  `replace_node_or_refuse`, and a drift/out-of-range refusal returns a 409-in-body
  error **before** any save is attempted; an API error inside a 2xx is surfaced as
  failure, never silently accepted; `nochange` is reported as `accepted: false`.

## Consequences

- The contract is a platform mechanism: the pure request/response/retry types
  (`action_contracts`) and the request-building/execution over an injected
  `HttpClient` (`action_executor`) live in `sp42-platform`; the domains consume
  them by re-export, and the server shell owns the route, session/CSRF gating,
  and action history. The module doc states the split: pure contracts inside,
  session/token handling outside.
- Content writes are two-token: a CSRF token to save, then an opportunistic
  patrol token to auto-mark the original revision reviewed — patrol failure is a
  logged warning, not an action failure.
- Pinned by contract tests (`action_contracts.rs`: no-token serialization,
  retryability, node-locator round-trip), executor tests (`action_executor.rs`:
  request bodies, HTTP-trait execution, non-2xx and API-error-in-2xx rejection,
  the `replace_exactly_once` refusals), and route/write-path tests
  (`sp42-server/src/tests.rs`: baserevid saves, drift/out-of-range/gate zero-write
  refusals, error-code mapping).

## Non-goals

- How a content edit mutates wikitext (the `WikitextNodeLocator`, drift refusals)
  — **ADR-0003**. This ADR only carries `node_locator` through the envelope.
- How generated content travels from proposed bytes to an applied edit — the
  propose/confirm pattern, **ADR-0010** (its apply half reuses this write path).
- The OAuth grant/token derivation itself — **ADR-0014**; the dev-auth bridge —
  **ADR-0002**.
- `FailedVerification` is wired but refuses with `501` pending an
  insert-after-`<ref>` primitive — an accepted, tested gap, not part of this
  contract's shipped surface.
