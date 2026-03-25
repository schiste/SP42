# SP42 Constitution

> These are laws, not guidelines. Code that violates them is rejected in review. Contributors who disagree propose amendments (Article 12). This Constitution exists because LiveRC died from accumulated compromise. We will not repeat that.

---

## Article 1 — Everything is testable

**1.1** No untestable code may be merged. Every module, function, and component must have automated tests. If code cannot be tested, it is designed wrong and must be refactored.

**1.2** Testing tiers:

| Tier | Tool | Requirement |
|------|------|-------------|
| Unit tests | `cargo test` | Every public function tested. Every scoring/filtering branch tested. Coverage ≥90% on sp42-core. |
| Integration tests | `cargo test --features integration` | Every API action has a mock-server test. |
| End-to-end tests | Playwright or cargo-based browser tests | Core workflows against test wiki (MediaWiki Docker in CI). |
| Property-based tests | `proptest` | Scoring monotonic in all signal directions. Queue dequeues highest. Codec round-trip is identity. |
| Component tests | `leptos::test` or `wasm-pack test` | Every Leptos component renders correctly with mock data. |

**1.3** Test isolation: All external dependencies (HTTP, storage, clock, RNG, WebSocket) behind trait interfaces. Tests use mock implementations. No network in unit tests.

**1.4** Determinism: Injected `Clock` trait (no direct `SystemTime::now()`). Injected `Rng` (no `rand::thread_rng()`). Explicit ordering of HashMap iterations when order matters. No sleep/timeout in tests.

---

## Article 2 — Deterministic behavior

**2.1** Same input, same output. Given the same edit event, configuration, and user score state, the scoring engine must produce the same composite score. Enforced by property-based tests on every commit.

**2.2** State is explicit. All mutable state in well-defined containers: priority queue, user score map, whitelist, session history, configuration. No hidden global mutable state. No module-level `static mut`.

**2.3** Side effects at the edges. Business logic (scoring, filtering, diffing, queue management) is pure. Side effects (API calls, IndexedDB writes, WebSocket sends, DOM manipulation) happen only at the boundary layer. `sp42-core` has no dependency on `web-sys`, `js-sys`, or any I/O crate.

**2.4** Error boundaries. Every Leptos panel is wrapped in `<ErrorBoundary>`. A panic in the scoring engine shows an error in the queue panel but the diff viewer and action panel keep working.

---

## Article 3 — Observability

**3.1** Structured logging via the `tracing` crate with structured spans and events. Levels: ERROR (unrecoverable), WARN (unexpected but handled), INFO (significant lifecycle), DEBUG (operation trace), TRACE (per-event granularity).

