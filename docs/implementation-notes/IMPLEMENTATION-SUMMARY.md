# Citation verification — overnight implementation summary

**Branch:** `impl/citation-verification` (worktree `../SP42-impl-citation`), off `origin/main`
@ `bf9a0a3` (the post-ADR-0004 `sp42-types` layout). **Local only — not pushed, no PR.**
**Date:** 2026-06-08. Built as an experiment to exercise PRD-0001 + ADR-0006/0007/0008/0009.

## What this is

A working, hermetically-tested Rust implementation of the citation-verification baseline,
ported faithfully from wikiharness/alex-citation-checker into `sp42-core` (+ a CLI command).
Every slice was TDD'd; the whole workspace is green (tests, clippy `-D warnings` all-features,
`cargo doc`, `cargo fmt --check`). ~115 new tests in `sp42-core::citation`; 0 workspace failures.

## The shape (all in `crates/sp42-core/src/citation/`)

Pure core (no I/O, the heart):
- `verdict` — two-axis `CitationVerdict{Judged(SupportLevel),SourceUnavailable}` (contract) +
  flat `Verdict` (algorithm currency); four canonical wire strings; **no numeric confidence**.
- `locate_quote` — the anti-fabrication locator (case-sensitive; NFC + whitespace-collapse +
  curly→straight only).
- `voting` — `n_class_vote`/`binary_vote` + measured `PanelAgreement` (counts, skeptical tiebreaker).
- `body_classifier` — the 7-detector GIGO gate (short-circuits to `SourceUnavailable`, no model call).
- `prompts` — verbatim two-step verifier prompt + "context only — do not quote" metadata block.
- `parsing` — verdict recovery (JSON-then-prose; no default-to-not-supported in the parser).
- `concurrency` — `map_with_concurrency` (bounded, input-order).
- `urls` — article URL + SSRF wiki-code guard + ETag→rev + Wayback `id_` rewrite + archive detect + resolve.
- `source_fetch` — Wayback recovery + `looks_like_html` + a first-cut `html_to_text` (see notes).
- `citoid` — the metadata sidecar (never grounded).
- `storage` — content-addressed snapshot + verdict-record envelopes over the `Storage` trait.

The spine (`verify`): the contract types, the per-model `build/execute/parse` edge over the
injected `HttpClient`, the **pure grounding gate** (`assemble_citation_finding` — votes, then
re-locates the winning quote in the fetched bytes; suppresses a `Supported`/`Partial` whose quote
does not locate), and the async `verify_citation_use_site` orchestration (fetch once → body gate →
bounded panel fan-out → assemble). GET/POST read-only — no write path.

CLI (`sp42-cli`): `--claim`/`--source-url` ad-hoc verification, `--format human|json|markdown`,
`--verdict-only`, `--with-metadata`; endpoint/panel/token from `SP42_INFERENCE_URL/MODELS/TOKEN`.
The API key is sent only to the inference host. (Run instructions in `PLAN.md`.)

## Anti-fabrication is enforced, with a property test

`assemble_citation_finding` re-grounds the winning quote against the fetched bytes; a fabricated
quote is suppressed to `not_supported`. Covered by a `proptest` (a quote drawn from a disjoint
alphabet can never be located → never surfaced as `Supported`), end-to-end suppression tests, and
the unreachable-source → `SourceUnavailable`-with-no-model-call path.

## What's NOT built (deferred, documented)

- **Article parse + between-markers claim extraction (S13)** — the whole-article / revision /
  by-index CLI selectors. Needs an HTML-parser dependency (a decision worth making with you).
  The ad-hoc path already exercises the full spine. See `ADR-CHANGE-NOTES.md` for the rule to
  implement (+ the two deliberate ADR-0007 deviations) and the dep choice.
- **Observable wiring (DoD 7):** `tracing` span + the `CitationFinding` field on
  `LiveOperatorView`. Additive.

## ADR changes surfaced (full detail in `ADR-CHANGE-NOTES.md`)

Minimal, as hoped. The only real one: **ADR-0008 §3's `build_citation_verify_request` signature
is incomplete** — the per-model prompt needs the fetched source body, which
`CitationVerificationRequest` doesn't carry; the implementation threads a prepared
`VerifyModelInputs`. The rest are notes (regex ReDoS-safety achieved via Rust's linear-time engine
not hand-bounds; `ModelRef.model` vs `.version` request-vs-record clarification; `regex` +
`unicode-normalization` deps for the pure algorithms; first-cut `html_to_text`). No verdict-value,
storage-format, or write-path decision changed.

## Reading order for review

1. This file → `PLAN.md` (sequencing, decisions, run instructions, DoD status) →
   `ADR-CHANGE-NOTES.md` (what the ADRs should change).
2. The code, in dependency order: `verdict` → `locate_quote` → `voting` → `body_classifier` →
   `prompts` → `parsing` → `urls`/`source_fetch`/`citoid` → `verify` (the spine) → `storage` →
   `sp42-cli`.
3. `git log` on this branch is one clean commit per slice.
4. `docs/implementation-notes/research/01..06` are the faithful per-area port specs the code follows.
