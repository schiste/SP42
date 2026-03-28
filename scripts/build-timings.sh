#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

source "$repo_root/scripts/lib/build-common.sh"

use_locked=true
use_frozen=false
use_offline=false

for arg in "$@"; do
  case "$arg" in
    --locked)
      use_locked=true
      ;;
    --frozen)
      use_frozen=true
      ;;
    --offline)
      use_offline=true
      ;;
    --help|-h)
      cat <<'EOF'
Usage: ./scripts/build-timings.sh [--locked] [--frozen] [--offline]

Generate Cargo timings reports for the workspace host build and the wasm browser build.
Reports are written under target/cargo-timings and target/wasm32-unknown-unknown/cargo-timings.
EOF
      exit 0
      ;;
    *)
      printf 'Unknown argument: %s\n' "$arg" >&2
      exit 1
      ;;
  esac
done

sp42_setup_build_env "$repo_root" ci

cargo_bin="$(sp42_cargo_bin)"
host_flags=(--workspace --all-targets --profile ci --timings)
wasm_flags=(-p sp42-app --target wasm32-unknown-unknown --profile ci --timings)

if [[ "$use_locked" == true ]]; then
  host_flags+=(--locked)
  wasm_flags+=(--locked)
fi

if [[ "$use_frozen" == true ]]; then
  host_flags+=(--frozen)
  wasm_flags+=(--frozen)
fi

if [[ "$use_offline" == true ]]; then
  host_flags+=(--offline)
  wasm_flags+=(--offline)
fi

"$cargo_bin" build "${host_flags[@]}"
"$cargo_bin" build "${wasm_flags[@]}"

printf 'Cargo timings written under %s/target.\n' "$repo_root"
