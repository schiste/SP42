# Citation evaluation corpus — methodology

**Drafter:** Claude Code (Opus 4.8)
**Editor:** Luis Villa
**Date:** 2026-06-23
**State:** Draft
**Companion:** PRD-0007 (LLM output-quality benchmarking) owns the runner,
metrics, and corpus *schema*; this document owns the corpus *content* — where
cases come from, how they are labeled and tagged, and how the set is grown
repeatably.

## Summary

To know whether SP42's citation analysis makes *good* judgments — not just
whether the code runs — we need a labeled set of real `(claim, source,
expected-verdict)` cases to measure it against. This document describes how that
set is built: sampled from the public WAFER dataset plus a curated seed and some
hand-built hard cases, labeled in two trust tiers (a hand-verified gold core of a
few hundred cases that can gate changes, and a larger weakly-labeled silver set
for trends), and tagged
by cohort (verdict class, source type, language, extractability, compoundness) so
we can see *where* the model fails rather than just an aggregate score. The set is
not a static file: it is the output of a **documented, re-runnable extraction
recipe** keyed to what SP42 can extract today, so it grows automatically as
extraction capability grows.

## Scope boundary

This owns **corpus content and its construction**:

- which sources cases come from, and the sampling/labeling/tagging method;
- the repeatable WAFER extraction recipe and how the set grows.

It deliberately excludes, deferring to **PRD-0007**:

- the **case schema** (field names, id derivation, validation) — PRD-0007;
- the **runner, metrics, replay/compare modes, and gates** — PRD-0007;
- anything about *acting on* a verdict (insertion/repair) — PRD-0009 (deferred).

Cases here are authored to fit PRD-0007's schema; if the two disagree, PRD-0007's
schema wins and this recipe adapts.

## Sources

Three, blended:

1. **WAFER** (public: `github.com/facebookresearch/side`, `datasets/WAFER.md`;
   JSONL at `dl.fbaipublicfiles.com/side/`). 3.8M train / 4,545 dev / 4,568 test,
   plus **`fail-dev` (725) + `fail-test` (730)** — citations that *failed
   verification*. We sample; we never ingest whole. Each record carries an
   in-context claim with a `[CIT]` tag, the cited source(s) with `url`/`title`/
   `text` (a 2021 web snapshot), and `meta` (section, claim offset, preceding
   sentences, `featured` flag).
2. **alex-cite-checker** — the GT-corrected 189-row dataset and its compoundness
   sidecar (`workbench/compound-corpus`). Already curated; the seed of the gold
   tier.
3. **Adversarial hand-built** — a small set targeting known failure modes
   (over-literal matching, compound under-decomposition, churnalism, anti-bot
   junk titles) to stress the decision boundary where real corpora are thin.

**Not used (yet):** fresh Wikipedia / frwiki sampling. Consequence below.

## Labels and trust tiers

WAFER gives two label signals, of different quality:

- **Noisy positives** (dev/test): the existing citation, presumed to support — but
  SIDE found **~⅓ do not**. A `supported` from here is ~67% reliable: silver-grade.
- **Labeled negatives** (`fail-*`, ~1,455): citations deliberately identified as
  failing verification — the hardest label to source elsewhere, and high value.

From these we build:

- **Gold core (~250–500, gating).** Hand-verified fine-grained verdicts
  (`supported` / `partial` / `not_supported` / `source_unavailable`), each with the
  located supporting quote where applicable. Drawn broadly (see Cohorts) from
  alex's 189, WAFER `fail-*`, *audited* WAFER positives, and the adversarial set.
  This is the only tier allowed to block a change.
- **Silver (larger, trend-only).** WAFER sample with weak labels (positive →
  supported-ish, `fail` → not-supported), cohort-tagged. Reports movement and
  per-cohort signal; **never gates**.

Labeling discipline follows the field reality: because the task is genuinely
subjective (SIDE inter-annotator κ = 0.11–0.27), gold labels are recorded with
the *evidence* (the supporting quote or the reason for failure), not just the
bucket — a label you can audit, not just trust.

## Cohorts

Every case is tagged so quality is reported per slice:

| Cohort | Values | Source |
|---|---|---|
| **verdict class** | supported / partial / not_supported / source_unavailable | label |
| **source type** | news / reference / journal / book / gov / blog / … | **derived** from URL domain (WAFER has no type label) |
| **extractability** | extractable-now / queued | derived from source type vs the current allowlist |
| **language** | en (only, for v1) | WAFER is English-only |
| **compoundness** | proposition count | alex's sidecar method |
| **featured** | yes / no | WAFER `meta.featured` — a free popularity-bias cohort (SIDE measured this bias) |

