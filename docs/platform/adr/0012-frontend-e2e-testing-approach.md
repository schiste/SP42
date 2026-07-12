# ADR-0012: Frontend end-to-end testing approach

**Status:** Proposed
**Date:** 2026-06-26
**Author:** Christophe Henner (drafted with Claude)
**Summary:** Frontend end-to-end tests use a Rust-native browser harness rather than Playwright/Node, keeping the E2E stack inside the all-Rust toolchain (ADR-0001).

## Context

The Leptos/Wasm browser shell (`sp42-app`) has **no end-to-end or browser-level
test coverage**. Unit tests and pure-core tests cannot catch a whole class of
failures that only appear in a real browser: a Wasm boot panic, a broken
hydration path, a mis-wired route, or a blank screen from a failed bundle load. A
green `cargo test` can coexist with a frontend that does not start.

Closing this gap requires a decision, because two binding documents disagree:

- **CONSTITUTION Article 5.2** lists the E2E check as **Playwright**.
- **ADR-0001** mandates **Rust-only** — no JavaScript/Node toolchain.

The repository today has **zero Node tooling** (the `.husky` hooks are plain
bash via `core.hooksPath`). Adding Playwright would introduce a JavaScript
toolchain and runtime, directly contradicting ADR-0001. So the E2E gate cannot be
implemented cleanly until this tension is resolved — which is what this ADR does.

## Decision (proposed)

1. **Adopt a Rust-native browser E2E harness; do not add Playwright/Node.** This
   keeps ADR-0001's Rust-only stance intact.
2. **Start with a `fantoccini` smoke test** (Rust WebDriver client) driving a
   headless `chromedriver` against the **trunk-built optimized bundle**: serve
   `target/dist/sp42-app`, load it, assert the patrol shell mounts and the
   console reports no errors. This validates the actually-shipped artifact
   end-to-end. `wasm-bindgen-test --headless` remains available for finer,
   per-component DOM assertions later.
3. **Run it on merge to `main`** (not every PR), matching the Constitution's
   "E2E … on merge to main only" intent and keeping PR latency low.
4. **Amend CONSTITUTION Article 5.2** so the E2E row reads *"Playwright **or a
   Rust-native WebDriver / wasm-bindgen-test harness**"*, reconciling §5.2 with
   ADR-0001.

## Alternatives considered

- **Playwright (JavaScript/Node).** Best real-browser fidelity and ecosystem, but
  introduces a second language toolchain and runtime — rejected as a direct
  ADR-0001 violation. Choosing it would require *amending* ADR-0001, a larger
  decision than this gap warrants.
- **No E2E.** Rejected: leaves the highest-risk layer (Wasm boot / app start)
  the one place with no automated coverage, while every backend path is gated.
- **`wasm-bindgen-test` only.** Rust-native and lighter, but exercises components
  in a synthetic harness rather than the shipped bundle; kept as a complementary
  layer, not the first smoke.

## Consequences

- Adds a Rust dev-dependency (`fantoccini`) and a `chromedriver` service in one
  merge-to-`main` CI job; no production dependency and no JS toolchain.
- The app must expose a few stable hooks (roles / `data-testid`) for assertions —
  a small, one-time frontend change tracked as the implementation follow-up.
- CONSTITUTION §5.2 is amended as above (recorded here per Article 4.1).
- Until this ADR is accepted and implemented, the E2E gate remains the one
  documented CI check not yet enforced; this ADR is the prerequisite, intentionally
  separated from the mechanical CI improvements that ship alongside it.
