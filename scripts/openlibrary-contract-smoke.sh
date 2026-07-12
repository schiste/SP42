#!/usr/bin/env bash
# Live read-contract smoke for the PRD-0009 book lane (ADR-0024).
#
# MANUAL, NETWORK-TOUCHING — never wired into CI (ADR-0009: tests replay
# fixtures; this script is how a human refreshes confidence in them). It
# walks the exact read chain `verify-page` uses, read-only GETs only:
#
#   1. Books API catalog lookup   (openlibrary.rs::build_catalog_lookup_request)
#   2. Read API scan availability (openlibrary.rs::build_scan_availability_request)
#   3. archive.org item metadata  (search_inside.rs::build_item_metadata_request)
#   4. BookReader search-inside   (search_inside.rs::build_search_inside_request)
#
# and asserts precisely the response fields the parsers read, so:
#   PASS  -> production still matches our fixtures/parsers;
#   FAIL  -> the printed payload (kept in the temp dir) is the refreshed
#            fixture material and names the drifted contract.
#
# By construction this script never touches the import-on-miss
# `/isbn/{isbn}.json` path or any write endpoint (ADR-0024 Decision 2).
#
# Note: Open Library fronts with bot protection that can 403 datacenter IPs.
# Run this from a residential/developer connection; a Cloudflare challenge
# page here predicts the same for any cloud-hosted SP42 deployment.
#
# Usage: scripts/openlibrary-contract-smoke.sh [ISBN13] ["search terms"]

set -euo pipefail

isbn="${1:-9780140328721}" # Matilda (Puffin 1988): held, scanned, indexed.
query="${2:-Matilda parents}"
ua="SP42/0.1.0 (+https://github.com/schiste/SP42; read-contract smoke)"

tmp="$(mktemp -d)"
keep_tmp=0
cleanup() {
  if [[ "$keep_tmp" == 1 ]]; then
    echo "payloads kept in $tmp for fixture refresh"
  else
    rm -rf "$tmp"
  fi
}
trap cleanup EXIT

fetch() {
  local url="$1" out="$2"
  printf 'GET %s\n' "$url"
  /usr/bin/curl -fsS --max-time 30 -A "$ua" "$url" -o "$out"
  sleep 1 # politeness between third-party calls
}

check() {
  local file="$1" step="$2"
  shift 2
  if ! python3 - "$file" "$@" <<'PY'; then
import json
import sys

path = sys.argv[1]
checks = sys.argv[2:]
with open(path, "rb") as handle:
    document = json.load(handle)

def resolve(node, dotted):
    for part in dotted.split("."):
        if isinstance(node, list):
            node = node[int(part)]
        else:
            node = node[part]
    return node

for spec in checks:
    # spec: "dotted.path" (exists), "dotted.path=literal", or
    # "dotted.path:type" with type in {str,int,list,dict}.
    if "=" in spec:
        dotted, expected = spec.split("=", 1)
        actual = resolve(document, dotted)
        if str(actual) != expected:
            sys.exit(f"{dotted} = {actual!r}, expected {expected!r}")
    elif ":" in spec:
        dotted, kind = spec.rsplit(":", 1)
        actual = resolve(document, dotted)
        expected_type = {"str": str, "int": int, "list": list, "dict": dict}[kind]
        if not isinstance(actual, expected_type):
            sys.exit(f"{dotted} is {type(actual).__name__}, expected {kind}")
    else:
        resolve(document, spec)
PY
    keep_tmp=1
    echo "CONTRACT DRIFT at step: $step (payload: $file)" >&2
    exit 1
  fi
  echo "ok: $step"
}

echo "== 1. Books API catalog lookup (side-effect-free resolve) =="
fetch "https://openlibrary.org/api/books?bibkeys=ISBN%3A${isbn}&jscmd=data&format=json" "$tmp/books.json"
# parse_catalog_lookup reads: the bibkey top key, then key/url/title,
# authors[].name, publishers[].name, identifiers.isbn_10/isbn_13, cover.
check "$tmp/books.json" "Books API record shape" \
  "ISBN:${isbn}:dict" \
  "ISBN:${isbn}.key:str" \
  "ISBN:${isbn}.title:str"

echo "== 2. Read API scan availability =="
fetch "https://openlibrary.org/api/volumes/brief/isbn/${isbn}.json" "$tmp/read.json"
# parse_scan_availability reads: items[].match / itemURL / status.
check "$tmp/read.json" "Read API items shape" "items:list"
ocaid="$(python3 - "$tmp/read.json" <<'PY'
import json
import sys
from urllib.parse import urlparse

with open(sys.argv[1], "rb") as handle:
    document = json.load(handle)
for item in document.get("items", []):
    if item.get("match") != "exact":
        continue
    url = urlparse(item["itemURL"])
    parts = [p for p in url.path.split("/") if p]
    if url.hostname in ("archive.org", "www.archive.org") and parts[:1] == ["details"]:
        print(parts[1])
        break
PY
)"
if [[ -z "$ocaid" ]]; then
  # Not drift by itself (a book can lose its scan), but this smoke needs one.
  keep_tmp=1
  echo "no exact-match scan for ISBN ${isbn}; pick a scanned edition (payload: $tmp/read.json)" >&2
  exit 1
fi
echo "ok: exact-match scan -> ocaid=${ocaid}"

echo "== 3. archive.org item metadata =="
fetch "https://archive.org/metadata/${ocaid}" "$tmp/metadata.json"
# parse_item_metadata reads: server, dir, metadata.mediatype == texts.
check "$tmp/metadata.json" "item metadata shape" \
  "server:str" "dir:str" "metadata.mediatype=texts"

echo "== 4. BookReader search-inside =="
search_url="$(python3 - "$tmp/metadata.json" "$ocaid" "$query" <<'PY'
import json
import sys
from urllib.parse import urlencode

with open(sys.argv[1], "rb") as handle:
    document = json.load(handle)
params = urlencode({
    "item_id": sys.argv[2],
    "doc": sys.argv[2],
    "path": document["dir"],
    "q": sys.argv[3],
})
print(f"https://{document['server']}/fulltext/inside.php?{params}")
PY
)"
fetch "$search_url" "$tmp/inside.json"
# parse_search_inside reads: matches[].text (with {{{...}}} markers) and
# matches[].par[0].page; `indexed` when present.
check "$tmp/inside.json" "search-inside shape" "matches:list"
if ! python3 - "$tmp/inside.json" <<'PY'; then
import json
import sys

with open(sys.argv[1], "rb") as handle:
    document = json.load(handle)
matches = document.get("matches", [])
if not matches:
    # An indexed scan with zero matches is a legitimate outcome; only an
    # un-JSON or shapeless response is drift. Still worth eyeballing.
    print("note: zero matches for this query; try broader terms")
    sys.exit(0)
first = matches[0]
if not isinstance(first.get("text"), str):
    sys.exit("matches[0].text is not a string")
par = first.get("par")
if not (isinstance(par, list) and par and isinstance(par[0].get("page"), int)):
    sys.exit("matches[0].par[0].page is not an integer")
print(f"first match p.{par[0]['page']}: {first['text'][:100]!r}")
PY
  keep_tmp=1
  echo "CONTRACT DRIFT at step: search-inside match shape (payload: $tmp/inside.json)" >&2
  exit 1
fi

echo
echo "Open Library / Internet Archive read-contract smoke passed."
