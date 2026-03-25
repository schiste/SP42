#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

if command -v cargo >/dev/null 2>&1; then
  cargo clean
fi

paths=(
  ".tmp"
  ".sp42-runtime"
  "dist"
  "coverage"
  "crates/sp42-app/dist"
  "crates/sp42-desktop/src-tauri/target"
)

for path in "${paths[@]}"; do
  /bin/rm -rf "$path"
done

/usr/bin/find "$repo_root" -name '.DS_Store' -delete

printf 'SP42 cleanup complete.\n'
