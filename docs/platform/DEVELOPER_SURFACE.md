# SP42 Developer Surface

This note is the short version of what the repo can do right now.

## Localhost Server Report

The server exposes a local readiness and operator-report surface for single-user development:

- `GET /healthz` reports local readiness and capability-probe availability
- `GET /debug/runtime` reports uptime, bootstrap state, auth state, and coordination data
- `GET /debug/summary` returns the server snapshot used by browser and operator tooling
- `GET /dev/auth/bootstrap/status` reports whether the local `.env.wikimedia.local` token path is available
- `GET /dev/auth/capabilities/{wiki_id}` returns a capability report for the selected wiki
- `GET /operator/storage/layout/{wiki_id}` returns the canonical on-wiki public storage layout and sample rendered pages

Runtime modes, bind addresses, public URLs, frontend API base URL behavior, and
local Wikimedia credentials are documented in
[docs/platform/RUNTIME_CONFIGURATION.md](RUNTIME_CONFIGURATION.md).

## Patrol Scenario Reporting

Patrol scenarios are currently reported through shared action and reporting paths:

- the core action workbench prepares rollback/patrol/undo reports and training exports
- `sp42-live` owns EventStreams ingestion, recentchanges/backlog polling, live query parsing, queue filtering, and live action preflight contracts
- the CLI exposes queue, action workbench, context, backlog, stream, parity-report, and operator-report modes
- the browser dashboard reuses the same shared reporting surface and shared shell-state model
- the desktop shell now renders the same shared shell-state and scenario/digest summaries as the other targets

## Live Boundary

Live-domain logic lives in `sp42-live`: EventStreams ingestion, stream cursor
runtime, recentchanges polling, backlog checkpoint runtime, live operator query
defaults, queue filtering, action preflight summaries, and live telemetry/status
contracts. The server owns runtime orchestration around that domain: route
handling, auth/session lookup, capability probing, storage handles, supervisor
tasks, HTTP clients, and public response assembly. The browser owns local UI
state and consumes the same `LiveOperatorQuery` and filtering helpers rather
than reimplementing queue matching.

## Action Boundary

Action request, response, token-kind, and retryability contracts live in
`sp42-core::action_contracts`. MediaWiki request builders, response parsing, and
HTTP-client execution live in `sp42-core::action_executor`. Server-side
session lookup, CSRF validation, capability checks, token fetching, content-edit
adapters, and action history writes stay in `sp42-server`.

This is the stabilization step before a future `sp42-actions` crate. The split
should wait until the shared type boundary avoids a crate cycle with
`sp42-core`, or until the remaining core callers move to their target domain
crates. Current validation is deterministic and local-first; authenticated live
Wikimedia write validation still requires real credentials and should be called
out in PR notes when action execution changes.

Node-anchored content editing (ADR-0003) follows the same split: the
`WikitextEditor` contract, locator types, and the deterministic scripted
double live in `sp42-core::wikitext_editor`; the Parsoid REST adapter lives
in `sp42-server::parsoid_editor`.

## Local Operator Smoke Flow

The repo includes a single local operator smoke entrypoint:

- `./scripts/local-operator-smoke.sh` runs targeted live/backlog tests, the authenticated multi-user coordination websocket test, builds the server/CLI/desktop/browser shells with the shared workspace cache, starts the localhost server, and exercises the raw server readiness/operator/history surfaces plus the CLI parity-report and session-digest paths
- it also checks the browser wasm build, the desktop shell snapshot, and the Tauri shell contract
- it stays local-first and does not require Wikimedia credentials
- it is the fastest way to sanity-check the current operator surface end to end without live dependencies

## Browser Telemetry and PWA

The browser shell is no longer just a renderer:

- it shows queue, diff, auth, coordination, runtime, and status panels
- it surfaces browser telemetry from the local server reports
- it includes a PWA shell for installability, update activation, iOS guidance, and offline-safe shell behavior

## Current Milestone Boundaries

Still pending:

- live Wikimedia OAuth registration and browser PKCE verification
- live authenticated API validation against Wikimedia
- final end-to-end browser/server flows against real credentials

Already in place:

- shared offline patrol logic
- coordination relay and room snapshots
- authenticated multi-user localhost coordination validation
- local token bootstrap path for single-user testing
- canonical on-wiki public storage conventions for personal and shared SP42 documents
- PWA shell, offline fallback, manifest shortcuts, and telemetry surfaces
- a local operator smoke flow that exercises the server, CLI, desktop shell, and Tauri shell together
