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

require_line "README.md" '- `Phase 1`: offline patrol core and queueing, effectively complete for local/offline development'
require_line "README.md" '- `Phase 2`: coordination and shared room state, effectively complete for local development'
require_line "README.md" '- `Phase 3`: browser, CLI, and desktop shells with shared reports, shared shell-state, telemetry, and the interactive patrol rail, effectively complete for local development'
require_line "README.md" '- `Phase 4`: live Wikimedia integration, pending real credentials and external verification'
require_line "README.md" '- `Phase 5`: PWA/offline packaging and installability, effectively complete for local development'

require_line "docs/STATUS.md" 'The offline patrol engine is now effectively complete for local development:'
require_line "docs/STATUS.md" 'Coordination and shared runtime state are now effectively complete for local development:'
require_line "docs/STATUS.md" 'Target shells are now effectively complete for local development and include an interactive patrol rail:'
require_line "docs/STATUS.md" 'Live Wikimedia integration is still gated by external credentials and verification:'
require_line "docs/STATUS.md" 'PWA packaging and offline installability are now effectively complete for local development:'

require_line "docs/DEVELOPER_SURFACE.md" '- it includes a PWA shell for installability, update activation, iOS guidance, and offline-safe shell behavior'
require_line "docs/DEVELOPER_SURFACE.md" '- PWA shell, offline fallback, manifest shortcuts, and telemetry surfaces'

printf 'SP42 docs/status consistency checks passed.\n'
