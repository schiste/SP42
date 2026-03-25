# SP42 Status

This document tracks the current implementation state by phase. It is meant to be updated as the codebase moves, so the README does not need to carry the full timeline.

## Phase 0

Foundations are complete:

- Cargo workspace and toolchain policy are in place
- `sp42-core` owns shared contracts, traits, errors, and pure logic
- CI enforces formatting, linting, tests, coverage, and dependency checks
- ADR-0001 records the foundational architecture decisions

## Phase 1

The offline patrol engine is now effectively complete for local development:

- EventStreams ingestion is implemented with typed filtering, timestamp normalization, persisted checkpoint restore, and batch-drain helpers
- recentchanges/backlog polling is implemented with stricter checkpoint semantics and runtime query/checkpoint inspection helpers
- scoring, queueing, diffing, and action-workbench preparation are implemented
- rollback, patrol, undo, and token flows now validate MediaWiki API-level error payloads instead of trusting HTTP success alone
- training export, user-risk parsing/cache, and LiftWing-aware context hooks are wired

## Phase 2

Coordination and shared runtime state are now effectively complete for local development:

- MessagePack coordination codec exists
- coordination state reduction is deterministic and shared
- a shared coordination runtime now couples transport with deterministic local room state
- the localhost coordination server exposes room snapshots, decoded room state, richer room inspections, readiness reports, and capability diagnostics
- browser coordination panels now surface collaboration narratives rather than only raw counts
- an authenticated multi-user websocket integration test now validates cross-client claim, presence, action, and race-resolution propagation

## Phase 3

Target shells are now effectively complete for local development and include an interactive patrol rail:

- the browser app has a dashboard, inspector panels, runtime adapters, and a shared shell-state panel
- the browser shell also exposes telemetry, PWA/installability state, and local action history
- the CLI has queue, action workbench, context, backlog, stream, parity-report, and operator-report modes with action-history visibility and shared shell-state rendering
- the desktop shell now renders the same shared shell-state, parity report, and operator summaries from core logic

## Phase 4

Live Wikimedia integration is still gated by external credentials and verification:

- the OAuth/PKCE flow structure exists
- the localhost dev-auth bridge supports a single-user local token path and canonical empty bootstrap payload
- final browser auth and live API validation still require real Wikimedia-side values
- the server keeps live Wikimedia calls behind probe/report boundaries so tests stay local-first
- ADR-0002 records the local dev-auth bridge contract and browser/server handoff

## Phase 5

PWA packaging and offline installability are now effectively complete for local development:

- manifest, shortcuts, branded icons, and a maskable icon are in place
- the service worker keeps auth/debug/coordination/API traffic out of caches while preserving the shell offline
- the browser shell exposes install, offline, and update state separately, including waiting-worker activation
- browser-specific guidance now covers Chromium install prompts, iOS Add-to-Home-Screen behavior, and standalone shell operation

## Current Verification

The workspace is currently kept green with:

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo build --workspace`
- `cargo build -p sp42-app --target wasm32-unknown-unknown`
- `cargo test --manifest-path crates/sp42-desktop/src-tauri/Cargo.toml`
- `cargo clippy --manifest-path crates/sp42-desktop/src-tauri/Cargo.toml --all-targets --all-features --no-deps -- -D warnings`
- `cargo audit`
- `cargo deny check bans licenses sources`
- README/STATUS drift checks in CI
- `bash scripts/local-operator-smoke.sh` for the local operator flow
- targeted multi-user coordination validation inside the local operator smoke path
