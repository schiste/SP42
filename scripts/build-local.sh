#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

source "$repo_root/scripts/lib/build-common.sh"

mode="dev"
use_locked=false
use_frozen=false
use_offline=false

for arg in "$@"; do
  case "$arg" in
    --release)
      mode="release"
      ;;
    --ci)
      mode="ci"
      ;;
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
Usage: ./scripts/build-local.sh [--release] [--ci] [--locked] [--frozen] [--offline]

Build all local SP42 host targets plus the wasm crates and the Tauri shell.
EOF
      exit 0
      ;;
    *)
      printf 'Unknown argument: %s\n' "$arg" >&2
      exit 1
      ;;
  esac
done

sp42_setup_build_env "$repo_root" "$mode"

cargo_bin="$(sp42_cargo_bin)"
build_flags=()
wasm_profile_flags=()

if [[ "$mode" == "release" ]]; then
  build_flags+=(--release)
  wasm_profile_flags+=(--profile web-release)
elif [[ "$mode" == "ci" ]]; then
  build_flags+=(--profile ci)
  wasm_profile_flags+=(--profile ci)
fi

if [[ "$use_locked" == true ]]; then
  build_flags+=(--locked)
  wasm_profile_flags+=(--locked)
fi

if [[ "$use_frozen" == true ]]; then
  build_flags+=(--frozen)
  wasm_profile_flags+=(--frozen)
fi

if [[ "$use_offline" == true ]]; then
  build_flags+=(--offline)
  wasm_profile_flags+=(--offline)
fi

"$cargo_bin" build --workspace --all-targets "${build_flags[@]}"
"$cargo_bin" build -p sp42-core --target wasm32-unknown-unknown "${wasm_profile_flags[@]}"
"$cargo_bin" build -p sp42-app --target wasm32-unknown-unknown "${wasm_profile_flags[@]}"
"$cargo_bin" build --manifest-path crates/sp42-desktop/src-tauri/Cargo.toml "${build_flags[@]}"

printf 'SP42 local build complete.\n'
