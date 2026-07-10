# PRD-0016: GA evidence appendix renderer

**Drafter:** Claude Code
**Editor:** Luis Villa
**Date:** 2026-07-10
**State:** Draft
**Discussion:** design conversation 2026-07-10; PR link pending.
**Spawned ADRs:** none expected — the renderer is pure composition over
existing report contracts (`PageVerificationReport`, ADR-0011; `StabilitySignal`,
PRD-0015), and the new `sp42-assessment` crate follows ADR-0013's layered
architecture via the `adding-a-domain.md` turnkey path exactly (domain policy
over existing contracts; no new platform mechanism, no new contract), so the
crate itself triggers no ADR. Later lanes carry their own artifacts: posting
the appendix is the ADR-0010 + insertion-extension + ADR-0018 work named in
the design sketch, and a machine-readable task-graph arm is roadmap there too.

## Changelog

- 2026-07-10: Drafted as the design sketch's first build step. The
  implementation sketch (`2026-07-10-ga-appendix-renderer.md`) was written the
  same day and fed back three corrections: the grounding axis joined the
  wording invariants (unlocated support renders as unconfirmed), the stability
  DoD items were staged behind PRD-0015's implementation, and the saved-report
  render was recognized as the core surface. All three open questions resolved
  same-day with the Editor, including reversing the counts-only proposal for
  supported findings in favor of a compact spot-check record.
- 2026-07-10 (review): primary persona set to the **on-ramp reader** — the GA
  reviewer/editor with zero SP42 context reading a pasted appendix cold — per
  the Editor. Consequences folded in: cold-reader legibility joins the wording
  invariants (no raw contract identifiers; copy-module vocabulary), the footer
  gains a what-is-this explainer link, and the copy-drift risk is reweighted
  as SP42's first impression.

## Scope boundary

This PRD owns **one renderer**: a pure function from SP42's existing article
evidence reports to a **GA-review-shaped wikitext evidence appendix** — the
artifact a Good-article reviewer pastes into `Talk:Article/GAn`. It is the
first build step of the GA review-assist design sketch
(`docs/design-plans/2026-07-09-ga-review-assist.md`) and deliberately its
smallest: no new verification, no new fetches, no writes, no inference.

Inputs are the contracts as they exist today:

- **`PageVerificationReport`** (ADR-0011) — required. Findings, skips,
  extraction failures, stats, archive fallbacks; book-citation outcomes
  (PRD-0009) appear here as they land in that contract.
- **`StabilitySignal`** (PRD-0015) — optional. Rendered when supplied; its
  absence is noted, never silently smoothed over.

It deliberately excludes:

- **Posting.** The appendix is pasteable output; landing it on the review
  subpage under the operator's account is the design sketch's step 4, blocked
  on the ADR-0003 insertion extension and ADR-0018, and not re-specified here.
- **Any new judgment.** The renderer adds wording, ordering, and layout — it
  must not add, drop, or soften a single finding. **Every disclosure the input
  reports carry renders** — skips, extraction failures, declined Layer B,
  whatever honesty arms the contracts grow — or the renderer is wrong.
- **A machine-readable arm.** The task-graph-shaped output for external agents
  (design sketch, external-ecosystem notes) is roadmap, not this PRD.
- **Hold-loop diffing.** Run-to-run comparability is designed in now (stable
  ordering and addressing), but rendering a *diff between runs* rides the
  single-use-site re-verify fast-follow (ADR-0011) and PRD-0014's re-verify
  route, later.
- **Non-GA processes.** DYK/FA have different checklists; the criterion copy
  is GA's. Nothing here may hard-code GA specifics into the shared report
  contracts — the GA shape lives entirely in this renderer.

## Problem

A reviewer who runs `verify-page` (and, once it lands, the stability signal)
holds exactly the evidence a GA spot-check needs — but in CLI/JSON shape. To
use it they hand-transcribe findings onto the review page, which is
transcription drudgery, loses the grounding that makes the findings defensible
(verbatim quotes, `ref_id` addressing), and — worst — invites ad-hoc paraphrase
that reads as verdicts: "citation failed" where the honest statement is "the
claim and the source disagree" (PRD-0014's framing), or "criterion 2: pass"
where SP42 makes no such judgment.

The primary user is the **GA reviewer with no SP42 context** — the on-ramp.
That is who reads a pasted appendix cold on a review page (reviewer, nominator,
talk-page watcher), and it is who a first-time operator is the day they try
SP42 because they saw one. The operator running the render is further along
the ramp, but the *default reader assumption* is zero tool familiarity: the
appendix must explain itself — its vocabulary, its provenance, and what it is
not (a verdict) — without the CLI, the docs, or prior exposure. The wording
discipline is built in precisely because the audience cannot be assumed to
know the posture.

## Proposal

One renderer, `reports → wikitext appendix`, organized by GA criterion:

