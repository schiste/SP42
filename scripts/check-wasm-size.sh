#!/usr/bin/env bash
# Wasm size gate (CONSTITUTION Article 5.2: "Regression blocks").
#
# Enforces a no-regression CEILING on the OPTIMIZED browser bundle. The caller
# must have produced an optimized build first (./scripts/build-web-release.sh);
# the ci-profile wasm is unoptimized and must NOT be measured here.
#
# The Constitution's aspirational target is 800KB raw / 400KB gzip. The current
# Leptos baseline is far above that, so this gate freezes the present size (plus
# small headroom) and blocks growth. Ratchet the ceilings DOWN as the bundle
# shrinks; never up without a recorded decision.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

dist="${1:-target/dist/sp42-app}"
# Ceilings (bytes). The article-level citation-review surface (per-finding
# evidence quote, source excerpt, Citoid metadata, grouped/color-coded cards)
# lands the optimized bundle at ~3.0 MiB raw / ~744 KiB gzip. Raise the ceilings
# to that plus ~256 KiB raw headroom; ratchet back DOWN as the bundle shrinks
# (e.g. moving inline styles to CSS), never up without a recorded decision.
# Recorded decision (PR #119 + the slim-report follow-up): the entity-diff
# surface first landed at ~860 KiB gzip; shipping the pre-rendered
# EntityDiffReport instead of the full wikibase model on the wire recovered
# ~28 KiB gzip (measured), so the ceiling ratchets back down to 860 KiB.
# Ratchet further DOWN as the bundle shrinks, never up without a recorded
# decision.
max_raw="${SP42_WASM_MAX_RAW_BYTES:-3407872}"   # 3328 KiB
max_gz="${SP42_WASM_MAX_GZIP_BYTES:-880640}"    # 860 KiB

wasm=""
for f in "$dist"/*.wasm; do
  [[ -e "$f" ]] && wasm="$f"
done
if [[ -z "$wasm" ]]; then
  printf 'SP42 wasm-size check: no .wasm under %s.\n  build first: ./scripts/build-web-release.sh\n' "$dist" >&2
  exit 1
fi

raw="$(wc -c < "$wasm" | tr -d ' ')"
gz="$(gzip -c "$wasm" | wc -c | tr -d ' ')"

printf '\n== wasm bundle size ==\n  file: %s\n  raw : %s bytes (ceiling %s)\n  gzip: %s bytes (ceiling %s)\n' \
  "$wasm" "$raw" "$max_raw" "$gz" "$max_gz"

status=0
(( raw > max_raw )) && { printf 'FAIL: raw size regressed past ceiling.\n' >&2; status=1; }
(( gz  > max_gz  )) && { printf 'FAIL: gzip size regressed past ceiling.\n' >&2; status=1; }

if (( status != 0 )); then
  printf '\nSP42 wasm-size check FAILED (Article 5.2: regression blocks).\n' >&2
  exit 1
fi
printf '\nSP42 wasm-size check passed.\n'
