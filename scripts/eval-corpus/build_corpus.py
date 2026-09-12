"""Build a WAFER citation eval corpus, conformed to PRD-0007's schema (cases.py).

Pipeline (docs/CITATION_EVAL_CORPUS.md, "The repeatable extraction recipe"):

    WAFER record -> emit (claim x source) cases (cases.EmittedCase)
                 -> derive source-type (sourcetype.classify) AT BUILD TIME
                 -> partition by extractability allowlist (active | queued)
                 -> stratified sample by (source-type x outcome), seeded
                 -> write cases.json + queued.json + manifest.json

Source type and extractability are *computed* facets (PRD-0007): they drive
sampling/partition here but are NOT stored in the case — only in the manifest.
Queued cases (e.g. Google Books, until book extraction lands) are counted and
written out, never silently dropped.

Usage:
    python3 build_corpus.py --input wafer-dev.jsonl:dev --input wafer-fail-dev.jsonl:fail-dev \
        --out OUT --per-stratum 200 --seed 1
    python3 build_corpus.py --input ... --dry-run        # manifest only
"""

from __future__ import annotations

import argparse
import json
import os
import random
from collections import Counter
from typing import Callable, Optional

import sourcetype as st
import wafer as wf
from cases import (  # re-exported for callers/tests
    NOT_SUPPORTED,
    SUPPORTED,
    EmittedCase,
    case_id,
    case_to_dict,
    wikipedia_licensing,
)

# WAFER is a fixed 2021 snapshot released 2022-01; its labels reflect that.
WAFER_AS_OF = "2022-01-20"


# --- WAFER -> cases ---------------------------------------------------------


def wafer_to_cases(case: wf.WaferCase) -> list[EmittedCase]:
    """One emitted case per (claim, source). Silver labels from the split."""
    out: list[EmittedCase] = []
    context = {
        "context_sentences": case.context_sentences,
        "section": case.section,
        "article_title": case.title,
    }
    licensing = {
        "claim": wikipedia_licensing(
            case.title, case.wikipedia_url, case.raw_meta.get("wikipedia_id")
        )
    }
    for src in case.sources:
        if not src.url:
            continue
        failed = case.is_fail_split or src.failed_verification
        out.append(
            EmittedCase(
                id=case_id(case.claim_text, src.url),
                claim=case.claim_text,
                claim_context=context,
                source_url=src.url,
                expected_outcome=NOT_SUPPORTED if failed else SUPPORTED,
                label_method="wafer-fail-split" if failed else "wafer-distant-supervision",
                label_as_of=WAFER_AS_OF,
                cohorts={
                    "language": "en",  # WAFER is English-only
                    "featured": case.featured,
                    "categories": case.categories,  # topic signal for bias set
                },
                licensing=licensing,
                provenance={
                    "dataset": "wafer",
                    "split": case.split,
                    "wafer_id": case.id,
                    "source_chunk_ids": list(src.chunk_ids),  # for later Sphere lookup
                },
                source_text=None,  # WAFER carries no inline text
            )
        )
    return out


# --- Partition + sample (compute source-type at build time) -----------------


def partition_extractable(
    cases: list[EmittedCase],
) -> tuple[list[EmittedCase], list[EmittedCase]]:
    """Split into (active, queued) by computed extractability. No drops."""
    active, queued = [], []
    for c in cases:
        stype = st.classify(c.source_url)
        (active if st.extractability(stype) == st.EXTRACTABLE else queued).append(c)
    return active, queued


def stratum_key(c: EmittedCase) -> tuple:
    return (st.classify(c.source_url), c.expected_outcome)


