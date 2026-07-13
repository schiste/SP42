# SP42 Status

This document tracks the current implementation state by phase. It is meant to be updated as the codebase moves, so the README does not need to carry the full timeline.

## Phase 0

Foundations are complete:

- Cargo workspace and toolchain policy are in place
- `sp42-platform` owns shared contracts, traits, scoring primitives, and runtime logic (extracted from `sp42-core`, now a retiring re-export facade)
- `sp42-wiki` owns wiki config parsing, registry/default selection, fixtures, and capability profiles
- `sp42-live` owns EventStreams ingestion, recentchanges/backlog polling, live queue filtering, and live operator contracts
- action contracts are split from MediaWiki execution and server session adapters; a future `sp42-actions` crate should wait until shared neutral types avoid a crate cycle
- CI enforces formatting, linting, tests, coverage, and dependency checks
- ADR-0001 records the foundational architecture decisions

## Phase 1

The offline patrol engine is now effectively complete for local development:

- `sp42-live` implements EventStreams ingestion with typed filtering, timestamp normalization, persisted checkpoint restore, and batch-drain helpers
- `sp42-live` implements recentchanges/backlog polling with stricter checkpoint semantics and runtime query/checkpoint inspection helpers
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
- node-anchored wikitext editing (ADR-0003) is implemented: a `WikitextEditor`
  contract with a Parsoid-backed adapter; `InlineEdit` accepts an optional
  node locator, and the literal fallback refuses ambiguous matches
- bare-URL reference repair (PRD-0008) has a testwiki-gated propose/confirm
  slice: Citoid-backed citation proposals and verbatim-replay applies over
  `/dev/citation/*` bridge routes, with CLI preview/execute flag-modes
  (ADR-0010); the live test.wikipedia.org repair gate remains manual
- article-level citation verification (ADR-0011) reads a revision and verifies
  every URL-bearing citation into a `PageVerificationReport` over the read-only
  `/dev/citation/verify-page` route; a shared `sp42-reporting` renderer turns it
  into a per-citation report and the CLI `verify-page` subcommand prints it
  (text/markdown/json), defaulting to the latest revision. `frwiki`, `enwiki`,
  and `testwiki` are registered. The browser Citations tab that renders the same
  report is in review (PR #81).
- book-citation grounding (PRD-0009, ADR-0024) has its read-only resolve and
  grounding lanes: validated book identifiers (ISBN/OCLC/LCCN/OLID) are extracted
  from cite-template `data-mw`, `verify-page` resolves each book ref through the
  side-effect-free Open Library lookups (Books API catalog + Read API
  exact-vs-similar scan availability), and a resolved book with an exact-edition
  scan is grounded against Internet Archive search-inside snippets — the snippet
  body feeds the existing verdict panel (with a provenance-scoped short-body
  bypass), cited-page-first with whole-book fallback, page-anchored deep links,
  and the honest `not_supported` vs `SourceUnavailable` split; unresolved books
  stay skipped with a refined reason and a Books report section shows every
  resolution; the enrichment lane (Layer 3) is
  implemented as mechanism + fixture tests per ADR-0025 — deterministic
  ISBN-completion candidates listed read-only in the Books section, and the
  apply machinery (per-operator S3-key login, REST lane with 403 fallback to
  a fail-closed edit-form adapter, per-session lane cache, client-side
  refuse-on-drift, post-apply read-back) — with the write lane disabled and
  unwired until the ADR-0025 enablement gate passes
- the shared Wikidata entity read model (ADR-0016) is implemented as the
  platform `wikibase` module: endpoint-agnostic entity/statement parsing,
  label lookup and claim rendering (promoted from `sp42-mcp`'s
  `verify_wikidata_statement`, which now consumes it), a full-depth
  `EntityDiff` with the never-a-no-op honesty invariant, the `ContentDiff`
  routing sum, per-revision content-model classification/capabilities, and an
  additive `EditEvent.content_model`. The patrol surface is wired end to end
  (PRD-0011 MVP read path): the server revision fetch carries
  `rvprop=contentmodel` and serves a content-model-routed
  `/operator/content-diff` route (entity pairs carry an `EntityDiff` with
  labels resolved server-side in one batched `wbgetentities` call;
  media-reference extraction is not invoked for entity content), and the
  browser diff pane renders entity revisions through an `EntityDiffViewer`
  (classified label/description/alias/sitelink/statement change rows) while
  wikitext revisions keep the existing viewer unchanged. The queue-level
  gates are in place: ingestion seeds `EditEvent.content_model` from the
  site's per-namespace defaults (Wikidata ns 0/120 → item/property; talk
  pages stay wikitext), entity events score a uniform base with no wikitext
  heuristics — so Wikidata queues order chronologically over the
  bot-excluded-by-default stream — and no LiftWing revertrisk request is
  built for entity content. Reviewer patrol/rollback on `testwikidatawiki`
  is covered by a mock-write-path test; the **live** action acceptance gate
  on test.wikidata.org remains a manual dogfood step to record in the
  closing PR (PRD-0011).

## Current Verification

The workspace is currently kept green with:

- `./scripts/build-local.sh`
- `./scripts/build-frontend.sh`
- `./scripts/build-web-release.sh`
- `./scripts/package-vps.sh`
- `./scripts/check-focused.sh`
- `./scripts/dev-local.sh --smoke`
- `./scripts/ci-all.sh`
- `./scripts/build-desktop.sh --platform macos --debug`
- `cargo test --manifest-path crates/sp42-desktop/src-tauri/Cargo.toml`
- README/STATUS drift checks in CI
- `bash scripts/local-operator-smoke.sh` for the local operator flow
- targeted multi-user coordination validation inside the local operator smoke path
- `bash scripts/openlibrary-contract-smoke.sh` for the live Open Library /
  Internet Archive read-contract check (manual, network-touching, never CI;
  asserts exactly the response fields the PRD-0009 parsers read)
