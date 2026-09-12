"""Build the adjudication review queue from an Opus first-pass result file.

Keeps only disagreements, splits judgment vs extraction (source_unavailable),
and drops multi-sentence (>=2) claims (the agreed single-cited-sentence standard).
Carries the quote-locate and metadata flags through for display.

    python3 build_queue.py --firstpass <opus-firstpass.json> --candidates <gold-candidates.json> --out <review-queue.json>
"""
from __future__ import annotations
import argparse, json, re

def nsent(t): return len([s for s in re.split(r'(?<=[.!?])\s+', (t or "").strip()) if s])

def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--firstpass", required=True)
    ap.add_argument("--candidates", required=True)
    ap.add_argument("--out", required=True)
    args = ap.parse_args(argv)
    results = json.load(open(args.firstpass))["results"]
    claims = {c["id"]: c["claim"] for c in json.load(open(args.candidates))}
    q = {"note": "Opus first-pass PROPOSALS for human review; NOT gold until confirmed.",
         "judgment_disagreements": [], "extraction_source_unavailable": [], "errors": []}
    dropped_multi = 0
    for r in results:
        if r.get("error"):
            q["errors"].append(r); continue
        if not r["opus"]:
            continue
        if nsent(claims.get(r["id"], "")) >= 2:  # single-cited-sentence standard
            dropped_multi += 1; continue
        if r["agrees"]:
            continue
        item = {k: r.get(k) for k in ("id", "corpus", "prior", "opus", "quote",
                                      "fetch_status", "quote_located", "has_metadata")}
        bucket = "extraction_source_unavailable" if r["opus"] == "source_unavailable" else "judgment_disagreements"
        q[bucket].append(item)
    json.dump(q, open(args.out, "w"), indent=1, ensure_ascii=False)
    print(json.dumps({"judgment": len(q["judgment_disagreements"]),
                      "extraction": len(q["extraction_source_unavailable"]),
                      "errors": len(q["errors"]), "dropped_multi_sentence": dropped_multi}, indent=2))

if __name__ == "__main__":
    main()
