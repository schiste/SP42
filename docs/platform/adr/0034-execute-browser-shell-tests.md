# ADR-0034: Execute the browser shell's unit tests

**Status:** Accepted
**Date:** 2026-09-28
**Author:** Christophe Henner (drafted with Claude)
**Summary:** The browser shell's `wasm`-gated unit tests are declared with `#[wasm_bindgen_test]` and executed by a new `scripts/check-wasm-tests.sh` in CI (and in `pre-commit` when `sp42-app` changes), replacing a compile-only check that let 88 tests be counted as covered while never running.

## Context

An audit of test coverage found that `sp42-app` and `sp42-desktop` — 15,740
lines, 16.4% of the workspace's Rust — had **zero executed tests**. The
mechanism was a combination of two things that cancelled out:

1. Every module in `sp42-app/src/lib.rs` is gated
   `#[cfg(target_arch = "wasm32")]`, so host `cargo ci-test` cannot compile
   them, let alone run them.
2. Their 88 test functions were declared with `#[test]`, the standard library
   attribute. `wasm-bindgen-test-runner` collects only functions carrying
   `#[wasm_bindgen_test]`.

CI compensated with a step that *type-checked* them:

```yaml
- name: Wasm test compile-check (sp42-app)
  # ... Compile-only (running needs wasm-bindgen-test).
  run: cargo check -p sp42-app --target wasm32-unknown-unknown --tests
```

The comment was accurate about the mechanism and wrong about the consequence:
`wasm-bindgen-test` was available, and the consequence of not running them is
that they rot. Turning them on exposed six real failures immediately — see
Consequences — which is the strongest possible evidence that the gap was live
rather than theoretical.

Constitution Art. 1.1 ("No untestable code may be merged… Every module, function,
and component must have automated tests") and Art. 1.2's component-test tier were
not being met for the browser shell, and Art. 5.2's coverage figure was computed
over a denominator that silently excluded these crates.

## Decision

1. **Declare the wasm-gated tests with `#[wasm_bindgen_test]`**, which the wasm
   runner actually collects. The 4 tests in `inspector.rs` — the one module that
   is *not* `wasm`-gated — keep `#[test]` and continue to run on the host, so
   they are not compiled twice.

2. **Add `scripts/check-wasm-tests.sh`**, which runs
   `wasm-pack test --release --node --locked`. CI calls it in place of the old
   compile-only step.

3. **Run it in `pre-commit` when `crates/sp42-app/` is staged**, so a change that
   breaks the browser shell is caught at commit time. It degrades to a printed
   notice when `wasm-pack` is not installed, since a missing optional tool must
   not block unrelated commits.

4. **Use the Node runner, not a headless browser.** These tests exercise pure
   presentation logic — diff shaping, label formatting, highlight extraction,
   config joining, request parsing — and never touch the DOM. A browser would add
   a chromedriver download per run and a class of CI flake for no added signal.

5. **`--release` is a correctness requirement, not an optimisation.** The debug
   wasm for this crate is ~180 MB; the runner's parse of it exhausted the disk
   and produced a truncated, all-zero output file (`magic header not detected`).
   The release wasm is small, and these tests have no timing sensitivity, so
   optimization level cannot affect their outcome. This is recorded in the script
   so nobody "optimizes" the flag away.

## Alternatives considered

**Keep the compile-only check and add a coverage exclusion note.** Rejected. The
tests are already written; the gap is that nothing runs them. Documenting an
unrun suite is not a fix.

**Headless Chrome via chromedriver.** Rejected for the current test set: none of
these tests need a DOM, and the runner added a browser download, a webdriver
session, and a reproducible `http status: 404` failure on the dev machine that
made it useless as a local gate. If DOM-level tests are added later, the script
gains a browser path and those tests set `run_in_browser` — the split is
per-test, not per-crate, so the pure-logic tests stay fast and reliable.

**Move the pure logic out of the `wasm`-gated modules into a host-testable
crate.** This is the more thorough fix and worth doing eventually — the diff and
label formatting code has no business being `wasm`-only. Rejected *here* because
it is a large refactor across ~20 files with real regression risk, and because
it would not cover the genuinely browser-bound tests. Running what already exists
delivers the coverage immediately; the module split can follow.

**Declare `sp42-app` as a non-`wasm` crate.** Rejected: `leptos`, `web-sys` and
`gloo-net` are `wasm`-only, so the components cannot be host-compiled at all
without a second abstraction layer. Verified by ungating `components` and
observing the resulting errors.

## Consequences

1. **88 tests now execute** and 4 more were added, for 92 total in the browser
   shell. `sp42-app` and `sp42-desktop` move from "zero measured coverage" to
   actually covered.

2. **Six pre-existing failures were found and fixed.** All were assertions that
   had drifted from the code they test and were never executed to reveal it:

   | Test | Actual behaviour | Resolution |
   |---|---|---|
   | `platform::auth::preview_contains_redirect_uri` | `build_notes` inserts a 4th note; the test asserted 3 | Corrected to 4 |
   | `platform::auth::callback_preview_masks_codes` | `mask_code` keeps 6 chars, producing `code=supers...`; the test asserted `code=super...` | Corrected, and added an assertion that the full secret never leaks |
   | `platform::coordination::room_inspection_lines_cover_presence_and_state` | Header renders `clients=`, not `connected_clients=`; mode is `quiet`, not `active` | Corrected, plus a new companion test that actually covers the `active` branch |
   | `platform::pwa::guidance_lines_call_out_ios_and_updates` | The `IosStandalone` fixture cannot emit the install line, which is guarded on `!display_mode_standalone` | Corrected to assert the non-emission, plus a new `BrowserTab` test that covers the instruction |
   | `components::patrol_scenario_panel::storyboard_lines_cover_queue_to_workbench_flow` | The Workbench section renders as `action rail` | Corrected |
   | `platform::runtime::local_storage_handles_missing_window_gracefully` | `get` propagated the missing-window error | **Behaviour fixed**, not the test — see below |

3. **One production fix fell out of it.** `LocalStorageBrowserStorage::get`
   returned `Err` when no `localStorage` was reachable, so a read failed outright
   in any environment where storage is unavailable (non-browser host, or a browser
   with site data disabled). A read is total: no reachable storage means the key
   has no value, which is `Ok(None)`. `set` still errors, because a write that
   cannot persist must not report success. The test now asserts both the
   totality and the `None`.

4. **`RenderedHunkSide::html` field access** was updated to the `html()` accessor
   introduced in ADR-0032 — the wasm build caught what the host build could not,
   since that file is `wasm`-gated.

5. **The coverage denominator is now honest**, subject to the remaining gap noted
   in ADR-0033: the `wasm` targets are measured under a separate runner, and
   `check-coverage.sh` still reports the host view. Making the coverage floor
   include the browser shell is follow-up work; the tests running is the
   prerequisite, and this ADR delivers it.

6. **Cost.** `wasm-pack` becomes a required tool for `pre-commit` when
   `sp42-app` changes and for CI. It is installed via `taiki-e/install-action`
   alongside the existing gate tools. The release-mode wasm build is the main
   added latency in the `checks` job.
