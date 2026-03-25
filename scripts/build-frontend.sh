#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

scripts/clean-house.sh

cd "$repo_root/crates/sp42-app"

toolchain_bin="$(dirname "$(command -v cargo)")"

if ! command -v trunk >/dev/null 2>&1; then
  echo "trunk is required for frontend builds. Install it with: cargo install trunk" >&2
  exit 1
fi

mkdir -p "$repo_root/target/frontend"
PATH="$toolchain_bin:$PATH" env \
  -u NO_COLOR \
  CLICOLOR=0 \
  CARGO_TARGET_DIR="$repo_root/target/frontend" \
  trunk build --release
