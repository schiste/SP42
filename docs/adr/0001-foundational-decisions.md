# ADR-0001: Foundational architectural decisions

**Status:** Accepted
**Date:** 2026-03-23
**Author:** Christophe Henner

## Context

SP42 is a Wikipedia patrolling platform intended to replace/unify LiveRC, Huggle, SWViewer, Twinkle, RedWarn/Ultraviolet, RTRC, AntiVandal, and STiki. It must be browser-native, cross-wiki, real-time, ML-integrated, and built to last a decade. This ADR captures all foundational decisions made before the first line of code.

## Decisions

### 1. Full Rust, no TypeScript

The entire project is written in Rust. The UI uses Leptos (Rust reactive framework compiling to Wasm). There is no TypeScript layer.

**Rationale:** Single type system eliminates type duplication and the wasm-bindgen bridge layer. One language compiles to all five targets (browser, PWA, Tauri desktop, CLI, server). Constitution enforcement is easier in Rust (no `as any`, no type escape hatches). AI-assisted development is more efficient with one language.

**Trade-off:** Narrower contributor pool. Mitigated by: funded core team does 95% of code; volunteers contribute configs, translations, docs, testing.

**Alternatives considered:** TypeScript UI shell with Rust Wasm core (original spec v1-v2). Rejected because: bridge layer complexity, type duplication, harder Constitution enforcement, cannot compile to native desktop.

### 2. Leptos for UI

Leptos 0.7+ is the reactive UI framework. It compiles to Wasm, uses fine-grained reactivity (signals), and supports SSR if needed later.

**Alternatives considered:** Yew (older, virtual DOM — less performant), Dioxus (newer, less ecosystem), Sycamore (smaller community). Leptos chosen for: maturity, performance, active development, community size.

### 3. Apache 2.0 license

**Rationale:** Broad compatibility with the Rust ecosystem (most crates are Apache 2.0 / MIT dual-licensed). Permissive enough for wide adoption. WMF's own code is GPL v2+ but external tools commonly use Apache/MIT.

### 4. French Wikipedia as first target

**Rationale:** LiveRC 2.0 successor narrative provides strongest community buy-in. The fr-wp patrolling community produced a detailed specification in 2022 that was never realized. SP42 fulfills that promise.

### 5. Toolforge as primary hosting

Static frontend assets and coordination server hosted on Wikimedia Toolforge (Kubernetes). Free, trusted, community-governed.

**Open question:** Verify that Toolforge supports persistent WebSocket connections for the coordination server. If not, fallback to a small VPS (€5/month).

### 6. No initial funding

Build Phase 0 (working prototype) before applying for grants. A demo is worth a thousand proposals.

**Funding targets after Phase 0:** Wikimédia France (€80-100k), WMF Community Fund ($80-150k).

### 7. Trait-based abstraction for all external I/O

All external dependencies (HTTP, EventSource, storage, clock, RNG, WebSocket) are accessed via trait interfaces defined in `sp42-core/src/traits.rs`. The core library has zero I/O dependencies. This enables:
- Unit testing with mocks (Constitution Article 1)
- Deterministic behavior (Constitution Article 2)
- Compilation to all targets from one codebase
- Swapping implementations without touching business logic

### 8. EventStreams as primary data source

MediaWiki EventStreams (SSE at stream.wikimedia.org) is the primary real-time edit feed. The MediaWiki list=recentchanges API is the secondary source for slow patrol / backlog mode.

### 9. LiftWing as ML scoring provider

LiftWing (successor to ORES) at api.wikimedia.org provides damage probability scores. Supplementary to local scoring engine, never required. Scores cached in IndexedDB keyed by rev_id (immutable per revision).

### 10. Working title: SP42

All branding strings live in `sp42-core/src/branding.rs`. Renaming is a one-file change plus directory rename. Final name TBD.

## Consequences

- The Rust toolchain (rustup, cargo, wasm-pack, trunk) is required for all development
- Contributors must know Rust (or be willing to learn) for code contributions
- The wasm-bindgen bridge layer is eliminated (Leptos handles Wasm compilation internally)
- Foundation build order is: traits → OAuth → EventStreams+EditEvent → Leptos layout shell (4 foundations, not 5)
- All five deployment targets share one codebase with zero code duplication