**3.2** Every decision is logged. Scoring signals at DEBUG. Filter reasons at DEBUG. Actions at INFO. Coordination messages at DEBUG. A developer must be able to reconstruct why any edit appeared (or didn't) in the queue.

**3.3** Error context chains. Errors carry full context: action, target, HTTP status, API error code, request parameters. `thiserror` for typed errors with `Display`. No swallowed errors. No empty catch equivalents.

**3.4** Debug panel. Collapsible Leptos component showing: live structured log stream (filterable by level/module), queue depth, scoring breakdown for current edit, coordination status, LiftWing cache hit rate, IndexedDB usage.

**3.5** Performance tracing. Timing spans on: event processing, scoring, diff computation, LiftWing round-trip, API round-trip, UI render. Visible in debug panel. Exportable.

---

## Article 4 — Decisions live with the code

**4.1** Architecture Decision Records (ADRs) in `docs/adr/NNNN-title.md`. Records: context, decision, alternatives, consequences. Immutable once merged — reversals get a new ADR that supersedes.

**4.2** No decisions in chat, email, or meetings only. "We decided in the meeting to use X" → "Link to the ADR or it didn't happen."

**4.3** Comments explain why, not what. If code needs a comment explaining what it does, it's too complex. Acceptable: `// LiftWing returns 0.0 for unscorable edits; treat as neutral, not safe.` Not acceptable: `// increment counter`.

**4.4** Documentation is tested. All code examples in docs compile and run in CI via Rust doc-tests.

---

## Article 5 — Zero tolerance CI

**5.1** No code merges without passing every CI check. No "merge with failing CI." The pipeline is the single source of truth.

**5.2** CI checks (all must pass):

| Check | Tool | Rule |
|-------|------|------|
| Compilation | `cargo build` (wasm32 + native) | Zero errors. Zero warnings. `deny(warnings)` in Cargo.toml. |
| Linting | `clippy` | Zero warnings. `clippy::pedantic` enabled. `#[allow]` prohibited except with approved issue link + comment. |
| Formatting | `rustfmt` | Exact match. No custom overrides. |
| Tests | `cargo test` + `wasm-pack test` | All pass. No `#[ignore]` without issue link. |
| Coverage | `cargo-tarpaulin` or `llvm-cov` | sp42-core: ≥90%. No merge that decreases coverage without exemption. |
| Dependency audit | `cargo audit` | Zero known vulnerabilities. Blocks pipeline. |
| License check | `cargo-deny` | All deps compatible with Apache 2.0. |
| Doc tests | `cargo test` (doc-tests) | All examples compile and run. |
| Wasm size | Custom check | <800KB uncompressed, <400KB gzipped. Regression blocks. |
| E2E | Playwright | Core workflows pass. On merge to main only. |

**5.3** Prohibited patterns (rejected in review):

- `#[allow(unused)]` or `#[allow(dead_code)]` without approved issue link
- `unwrap()` in production code — use `expect("descriptive message")` or `?`
- Empty `match` arms
- `TODO` without issue link
- Commented-out code (delete it; git preserves history)
- `unsafe` blocks without `// SAFETY:` comment and lead approval

---

## Article 6 — Abstraction and no duplication

**6.1** Single source of truth. Every type, constant, and rule exists in one place. `EditEvent` lives in `sp42-core/src/types.rs` and is used directly by all crates. No copies. No conversion layers.

**6.2** Trait-based abstraction. All external dependencies via traits defined in `sp42-core/src/traits.rs`. Core never names a concrete implementation. Enables: testing with mocks, compiling to different targets.

**6.3** Domain-specific error types. Each module has its own error enum (`ScoringError`, `DiffError`, `ActionError`). No `anyhow::Error` in public interfaces.

**6.4** Function discipline:
- Functions do one thing. No "and" in names.
- ≤40 lines (strong signal, not hard limit).
- ≤4 parameters (group into struct otherwise).
- Pure where possible.

---

## Article 7 — Dependency discipline

**7.1** Every dependency is a liability. Adding one means: trusting its maintainers, accepting transitives, tracking advisories, coupling to its release cycle.

**7.2** Before adding a dependency, document in the PR: what it does, why not self-built in <200 lines, maintenance status (last release, open issues, bus factor), license, transitive count. >50 transitives requires lead approval.

**7.3** `Cargo.lock` is committed. `cargo update` is an explicit, reviewed PR.

---

## Article 8 — Git discipline

**8.1** Conventional Commits: `type(scope): description`. Types: feat, fix, refactor, test, docs, ci, perf, chore. Scope = module name.

**8.2** `main` is always releasable. Feature branches, short-lived (<1 week). Squash-merge. Force-push to main prohibited.

**8.3** Code review: every PR requires approving review from project lead. Constitution changes require unanimous lead approval. Self-merge prohibited.

---

## Article 9 — Contracts and protocols

**9.1** Every external interface has a schema. Coordination messages = Rust enums with serde. Config YAML has JSON Schema. All versioned.

**9.2** Breaking changes increment version. Backward compat for one release cycle.

**9.3** API action wrappers document idempotency. Non-idempotent actions use pre-checks.

---

## Article 10 — Security

**10.1** OAuth tokens in memory only. Never IndexedDB, localStorage, sessionStorage. Tab close = token gone.

**10.2** No `eval`, no `innerHTML` equivalent with untrusted content. Leptos auto-escapes. Diff rendering uses sanitized allowlist.

**10.3** `cargo audit` in CI. Lockfile integrity verified. Wasm built from source in CI.

**10.4** User data stays local. No telemetry, no analytics, no tracking. Training data export is explicit opt-in.

---

## Article 11 — Performance contracts

| Metric | Browser (Wasm) | Tauri (native) |
|--------|----------------|----------------|
| First meaningful paint | <2s on 3G | <0.5s |
| Wasm binary size | <400KB gzipped | N/A |
| Event processing | <1ms per event | <0.2ms |
| Diff computation (10KB) | <15ms | <5ms |
| RAM at idle | <50MB | <25MB |
| RAM after 4 hours | <80MB (no leaks) | <35MB |

Regressions are bugs. CI enforces where measurable.

---

## Article 12 — Amendments

Amending this Constitution requires:

1. Written proposal (GitHub issue) with proposed change, rationale, and impact
2. 7-day comment period for all active contributors
3. Unanimous approval from project leads
4. Recorded as ADR; Constitution updated in same PR

The bar is deliberately high.

---

*Un grand pouvoir implique de grandes responsabilités. The same applies to the code that wields it.*
