#!/usr/bin/env bash
# SP42 build-artifact cleanup + self-heal.
#
# Modes:
#   (no args)        Light clean: remove generated runtime/packaging artifacts
#                    (.tmp, coverage, dist trees, .DS_Store). The served `dist`
#                    trees are KEPT (with a warning) while a local dev stack is
#                    running, so the live frontend is never pulled out from under
#                    `trunk serve` (which would 404 the app).
#   --auto           Self-heal (used by pre-push / ci-all): the light clean PLUS
#   --keep-last      keep only the MOST RECENT build session's data — every other
#                    profile/target tree (older profiles, prior CI runs, orphaned
#                    worktree build dirs) under target/ and target/.build is
#                    dropped, along with disposable doc / llvm-cov trees. The
#                    active session's cache stays warm; anything else cold-rebuilds
#                    on next use. `--auto` and `--keep-last` are synonyms.
#   --purge-target   Remove the entire Cargo target dir (next build is fully cold).
#
# Opt-out / tuning (env):
#   SP42_KEEP_ARTIFACTS=1     Skip the self-heal entirely (keep everything).
#   SP42_BUILD_GRACE_MIN=N    Build trees modified within N minutes of the newest
#                             count as part of "the last build" and are kept, so a
#                             single session that touches several profiles (e.g.
#                             debug + wasm for dev-local, or ci for the gates) is
#                             kept together. Default 45.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

mode="light"
for arg in "$@"; do
  case "$arg" in
    --auto | --keep-last) mode="auto" ;;
    --purge-target) mode="purge" ;;
    --help | -h)
      sed -n '2,32p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      printf 'Unknown argument: %s\n' "$arg" >&2
      exit 1
      ;;
  esac
done

# --auto/--keep-last is the only mode that honors the keep-artifacts opt-out;
# explicit manual runs always do what they say.
if [[ "$mode" == "auto" && "${SP42_KEEP_ARTIFACTS:-0}" == "1" ]]; then
  printf 'SP42 self-heal skipped (SP42_KEEP_ARTIFACTS=1).\n'
  exit 0
fi

human() { du -sh "$1" 2>/dev/null | cut -f1 || echo 0; }

# A live dev stack holds the served dist tree open; deleting it 404s the app until
# the next rebuild. Detect trunk serve / the running sp42-server so the light clean
# can spare those trees.
dev_stack_running() {
  pgrep -f 'trunk serve' >/dev/null 2>&1 || pgrep -f '[s]p42-server' >/dev/null 2>&1
}

# --- 1. Always: light clean (runtime/packaging junk + editor cruft) -----------
# Safe to remove regardless of a running stack (not served live).
light_paths=(
  ".tmp"
  ".sp42-runtime"
  "coverage"
  "crates/sp42-desktop/src-tauri/target"
)
# Served by trunk serve / the browser shell — only removed when no stack is up.
dist_paths=(
  "dist"
  "crates/sp42-app/dist"
  "target/dist"
)
for path in "${light_paths[@]}"; do
  /bin/rm -rf "$path"
done
if dev_stack_running; then
  printf 'SP42 cleanup: dev stack is running — keeping dist trees so the live app does not 404 (re-run after stopping it to reclaim them).\n' >&2
else
  for path in "${dist_paths[@]}"; do
    /bin/rm -rf "$path"
  done
fi
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

# --- 2. Self-heal: keep only the most recent build session --------------------
[[ -d target ]] || { printf 'SP42 self-heal: no target dir, nothing to do.\n'; exit 0; }

before="$(human target)"
grace_min="${SP42_BUILD_GRACE_MIN:-45}"
grace_s=$((grace_min * 60))

# 2a. Disposable final-artifact trees — regenerated on demand by their gate.
/bin/rm -rf target/doc target/llvm-cov-target 2>/dev/null || true

# 2b. Keep only the last build session's trees. Cargo's split build-dir keys a
#     per-profile intermediate tree at target/.build/<hash>/<leaf>/<profile> (the
#     bulk of the disk), with matching final artifacts at target/<profile>. We key
#     recency on each tree's newest *file* mtime — directory mtimes are unreliable
#     (cargo writes into deep subdirs, and stat/clean passes touch the parents).
#     Any profile whose newest file predates the global newest by more than the
#     grace window — older profiles, stale CI runs, orphaned worktree hashes — is
#     dropped, along with its matching top-level finals.
# Epoch mtime of a file, portably: GNU coreutils uses `stat -c %Y`, BSD/macOS
# uses `stat -f %m`. Probe once and reuse — getting this wrong returns no mtime,
# which silently disables the whole keep-last pass (every tree looks un-aged).
if stat -c '%Y' . >/dev/null 2>&1; then
  stat_mtime=(stat -c '%Y')   # GNU
else
  stat_mtime=(stat -f '%m')   # BSD/macOS
fi
newest_file_mtime() {
  # `head -1` closes the pipe early; with `set -o pipefail` the SIGPIPE'd `sort`
  # would otherwise abort the script under `set -e`. The stdout is still captured.
  /usr/bin/find "$1" -type f -exec "${stat_mtime[@]}" {} + 2>/dev/null | sort -rn | head -1 || true
}

if [[ -d target/.build ]]; then
  profile_trees=()
  profile_mtimes=()
  newest=0
  while IFS= read -r profile_tree; do
    [[ -d "$profile_tree" ]] || continue
    modified="$(newest_file_mtime "$profile_tree")"
    [[ -n "$modified" ]] || continue
    profile_trees+=("$profile_tree")
    profile_mtimes+=("$modified")
    # if-guard, not `(( … )) && …`: a false arithmetic command returns exit 1,
    # which would trip `set -e`.
    if (( modified > newest )); then
      newest="$modified"
    fi
  done < <(/usr/bin/find target/.build -mindepth 3 -maxdepth 3 -type d)

  if (( newest > 0 )); then
    for i in "${!profile_trees[@]}"; do
      if (( newest - profile_mtimes[i] > grace_s )); then
        profile="$(basename "${profile_trees[i]}")"
        /bin/rm -rf "${profile_trees[i]}"     # intermediates for this profile/target
        # matching final-artifact dir: ci / debug / release / <target-triple>
        [[ -n "$profile" ]] && /bin/rm -rf "target/$profile"
      fi
    done
  fi
  # drop now-empty leaf / hash dirs left behind
  /usr/bin/find target/.build -mindepth 1 -type d -empty -delete 2>/dev/null || true
fi

after="$(human target)"
printf 'SP42 self-heal: kept the last build session — target %s -> %s (grace %smin; SP42_KEEP_ARTIFACTS=1 to skip).\n' \
  "$before" "$after" "$grace_min"
