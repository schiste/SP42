# References / citation verification

A SP42 domain: LLM-assisted checking of whether a cited source actually supports
the claim it backs, plus citation repair. Verdicts are **informational** — SP42
reports support, contradiction, or insufficiency (with a locatable supporting
passage when it claims support) and never edits autonomously.

Citation logic lives in the `sp42-citation` domain crate, calls models through
the `sp42-inference` platform crate, and is exposed to agents via the `sp42-mcp`
shell. It builds on the platform LLM interface
([ADR-0006](../../platform/adr/0006-using-llms.md)).

Operational guidance for the model panel is in [model-panel.md](model-panel.md).

## Product Requirements

- [PRD-0001 — Citation verification](prd/0001-citation-verification.md) — operator-facing capability, CLI-first surface, panel voting and agreement signal
- [PRD-0007 — LLM output-quality benchmarking](prd/0007-llm-output-benchmarking.md) — eval corpus, deterministic hard gates, and baseline/compare framework for LLM outputs
- [PRD-0008 — Bare-URL repair](prd/0008-bare-url-repair.md) — fill a bare URL into a complete cite template via propose/confirm
- [PRD-0009 — Book-citation grounding and Open Library enrichment](prd/0009-book-citation-grounding-and-open-library-enrichment.md) — verify book citations against Internet Archive full-text and offer sourced, operator-confirmed Open Library metadata improvements
- [PRD-0010 — Citation-verification agent surface (MCP)](prd/0010-citation-verification-mcp-surface.md) — expose probe/verify verbs to agent clients via the `sp42-mcp` shell
- [PRD-0012 — Citation insertion for unsourced claims](prd/0012-citation-insertion.md) — ground a candidate source against a sentence's atomic claims, then propose a `<ref>` insert
- [PRD-0014 — Citation repair and insertion, browser surface](prd/0014-citation-repair-insertion-browser-surface.md) — per-finding action row in the Citations tab routing to text-edit or citation-fix, operator always chooses

## Architecture Decision Records

- [ADR-0007 — Verdict and anti-fabrication semantics](adr/0007-citation-verification-semantics.md)
- [ADR-0008 — Request/response contract and crate placement](adr/0008-citation-verification-contract.md)
- [ADR-0009 — Source-snapshot storage for reproducibility](adr/0009-citation-source-snapshot-storage.md)
- [ADR-0011 — Article-level citation verification (the review path)](adr/0011-article-citation-verification.md)
- [ADR-0015 — Rules-compliant read-only fetch edge](adr/0015-rules-compliant-read-only-fetch-edge.md)

The shared LLM posture these depend on (model panel, measured agreement, keyless
inference endpoint) is a platform decision,
[ADR-0006](../../platform/adr/0006-using-llms.md); the propose/confirm editing
pattern PRD-0008 builds on is
[ADR-0010](../../platform/adr/0010-operator-confirmed-content-proposals.md).