The deliberate aim is a **broad net**, balanced across verdict class × source type
× extractability — not a set dominated by easy `supported` news links.

> **Finding (2026-06-23, real `wafer-fail-dev`, 724 sources):** the domain→type
> classifier catches the big buckets (news 73, gov 20, blog 13) but **~85% (617)
> fall to `other`** — a true long tail (534 distinct hosts, top host = 8). So a
> fine source-type taxonomy via domain map cannot finely stratify real WAFER;
> `other` will always be the dominant stratum. Two readings: (a) accept `other`
> as a first-class stratum and balance across the *identifiable* types + `other`;
> (b) get a better signal (TLD/registrar class, or a content classifier) later.
> Also: many `other` hosts are **regional/international news** (indianexpress,
> ndtv, zeenews, spiegel, assamtribune…) the US/UK-centric news list misses — a
> global-coverage gap worth closing both for stratification and because it
> overlaps the contested-topic/bias concern.

## Source resolution (extraction vs judgment)

Each case stores the **URL**. At eval time the source text is resolved **two
ways** where possible:

- **Live re-fetch** through SP42's own extractor — exercises the real pipeline,
  including extraction quality (the thing we are growing).
- **WAFER snapshot** — a clean, extraction-independent oracle, and a fallback for
  link rot.

Running the model against both lets us **attribute a wrong verdict to extraction
or to judgment**: right-on-snapshot but wrong-on-live ⇒ extraction failure;
wrong on both ⇒ judgment failure. This is what makes the corpus a measure of
*decision* quality rather than a blur of the two.

> **Finding (2026-06-23, verified against `wafer-fail-dev`):** the snapshot text
> is **not inline** in the records. A record's `output[].provenance[]` carries
> only `chunk_id` + `url`; the source text lives in the separate Sphere chunk
> corpus (134M passages), keyed by `chunk_id`. So the snapshot-oracle half is not
> free: it needs either (a) the positive `dev`/`test` splits *if* they inline text
> (unverified), or (b) ingesting the relevant Sphere chunks by id, or (c) for
> `fail-dev`, accepting **live-fetch only** — and these URLs are old, so expect
> heavy link rot landing many cases at `source_unavailable` (itself a measured
> outcome). Open question added below.

## The repeatable extraction recipe

The corpus is the output of a documented, re-runnable pipeline — not a hand-built
file:

```
for a WAFER split:
  1. derive source-type      domain classifier (url → news/reference/book/…)
  2. partition by allowlist  EXTRACTABLE_SOURCE_TYPES → extractable-now | queued
                             (queued cases are TAGGED and COUNTED, never dropped silently)
  3. stratified sample       balance verdict-class × source-type × extractability
                             (record the sampling seed)
  4. resolve source text     live re-fetch (SP42 extractor) + WAFER snapshot
  5. emit                    cases in PRD-0007's schema, with cohort tags
```

