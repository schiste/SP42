#!/usr/bin/env bash
# Coverage gates (CONSTITUTION Article 5.2). Uses cargo-llvm-cov; the toolchain
# ships llvm-tools-preview.
#
#   - sp42-core line coverage >= SP42_COVERAGE_MIN (default 90) — the binding
#     floor from the Constitution.
#   - workspace line coverage, excluding the `xtask` build-tooling crate, >=
#     SP42_WORKSPACE_COVERAGE_MIN (default 80) — so coverage cannot silently
#     erode outside core. Ratchet this upward as coverage improves.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

core_min="${SP42_COVERAGE_MIN:-90}"
ws_min="${SP42_WORKSPACE_COVERAGE_MIN:-80}"

command -v cargo-llvm-cov >/dev/null 2>&1 || {
  printf 'SP42 coverage check: `cargo-llvm-cov` is not installed.\n  install: cargo install --locked cargo-llvm-cov\n' >&2
  exit 1
}

printf '\n== sp42-core line coverage (must be >= %s%%) ==\n' "$core_min"
RUST_TEST_THREADS="${RUST_TEST_THREADS:-1}" \
  cargo llvm-cov -p sp42-core --fail-under-lines "$core_min"

printf '\n== workspace line coverage, excl. xtask (must be >= %s%%) ==\n' "$ws_min"
RUST_TEST_THREADS="${RUST_TEST_THREADS:-1}" \
  cargo llvm-cov --workspace --exclude xtask --fail-under-lines "$ws_min"

printf '\nSP42 coverage check passed (sp42-core >= %s%%, workspace excl. xtask >= %s%%).\n' \
  "$core_min" "$ws_min"
