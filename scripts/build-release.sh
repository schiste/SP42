#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

source "$repo_root/scripts/lib/build-common.sh"

build_flags=(--release --locked)
timings=false
timings_flags=(--locked)

for arg in "$@"; do
  case "$arg" in
    --frozen|--offline)
      build_flags+=("$arg")
      timings_flags+=("$arg")
      ;;
    --timings)
      timings=true
      ;;
    --help|-h)
      cat <<'EOF'
Usage: ./scripts/build-release.sh [--frozen] [--offline] [--timings]

Build the full SP42 release artifact set with locked, reproducible settings.
EOF
      exit 0
      ;;
    *)
      printf 'Unknown argument: %s\n' "$arg" >&2
      exit 1
      ;;
  esac
done

"$repo_root/scripts/build-local.sh" "${build_flags[@]}"
"$repo_root/scripts/build-frontend.sh" "${build_flags[@]}"

if [[ "$timings" == true ]]; then
  "$repo_root/scripts/build-timings.sh" "${timings_flags[@]}"
fi

printf 'SP42 release build complete.\n'
