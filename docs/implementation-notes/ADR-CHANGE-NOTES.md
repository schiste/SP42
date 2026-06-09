# ADR-change notes from implementing PRD-0001 + ADR-0006/0007/0008/0009

Living notes captured while implementing the citation-verification baseline as an experiment to
exercise the ADRs (branch `impl/citation-verification`, 2026-06-08). Each entry: what the ADR says,
what implementation reality surfaced, and the suggested ADR edit (if any). Hopefully minimal.

Legend: **[note]** = worth recording, no change needed · **[edit]** = suggests an ADR wording change ·
**[gap]** = ADR is silent on something implementation had to decide.

---

## 1. [edit] Dependencies the pure algorithms require (regex + unicode-normalization)

- **ADR says:** ADR-0008 Consequences — "adds **no new runtime dependency**" (in context: the *model
  edge* needs no vendor SDK, reached over the existing reqwest `HttpClient`). ADR-0007 Cross-cutting —
  "No LLM dependency enters the … graph without a `cargo-deny` clearance."
- **Reality:** The *model edge* claim holds (no SDK added). But the pure algorithms need two crates
  not in the workspace's direct deps:
  - `regex` (already transitive in `Cargo.lock` @ 1.12.3) — body-classifier, verdict parser, URL
    helpers. Rust's `regex` is linear-time/no-backtracking, which is exactly how the ADRs' repeated
    "ReDoS-safe" requirement is *satisfied structurally* (better than hand-rolled scanning).
  - `unicode-normalization` (NEW, pulls only `tinyvec`) — the NFC step ADR-0007 §5 names in the
    locator's "conservative normalization (Unicode NFC, whitespace collapse, curly→straight quotes)".
  - `futures` (already a dev-dep; promoted to runtime) — bounded panel concurrency.
  All are on `deny.toml`'s allow-list (MIT/Apache/Zlib); transitive count added is ~2, far under the
  Art. 7.2 >50 threshold.
- **Suggested ADR edit:** ADR-0008's "no new runtime dependency" sentence should be scoped explicitly
  to the *model edge* (it already is, in spirit), and a one-line Consequence added that the verdict
  *mechanics* pull `regex` + `unicode-normalization` (allow-listed, ReDoS-safety via `regex`'s
  linear-time engine). Minor.

---

## (further entries appended as implementation proceeds)
