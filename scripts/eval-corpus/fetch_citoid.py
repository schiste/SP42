"""Fetch Citoid bibliographic metadata per case source URL, mapped to SP42's
CitoidMetadata shape (publication / published / author / title). Cached so the
Opus re-run can include the same context-only metadata block the real pipeline
gives the model (ADR-0007 Alt (e)).

    python3 fetch_citoid.py --candidates <gold-candidates.json> --out <citoid.json> [--workers 6]
"""
from __future__ import annotations
import argparse, json, sys, urllib.parse, urllib.request
from concurrent.futures import ThreadPoolExecutor, as_completed

ENDPOINT = "https://en.wikipedia.org/api/rest_v1/data/citation/mediawiki/"
UA = "SP42-citation-eval/0.1 (luis@lu.is)"


def _authors(raw):
    out = []
    for a in raw or []:
        out.append(" ".join(str(p) for p in a if p).strip())
    return "; ".join(x for x in out if x) or None


def fetch_one(case):
    url = case["source_url"]
    try:
        req = urllib.request.Request(
            ENDPOINT + urllib.parse.quote(url, safe=""),
            headers={"User-Agent": UA, "accept": "application/json"})
        d = json.loads(urllib.request.urlopen(req, timeout=30).read())[0]
        meta = {
            "publication": d.get("publicationTitle") or d.get("websiteTitle"),
            "published": d.get("date"),
            "author": _authors(d.get("author")),
            "title": d.get("title"),
        }
        meta = {k: v for k, v in meta.items() if v}
        return case["id"], (meta or None)
    except Exception:
        return case["id"], None


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--candidates", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--workers", type=int, default=6)
    args = ap.parse_args(argv)
    cands = json.load(open(args.candidates))
    res, got, done = {}, 0, 0
    with ThreadPoolExecutor(max_workers=args.workers) as ex:
        futs = [ex.submit(fetch_one, c) for c in cands]
        for f in as_completed(futs):
            cid, meta = f.result()
            res[cid] = meta
            got += 1 if meta else 0
            done += 1
            if done % 25 == 0:
                print(f"  {done}/{len(cands)} ({got} with metadata)", file=sys.stderr, flush=True)
    json.dump(res, open(args.out, "w"), indent=1, ensure_ascii=False)
    print(json.dumps({"total": len(cands), "with_metadata": got,
                      "rate": round(got / len(cands), 3)}, indent=2))


if __name__ == "__main__":
    main()
