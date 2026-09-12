"""Fetch source text for corpus cases: live URL, falling back to the Wayback
Machine (public read API; no credentials needed to read existing snapshots).

Reports how many 2014-era links are live / recovered-from-archive / dead, and
caches extracted text so the Opus verification pass can run offline afterwards.

Usage:
    python3 fetch_sources.py --cases <cases.json> --out <fetched.json> [--workers 10] [--limit N]
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import urllib.parse
import urllib.request
from concurrent.futures import ThreadPoolExecutor, as_completed

UA = "Mozilla/5.0 (compatible; SP42-citation-eval/0.1; +https://lu.is)"
LIVE, ARCHIVE, DEAD, ERROR = "live", "archive", "dead", "error"
MIN_TEXT = 500       # below this, treat as unusable -> try archive
MAX_TEXT = 8000      # cap stored text
TIMEOUT = 20

_TAG = re.compile(r"<(script|style)[^>]*>.*?</\1>", re.S | re.I)
_TAGS = re.compile(r"<[^>]+>")
_WS = re.compile(r"\s+")


def extract_text(html: str) -> str:
    html = _TAG.sub(" ", html)
    text = _TAGS.sub(" ", html)
    text = (
        text.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">")
        .replace("&quot;", '"').replace("&#39;", "'").replace("&nbsp;", " ")
    )
    return _WS.sub(" ", text).strip()[:MAX_TEXT]


def _get(url: str):
    req = urllib.request.Request(url, headers={"User-Agent": UA})
    with urllib.request.urlopen(req, timeout=TIMEOUT) as r:
        ctype = r.headers.get("Content-Type", "")
        body = r.read(2_000_000)
        enc = r.headers.get_content_charset() or "utf-8"
        return r.status, ctype, body.decode(enc, errors="replace")


def fetch_live(url: str):
    try:
        status, ctype, html = _get(url)
        if status == 200 and ("html" in ctype or "text" in ctype or not ctype):
            text = extract_text(html)
            if len(text) >= MIN_TEXT:
                return LIVE, url, text
        return None
    except Exception:
        return None


def fetch_archive(url: str):
    try:
        api = "https://archive.org/wayback/available?url=" + urllib.parse.quote(url, safe="")
        _, _, body = _get(api)
        snap = json.loads(body).get("archived_snapshots", {}).get("closest")
        if not snap or not snap.get("available"):
            return None
        snap_url = snap["url"].replace("http://", "https://", 1)
        _, _, html = _get(snap_url)
        text = extract_text(html)
        if len(text) >= MIN_TEXT:
            return ARCHIVE, snap_url, text
        return None
    except Exception:
        return None


def fetch_one(case: dict) -> dict:
    url = case["source_url"]
    try:
        live = fetch_live(url)
        if live:
            status, used, text = live
        else:
            arch = fetch_archive(url)
            if arch:
                status, used, text = arch
            else:
                status, used, text = DEAD, url, ""
    except Exception as e:  # pragma: no cover
        status, used, text = ERROR, url, str(e)[:200]
    return {"id": case["id"], "status": status, "url_used": used, "text": text}


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--cases", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--workers", type=int, default=10)
    ap.add_argument("--limit", type=int, default=0)
    args = ap.parse_args(argv)

    cases = json.load(open(args.cases))
    if args.limit:
        cases = cases[: args.limit]
    results = {}
    counts = {LIVE: 0, ARCHIVE: 0, DEAD: 0, ERROR: 0}
    done = 0
    with ThreadPoolExecutor(max_workers=args.workers) as ex:
        futs = {ex.submit(fetch_one, c): c for c in cases}
        for fut in as_completed(futs):
            r = fut.result()
            results[r["id"]] = r
            counts[r["status"]] += 1
            done += 1
            if done % 50 == 0:
                print(f"  {done}/{len(cases)}  {counts}", file=sys.stderr, flush=True)

    json.dump(results, open(args.out, "w"), indent=1)
    total = len(cases)
    print(json.dumps({
        "total": total,
        "counts": counts,
        "rates": {k: round(v / total, 3) for k, v in counts.items()},
        "gradeable": counts[LIVE] + counts[ARCHIVE],
    }, indent=2))


if __name__ == "__main__":
    main()
