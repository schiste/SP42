#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

chmod +x .husky/pre-commit .husky/commit-msg .husky/pre-push
git config core.hooksPath .husky

printf 'SP42 Git hooks installed via core.hooksPath=.husky\n'
printf 'Use SP42_SKIP_GIT_HOOKS=1 for an emergency one-off bypass.\n'
