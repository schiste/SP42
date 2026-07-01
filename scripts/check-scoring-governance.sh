#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

require_file() {
  local path="$1"
  if [[ ! -f "$path" ]]; then
    printf 'missing required file: %s\n' "$path" >&2
    exit 1
  fi
}

require_line() {
  local file="$1"
  local needle="$2"
  if ! grep -Fq -- "$needle" "$file"; then
    printf 'missing required line in %s: %s\n' "$file" "$needle" >&2
    exit 1
  fi
}

require_json_valid() {
  local path="$1"
  python3 - <<'PY' "$path"
import json
import sys

path = sys.argv[1]
with open(path, "r", encoding="utf-8") as handle:
    json.load(handle)
PY
}

require_file "docs/platform/scoring/SCORING_CONSTITUTION.md"
require_file "docs/platform/scoring/POLICY_LAYOUT.md"
require_file "schemas/scoring-policy.schema.json"
require_file "schemas/scoring-evaluation-profile.schema.json"
require_file "configs/scoring/active/frwiki-vandalism.yaml"
require_file "configs/scoring/candidate/frwiki-vandalism-tighten-identity-cap.yaml"
require_file "configs/scoring/suggested/README.md"
require_file "evals/scoring/profiles/vandalism_patrol_default.yaml"
require_file "evals/scoring/fixtures/vandalism_patrol/frwiki/regression.yaml"
require_file "evals/scoring/fixtures/vandalism_patrol/frwiki/ranking.yaml"
require_file "evals/scoring/fixtures/vandalism_patrol/frwiki/invariants.yaml"
require_file "evals/scoring/fixtures/vandalism_patrol/frwiki/fairness.yaml"

require_json_valid "schemas/scoring-policy.schema.json"
require_json_valid "schemas/scoring-evaluation-profile.schema.json"

require_line "configs/scoring/active/frwiki-vandalism.yaml" "domain: vandalism_patrol"
require_line "configs/scoring/active/frwiki-vandalism.yaml" "lifecycle: active"
require_line "configs/scoring/active/frwiki-vandalism.yaml" "evaluation_profile: vandalism_patrol_default"
require_line "configs/scoring/active/frwiki-vandalism.yaml" "contribution_cap:"

require_line "configs/scoring/candidate/frwiki-vandalism-tighten-identity-cap.yaml" "lifecycle: candidate"
require_line "evals/scoring/profiles/vandalism_patrol_default.yaml" "name: vandalism_patrol_default"
require_line "evals/scoring/profiles/vandalism_patrol_default.yaml" "- fairness_checks"
require_line "docs/platform/scoring/SCORING_CONSTITUTION.md" "## 14. Technical Constitution"
require_line "docs/platform/scoring/POLICY_LAYOUT.md" "configs/scoring/"

printf 'SP42 scoring governance checks passed.\n'
