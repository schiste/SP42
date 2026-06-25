# References / citation verification

An incoming SP42 domain: LLM-assisted checking of whether a cited source actually
supports the claim it backs. The verdict is **informational** — SP42 reports
support, contradiction, or insufficiency (with a locatable supporting passage when
it claims support) and never edits autonomously.

There is no dedicated crate yet; the domain is specified by the documents below and
builds on the platform LLM interface ([ADR-0006](../../platform/adr/0006-using-llms.md)).

## Product Requirements

- [PRD-0001 — Citation verification](prd/0001-citation-verification.md) — operator-facing capability, CLI-first surface, panel voting and agreement signal

## Architecture Decision Records

- [ADR-0007 — Verdict and anti-fabrication semantics](adr/0007-citation-verification-semantics.md)
- [ADR-0008 — Request/response contract and crate placement](adr/0008-citation-verification-contract.md)
- [ADR-0009 — Source-snapshot storage for reproducibility](adr/0009-citation-source-snapshot-storage.md)

The shared LLM posture they depend on (model panel, measured agreement, keyless
inference endpoint) is a platform decision:
[ADR-0006](../../platform/adr/0006-using-llms.md).
