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
# lands the optimized bundle at ~3.0 MiB raw / ~744 KiB gzip. Two recorded
# decisions stack on top (Art. 5.2):
#   - The citation repair/insertion action-row surface (PRD-0014: per-finding
#     edit/fix/flag/re-verify controls and their inline panels) adds ~6 KiB
#     gzip (ceiling 848 KiB).
#   - The entity-diff surface (PR #119 + the slim-report follow-up) first
#     landed at ~860 KiB gzip; shipping the pre-rendered EntityDiffReport
#     instead of the full wikibase model on the wire recovered ~28 KiB gzip
#     (measured), for a 860 KiB ceiling on its own.
# With BOTH surfaces in one bundle CI measures 866.2 KiB gzip — the two
# increases are additive, so the merged ceiling is their sum plus small
# headroom: 872 KiB (recorded decision, Art. 5.2). Ratchet back DOWN as the
# bundle shrinks (e.g. moving inline styles to CSS), never up without a
# recorded decision.
# PR #147 (book-citation grounding): the Books report section and its types
# render in the browser Citations tab, growing the measured bundle to
# 3461702 raw / 908535 gzip in CI. Recorded decision (Art. 5.2): ceilings
# move to the next 64-KiB multiples above those measurements.
max_raw="${SP42_WASM_MAX_RAW_BYTES:-3473408}"   # 3392 KiB
max_gz="${SP42_WASM_MAX_GZIP_BYTES:-917504}"    # 896 KiB

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
