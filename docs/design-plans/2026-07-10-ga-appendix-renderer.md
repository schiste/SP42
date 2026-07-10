# GA evidence appendix renderer — implementation sketch

**Date:** 2026-07-10
**Status:** Sketch (pre-implementation)
**Governs the *how* for:** PRD-0016 (GA evidence appendix renderer). Workflow
context is `2026-07-09-ga-review-assist.md`.

## Codebase verified: 2026-07-10

The sketch is grounded in the tree as it stands; these are the facts that
shaped it (and fed corrections back into PRD-0016 — see the closing section):

- `PageVerificationReport` lives in `crates/sp42-citation/src/citation/page.rs`
  and already carries `wiki_id`, `title`, `rev_id`, `findings`, `skipped`,
  `extraction_failures`, and a `stats` arm with the full verdict tally and the
  unreachable/unusable split — the provenance footer and stats line need
  nothing new from the contract. (`PageVerificationRequest.rev_id: 0` is the
  documented latest-revision sentinel; the report records the resolved id.)
- `CitationFinding` is richer than PRD-0016's first draft assumed: alongside
  verdict/passage/provenance it carries `grounding_status` (a `Supported`
  verdict can be **`Unlocated`** — judged supported but the quote could not be
  re-located), `source_excerpt`, `unusable_reason`, Citoid `metadata`, and
  `archive_of`. The appendix has to render the grounding axis honestly.
- A shared flat renderer abstraction exists: `sp42-reporting::ReportDocument`
  (title / lead lines / sections of plain strings) with text and markdown
  renderers, used by `sp42-citation::citation_page_report` and the patrol
  reports. It carries no link/quote structure and its renderers own their own
  decoration — the GA appendix's wikitext layout, escaping, and wording
  invariants live in a *builder*, so forcing them through `ReportDocument`
  buys nothing. The appendix builder emits wikitext directly.
- The references domain crate **exists** (`crates/sp42-citation`) — the
  domain-index claim of "no crate yet" is stale. Workspace layout is flat
  (`crates/sp42-*`), not the `crates/domains/…` nesting `adding-a-domain.md`
  describes. The new crate is `crates/sp42-assessment`, flat, depending on
  `sp42-citation` for the report types (domain→domain dependency is allowed).
- The CLI's `OutputFormat` is `Text | Json | Markdown` (`sp42-cli/src/main.rs`),
  flattened into each rendering command; `verify-page` goes through the dev
  bridge (session + CSRF, ADR-0011 Decision 5). A render-from-saved-report
  mode therefore has a nice property the PRD hadn't noticed: it needs **no
  bridge, no session, no server at all** — it is a pure local transform of an
  ADR-0009-style snapshot.

## Architecture

```
sp42-citation (PageVerificationReport, CitationFinding)   [exists]
        ▲
sp42-assessment (new, domain)
  ga_appendix.rs — pure builder: report(s) → wikitext String
  copy.rs        — all GA-facing English strings in one module
        ▲
sp42-cli (shell)
  verify-page --format ga-appendix   (bridge path, convenience)
  render-report <file> --format …    (new, pure, no network/session)
```

- The builder is one pure function per PRD-0016: deterministic, no I/O, no
  inference. Wording invariants (no pass/fail, PRD-0014 mismatch framing,
  grounding-axis honesty) are properties of the builder's output, pinned by
  tests over fixtures — not conventions.
- `<nowiki>` escaping is a tiny local helper applied to every verbatim field
  (claims, quotes, excerpts, edit summaries later): quoted evidence is
  arbitrary text and must never transclude or break page markup.
- `StabilitySignal` (PRD-0015) does not exist in the tree yet. The builder's
  seam is an `Option`-shaped stability input added **when the type lands**; in
  the meantime criterion 5 renders in the "not assessed by SP42" line with the
  other silent criteria. The appendix ships citations-first.

## Phases

Each phase ends green (`cargo test --workspace`, `check-layering.sh`, fmt,
clippy).

1. **Crate + builder core.** `crates/sp42-assessment` (workspace member,
   lints inherited, layer-check clean); `ga_appendix.rs` building the
   criterion-2 section from a `PageVerificationReport`: stats summary line,
   actionable-first sublists (dead links with `archive_of`, unusable with
   `unusable_reason` framed as tool limitation, not-supported/partial with
   claim + `ref_id` + verdict + located quote + source link), skips and
   extraction failures as first-class lists, "not assessed" line, provenance
   footer. `copy.rs` holds every English string. Tests: structure, wording
   invariants (incl. `Unlocated` rendering as unconfirmed support), nowiki
   escaping over a malicious-quote fixture, determinism.
2. **CLI surface.** `OutputFormat::GaAppendix` accepted by `verify-page`; new
   `render-report` subcommand taking a saved report JSON (and later the
   stability snapshot) — pure, asserts no bridge/session/network. Legacy-argv
   rewriting untouched. Round-trip test: saved fixture → identical appendix
   bytes.
3. **Fixture realism + docs.** Render a real-article-shaped fixture (the
   ADR-0011 smoke-test articles are the model) and hand-check the paste on a
   sandbox talk page; adjust copy. Update the assessment README (crate born)
   and the stale references-domain "no crate yet" line while in there. PRD
   moves toward `Implemented` with test ids recorded.
4. **Stability section (staged; lands with PRD-0015's implementation).** Add
   the `StabilitySignal` input and criterion-5 section: facts before labeled
   interpretation, conduct posture (participants counted, usernames only in
   evidence links), triage/knob disclosure, oversized/declined honesty. Its
   DoD items in PRD-0016 gate on this phase, not on the MVP.

Out of scope (committed elsewhere): posting (sketch step 4), browser Citations
tab rendering, machine-readable arm, hold-loop diff rendering.

## What the sketch taught the PRD

Three corrections fed back into PRD-0016 in the same change:

1. **The grounding axis was missing from the wording invariants.** A
   `Supported` verdict with `grounding_status: Unlocated` must render as
   *unconfirmed* support — "the panel judged this supported but the quote
   could not be re-located" — never alongside grounded findings as if it had
   evidence. Hand-transcription loses exactly this nuance; the renderer must
   not.
2. **The stability DoD items were unstaged.** PRD-0015 is unimplemented; as
   written, PRD-0016's closing PR could never check them off. They now gate on
   the staged phase that lands with PRD-0015's implementation, and the
   citations-only appendix is the MVP acceptance gate.
3. **The saved-report mode is the core surface, not a convenience.** It
   requires no bridge session and no server, so it is the pure, replayable
   heart of the renderer (and the natural fixture-test entry point); the
   `--format` flag on `verify-page` is the convenience layered on top. Q1's
   proposed answer is now stated that way around.
