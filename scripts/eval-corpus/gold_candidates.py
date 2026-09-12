"""Assemble the Opus gold first-pass candidate set.

Merges gradeable cases — alex-189 (inline fair-use source_text) + wafer-en cases
whose source was fetched (live or archive) — into a flat list the Opus workflow
fans out over. Source text is capped so the candidate batch fits in workflow args.

Output candidate: {id, corpus, claim, source_url, source_text, prior_outcome, cohorts}

Usage:
    python3 gold_candidates.py --alex <alex/cases.json> --wafer <wafer/cases.json> \
        --fetched <wafer-fetched.json> --out <gold-candidates.json> \
        [--max-alex 75 --max-wafer 75 --text-cap 3000 --seed 1]
"""

from __future__ import annotations

import argparse
import json
import random
from collections import defaultdict


def _stratified(items, key, cap, seed):
    buckets = defaultdict(list)
    for it in items:
        buckets[key(it)].append(it)
    rng = random.Random(seed)
    per = max(1, cap // max(1, len(buckets)))
    out = []
    for k in sorted(buckets):
        b = sorted(buckets[k], key=lambda c: c["id"])
        rng.shuffle(b)
        out.extend(b[:per])
    return out[:cap]


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--alex", required=True)
    ap.add_argument("--wafer", required=True)
    ap.add_argument("--fetched", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--max-alex", type=int, default=75)
    ap.add_argument("--max-wafer", type=int, default=75)
    ap.add_argument("--text-cap", type=int, default=3000)
    ap.add_argument("--seed", type=int, default=1)
    args = ap.parse_args(argv)

    cap = args.text_cap
    candidates = []

    alex = json.load(open(args.alex))
    alex_sel = _stratified(alex, lambda c: c["expected_outcome"], args.max_alex, args.seed)
    for c in alex_sel:
        if not c.get("source_text"):
            continue
        candidates.append({
            "id": c["id"], "corpus": "alex-189", "claim": c["claim"],
            "source_url": c["source_url"], "source_text": c["source_text"][:cap],
            "prior_outcome": c["expected_outcome"], "cohorts": c.get("cohorts", {}),
        })

    wafer = json.load(open(args.wafer))
    fetched = json.load(open(args.fetched))
    gradeable = [
        c for c in wafer
        if fetched.get(c["id"], {}).get("status") in ("live", "archive")
        and fetched[c["id"]].get("text")
    ]
    wafer_sel = _stratified(
        gradeable, lambda c: c["expected_outcome"], args.max_wafer, args.seed
    )
    for c in wafer_sel:
        f = fetched[c["id"]]
        candidates.append({
            "id": c["id"], "corpus": "wafer-en", "claim": c["claim"],
            "source_url": f["url_used"], "source_text": f["text"][:cap],
            "prior_outcome": c["expected_outcome"], "cohorts": c.get("cohorts", {}),
            "fetch_status": f["status"],
        })

    json.dump(candidates, open(args.out, "w"), ensure_ascii=False)
    from collections import Counter
    print(json.dumps({
        "total": len(candidates),
        "by_corpus": dict(Counter(c["corpus"] for c in candidates)),
        "by_prior_outcome": dict(Counter(c["prior_outcome"] for c in candidates)),
        "approx_args_kb": round(len(json.dumps(candidates)) / 1024),
    }, indent=2))


if __name__ == "__main__":
    main()
