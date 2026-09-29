#!/usr/bin/env bash
# Execute the browser shell's unit tests in a real wasm runtime (ADR-0012, ADR-0034).
#
# Why this exists: `sp42-app`'s tests live in `#[cfg(target_arch = "wasm32")]`
# modules, so host `cargo test` cannot compile them. They were previously
# declared with `#[test]`, which no wasm runner collects, so CI only *type-checked*
# them — 88 tests counted as covered while never running. They are now declared
# with `#[wasm_bindgen_test]` and executed here.
#
# --release is required, not an optimisation: the debug wasm for this crate is
# ~180 MB, and the runner's own parse of it exhausts the disk on small volumes
# (observed: "magic header not detected", because the output file was truncated
# to zeros). The release wasm is small and the tests are pure presentation logic
# with no timing sensitivity, so opt-level makes no difference to correctness.
#
# The Node runner is used rather than a headless browser because these tests do
# not touch the DOM: they cover diff shaping, label formatting, highlight
# extraction, config joining and request parsing. A browser adds a chromedriver
# download and a whole class of CI flake for no added signal. Tests that need a
# real DOM will need `run_in_browser` plus this script's browser path.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root/crates/sp42-app"

if ! command -v wasm-pack >/dev/null 2>&1; then
  cat >&2 <<'MSG'
`wasm-pack` is required to run the browser shell's tests.
  install: cargo install --locked wasm-pack
MSG
  exit 1
fi

printf '\n== sp42-app wasm unit tests (wasm-bindgen-test, node runner, release) ==\n'

# `--locked` so a CI run cannot silently resolve a different wasm-bindgen-test.
wasm-pack test --release --node --locked

printf '\nSP42 wasm unit tests passed.\n'
