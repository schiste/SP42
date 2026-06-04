#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

fail=0

note_failure() {
  printf '%s\n' "$1" >&2
  fail=1
}

is_generated_path() {
  local path="$1"

  case "$path" in
    target/*|*/target/*) return 0 ;;
    dist/*|*/dist/*) return 0 ;;
    coverage/*|*/coverage/*) return 0 ;;
    .tmp/*|*/.tmp/*) return 0 ;;
    .sp42-runtime/*|*/.sp42-runtime/*) return 0 ;;
    crates/sp42-desktop/src-tauri/gen/*) return 0 ;;
    crates/sp42-desktop/src-tauri/binaries/*)
      [[ "$path" != "crates/sp42-desktop/src-tauri/binaries/.gitignore" ]]
      return
      ;;
  esac

  return 1
}

required_ignored_paths=(
  ".tmp/sp42-release-audit"
  "crates/sp42-server/.tmp/sp42-release-audit"
  ".sp42-runtime/sp42-release-audit"
  "crates/sp42-server/.sp42-runtime/sp42-release-audit"
  "target/sp42-release-audit"
  "target/dist/sp42-app/sp42-release-audit"
  "dist/sp42-release-audit"
  "coverage/sp42-release-audit"
  "crates/sp42-desktop/src-tauri/gen/schema.json"
  "crates/sp42-desktop/src-tauri/binaries/sp42-server-audit"
  "docs/REPO_CLEANUP_PLAN.md"
)

for path in "${required_ignored_paths[@]}"; do
  if ! git check-ignore -q -- "$path"; then
    note_failure "expected path is not ignored: $path"
  fi
done

tracked_generated=()
while IFS= read -r path; do
  if is_generated_path "$path"; then
    tracked_generated+=("$path")
  fi
done < <(git ls-files)

if [[ "${#tracked_generated[@]}" -gt 0 ]]; then
  note_failure "generated build/runtime paths are tracked:"
  printf '  %s\n' "${tracked_generated[@]}" >&2
fi

untracked=()
while IFS= read -r path; do
  case "$path" in
    .chau7|.chau7/*) continue ;;
  esac
  untracked+=("$path")
done < <(git ls-files --others --exclude-standard)

if [[ "${#untracked[@]}" -gt 0 ]]; then
  note_failure "non-ignored untracked files are present:"
  printf '  %s\n' "${untracked[@]}" >&2
fi

if [[ "$fail" -ne 0 ]]; then
  printf '\nRelease tree audit failed.\n' >&2
  exit 1
fi

printf 'SP42 release tree audit passed.\n'
