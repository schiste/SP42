# References / citation verification

A SP42 domain: LLM-assisted checking of whether a cited source actually supports
the claim it backs, plus citation repair. Verdicts are **informational** — SP42
reports support, contradiction, or insufficiency (with a locatable supporting
passage when it claims support) and never edits autonomously.

Citation logic lives in `sp42-core::citation` and the `sp42-inference` crate, and
builds on the platform LLM interface ([ADR-0006](../../platform/adr/0006-using-llms.md)).

## Product Requirements

- [PRD-0001 — Citation verification](prd/0001-citation-verification.md) — operator-facing capability, CLI-first surface, panel voting and agreement signal
- [PRD-0007 — LLM output-quality benchmarking](prd/0007-llm-output-benchmarking.md) — eval corpus, deterministic hard gates, and baseline/compare framework for LLM outputs
- [PRD-0008 — Bare-URL repair](prd/0008-bare-url-repair.md) — fill a bare URL into a complete cite template via propose/confirm

## Architecture Decision Records

- [ADR-0007 — Verdict and anti-fabrication semantics](adr/0007-citation-verification-semantics.md)
- [ADR-0008 — Request/response contract and crate placement](adr/0008-citation-verification-contract.md)
- [ADR-0009 — Source-snapshot storage for reproducibility](adr/0009-citation-source-snapshot-storage.md)
- [ADR-0011 — Article-level citation verification (the review path)](adr/0011-article-citation-verification.md)

The shared LLM posture these depend on (model panel, measured agreement, keyless
inference endpoint) is a platform decision,
[ADR-0006](../../platform/adr/0006-using-llms.md); the propose/confirm editing
pattern PRD-0008 builds on is
[ADR-0010](../../platform/adr/0010-operator-confirmed-content-proposals.md).
