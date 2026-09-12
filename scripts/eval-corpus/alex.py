"""Import the alex-cite-checker GT-corrected 189 into the shared case schema.

Source: alex-cite-checker/citation-checker-script/benchmark/dataset.json. Each
row has a curated 3-class `ground_truth`, the claim, a claim container, the cited
URL, and — unlike WAFER — an inline `source_text` (a bounded fair-use extract),
plus an `article_url` carrying the `oldid` revision.

Conformance to PRD-0007 (cases.py): row-number `id` and `dataset_version` are
banned keys — we re-derive a content-hash id and drop the version token. The
claim payload is CC BY-SA (Wikipedia, attributed to article + revision); the
`source_text` payload is labeled fair use.
"""

from __future__ import annotations

import argparse
import json
from typing import Optional

from cases import (
    NOT_SUPPORTED,
    PARTIAL,
    SUPPORTED,
    EmittedCase,
    case_id,
    case_to_dict,
    fair_use_licensing,
    wikipedia_licensing,
)

# The alex GT-corrected dataset's labeling date (PR #205 corrections).
ALEX_AS_OF = "2026-05-09"

_GT = {
    "Supported": SUPPORTED,
    "Partially supported": PARTIAL,
    "Not supported": NOT_SUPPORTED,
}


def load_rows(path: str) -> list[dict]:
    """dataset.json is a dict; the rows are its one list-of-dicts value."""
    data = json.load(open(path, encoding="utf-8"))
    if isinstance(data, list):
        return data
    for value in data.values():
        if isinstance(value, list) and value and isinstance(value[0], dict):
            return value
    raise ValueError("no list-of-rows found in dataset.json")


def row_to_case(row: dict) -> Optional[EmittedCase]:
    outcome = _GT.get(str(row.get("ground_truth")))
    claim = row.get("claim_text", "")
    url = row.get("source_url", "")
    if outcome is None or not claim or not url:
        return None
    article_url = row.get("article_url")
    source_text = row.get("source_text") or None
    licensing = {
        "claim": wikipedia_licensing(row.get("article_title"), article_url),
    }
    if source_text:
        licensing["source_text"] = fair_use_licensing(url)
    return EmittedCase(
        id=case_id(claim, url),
        claim=claim,
        claim_context={
            "context_sentences": [],
            "section": None,
            "article_title": row.get("article_title"),
            "container": row.get("claim_container"),
        },
        source_url=url,
        expected_outcome=outcome,
        label_method="alex-cite-checker-curated",
        label_as_of=ALEX_AS_OF,
        cohorts={"language": "en", "featured": None, "categories": []},
        licensing=licensing,
        provenance={
            "dataset": "alex-cite-checker",
            "alex_id": row.get("id"),
            "article_url": article_url,
            "citation_number": row.get("citation_number"),
            "occurrence": row.get("occurrence"),
            "total_occurrences": row.get("total_occurrences"),
            "extraction_status": row.get("extraction_status"),
            "needs_manual_review": row.get("needs_manual_review"),
        },
        source_text=source_text,
    )


def import_cases(path: str) -> list[EmittedCase]:
    cases, seen = [], set()
    for row in load_rows(path):
        c = row_to_case(row)
        if c and c.id not in seen:
            seen.add(c.id)
            cases.append(c)
    return cases


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--dataset", required=True, help="alex dataset.json path")
    ap.add_argument("--out", default=None, help="write cases.json here")
    args = ap.parse_args(argv)
    cases = import_cases(args.dataset)
    from collections import Counter

    dist = dict(sorted(Counter(c.expected_outcome for c in cases).items()))
    print(json.dumps({"count": len(cases), "by_outcome": dist}, indent=2))
    if args.out:
        with open(args.out, "w", encoding="utf-8") as fh:
            json.dump([case_to_dict(c) for c in cases], fh, indent=2, ensure_ascii=False)


if __name__ == "__main__":
    main()
