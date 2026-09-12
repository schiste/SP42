# Citation eval-corpus extraction recipe

Builds the labeled `(claim, source, expected-verdict)` corpus that measures
SP42's citation-verdict quality. This is the **content/recipe** side; the runner,
metrics, schema, and gates live in **PRD-0007**. Methodology: `docs/CITATION_EVAL_CORPUS.md`.

> **Status: generates a real silver corpus.** Verified against downloaded
> `wafer-dev` (positives) + `wafer-fail-dev` (negatives); produces ~900 balanced
> cases with single-sentence claims (claim = last entry of `meta.sentences`, the
> sentence `[CIT]` follows; context = the preceding entries). Labels are **silver**
> (weak): dev positives → `supported` (~⅓ noisy per SIDE), fail-dev → `not_supported`.
> Still TODO: gold hand-labeling, alex-cite-checker + adversarial sources. The
> emitted case schema is **provisional** until PRD-0007 freezes it (`case_to_dict`
> is the single reconcile point).
>
> Generate from real splits:
> ```bash
> python3 build_corpus.py \
>   --input wafer-dev.jsonl:dev --input wafer-fail-dev.jsonl:fail-dev \
>   --out OUT --per-stratum 200 --seed 1
> ```

## Layout

| File | Role |
|---|---|
| `sourcetype.py` | URL → source-type classifier + extractability allowlist (WAFER has no type label) |
| `wafer.py` | parse WAFER JSONL records into a normalized `WaferCase` |
| `build_corpus.py` | pipeline: emit (claim×source) cases → derive type → partition → stratified sample → manifest |
| `fixtures/wafer-sample.jsonl` | 3 synthetic WAFER records (news positive / book fail / reference positive) |
| `test_*.py` | unit tests (stdlib `unittest`) |

## Run

```bash
cd scripts/eval-corpus
python3 -m unittest discover -s . -p 'test_*.py'                 # tests
python3 build_corpus.py --wafer fixtures/wafer-sample.jsonl --dry-run   # manifest only
python3 build_corpus.py --wafer <real.jsonl> --split fail-test --out /tmp/corpus --per-stratum 200 --seed 1
```

The manifest records the extractability allowlist, the seed, and per-source-type
active/queued/sampled counts — so a re-run after a new extractor lands is
auditable and nothing is silently dropped.

## How growth works

Extractability is keyed to `EXTRACTABLE_SOURCE_TYPES` in `sourcetype.py`. Cases
whose source type isn't extractable yet (e.g. `book` → Google Books) are tagged
`queued` and written to `queued.json`, never dropped. When the extractor for that
type lands, add the type to the allowlist and re-run — the queued cases are
harvested in bulk.

## Next steps (not done)

1. **Download a real WAFER split** (`dl.fbaipublicfiles.com/side/`, or their
   `data/download_data.py`) and **verify the nested field names** in `wafer.py`
   against an actual record (see `_FIELD_TODO`). Start with `fail-dev` (725) — the
   smallest, and the labeled negatives.
2. **Merge alex-cite-checker's GT-corrected 189** and **adversarial hand-built**
   cases into the pipeline (new loaders alongside `wafer_to_cases`).
3. **Reconcile the schema with PRD-0007**, then settle on the corpus location
   (`evals/citation/`).
4. **Gold tier**: hand-verify ~250–500 cases (fine-grained verdicts + evidence);
   silver labels from WAFER are weak (positives are ~⅓ noisy per SIDE).
5. **Compoundness** cohort is currently `null` — filled once the decomposition
   work (parallel session) provides atom counts.
