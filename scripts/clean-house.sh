#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

purge_target=false

for arg in "$@"; do
  case "$arg" in
    --purge-target)
      purge_target=true
      ;;
    --help|-h)
      cat <<'EOF'
Usage: ./scripts/clean-house.sh [--purge-target]

Remove generated runtime and packaging artifacts.
Pass --purge-target to also remove the shared Cargo target directory.
EOF
      exit 0
      ;;
    *)
      printf 'Unknown argument: %s\n' "$arg" >&2
      exit 1
      ;;
  esac
done

paths=(
  ".tmp"
  ".sp42-runtime"
  "coverage"
  "dist"
  "crates/sp42-app/dist"
  "target/dist"
  "crates/sp42-desktop/src-tauri/target"
)

if [[ "$purge_target" == true ]]; then
  paths+=("target")
fi

for path in "${paths[@]}"; do
  /bin/rm -rf "$path"
done

/usr/bin/find "$repo_root" -name '.DS_Store' -delete

printf 'SP42 cleanup complete.\n'
