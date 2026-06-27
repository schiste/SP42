#!/usr/bin/env bash
# SP42 build-artifact cleanup + self-heal.
#
# Modes:
#   (no args)        Remove generated runtime/packaging artifacts (the original
#                    light clean): .tmp, coverage, dist trees, .DS_Store.
#   --auto           Self-heal: the light clean PLUS bound the Cargo target so it
#                    cannot grow without limit — drop disposable trees (doc,
#                    llvm-cov), prune stale per-worktree build dirs, and shed the
#                    duplicate `debug` profile / size-cap when over budget, while
#                    KEEPING the active profile's cache warm. Designed to run at
#                    the end of every heavy local build (ci-all, pre-push).
#   --purge-target   Also remove the entire Cargo target dir (cold rebuild next).
#
# Opt-out / tuning (env):
#   SP42_KEEP_ARTIFACTS=1     Skip --auto self-heal entirely (keep everything).
#   SP42_TARGET_CAP_GB=N      Target-size budget for --auto (default 8).
#   SP42_BUILD_STALE_DAYS=N   Age (days) after which a per-worktree build tree is
#                             pruned in --auto (default 3).
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

mode="light"
for arg in "$@"; do
  case "$arg" in
    --auto) mode="auto" ;;
    --purge-target) mode="purge" ;;
    --help | -h)
      sed -n '2,28p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      printf 'Unknown argument: %s\n' "$arg" >&2
      exit 1
      ;;
  esac
done

# --auto is the only mode that honors the keep-artifacts opt-out; explicit manual
# runs always do what they say.
if [[ "$mode" == "auto" && "${SP42_KEEP_ARTIFACTS:-0}" == "1" ]]; then
  printf 'SP42 self-heal skipped (SP42_KEEP_ARTIFACTS=1).\n'
  exit 0
fi

dir_kb() { du -sk "$1" 2>/dev/null | cut -f1 || echo 0; }
human() { du -sh "$1" 2>/dev/null | cut -f1 || echo 0; }

# --- 1. Always: light clean (runtime/packaging junk + editor cruft) -----------
light_paths=(
  ".tmp"
  ".sp42-runtime"
  "coverage"
  "dist"
  "crates/sp42-app/dist"
  "target/dist"
  "crates/sp42-desktop/src-tauri/target"
)
for path in "${light_paths[@]}"; do
  /bin/rm -rf "$path"
done
/usr/bin/find "$repo_root" -name '.DS_Store' -delete 2>/dev/null || true

# Build-output tee logs live outside the repo (regenerated on demand).
/bin/rm -rf "$HOME/Library/Application Support/rtk/tee" 2>/dev/null || true

if [[ "$mode" == "light" ]]; then
  printf 'SP42 cleanup complete (light).\n'
  exit 0
fi

if [[ "$mode" == "purge" ]]; then
  /bin/rm -rf target
  printf 'SP42 cleanup complete (target purged — next build is cold).\n'
  exit 0
fi

# --- 2. --auto self-heal: bound target/, keep the warm cache ------------------
[[ -d target ]] || { printf 'SP42 self-heal: no target dir, nothing to do.\n'; exit 0; }

before="$(human target)"
cap_gb="${SP42_TARGET_CAP_GB:-8}"
stale_days="${SP42_BUILD_STALE_DAYS:-3}"
cap_kb=$((cap_gb * 1024 * 1024))

# 2a. Disposable final-artifact trees — regenerated on demand by their gate.
/bin/rm -rf target/doc target/llvm-cov-target 2>/dev/null || true

# 2b. Prune stale per-worktree/profile build trees. The `build-dir` config keys
#     a full build tree per workspace path (each git worktree + sub-workspace);
#     ones not touched in $stale_days are almost certainly orphaned worktrees.
if [[ -d target/.build ]]; then
  /usr/bin/find target/.build -mindepth 2 -maxdepth 2 -type d -mtime "+${stale_days}" \
    -exec /bin/rm -rf {} + 2>/dev/null || true
  # drop now-empty hash-prefix dirs left behind
  /usr/bin/find target/.build -mindepth 1 -maxdepth 1 -type d -empty -delete 2>/dev/null || true
fi

# 2c. Still over budget? Shed the duplicate `debug` profile — the `ci` profile is
#     the canonical local one (see .cargo aliases); a stray `debug` tree is the
#     usual culprit. Only when `ci` artifacts exist, so we never strand the only
#     cache present.
if [[ $(dir_kb target) -gt $cap_kb && -d target/ci ]]; then
  /bin/rm -rf target/debug 2>/dev/null || true
  /usr/bin/find target/.build -mindepth 3 -maxdepth 3 -type d -name debug \
    -exec /bin/rm -rf {} + 2>/dev/null || true
fi

# 2d. Last resort: if cargo-sweep is installed, LRU-evict down toward the cap.
if [[ $(dir_kb target) -gt $cap_kb ]] && command -v cargo-sweep >/dev/null 2>&1; then
  cargo sweep --maxsize "$((cap_gb * 1024))" >/dev/null 2>&1 || true
fi

after="$(human target)"
printf 'SP42 self-heal: target %s -> %s (cap %sGB; set SP42_KEEP_ARTIFACTS=1 to skip).\n' \
  "$before" "$after" "$cap_gb"
