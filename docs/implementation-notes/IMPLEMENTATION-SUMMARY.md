# Citation verification — implementation summary

**Branch:** `impl/citation-verification` (worktree `.worktrees/louie/citation-work`), off
`origin/main` @ `bf9a0a3` (the post-ADR-0004 `sp42-types` layout). **Pushed; no PR yet.**
**Date:** overnight run 2026-06-08; updated 2026-06-09 (SP42#25 layer work — see
*Since the overnight run*). Built as an experiment to exercise PRD-0001 + ADR-0006/0007/0008/0009.

## What this is

A working, hermetically-tested Rust implementation of the citation-verification baseline,
ported faithfully from wikiharness/alex-citation-checker into `sp42-core` (+ a CLI command).
Every slice was TDD'd; the whole workspace is green (tests, clippy `-D warnings` all-features,
`cargo doc`, `cargo fmt --check`). ~115 new tests in `sp42-core::citation`; 0 workspace failures.

## The shape (all in `crates/sp42-core/src/citation/`)

Pure core (no I/O, the heart):
- `verdict` — two-axis `CitationVerdict{Judged(SupportLevel),SourceUnavailable}` (contract) +
  flat `Verdict` (algorithm currency); four canonical wire strings; **no numeric confidence**.
- `locate_quote` — the anti-fabrication locator, folding transcription artifacts only: NFC,
  whitespace-collapse, curly→straight, case, dash unification, zero-width stripping (SP42#25
  layer 1); ellipsis-elided quotes match fragment-by-fragment in order within a bounded window
  (layer 2); plus `locate_quote_fuzzy`, the guarded last resort (layer 5: anchor-token windows,
  in-order token LCS ≥ 85% in integer math, digit tokens exact, ≥ 5 tokens — returns the
  SOURCE's own span).
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
provider-agnostic `ModelClient` boundary (ADR-0006 D7; source fetch stays on `HttpClient`), the
**pure grounding gate** (`assemble_citation_finding` — votes, then independently re-locates the
winning quote in the fetched bytes; the verdict is NEVER rewritten — the finding carries an
orthogonal `grounding_status ∈ {Located, LocatedFuzzy, Unlocated, NotApplicable}`, and
`is_groundable_support` — exact `Located` only — is the sole autonomous-action gate), the bounded
**repair turn** (layer 3: one extra call per non-locating support quote asking for the exact
shortest verbatim span or `NO_SPAN`; transcription only, never re-litigates the verdict;
audit-trailed on `ModelVote`), and the async `verify_citation_use_site` orchestration (fetch once
→ body gate → bounded panel fan-out → repair pass → assemble). GET/POST read-only — no write path.

CLI (`sp42-cli`): `--claim`/`--source-url` ad-hoc verification, `--format human|json|markdown`,
`--verdict-only`, `--with-metadata`, `--no-repair` (disable the repair turn); measurement
instruments `--debug-votes` (full outcome + per-model votes as JSON) and
`--locate-probe --quote <q>` (offline exact+fuzzy locate replay over STDIN source);
endpoint/panel/token from `SP42_INFERENCE_URL/MODELS/TOKEN`. The API key is sent only to the
inference host. (Run instructions in `PLAN.md`.)

## Anti-fabrication is enforced, with property tests

`assemble_citation_finding` re-grounds the winning quote against the fetched bytes; a fabricated
quote can never become **groundable** — `is_groundable_support` requires exact `Located`, and the
verdict is surfaced honestly with `grounding_status: Unlocated` instead of being rewritten
(the gate verifies the evidence EXISTS in the fetched bytes; it cannot verify the model USED it —
ADR-0007 §5 as amended). Covered by proptests: a disjoint-alphabet quote never locates, never
fuzzy-locates, never grounds via a repair span; plus the unreachable-source →
`SourceUnavailable`-with-no-model-call path.

## Since the overnight run (2026-06-09, SP42#25)

- **ModelClient boundary:** model calls moved from raw `HttpClient` to the provider-agnostic
  `ModelClient` (ADR-0006 D7) with the genai adapter in the CLI shell.
- **Locate layers 1+2:** case folding, dash/zero-width folding, multi-fragment ellipsis matching.
- **Verdict↔grounding decouple:** suppression replaced by the two-axis surface
  (`grounding_status` + `is_groundable_support`).
- **Layer 3 (repair turn)** and **layer 5 (guarded fuzzy, `LocatedFuzzy`)**, both gated by new
  fabrication proptests.
- **Measured** (alex 185-case benchmark, mistral+granite+gemma): GT accuracy 68.0% vs alex 66.1%;
  support-vote located rate 82.1% (exact) → 89.8% (repair) → 93.8% (fuzzy). Numbers are local-only
  until PRD-0007's committed-corpus benchmark exists.
- **Docs:** ADR-CHANGE-NOTES entries 8–9 (the §5 reconcile + the existence-vs-use limit);
  ADR-0007 §5 amended on `docs/citation-verification-adrs` ("what the gate establishes — and what
  it cannot"); PRD-0007 (LLM output-quality benchmarking) drafted, PR #37.

## What's NOT built (deferred, documented)

- **Article parse + between-markers claim extraction (S13)** — the whole-article / revision /
  by-index CLI selectors. Needs an HTML-parser dependency (a decision worth making with you).
  The ad-hoc path already exercises the full spine. See `ADR-CHANGE-NOTES.md` for the rule to
  implement (+ the two deliberate ADR-0007 deviations) and the dep choice.
- **Observable wiring (DoD 7):** `tracing` span + the `CitationFinding` field on
  `LiveOperatorView`. Additive.

## ADR changes surfaced (full detail in `ADR-CHANGE-NOTES.md`)

From the overnight run, minimal as hoped (the SP42#25 layer work later added entries 8–9 — the
§5 reconcile and the existence-vs-use limit — see above). The only real overnight one:
**ADR-0008 §3's `build_citation_verify_request` signature
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
