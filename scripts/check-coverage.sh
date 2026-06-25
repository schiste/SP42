#!/usr/bin/env bash
# Coverage gate: sp42-core line coverage must be >= the threshold (CONSTITUTION
# Article 5.2). Uses cargo-llvm-cov; the toolchain ships llvm-tools-preview.
#
# Threshold defaults to 90 and can be overridden with SP42_COVERAGE_MIN.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

min="${SP42_COVERAGE_MIN:-90}"

command -v cargo-llvm-cov >/dev/null 2>&1 || {
  printf 'SP42 coverage check: `cargo-llvm-cov` is not installed.\n  install: cargo install --locked cargo-llvm-cov\n' >&2
  exit 1
}

printf '\n== sp42-core line coverage (must be >= %s%%) ==\n' "$min"
# Determinism per Article 1.4: single test thread.
RUST_TEST_THREADS="${RUST_TEST_THREADS:-1}" \
  cargo llvm-cov -p sp42-core --fail-under-lines "$min"

printf '\nSP42 coverage check passed (sp42-core lines >= %s%%).\n' "$min"