The recipe is **keyed to extraction capability** via `EXTRACTABLE_SOURCE_TYPES`.
Today that excludes, e.g., Google Books (we can't extract book interiors): those
cases are tagged `queued`, counted, and left in the pool. When GBS parsing lands,
add `book` to the allowlist and **re-run** — the queued Google Books cases are
harvested in bulk, no re-curation. Every run records the allowlist, the
per-source-type queued/excluded counts, and the seed: growth is auditable and
truncation is never silent.

## Growth path

- **Source types** widen as extraction does (Google Books → PDFs → paywalled
  proxies …): each is an allowlist entry + a re-run.
- **Languages**: WAFER is English-only, so the **non-English cohort is a known
  gap**, queued behind fresh frwiki sampling (deselected for v1). frwiki is an SP42
  target wiki, so this is a real future need, tracked here, not hidden.
- **Volume**: the silver tier scales by raising the sample size; the gold tier
  grows only by hand-verification. We expect to ingest **all of WAFER `dev`**
  (4,545) over time, not just `fail-dev` — `dev` is the first pull only because
  it is small and the labeled negatives are the scarce kind.
- **Bias / contested-topic test set (TODO, deliberate).** A general WAFER sample
  under-represents the cases where citation judgment is most consequential and
  most prone to model bias: topics where a powerful actor has an interest in the
  verdict. We need a dedicated survey of bias areas and a hand-built test set
  spanning them — e.g. topics contested by the Chinese government (Tiananmen,
  Xinjiang, Taiwan, Hong Kong), and by the current US administration (trans
  rights/healthcare, climate science). The point is to measure whether grounding
  holds the line on contested claims or inherits a model's political/cultural
  prior — the failure mode SIDE's popularity-bias finding only hints at. This is
  its own cohort and its own curation effort; track separately.

## Open questions

Each with a proposed answer to react to.

1. **Gold size and who labels it.** Target: **~250–500** hand-verified cases —
   large enough that per-cohort slices (verdict class × source type) stay
   statistically readable rather than a handful each. **Labeling workflow
   (decided 2026-06-23):** **Opus as a first-pass gold verifier** (proposes a
   fine-grained verdict + the supporting/contradicting evidence per case), with
   **the Editor as a follow-up read**. This makes a few-hundred-case gold tier
   tractable without full from-scratch dual-labeling; the Editor's pass is the
   trust anchor. (Note the irony to watch: the system under evaluation and the
   first-pass labeler are both LLMs — the Editor read is what keeps the gold
   labels independent of the thing being measured.)
2. **Where the corpus and recipe live.** Proposed: `evals/citation/` (matching
   PRD-0007's `evals/<task>/` shape) — `corpus.json` (cases), the recipe script,
   and this doc's operational form as its README. Deferred to PRD-0007's
   directory decision to avoid collision.
3. **Snapshot staleness.** WAFER's text is from 2021; live pages drift. Proposed:
   keep both (above); when they diverge materially, prefer the snapshot for the
   *judgment* metric and treat the live/snapshot gap as itself a reportable
   signal (link rot + extraction drift).
4. **Source-type classifier fidelity.** A domain→type map is approximate, and the
   real long tail (above) means `other` dominates. Proposed: treat `other` as a
   first-class stratum, expand the map opportunistically for the high-frequency
   and regional-news hosts the sample surfaces, and revisit a better signal
   (TLD/registrar class or a content classifier) only if `other` proves too coarse
   to read. Accuracy only needs to be good enough to stratify, not to label.

5. **Snapshot-oracle source. RESOLVED (2026-06-23):** `dev` was downloaded and
   **also has no inline text** — provenance is `url` + `chunk_id` only across all
   65k chunks. So *no* split inlines source text. The snapshot oracle therefore
   requires **ingesting Sphere chunks by `chunk_id`**, or we accept
   **live-fetch-only** (with link rot → more `source_unavailable`). Decision still
   open between those two, but the "check the positive splits" path is closed.

Resolved while verifying (2026-06-23):
- `dev` source URL = the `answer` field (the gold citation); provenance is a
  ~14-url candidate pool. fail-dev's `answer` is "{Failed verification}" and its
  single provenance url *is* the failed source. Parser handles both.
- `dev` meta is richer: it **has `featured`** (727/4,545 ≈ 16%) and
  **`categories`** (a comma-joined string, 17,321 distinct — the topic signal the
  contested-topic/bias set needs). fail-dev meta has neither.
```

## Harness architecture: drive the product verifier, don't re-implement it

Corpus creation and verdict-quality testing is an **ongoing** process, not
one-off scaffolding. The harness therefore treats `sp42-cli --verify` as the
single source of truth for verification and **drives the binary**, rather than
mirroring the product's internals in Python. Prompt building, response parsing,
grounding (`locate_quote`), and panel voting then all come from the product — so
any future verifier change is reflected in the benchmark automatically. Python
keeps what it is good at: WAFER/corpus parsing, candidate selection,
adjudication, queue/review building, and orchestration.

This retires the parts of `opus_verify.py` that currently copy product internals
— the verbatim `SYSTEM` prompt, the `metadata_section` rendering, the regex
response parse, and the naive substring quote-locate (the product's real
`grounding_status` is a strict upgrade over the substring check).

Status / dependencies:

- The prompt text now lives in editable data files loaded via `include_str!`
  (PR #95). Until the harness switches to driving the CLI, reading that file
  (plus a byte-identical guard test) is the **interim** single-source for the
  prompt text only.
- Driving the CLI for model runs is **blocked on #94** — `--verify` needs an
  offline source body, a metadata sidecar, model/panel selection, and a JSONL
  batch mode. (`--verify --debug-votes` already emits the full
  `VerificationOutcome` the harness records, so output is not a blocker.)
- This is the eval-side instance of the broader single-source-of-truth question
  in **#93** (content/data as editable artifacts).