- **Criterion 2 section.** A stats summary line (from the report's `stats` arm,
  no re-aggregation), then actionable sublists ordered most-actionable-first:
  dead links (with the `archive_of` repair handles), unreadable sources
  (`unusable` split, honestly framed as a tool limitation), and
  `NotSupported`/`Partial` findings — each line carrying the claim, its
  `ref_id`, the verdict, the verbatim located quote, and the source link.
  Supported findings render as a **compact one-line-each list** — `ref_id`,
  claim prefix, grounding marker — documenting the positive half of the
  spot-check (the reviewing guide expects the reviewer to say what they
  checked; "12 supported" without naming which refs is not a spot-check
  record) while keeping quote bulk out of the appendix; their full detail
  stays in the CLI/structured rendering. Unconfirmed supports (unlocated
  grounding) form their own visible sublist, never blended into the supported
  list. Skips and extraction failures render as their own first-class lists.
  Book citations render with their resolve/ground outcomes (deep links to
  scanned pages when grounded).
- **Criterion 5 section** (when a `StabilitySignal` is supplied): Layer A facts
  first (timeline, phase markers, marker inventory, triage outcome and knob
  disclosure), then the labeled Layer B characterizations with their cited
  evidence. The PRD-0015 conduct posture binds the rendering: participants
  counted, never named outside evidence/diff links.
- **A "not assessed by SP42" line** naming the criteria the appendix says
  nothing about (1a, 2c, 3, 4, 6 — and 5 until the stability section lands),
  so silence cannot be read as endorsement. One line; the appendix's own
  honesty arm.
- **Provenance footer**, always: article and `rev_id`, run date, SP42 version,
  an explicit framing line — this is a tool-generated evidence appendix; the
  criteria judgments and the pass/hold/fail are the reviewer's — and a
  **"what is this?" link** to a stable explainer of the tool and its verdict
  vocabulary, because the default reader has never heard of SP42.

Wording invariants, enforced as contract rather than style:

- **No criterion pass/fail wording, anywhere.**
- **PRD-0014's mismatch framing verbatim**: a `NotSupported`/`Partial` finding
  is rendered as claim-and-source disagreement, never as a citation failure —
  the article text may be the wrong side.
- **The grounding axis renders honestly.** A `Supported`/`Partial` verdict
  whose `grounding_status` is unlocated renders as *unconfirmed* support —
  the panel's judgment without a re-locatable quote — visually and verbally
  distinct from grounded findings. This is precisely the nuance
  hand-transcription loses.
- **Cold-reader legibility.** No raw contract identifiers in the output —
  verdict and status vocabulary renders through the reader-facing copy module
  ("the source did not support this claim", never `NotSupported`), and refs
  are addressed by their human-facing citation markers (`[1]`, named refs),
  which is what `ref_id` already carries (ADR-0011).
