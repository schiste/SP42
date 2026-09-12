"""Apply human adjudication decisions (exported from review.html) to the corpora.

Decisions are {id: "verdict"} or {id: {"v":"verdict","n":"note"}}, where verdict
is one of the four outcomes or "drop". Writes only OVERRIDES:
  - verdict != stored prior  -> corrections.json (method: human-review)
  - verdict == prior         -> no-op (prior label stood)
  - "drop"                   -> exclusions.json (bad case / extraction artifact)

    python3 adjudicate.py apply <decisions.json> <data-repo-dir>
    python3 adjudicate.py status <decisions.json>
"""
from __future__ import annotations
import json, os, sys

SCR = "/tmp/claude-1000/-var-home-louie-Projects-Volunteering-Consulting-SP42/d2a59721-e423-4fec-a993-95ebd3386eaf/scratchpad"
QUEUE = f"{SCR}/review-queue.json"
OUTCOMES = {"supported", "partial", "not_supported", "source_unavailable"}


def _cases():
    q = json.load(open(QUEUE))
    out = {}
    for g in ("judgment_disagreements", "extraction_source_unavailable"):
        for r in q.get(g, []):
            out[r["id"]] = r
    return out


def _load(decisions_path):
    d = json.load(open(decisions_path))
    norm = {}
    for cid, val in d.items():
        if isinstance(val, dict):
            norm[cid] = (val.get("v"), val.get("n", ""))
        else:
            norm[cid] = (val, "")
    return norm


def status(decisions_path):
    cases, dec = _cases(), _load(decisions_path)
    print(f"decisions: {len(dec)} / {len(cases)} queued")
    from collections import Counter
    print("by verdict:", dict(Counter(v for v, _ in dec.values())))


def apply(decisions_path, repo):
    cases, dec = _cases(), _load(decisions_path)
    corr, excl, noop, unknown = {}, {}, 0, 0
    for cid, (v, note) in dec.items():
        r = cases.get(cid)
        if not r:
            unknown += 1
            continue
        corpus = r["corpus"]
        if v == "drop":
            excl.setdefault(corpus, {})[cid] = {"reason": "human-dropped", "note": note,
                                                "was": f"{r['prior']}->{r['opus']}"}
        elif v in OUTCOMES and v != r["prior"]:
            corr.setdefault(corpus, {})[cid] = {"expected_outcome": v, "method": "human-review",
                "note": note or f"opus first-pass flagged {r['prior']}->{r['opus']}; human set {v}"}
        elif v == r["prior"]:
            noop += 1
        else:
            unknown += 1

    for corpus, entries in corr.items():
        path = f"{repo}/corpora/{corpus}/corrections.json"
        doc = json.load(open(path)) if os.path.exists(path) else {"as_of": None, "corrections": {}}
        doc["corrections"].update(entries)
        json.dump(doc, open(path, "w"), indent=2, ensure_ascii=False)
        print(f"{corpus}: +{len(entries)} corrections -> {path}")
    for corpus, entries in excl.items():
        path = f"{repo}/corpora/{corpus}/exclusions.json"
        doc = json.load(open(path)) if os.path.exists(path) else {"exclusions": {}}
        doc["exclusions"].update(entries)
        json.dump(doc, open(path, "w"), indent=2, ensure_ascii=False)
        print(f"{corpus}: +{len(entries)} exclusions -> {path}")
    print(f"no-op (prior stood): {noop} | unknown/unmatched: {unknown}")


if __name__ == "__main__":
    cmd = sys.argv[1]
    if cmd == "apply":
        apply(sys.argv[2], sys.argv[3])
    elif cmd == "status":
        status(sys.argv[2])
