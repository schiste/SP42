# Citation Claim Context (SIDE-style context-passing) Design

## Summary

The citation verifier today accepts a claim and a source URL and judges them in isolation: it
fetches the source, runs a panel of model calls, votes on whether the source supports the claim,
and verifies that the winning quote is physically present in the fetched bytes. This works well
for self-contained claims but breaks down for sentences that use pronouns, elliptical references,
or terms that only resolve when you know the article section they came from — roughly a third of
real sentences fall into this category according to the SIDE research.

This design adds a `ClaimContext` value — article title, optional section title, and preceding
sentences — that can be supplied alongside the claim to help the model interpret what the claim
means. When present, the context is rendered into the verification prompt as a clearly-labeled,
context-only block that the model may read for interpretation but may never quote as support; the
structural guarantee is that the grounding gate only searches for quotes in the fetched source
body, so a model that cites a preceding sentence instead of the source produces an `Unlocated`
finding, not a false positive. When context is absent or empty, the rendered prompt is
byte-identical to today's, preserving a clean A/B control arm. No verdict shapes, output types, or
downstream consumers change.

## Definition of Done

The citation verifier can be given a co-reference **context window** for a claim —
the article title, the section title, and the preceding sentences — and renders it
into the verification prompt as clearly-labeled, context-only material that the model
may use to interpret the claim but may never quote as support. The claim text and the
verification path (panel, skeptical vote, repair turn, grounding gate, `CitationFinding`
output) are otherwise unchanged.

Done when:

1. A `ClaimContext` value can be supplied to `verify_citation_use_site`; supplying
   `None`/empty produces a prompt **byte-identical** to today's (the A/B control arm),
   pinned by a characterization test.
2. A populated `ClaimContext` renders a context-only block, labeled so the model is told
   its supporting quote must still come verbatim from the SOURCE, ordered before the
   source body — following the existing bibliographic-metadata block pattern.
3. The grounding gate is unaffected: a quote that appears only in the context window (not
   in the fetched source body) does **not** ground — it surfaces `Unlocated`, never as a
   located passage. Proven by test.
4. The CLI can supply context (section title, preceding sentences) via optional flags,
   defaulting to empty (control); existing output types and `render_verify_text` are
   unchanged.
5. All tests pass, clippy clean (`-D warnings`), no live network in tests (replayed
   fixtures / in-memory clients).

Out of scope (deferred to a follow-up "learn from Molecular Facts" issue): rewriting the
claim to be self-contained (decontextualization), decomposing a compound claim into atoms,
per-atom grounding, and deterministic roll-up. This design passes context *alongside* the
claim; it does not rewrite or split the claim.

## Glossary

- **SIDE (Petroni et al. 2023)**: A research paper on verifying Wikipedia citations that
  introduced the practice of passing a claim's surrounding article context — title, section,
  preceding sentences — to a verifier model rather than rewriting the claim. SP42 adopts this
  "pass-alongside" strategy.
- **co-reference context window**: The block of interpreting material (article title, section
  title, preceding sentences) supplied with a claim so the model can resolve pronouns and
  elliptical references. Distinct from the source text being verified against.
- **grounding gate**: The post-vote step that re-locates the model's winning quote as a literal
  substring in the fetched source bytes. A quote that cannot be found there produces an
  `Unlocated` finding. This is what makes context-only blocks structurally safe.
- **panel**: The set of parallel model calls that each independently judge whether the source
  supports the claim; their verdicts are then aggregated by vote.
- **characterization test**: A test that pins the current behavior of a system as a baseline —
  here, proving that empty/absent context produces a prompt byte-identical to today's output,
  establishing the A/B control arm.
- **`CitationFinding` / `VerificationOutcome`**: The output types returned by the verifier; they
  carry the verdict, agreement level, located quote, and grounding status. This design leaves
  their shapes unchanged.
- **`CitationVerificationRequest`**: The existing record type holding the claim text, source URL,
  and revision. This design deliberately keeps context off this type, passing it as a separate
  argument instead.
