#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

source "$repo_root/scripts/lib/build-common.sh"

mode="release"
use_locked=true
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
    --debug)
      mode="dev"
      use_locked=false
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
Usage: ./scripts/build-frontend.sh [--release] [--ci] [--debug] [--locked] [--frozen] [--offline]

Build the browser bundle with Trunk using the canonical workspace config.
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

toolchain_bin="$(dirname "$(sp42_cargo_bin)")"
trunk_flags=(--config "$repo_root/Trunk.toml")

if ! command -v trunk >/dev/null 2>&1; then
  echo "trunk is required for frontend builds. Install it with: cargo install trunk" >&2
  exit 1
fi

if [[ "$mode" == "release" ]]; then
  trunk_flags+=(--cargo-profile web-release --release)
elif [[ "$mode" == "ci" ]]; then
  trunk_flags+=(--cargo-profile ci)
fi

if [[ "$use_locked" == true ]]; then
  trunk_flags+=(--locked)
fi

if [[ "$use_frozen" == true ]]; then
  trunk_flags+=(--frozen)
fi

if [[ "$use_offline" == true ]]; then
  trunk_flags+=(--offline)
fi

mkdir -p "$(sp42_frontend_dist_dir "$repo_root")"
PATH="$toolchain_bin:$PATH" env \
  SP42_APP_DIST_DIR="$(sp42_frontend_dist_dir "$repo_root")" \
  trunk build "${trunk_flags[@]}"
