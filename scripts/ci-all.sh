#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

# Self-heal build artifacts when this run ends (success or failure) so the local
# target never grows unbounded. Skipped if SP42_KEEP_ARTIFACTS=1. See clean-house.sh.
trap '"$repo_root/scripts/clean-house.sh" --auto || true' EXIT

source "$repo_root/scripts/lib/build-common.sh"
sp42_run_xtask "$repo_root" ci-all "$@"