- **Evidence phrasing throughout** ("12 of 14 URL citations verified
  supported; 2 dead links: …").

Mechanics:

- **Plain wikitext only** — headings, lists, links. No template dependencies,
  so the output is portable and cannot break when a wiki lacks a template.
  Verbatim quoted evidence is `<nowiki>`-escaped: a grounded quote is
  arbitrary text, and pasting it must never transclude a template or break the
  page's markup.
- **Stable ordering and addressing.** Findings render in report order, keyed
  by `ref_id`/`use_site_ordinal`, so two runs over the same article produce
  line-comparable appendices (the hold-loop comparability the design sketch
  commits to).
- **Placement per the layering rule**: the renderer is assessment-domain
  *policy* (GA-shaped wording is process-specific) over `sp42-core` contracts —
  the first code in an `sp42-assessment` domain crate, wired per
  `adding-a-domain.md`. The criterion copy lives in one module for later
  localization; MVP copy is English/enwiki-GA.
- **Surface**: CLI-first — a `ga-appendix` output format on the page-verify
  path, and equally a render of a *saved* report (the replay-friendly core: a
  stored `PageVerificationReport` + optional `StabilitySignal` render with no
  network and no inference).

## Definition of Done

*Names planned coverage; the closing PR records exact test ids. All renderer
tests run over fixture reports — no live network, no inference (the renderer
is pure).*

- [ ] A fixture `PageVerificationReport` renders to an appendix with the
      criterion-2 structure above: stats line, actionable-first sublists, each
      finding line carrying claim, `ref_id`, verdict, verbatim quote, and
      source link; dead-link lines carry their `archive_of` URLs — verified by
      renderer tests.
- [ ] Skips and extraction failures render as distinct first-class lists and
      are never dropped, verified over a fixture containing both.
- [ ] The wording invariants hold: no pass/fail phrasing, and
      `NotSupported`/`Partial` findings render in mismatch framing (no
      "failed citation"-style wording), verified by renderer assertions over
      an adversarial fixture.
- [ ] *(staged: lands with PRD-0015's implementation, not the MVP gate)* A
      supplied `StabilitySignal` renders facts-then-interpretation with the
      conduct posture enforced (no usernames outside evidence links); until
      then — and whenever the signal is absent — criterion 5 appears in the
      "not assessed" line, verified by an absent-signal fixture. The
      citations-only appendix is this PRD's MVP acceptance gate.
- [ ] An unlocated-support finding (`Supported`/`Partial` with
      `grounding_status` unlocated) renders as unconfirmed support, distinct
      from grounded findings, verified by a renderer test.
- [ ] Supported findings render as compact one-line entries (`ref_id`, claim
      prefix, grounding marker) with no quotes, and unconfirmed supports
      render in their own sublist rather than inside the supported list,
      verified by renderer tests.
- [ ] The "not assessed by SP42" line names the criteria the supplied inputs
      say nothing about — criterion 5 leaves the list exactly when a
      `StabilitySignal` renders — verified by renderer tests over both input
      shapes.
- [ ] Quoted evidence containing wikitext markup (templates, refs, links) is
      `<nowiki>`-escaped so the appendix never transcludes or breaks page
      markup, verified by a malicious-quote fixture.
- [ ] The provenance footer (article, `rev_id`, run date, version, framing
      line, what-is-this explainer link) is always present, verified by a
      renderer test.
- [ ] No raw contract identifiers (`NotSupported`, `SourceUnavailable`, enum
      variant names generally) appear in the appendix; all verdict/status
      vocabulary comes from the reader-facing copy module, verified by a
      renderer assertion scanning output over a fixture exercising every
      verdict and status.
- [ ] Rendering is deterministic: the same input reports produce a
      byte-identical appendix, verified by a replay test.
- [ ] Rendering a saved report performs no network and no inference, verified
      by asserting mock clients are never invoked.
- [ ] The `sp42-assessment` crate passes the layer check (platform ◄ domain ◄
      shell; no shell dependency), verified by `check-layering.sh` in CI.

## Alternatives

- *Status quo (reviewer transcribes by hand).* The cost being removed;
  transcription also loses grounding and invites verdict-flavored paraphrase.
- *Browser-first rendering.* Rejected for sequencing: the CLI-first pattern is
  the house norm (PRD-0008/0009/0015), and the pasteable artifact is the
  point; the browser Citations tab is the committed eventual home, as a
  follow-on.
- *Use GA review templates for richer layout.* Deferred: template dependencies
  vary per wiki and can break the paste; plain wikitext is portable. Revisit
  with real reviewer feedback.
- *Auto-post to the review subpage.* Rejected here — that is the design
  sketch's step 4 with its own blockers (insertion extension, ADR-0018), and
  posting without them would bypass the ADR-0010 discipline.
- *Put the renderer in `sp42-reporting`.* Rejected: that crate is patrolling-
  domain reporting; GA wording is assessment policy, and the layering rule
  puts process-specific policy in its own domain crate.

## Risks

- **A pasted appendix reads as the review.** The strongest mitigation is
  structural: no pass/fail wording exists to quote, and the footer states the
  judgment is the reviewer's. Alpha feedback from actual GA reviewers is the
  check on whether the framing lands.
- **Verbatim quotes on a public page.** Short located quotes are fair-use-
  shaped and already the verifier's grounding currency, but they are arbitrary
  text: the `<nowiki>` escaping (tested) removes the markup-injection hazard,
  and quote length stays bounded by the verifier's existing passage discipline.
- **Appendix length on citation-heavy articles.** A 200-citation article
  yields a long appendix. MVP posture: render everything, actionable-first,
  with the stats line as the executive summary; if alpha shows reviewers
  truncating by hand, add layout options then rather than guess now.
- **Copy drift vs. GA community expectations.** The criterion wording is
  centralized in one module and reviewed with real reviewers during alpha;
  it is renderer copy, not contract, so it can change without touching
  report semantics. This risk carries extra weight under the on-ramp persona:
  the appendix is SP42's public face, and for most readers the first SP42
  artifact they ever see — the copy *is* the first impression.

## Resolved questions

All three carry the Editor's decided answers (design discussion 2026-07-10),
folded into the body above; they remain open to reviewer reaction until
acceptance.

1. **Surface shape.** Resolved: **both ship in the MVP.** The
   render-from-saved-report mode is the core — the implementation sketch
   (`2026-07-10-ga-appendix-renderer.md`) confirmed it needs no bridge
   session, no server, and no network, making it the pure, replayable heart
   of the renderer and the fixture-test entry point — and the `verify-page`
   format flag is the convenience reviewers will actually type, a few lines
   once the builder exists.
2. **Supported findings: how much detail?** Resolved: **a compact
   one-line-each list, not counts-only.** The originally proposed counts-only
   answer was revised during discussion: the GA spot-check's purpose is to
   document that sources *support* the text, and a reviewer's record must say
   *which* refs were checked — counts alone don't serve that. Supported
   findings render one line each (`ref_id`, claim prefix, grounding marker),
   quotes stay in the CLI/structured rendering, and unconfirmed supports form
   their own visible sublist rather than blending into the supported list.
3. **Section for criteria SP42 says nothing about.** Resolved: **keep the
   "not assessed by SP42" line.** One line, the appendix's own honesty arm;
   it protects the reviewer (no one can later claim the tool vouched for
   neutrality or breadth), which outweighs any reads-as-noise concern.
   Criterion 5 leaves the list exactly when a `StabilitySignal` renders.