def stratified_sample(
    cases: list[EmittedCase],
    per_stratum: int,
    seed: int,
    key_fn: Callable[[EmittedCase], tuple] = stratum_key,
) -> list[EmittedCase]:
    """Up to `per_stratum` per cohort key, deterministically seeded."""
    buckets: dict[tuple, list[EmittedCase]] = {}
    for c in cases:
        buckets.setdefault(key_fn(c), []).append(c)
    rng = random.Random(seed)
    sampled: list[EmittedCase] = []
    for key in sorted(buckets, key=lambda k: tuple(str(x) for x in k)):
        bucket = sorted(buckets[key], key=lambda c: c.id)
        rng.shuffle(bucket)
        sampled.extend(bucket[:per_stratum])
    return sampled


# --- Manifest ---------------------------------------------------------------


def build_manifest(*, active, queued, sampled, per_stratum, seed) -> dict:
    def by_type(cases):
        return dict(sorted(Counter(st.classify(c.source_url) for c in cases).items()))

    def by_outcome(cases):
        return dict(sorted(Counter(c.expected_outcome for c in cases).items()))

    return {
        "extractable_source_types": sorted(st.EXTRACTABLE_SOURCE_TYPES),
        "seed": seed,
        "per_stratum": per_stratum,
        "counts": {
            "active_total": len(active),
            "queued_total": len(queued),
            "sampled_total": len(sampled),
            "active_by_source_type": by_type(active),
            "queued_by_source_type": by_type(queued),
            "sampled_by_source_type": by_type(sampled),
            "sampled_by_outcome": by_outcome(sampled),
        },
        "note": "source_type/extractability are computed facets (not stored in cases); "
        "queued cases are retained for a re-run once their source type becomes extractable",
    }


# --- Driver -----------------------------------------------------------------


def run_corpus(inputs: list[tuple[str, Optional[str]]], per_stratum: int, seed: int):
    all_cases: list[EmittedCase] = []
    seen: set[str] = set()
    for path, split in inputs:
        for wc in wf.iter_jsonl(path, split=split):
            for case in wafer_to_cases(wc):
                if case.id in seen:
                    continue
                seen.add(case.id)
                all_cases.append(case)
    active, queued = partition_extractable(all_cases)
    sampled = stratified_sample(active, per_stratum=per_stratum, seed=seed)
    manifest = build_manifest(
        active=active, queued=queued, sampled=sampled, per_stratum=per_stratum, seed=seed
    )
    return sampled, queued, manifest


def run(wafer_path: str, split: Optional[str], per_stratum: int, seed: int):
    return run_corpus([(wafer_path, split)], per_stratum, seed)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--wafer", default=None, help="single WAFER JSONL path")
    ap.add_argument("--split", default=None, help="split name for --wafer (e.g. fail-dev)")
    ap.add_argument("--input", action="append", default=[], help="PATH:SPLIT, repeatable")
    ap.add_argument("--out", default=None, help="output dir for cases.json + manifest.json")
    ap.add_argument("--per-stratum", type=int, default=50)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--dry-run", action="store_true", help="print manifest, write nothing")
    args = ap.parse_args(argv)

    if args.input:
        inputs: list[tuple[str, Optional[str]]] = []
        for item in args.input:
            head, sep, tail = item.rpartition(":")
            inputs.append((head, tail or None) if sep else (tail, None))
    elif args.wafer:
        inputs = [(args.wafer, args.split)]
    else:
        ap.error("provide --wafer or at least one --input")

    sampled, queued, manifest = run_corpus(inputs, args.per_stratum, args.seed)

    if args.dry_run or not args.out:
        print(json.dumps(manifest, indent=2))
        return

    os.makedirs(args.out, exist_ok=True)
    with open(os.path.join(args.out, "cases.json"), "w", encoding="utf-8") as fh:
        json.dump([case_to_dict(c) for c in sampled], fh, indent=2, ensure_ascii=False)
    with open(os.path.join(args.out, "queued.json"), "w", encoding="utf-8") as fh:
        json.dump([case_to_dict(c) for c in queued], fh, indent=2, ensure_ascii=False)
    with open(os.path.join(args.out, "manifest.json"), "w", encoding="utf-8") as fh:
        json.dump(manifest, fh, indent=2)
    print(json.dumps(manifest, indent=2))


if __name__ == "__main__":
    main()
