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

## Patrol Scenario Reporting

Patrol scenarios are currently reported through shared action and reporting paths:

- the core action workbench prepares rollback/patrol/undo reports and training exports
- the CLI exposes queue, action workbench, context, backlog, stream, parity-report, and operator-report modes
- the browser dashboard reuses the same shared reporting surface and shared shell-state model
- the desktop shell now renders the same shared shell-state and scenario/digest summaries as the other targets

## Local Operator Smoke Flow

The repo includes a single local operator smoke entrypoint:

- `./scripts/local-operator-smoke.sh` runs targeted `sp42-core` backlog tests, the authenticated multi-user coordination websocket test, builds the server/CLI/desktop/browser shells, starts the localhost server, and exercises the raw server readiness/operator/history surfaces plus the CLI parity-report and session-digest paths
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
