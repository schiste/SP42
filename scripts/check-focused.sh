#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

source "$repo_root/scripts/lib/build-common.sh"
sp42_setup_build_env "$repo_root" dev
export SP42_APP_DIST_DIR="${SP42_APP_DIST_DIR:-$(sp42_frontend_dist_dir "$repo_root")}"

CARGO_BIN="$(sp42_cargo_bin)"
TRUNK_BIN="${TRUNK_BIN:-trunk}"

run_step() {
  printf '\n== %s ==\n' "$1"
  shift
  "$@"
}

if ! command -v "$TRUNK_BIN" >/dev/null 2>&1; then
  echo "trunk is required for ./scripts/check-focused.sh" >&2
  exit 1
fi
mkdir -p "$SP42_APP_DIST_DIR"

# Every host-testable workspace crate. sp42-app is wasm-only (covered by the
# trunk build below). When adding a crate to the workspace, add it here too —
# a crate missing from this list can be broken by a dependency change without
# the local loop noticing (that is how sp42-mcp's tests broke silently when
# BlockRef gained a field).
FOCUSED_CRATES=(
  -p sp42-core -p sp42-platform -p sp42-types
  -p sp42-patrol -p sp42-citation
  -p sp42-coordination -p sp42-wiki -p sp42-live -p sp42-reporting
  -p sp42-inference -p sp42-fetch -p sp42-parsoid
  -p sp42-server -p sp42-cli -p sp42-devtools -p sp42-desktop -p sp42-mcp -p sp42-ui
)

run_step "focused cargo check" \
  "$CARGO_BIN" check "${FOCUSED_CRATES[@]}"

run_step "focused cargo test" \
  env RUST_TEST_THREADS="${RUST_TEST_THREADS:-1}" \
  "$CARGO_BIN" test "${FOCUSED_CRATES[@]}"

run_step "focused trunk build" \
  "$TRUNK_BIN" build --config Trunk.toml

printf '\nSP42 focused checks passed.\n'
