#!/usr/bin/env bash
# Coverage gates (CONSTITUTION Article 5.2; ADR-0013). Uses cargo-llvm-cov; the
# toolchain ships llvm-tools-preview.
#
#   - Platform-independent logic line coverage >= SP42_COVERAGE_MIN (default 90)
#     — the binding ≥90% floor. This logic is now split across two crates:
#     sp42-platform (scoring engine, policy compiler, action/wikitext/storage
#     machinery, shared types/traits) and sp42-core (citation verification +
#     patrol review workflow). They are measured TOGETHER so the split does not
#     change the bar that the combined code has always had to meet.
#   - workspace line coverage, excluding the `xtask` build-tooling crate, >=
#     SP42_WORKSPACE_COVERAGE_MIN (default 80) — so coverage cannot silently
#     erode outside the platform-independent crates. Ratchet upward over time.
#
# The combined run instruments the crates' shared dependency (sp42-types) too;
# it is excluded via --ignore-filename-regex so the floor is measured on the
# platform-independent code itself.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

core_min="${SP42_COVERAGE_MIN:-90}"
ws_min="${SP42_WORKSPACE_COVERAGE_MIN:-80}"

command -v cargo-llvm-cov >/dev/null 2>&1 || {
  printf 'SP42 coverage check: `cargo-llvm-cov` is not installed.\n  install: cargo install --locked cargo-llvm-cov\n' >&2
  exit 1
}

printf '\n== platform-independent logic (sp42-platform + sp42-core) line coverage (must be >= %s%%) ==\n' "$core_min"
RUST_TEST_THREADS="${RUST_TEST_THREADS:-1}" \
  cargo llvm-cov -p sp42-platform -p sp42-core \
    --ignore-filename-regex 'crates/sp42-types/' \
    --fail-under-lines "$core_min"

printf '\n== workspace line coverage, excl. xtask (must be >= %s%%) ==\n' "$ws_min"
RUST_TEST_THREADS="${RUST_TEST_THREADS:-1}" \
  cargo llvm-cov --workspace --exclude xtask --fail-under-lines "$ws_min"

printf '\nSP42 coverage check passed (platform-independent logic >= %s%%, workspace excl. xtask >= %s%%).\n' \
  "$core_min" "$ws_min"
