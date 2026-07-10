# Assessment

A SP42 domain: assisting per-article quality assessments — first target, English
Wikipedia's [Good-article review](https://en.wikipedia.org/wiki/Wikipedia:Good_article_instructions).
SP42 assembles grounded evidence against the assessable criteria (citation
support via the references domain, stability, media licensing, structural lint)
and renders it in the shape a reviewer uses; the criterion judgments and the
pass/fail stay human. Evidence, not verdicts — the same informational posture as
the references domain (ADR-0007), with every write ADR-0010 propose/confirm.

No crate yet. The end-to-end workflow and build sequencing live in the design
sketch
[2026-07-09-ga-review-assist.md](../../design-plans/2026-07-09-ga-review-assist.md);
citation-evidence mechanics are owned by the references domain (ADR-0011,
PRD-0009), not re-specified here.

## Product Requirements

- [PRD-0015 — Article stability signal](prd/0015-article-stability-signal.md) —
  two-layer stability evidence (deterministic sensor/gate + panel interpretation
  of the ambiguous middle), shaped for GA criterion 5
- [PRD-0016 — GA evidence appendix renderer](prd/0016-ga-evidence-appendix-renderer.md) —
  pure renderer from the existing report contracts to the pasteable
  `Talk:Article/GAn` wikitext evidence appendix; the design sketch's first
  build step and the domain's first crate

## Architecture Decision Records

None yet. If accepted, PRD-0015 expects a thin platform ADR pinning its
mechanism/contract placement (history fetch edge, revert-chain reducer, and
talk-activity sensor in platform; `StabilitySignal` in `sp42-core`; policy in
this domain).