- **bibliographic metadata block**: An existing context-only section in the verification prompt,
  rendered from Citoid metadata, that tells the model contextual facts about the source but
  explicitly forbids quoting from it. The new context block follows the same pattern.
- **Citoid**: MediaWiki's metadata-extraction service; SP42 already fetches bibliographic
  metadata (author, date, publisher) from it and renders it as a context-only block in the prompt.
- **ADR**: Architecture Decision Record — short numbered documents in the repo capturing design
  decisions. Several are referenced (ADR-0007, ADR-0008) as precedents for this design.
- **`Unlocated`**: A grounding result indicating the model's quoted text could not be found in the
  fetched source body. The expected outcome when a model mistakenly quotes the context window
  instead of the source.
- **decontextualization / atomization**: Out-of-scope techniques (referenced as the "Molecular
  Facts" follow-up) that would rewrite a claim to be self-contained or split a compound claim into
  independently-verifiable atoms. This design explicitly defers both.

## Architecture

The verifier today judges a whole claim against a fetched source: `verify_citation_use_site`
(in `crates/sp42-core/src/citation/verify.rs`) fetches the source once, runs the deterministic
body-usability gate, fans a model panel over the claim, votes, and re-locates the winning quote
in the fetched bytes (the grounding gate). The prompt is built by `build_verify_prompt` (in
`crates/sp42-core/src/citation/prompts.rs`), which already renders an optional **bibliographic
metadata** block as *context-only* text — explicitly told never to be the source of a supporting
quote, and structurally safe because the grounding gate only ever locates quotes in the source
body.

This design adds a second context-only input: the SIDE co-reference **context window**. SIDE
(Petroni et al. 2023) supplies a claim's interpreting context — article title, section title,
preceding sentences — to its models rather than rewriting the claim; roughly one-third of
sentences need such context to be interpretable. We follow the pass-alongside approach, not the
rewrite approach, because the rewrite is the failure-prone step and its benefit (for our strong
panel verifier) is unproven.

**Components:**

- `ClaimContext` (new, `verify.rs`) — the context window. Carries the new contextual material
  only; the claim stays single-source as `CitationVerificationRequest.claim`.

  ```rust
  pub struct ClaimContext {
      pub article_title: String,
      pub section_title: Option<String>,
      pub preceding_sentences: Vec<String>,
  }
  ```

- `build_verify_prompt` (extended, `prompts.rs`) — gains a `context: Option<&ClaimContext>`
  parameter and renders a context-only block (a new `context_section` helper, sibling to the
  existing `metadata_section`). When `context` is `None` or every field is empty, the rendered
  prompt is byte-identical to today's.

- `VerifyModelInputs` (extended, `verify.rs`) — the per-model prompt input gains
  `context: Option<&'a ClaimContext>`, carried into `build_verify_completion_request` →
  `build_verify_prompt`.

- `verify_citation_use_site` (extended, `verify.rs`) — gains a `context: Option<&ClaimContext>`
  argument, threaded into `VerifyModelInputs`. Fetch, usability gate, panel, vote, repair, and
  grounding are otherwise untouched.

- CLI (`crates/sp42-cli/src/main.rs`) — `run_verify` assembles an optional `ClaimContext` from
  new flags and passes it through. Output rendering is unchanged.

**Data flow:** unchanged except that the context window, when present, is rendered into each
panel member's prompt as labeled context-only text. The verdict, agreement, grounding, and
`CitationFinding`/`VerificationOutcome` shapes are identical to today.

**Safety property:** context can never become groundable. The grounding gate
(`assemble_citation_finding`) re-locates the winning quote only in the fetched source body, so
a model that quotes the article's own preceding sentence produces an `Unlocated` finding rather
than a false located passage. The prompt also instructs the model that quotes must come from the
source. The first guarantee is structural; the second is belt-and-suspenders.

## Existing Patterns

This design follows the **context-only block** pattern already established for bibliographic
metadata in `prompts.rs`:

- `metadata_section(&CitoidMetadata) -> String` renders labeled, context-only text ("DO NOT
  quote from here; your supporting quote MUST come verbatim from the SOURCE text below") and
  returns an empty string when no field is present, keeping the no-metadata prompt byte-identical
  to the bare form. `context_section` mirrors this exactly, including the empty-input invariant.

- The grounding gate's "only the source body is groundable" invariant (ADR-0007 Alt (e),
  refined by SP42#25 layer 6 in `verify.rs`) is what makes a second context-only block safe with
  no new mechanism.

This design also follows the existing decision to keep `CitationVerificationRequest` a clean
claim+url+revision record (ADR-0008 §1): the new context rides a **separate** `ClaimContext`
argument rather than bloating the request type with fields only the prompt builder uses.

CLI flags follow the existing flag-mode driver pattern in `sp42-cli` (e.g. the bare-url
preview/execute modes): thin, unit-tested, no network in tests.

No divergence from existing patterns. No new architectural mechanism is introduced.

## Implementation Phases

### Phase 1: ClaimContext type + context-only prompt rendering
**Goal:** The verifier prompt can render a co-reference context window as context-only text,
with empty/absent context preserving today's exact prompt.

**Components:**
- `ClaimContext` type in `crates/sp42-core/src/citation/verify.rs` (or a small sibling module,
  re-exported), exported from `lib.rs`.
- `context_section(&ClaimContext) -> String` and an extended
  `build_verify_prompt(claim, source_text, source_url, metadata, context)` in
  `crates/sp42-core/src/citation/prompts.rs`.

**Dependencies:** None.

**Done when:** Unit tests prove (a) `None`/all-empty context ⇒ prompt byte-identical to today;
(b) a populated context renders a labeled context-only block stating quotes must come from the
source; (c) the context block is ordered before the SOURCE block; (d) when both metadata and
context are present, both render. All tests pass, clippy clean.

### Phase 2: Thread context through the edge and orchestration
**Goal:** A `ClaimContext` supplied to `verify_citation_use_site` reaches every panel member's
prompt, with grounding and verdict behavior unchanged.

**Components:**
- `VerifyModelInputs.context` field and `build_verify_completion_request` wiring in `verify.rs`.
- `context` parameter on `execute_citation_verify` and `verify_citation_use_site` in `verify.rs`.

**Dependencies:** Phase 1.

**Done when:** Tests prove (a) context supplied to `verify_citation_use_site` appears in the
model request prompt (in-memory `ModelClient`); (b) empty/`None` context yields a finding
identical to today's path (characterization test); (c) a quote present only in the context
window (not the source body) does not ground — the finding is `Unlocated`/no located passage.
All tests pass, clippy clean.

### Phase 3: CLI context flags
**Goal:** An operator can pass a context window to the verifier from the CLI; default empty
preserves the control arm.

**Components:**
- Optional flags in `crates/sp42-cli/src/main.rs` (`run_verify`) to supply section title and
  preceding sentences (repeatable), assembled into an optional `ClaimContext`. Article title
  reuses the existing title input.

**Dependencies:** Phase 2.

**Done when:** Flag parsing / context assembly is unit-tested (no network); omitting the flags
produces the control behavior; existing output types and `render_verify_text` are unchanged.
All tests pass, clippy clean.

## Additional Considerations

**A/B measurement:** The control (empty context) and treatment (populated context) arms differ
only by the context block, so the eval corpus (outcome-level verdicts, separate session) can
measure whether context-passing moves verdict quality without confounds. Keeping the empty-context
prompt byte-identical to today is what makes the comparison clean.

**Error handling:** Context is best-effort interpreting material, never required. Missing section
title or empty preceding sentences degrade silently to a smaller (or absent) context block — never
an error, and never fewer guarantees than today's no-context path.

**Future extensibility:** This design deliberately stops at context-passing. Rewriting the claim
to be self-contained and decomposing it into independently-grounded atoms (with a dedicated
`AtomizedFinding` type, conjunctive roll-up, and an `atom_count > 3` compound cohort tag) is a
worked-out but deferred follow-up, gated behind corpus evidence that the higher-risk rewrite earns
its place. Tracked in the "learn from Molecular Facts" issue.
