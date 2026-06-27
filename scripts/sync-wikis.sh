#!/usr/bin/env bash
# Regenerate the embedded authoritative Wikimedia site list from the live
# SiteMatrix API. The result (crates/sp42-wiki/data/wikimedia-sites.json) is
# committed and compiled into the binary, so SP42 resolves any Wikimedia project
# offline with no runtime network dependency (ADR-0014). Run manually to refresh.
#
#   ./scripts/sync-wikis.sh
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

out="crates/sp42-wiki/data/wikimedia-sites.json"
# No smtype -> includes both language sites and specials (commons, wikidata,
# meta, species, …). smstate=all -> include closed wikis too (still readable).
url="https://meta.wikimedia.org/w/api.php?action=sitematrix&format=json&smsiteprop=dbname%7Curl&smstate=all"

mkdir -p "$(dirname "$out")"
tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT
printf 'Fetching SiteMatrix from meta.wikimedia.org…\n'
curl -fsS --max-time 60 "$url" -o "$tmp"

python3 - "$tmp" "$out" <<'PY'
import json, sys

src, out = sys.argv[1], sys.argv[2]
sm = json.load(open(src))["sitematrix"]
sites = {}

def add(s):
    db, u = s.get("dbname"), s.get("url")
    if db and u:
        sites[db] = u

for key, value in sm.items():
    if key.isdigit():                       # language groups
        for s in value.get("site", []):
            add(s)
    elif key == "specials":                 # commons, wikidata, meta, …
        for s in value:
            add(s)

with open(out, "w", encoding="utf-8") as f:
    json.dump(dict(sorted(sites.items())), f, separators=(",", ":"))
    f.write("\n")
print(f"wrote {len(sites)} sites to {out}")
PY
printf 'Done. Review the diff and commit %s\n' "$out"
