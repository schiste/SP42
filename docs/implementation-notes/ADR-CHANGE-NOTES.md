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

## 2. [edit] ADR-0008 §3 `build_citation_verify_request` signature is incomplete

- **ADR says:** ADR-0008 §3 — `build_citation_verify_request(config, model, req:
  &CitationVerificationRequest) -> HttpRequest`, where `CitationVerificationRequest` is
  `{ wiki_id, rev_id, title, claim, source_url }` (§1).
- **Reality:** The per-model request builds the **prompt**, which needs the *fetched source
  body* (and the optional Citoid metadata). `CitationVerificationRequest` carries only the
  claim + URL, not the fetched bytes — so that exact signature cannot construct the prompt.
- **What I did:** Kept `CitationVerificationRequest` as the operator-facing request (§1) and
  introduced a prepared `VerifyModelInputs { claim, source_text, source_url, metadata }` that
  the per-model edge takes. The async orchestration (`verify_citation_use_site`) fetches the
  source once, then threads `VerifyModelInputs` to each model call.
- **Suggested ADR edit:** ADR-0008 §3 should show the per-model `build_*`/`execute_*` taking the
  fetched source body (a prepared-inputs struct), distinct from the operator
  `CitationVerificationRequest`. Small but real; the contract types are otherwise unchanged.

## 3. [note] ReDoS-safety is achieved differently in Rust than the JS original

- **ADR says:** repeatedly requires the regexes be "ReDoS-safe" (ADR-0007 §4/§5, body gate).
- **Reality:** Rust's `regex` crate is a **linear-time, non-backtracking** engine, so ReDoS is
  prevented *structurally* by the engine — the JS originals' explicit bound guards (e.g. the
  verdict parser's `{0,50000}` fenced-block bound, the `{1,2000}` quoted-span bound) are
  unnecessary. Worse, a large *bounded* repetition like `[\s\S]{0,50000}?` makes Rust's engine
  **unroll** ~50k states and can exceed the compiled-program size limit (it did, in testing —
  a latent panic). I replaced it with unbounded lazy `[\s\S]*?` (safe in Rust). Smaller bounds
  (the body-classifier's `\s{0,20}`, `\d{1,9}`) were kept as faithful semantics, not as ReDoS
  guards.
- **Suggested ADR edit:** none required, but ADR-0007 could note that "ReDoS-safe" is satisfied
  by the `regex` engine's linear-time guarantee, not by hand-bounding quantifiers.

## 4. [gap] `ModelRef.model` vs `.version` — which is sent vs recorded

- **ADR says:** ADR-0006 Decision 8 — `ModelRef { provider, model, version }`, `version` = "the
  pinned model id used".
- **Reality:** An OpenAI-compatible request takes a single `model` string. The implementation
  sends `ModelRef.model` as the request id and treats `version` as additional recorded
  provenance (often equal to `model`). The model-vs-version split has no operational effect yet.
- **Suggested ADR edit:** ADR-0006 D8 could clarify which field is the request id and which is
  the recorded pin (and that they may coincide for gateways like OpenRouter).

## 5. [note] Suppressed support keeps the measured agreement

- **Context:** When the panel votes `Supported` but no winning-class quote locates in the
  fetched body, the gate suppresses the verdict to `not_supported` (ADR-0007 §5). The finding's
  `agreement` still reports the *measured* vote (e.g. 3/3 backed Supported). So an operator can
  see `not_supported` alongside high agreement — which is honest (the panel did agree; the gate
  overrode it for lack of evidence) but could read oddly. No ADR change; recorded as a
  display/UX consideration for when the finding surfaces on `LiveOperatorView`.

## 6. [note] First-cut `html_to_text` vs a real readability extractor

- **ADR says:** ADR-0009 / ADR-0011 (in wikiharness) extract main content via a readability
  library (Defuddle); the grounded bytes are the extracted text.
- **Reality:** To keep the dependency footprint minimal for this experiment, `html_to_text` is a
  hand-rolled first-cut (strip script/style/comments, separate block elements, strip tags,
  decode numeric + ~60 common named entities, collapse whitespace). It is **fail-closed**: any
  entity/boundary it misses can only cause a quote to *not* locate → suppression, never a false
  `Supported`. Production should adopt a real readability/main-content extractor behind an
  `HtmlExtractor`-style edge (the ADR-0011 analog) — noted as a follow-up, not a contract change.

## 7. [note] Voting/parsing operate on a flat `Verdict`; the contract type is two-axis

- The algorithms (vote tally, tiebreaker, parser) are cleanest over a flat four-value `Verdict`;
  the ADR-0007 contract type is the two-axis `CitationVerdict {Judged(SupportLevel),
  SourceUnavailable}`. Both serialize to the same four wire strings; lossless conversion between
  them. This is an implementation convenience fully consistent with ADR-0007 §1 (no ADR change).

## Deferred (tracked for the follow-on, not ADR changes)

- **S13 article parse + between-markers claim extraction** — needs an HTML-parser dep; implement
  the ADR-0007 §7 rule with its two deliberate deviations from wikiharness: bundled markers
  **SHARE** the preceding span (wikiharness drops the empty span), and **strip maintenance tags**
  (e.g. `[citation needed]`). `use_site_ordinal` = document-order index.
- **Observable wiring (DoD 7):** a `tracing` span on the verify path + the read-only
  `CitationFinding` field on `LiveOperatorView` (ADR-0008 §5). Additive.
- **Multi-turn/agentic + heterogeneous panel** — ADR-0006 §7 / ADR-0008 (f) defer the
  `ModelClient` trait; the `HttpClient` edge suffices here.
