# Citation verification — implementation plan (experimental, exercising PRD-0001 + ADR-0006/0007/0008/0009)

**Branch:** `impl/citation-verification` (off `origin/main` @ bf9a0a3, which has the `sp42-types` extraction).
**Status:** in progress (overnight first attempt, 2026-06-08).
**Discipline:** TDD (red→green→refactor), keep `cargo test -p sp42-core` + `cargo clippy --workspace --all-targets --all-features -- -D warnings` green at every commit. **No push, no PR** — local for Luis's review.

This plan is the *sequencing + decisions*. The faithful per-algorithm spec is in
`docs/implementation-notes/research/01..06`. ADR-change notes accumulate in
`docs/implementation-notes/ADR-CHANGE-NOTES.md`.

## Architecture (layering)

Pure `sp42-core::citation` (no I/O — the heart, hermetically testable):
verdict types · `locate_quote` (anti-fabrication gate) · voting/agreement ·
body-usability (GIGO) gate · verify prompt · verdict parser · URL helpers ·
wayback/citoid pure fns · `html_to_text` (first-cut) · the grounding/assemble gate ·
snapshot/verdict serde.

I/O at the edges (async, generic over the injected `HttpClient`/`Storage`/`Clock` traits):
source fetch · per-model verify call · bounded panel fan-out · snapshot/verdict persistence.

Shell (`sp42-cli`): a read-only `--verify` mode, 4 output formats. The real model API
key lives in the shell `HttpClient` adapter, never in core (ADR-0008 Decision 7).

## Dependency decisions

Added to `sp42-core` (all allow-listed in `deny.toml`; first two already in `Cargo.lock`):
- `regex` 1.12 — body-classifier / parser / url helpers. Rust `regex` is linear-time → satisfies the ADRs' ReDoS-safety requirement structurally.
- `futures` 0.3 — promote from dev-dep to runtime for bounded panel concurrency (`buffer_unordered`).
- `unicode-normalization` 0.1 — NEW (pulls only `tinyvec`). NFC step of `locate_quote` (ADR-0007 §5). See ADR-CHANGE-NOTES.

No model SDK / no new LLM dep — the model endpoint is reached over the existing `HttpClient`
edge (ADR-0006 §4, ADR-0008 Consequences hold).

## Key local decisions (recorded for review)

1. **Two verdict enums.** Internal flat `Verdict {Supported,Partial,NotSupported,SourceUnavailable}`
   is the algorithm currency (voting, parsing, tiebreaker). Public contract type is the two-axis
   `CitationVerdict {Judged(SupportLevel), SourceUnavailable}` (ADR-0007 §1). Both serialize to the
   same four snake_case wire strings; lossless conversion between them.
2. **`locate_quote` offset** = byte offset into the *original* source where the normalized match
   begins. The load-bearing output is found/not-found; the offset is informational (SP42's article
   anchor is `use_site_ordinal`, not this offset — research note 05 §1).
3. **`PanelAgreement {panel_size:u8, winner_votes:u8}`** carries measured counts only; the fraction
   is derived at display, never stored (ADR-0006/0008). No float anywhere on the verdict/finding →
   `CitationFinding` derives `Eq`.
4. **First-cut `html_to_text`** is a documented, dependency-light extractor (strip script/style,
   strip tags, decode entities incl. numeric, collapse whitespace). A real readability extractor
   (the wikiharness `HtmlExtractor`/Defuddle analog, ADR-0011) is a noted follow-up.
5. **Grounding gate is pure + independent** (`assemble_citation_finding`): votes the panel, then
   re-locates the winner's quote in the fetched source bytes; a `Supported`/`Partial` whose quote
   does not locate is suppressed to `NotSupported` pre-surface. The model is never trusted on its word.
6. **CLI** follows the house style: hand-rolled flag parser, `--verify` mode, `futures::block_on`
   at the boundary, `OutputFormat::{Text,Json,Markdown,Verdict}`. No clap/tokio.

## Slices (TDD, commit per slice)

