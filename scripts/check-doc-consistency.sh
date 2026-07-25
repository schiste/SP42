#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

require_line() {
  local file="$1"
  local needle="$2"

  if ! grep -Fq -- "$needle" "$file"; then
    printf 'missing required line in %s: %s\n' "$file" "$needle" >&2
    exit 1
  fi
}

# The README carries only headline status claims and points at docs/STATUS.md
# for the phase timeline; pin the claims that must stay in lockstep with STATUS.
require_line "README.md" '- Live Wikimedia integration is still gated by external credentials and verification'
require_line "README.md" '- Multi-user production auth is not implemented yet'
require_line "README.md" '[docs/STATUS.md](docs/STATUS.md)'

require_line "docs/STATUS.md" 'The offline patrol engine is now effectively complete for local development:'
require_line "docs/STATUS.md" 'Coordination and shared runtime state are now effectively complete for local development:'
require_line "docs/STATUS.md" 'Target shells are now effectively complete for local development and include an interactive patrol rail:'
require_line "docs/STATUS.md" 'Live Wikimedia integration is still gated by external credentials and verification:'
require_line "docs/STATUS.md" 'PWA packaging and offline installability are now effectively complete for local development:'

require_line "docs/platform/DEVELOPER_SURFACE.md" '- it includes a PWA shell for installability, update activation, iOS guidance, and offline-safe shell behavior'
require_line "docs/platform/DEVELOPER_SURFACE.md" '- PWA shell, offline fallback, manifest shortcuts, and telemetry surfaces'

printf 'SP42 docs/status consistency checks passed.\n'
