#!/usr/bin/env bash
# Coverage gates (CONSTITUTION Article 5.2; ADR-0013). Uses cargo-llvm-cov; the
# toolchain ships llvm-tools-preview.
#
#   - Platform-independent logic line coverage >= SP42_COVERAGE_MIN (default 90)
#     — the binding ≥90% floor. This logic is split across crates as it is
#     extracted: sp42-platform (scoring engine/policy, action/wikitext/storage
#     machinery, shared types/traits/review-workbench), sp42-citation (references
#     domain), and sp42-core (remaining patrol scoring-eval). They are measured
#     TOGETHER so the split does not change the bar the combined code must meet.
#     Add each newly-extracted crate's `-p` here in the same PR that creates it.
#   - workspace line coverage, excluding the `xtask` build-tooling crate, >=
#     SP42_WORKSPACE_COVERAGE_MIN (default 80) — so coverage cannot silently
#     erode outside the platform-independent crates. Ratchet upward over time.
#   - sp42-server line coverage >= SP42_SERVER_COVERAGE_MIN (default 60).
#     sp42-server is a GOVERNANCE.md "protected area" holding every
#     auth/session/deployment-mode code path, and the two aggregate floors above
#     let it sit far below the rest of the workspace: an 83% workspace average
#     coexisted with sp42-server at roughly 64%, and with
#     revision_artifacts.rs — the rendered-HTML trust boundary of ADR-0032 — at
#     26%. A protected area needs its own floor, or it is protected only in
#     review and not in CI. 60 is deliberately below the measured value so it
#     cannot fail spuriously; raise it as the server's tests improve.
#
# The combined run instruments the crates' shared dependency (sp42-types) too;
# it is excluded via --ignore-filename-regex so the floor is measured on the
# platform-independent code itself.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

core_min="${SP42_COVERAGE_MIN:-90}"
ws_min="${SP42_WORKSPACE_COVERAGE_MIN:-80}"
server_min="${SP42_SERVER_COVERAGE_MIN:-60}"

command -v cargo-llvm-cov >/dev/null 2>&1 || {
  printf 'SP42 coverage check: `cargo-llvm-cov` is not installed.\n  install: cargo install --locked cargo-llvm-cov\n' >&2
  exit 1
}

# On filesystems that cannot store resource forks natively (exFAT, which is what
# an external SSD is usually formatted as), macOS writes AppleDouble sidecar
# files next to every artifact: `._SP42-<pid>-<n>_0.profraw`. cargo-llvm-cov
# globs `*.profraw`, so llvm-profdata is handed those sidecars and aborts the
# whole merge with "unrecognized instrumentation profile encoding format" ->
# "no profile can be merged" — the coverage gate then fails for a reason that has
# nothing to do with the code. Purging the sidecars between the test run and the
# merge is harmless on APFS/ext4 (where the glob finds nothing to delete) and
# makes the gate work on an external volume. Set SP42_SKIP_APPLEDOUBLE_SWEEP=1
# to bypass.
purge_appledouble() {
  if [[ "${SP42_SKIP_APPLEDOUBLE_SWEEP:-0}" == "1" ]]; then
    return 0
  fi
  local dir="target/llvm-cov-target"
  [[ -d "$dir" ]] || return 0
  find "$dir" -name '._*' -type f -delete 2>/dev/null || true
}

# `cargo llvm-cov` merges profiles as part of the same invocation that runs the
# tests, so the sweep has to happen in between: run with --no-report to produce
# the .profraw files, purge the sidecars, then `report` to merge and measure.
run_floor() {
  local label="$1" floor="$2"
  shift 2
  printf '\n== %s line coverage (must be >= %s%%) ==\n' "$label" "$floor"
  RUST_TEST_THREADS="${RUST_TEST_THREADS:-1}" cargo llvm-cov "$@" --no-report
  purge_appledouble
  RUST_TEST_THREADS="${RUST_TEST_THREADS:-1}" \
    cargo llvm-cov report "$@" --fail-under-lines "$floor"
}

run_floor \
  'platform-independent logic (sp42-platform + sp42-core + sp42-citation + sp42-patrol)' \
  "$core_min" \
  -p sp42-platform -p sp42-core -p sp42-citation -p sp42-patrol \
  --ignore-filename-regex 'crates/sp42-types/'

run_floor 'sp42-server (protected area: auth, session, deployment mode)' \
  "$server_min" -p sp42-server

run_floor 'workspace, excl. xtask' "$ws_min" --workspace --exclude xtask

printf '\nSP42 coverage check passed (platform-independent logic >= %s%%, sp42-server >= %s%%, workspace excl. xtask >= %s%%).\n' \
  "$core_min" "$server_min" "$ws_min"