- [ ] S0  deps + `pub mod citation;` skeleton + `CitationVerifyError`/`CitationStorageError` in errors.rs (green build)
- [ ] S1  `citation/verdict.rs` — Verdict + CitationVerdict/SupportLevel + wire serde + CitationFindingKind
- [ ] S2  `citation/locate_quote.rs` — anti-fabrication locator (NFC + quote-sub + ws-collapse, case-sensitive)
- [ ] S3  `citation/voting.rs` — n_class_vote/binary_vote + PanelAgreement (skeptical tiebreaker)
- [ ] S4  `citation/body_classifier.rs` — 7 detectors in order (GIGO gate)
- [ ] S5  `citation/prompts.rs` — build_verify_prompt (verbatim SYSTEM/USER) + metadata context-only section
- [ ] S6  `citation/parsing.rs` — canonicalize_verdict + parse_verdict_response (JSON-then-prose)
- [ ] S7  `citation/concurrency.rs` — map_with_concurrency (bounded, input-order)
- [ ] S8  `citation/urls.rs` — article-html url + wiki-code SSRF guard + etag + wayback rewrite + archive detect + resolve
- [ ] S9  `citation/source_fetch.rs` — recover_wayback_body + looks_like_html + html_to_text
- [ ] S10 `citation/citoid.rs` — build/parse citoid + build_citoid_header + format_authors
- [ ] S11 `citation/verify.rs` — request/finding/grounding/modelref types + build/execute/parse per-model edge + assemble gate (pure) + verify_citation_use_site (async)
- [ ] S12 `citation/storage.rs` — snapshot + verdict envelopes (sha256 content-addressed) over the Storage trait
- [ ] S13 `citation/article.rs` — parse_article + between-markers claim extraction (SHARE bundled, strip maintenance tags) — IF a clean HTML-parser dep works out, else deferred with a note
- [ ] S14 `sp42-cli` — `--verify` mode (ad-hoc claim+source-url end-to-end first; then article/rev), 4 formats
- [ ] S15 full local gate (`./scripts/ci-all.sh`) + final notes

DoD mapping (PRD-0001): S1/S6 → item 1; S3 → item 2; S2/S11 → item 3 (load-bearing); S4/S11 → item 4;
S11/S14 → item 5 (no writes); S12 → item 6 (replay); S12/S14 → item 7 (observable); S14 → item 8 (CLI).

## Outcome (overnight run, 2026-06-08)

**Done + committed (TDD, each slice clippy-clean + tests-green):** S0–S12 and S14, S15.
Full host gate green: `cargo test --workspace`, `cargo clippy --workspace --all-targets
--all-features -- -D warnings`, `cargo doc --workspace --no-deps`, `cargo fmt --all --check`
all pass. (~115 new citation tests across the modules; whole workspace 0 failures.) The
wasm/trunk/tauri steps of `ci-all` target the untouched `sp42-app`/`sp42-desktop` and were
not run here.

**Deferred — S13 (article parse + between-markers claim extraction):** NOT built. It needs a
real HTML parser dependency (querySelector / getElementById / ancestor-walk over Parsoid REST
HTML) which is a heavier dep decision best made with Luis; and the ad-hoc (claim + source-URL)
CLI path already exercises the entire verification spine end-to-end (the "smoke-test the
verifier in isolation" mode PRD-0001 calls out). Building S13 unlocks the whole-article /
revision / by-index CLI selectors (the rest of DoD item 8). See ADR-CHANGE-NOTES for the
between-markers rule + the two ADR-0007 deviations (SHARE bundled markers, strip maintenance
tags) to implement, and the dep decision.

**DoD status:** items 1, 2, 3 (load-bearing anti-fabrication), 4, 5, 6 — met (unit/property/
integration tests). Item 7 (observable) — the finding + provenance + grounding are the durable
record (storage); a `tracing` span + `LiveOperatorView` wiring is a small additive follow-up.
Item 8 (CLI) — met for ad-hoc mode; article/revision/index selectors gated on S13.

**How to run the CLI** (ad-hoc verification against a real open-model panel):
```
SP42_INFERENCE_URL=https://openrouter.ai/api/v1/chat/completions \
SP42_INFERENCE_MODELS=meta-llama/llama-3.3-70b-instruct,qwen/qwen-2.5-72b-instruct \
SP42_INFERENCE_TOKEN=<key> \
cargo run -p sp42-cli -- --claim "<claim text>" --source-url "<url>" --format human
```
`--format json|markdown`, `--verdict-only`, and `--with-metadata` (Citoid sidecar) are also
supported. The bearer token is sent only to the inference host, never to source sites.
