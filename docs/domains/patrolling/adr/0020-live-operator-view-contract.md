# ADR-0020: Live-operator-view contract — the server-assembled patrol payload

**Status:** Accepted
**Date:** 2026-07-12
**Author:** Luis Villa (drafted by Claude Code)

**As-built:** retroactive characterization of a shipped contract (PRD-0002).
There is no forward "closing PR"; this records the `GET /operator/live/{wiki_id}`
payload and its assembly as they exist, pinned by the `operator_live` tests.
Spawned from issue #18.

## Context

PRD-0002 (patrol review workflow) defines the operator-facing loop, but the
structural contract behind the review workbench had no ADR. The browser shell
does not fetch the queue, the selected diff, the scoring context, the action
readiness, and the session/backlog state as separate calls — it reads **one
payload** from `GET /operator/live/{wiki_id}`, and that payload's shape and
assembly order are load-bearing:

- Scoring depends on the diff (diff flags feed the scoring context), and the
  per-selection action preflight depends on the resulting score. Assembling
  these client-side would force three ordered round-trips and expose cross-call
  races.
- The queue is backed by a durable ingestion supervisor with persisted
  recent-changes checkpoints, not a per-request fetch, so consistency across
  concurrent requests is a server concern.

## Decision

### 1. One server-assembled payload: `LiveOperatorView`
`GET /operator/live/{wiki_id}` returns a single `LiveOperatorView`
(`sp42-patrol::live_operator_view`) composing: the ranked `queue`
(`Vec<QueuedEdit>`) and `selected_index`; the selection's `scoring_context`,
`diff` (`StructuredDiff`), `media_diff`, and `review_workbench`; `stream_status`
/ `backlog_status` (session/backlog state); `capabilities` / `auth`
(dev-auth readiness); `action_status` / `action_history` /
`action_preflight`; coordination summaries; telemetry; and a debug snapshot.
The client gets one consistent snapshot, never a set of racing calls.

### 2. The dependency chain is resolved server-side, in order
The assembly (in the `sp42-server` shell) is phased and telemetry-timed:
bootstrap → public documents → recent-changes → queue → selection. For the
selection it fetches the diff, analyzes it into scoring-context flags, folds in
the LiftWing risk, scores the edit, and **back-patches** that score into the
queue item so the preflight sees the enriched score. The ordering is an
implementation detail the client never has to reproduce.

### 3. The action preflight ships ready-to-execute requests, not booleans
`LiveOperatorActionPreflight` (`sp42-live`) carries, per candidate disposition,
a `LiveOperatorActionRecommendation { kind, request, available, recommended,
retry_class, reasons }`, where `request` is a ready-to-POST
`SessionActionExecutionRequest` (the action contract, ADR-0022). Availability
gates on capability-probe readiness first (nothing is available until
`capabilities.checked` with no probe error), then per-kind rights and token
readiness (rollback/patrol/CSRF). `retry_class` tells the client *how* to
recover — session refresh vs backoff vs operator change — rather than blindly
retrying. `recommended` is scoring-driven patrol judgment.

### 4. Auth short-circuits to 401 before assembly
A request with no resolvable session returns **401** (so the wasm client
re-gates to login) rather than letting the assembly surface a generic 502; any
genuine assembly failure maps to 502.

## Consequences

- The contract types are split by layer even though they serialize into one
  payload: the preflight *mechanism* and live runtime status are platform
  (`sp42-live`); the underlying value types (`QueuedEdit`, `ScoringContext`,
  `StructuredDiff`, capability reports, action reports) are platform
  (`sp42-platform`); the `LiveOperatorView` aggregation and the patrol reports
  it embeds are patrolling-domain (`sp42-patrol`); the assembly glue is the
  server shell. The recommendation policy (which kinds surface, the score
  thresholds) is patrol judgment living inside the platform preflight mechanism.
- The payload is broad by design — one fetch, one consistent view — at the cost
  of a large struct. Phase-granular telemetry (`LiveOperatorTelemetry`) makes
  the multi-fetch assembly observable rather than a black box.
- Pinned by `crates/sp42-server/tests/operator_live.rs` (real spawned binary
  against a mock MediaWiki: payload shape, checkpoint reuse, backlog advance,
  8-way concurrency) and the in-process handler tests in
  `crates/sp42-server/src/tests.rs` (401-without-session, preflight
  recommendation, cached backlog).

## Non-goals

- The disposition set, execute route, tokens, and CSRF/`baserevid` enforcement
  — that is the action contract, **ADR-0022**. This ADR only carries the
  ready-to-POST request through the preflight.
- Scoring internals (the composite score and policy) — **ADR-0021**.
- Coordination room state and fan-out — **ADR-0023** (summaries are embedded
  here read-only).
- The `crates/{platform,domains,shells}/` relocation — pending, ADR-0013.
