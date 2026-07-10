# PRD-0016: GA evidence appendix renderer

**Drafter:** Claude Code
**Editor:** Luis Villa
**Date:** 2026-07-10
**State:** Draft
**Discussion:** design conversation 2026-07-10; PR link pending.
**Spawned ADRs:** none expected — the renderer is pure composition over
existing report contracts (`PageVerificationReport`, ADR-0011; `StabilitySignal`,
PRD-0015). Later lanes carry their own artifacts: posting the appendix is the
ADR-0010 + insertion-extension + ADR-0018 work named in the design sketch, and a
machine-readable task-graph arm is roadmap there too.

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
  must not add, drop, or soften a single finding. The honesty arms (skips,
  extraction failures, declined Layer B, truncations) render or the renderer
  is wrong.
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

This is for the GA reviewer already using SP42's evidence runs: they want the
paste-ready appendix, with the wording discipline built in.

## Proposal

One renderer, `reports → wikitext appendix`, organized by GA criterion:

- **Criterion 2 section.** A stats summary line (from the report's `stats` arm,
  no re-aggregation), then actionable sublists ordered most-actionable-first:
  dead links (with the `archive_of` repair handles), unreadable sources
  (`unusable` split, honestly framed as a tool limitation), and
  `NotSupported`/`Partial` findings — each line carrying the claim, its
  `ref_id`, the verdict, the verbatim located quote, and the source link.
  Skips and extraction failures render as their own first-class lists. Book
  citations render with their resolve/ground outcomes (deep links to scanned
  pages when grounded).
- **Criterion 5 section** (when a `StabilitySignal` is supplied): Layer A facts
  first (timeline, phase markers, marker inventory, triage outcome and knob
  disclosure), then the labeled Layer B characterizations with their cited
  evidence. The PRD-0015 conduct posture binds the rendering: participants
  counted, never named outside evidence/diff links.
- **Provenance footer**, always: article and `rev_id`, run date, SP42 version,
  and an explicit framing line — this is a tool-generated evidence appendix;
  the criteria judgments and the pass/hold/fail are the reviewer's.

Wording invariants, enforced as contract rather than style:

- **No criterion pass/fail wording, anywhere.**
- **PRD-0014's mismatch framing verbatim**: a `NotSupported`/`Partial` finding
  is rendered as claim-and-source disagreement, never as a citation failure —
  the article text may be the wrong side.
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
- [ ] A supplied `StabilitySignal` renders facts-then-interpretation with the
      conduct posture enforced (no usernames outside evidence links); an
      absent one renders as an explicit "stability not run" note — verified
      over PRD-0015's fixtures and an absent-signal fixture.
- [ ] Quoted evidence containing wikitext markup (templates, refs, links) is
      `<nowiki>`-escaped so the appendix never transcludes or breaks page
      markup, verified by a malicious-quote fixture.
- [ ] The provenance footer (article, `rev_id`, run date, version, framing
      line) is always present, verified by a renderer test.
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
  report semantics.

## Open questions

1. **Surface shape.** Proposed: both a `ga-appendix` format flag on the
   existing page-verify CLI path *and* a render-from-saved-report mode, with
   the saved-report render as the core (pure, replayable) and the flag as
   convenience. React if one should ship without the other.
2. **Supported findings: how much detail?** Proposed: supported findings
   appear in the stats line and as a compact count per section, with detail
   lines only for actionable findings (dead, unusable, not-supported, partial,
   skipped) — bounding appendix length while keeping every honesty arm
   visible. The full per-finding detail remains in the CLI/structured
   rendering. React if supported findings should render their quotes too.
3. **Section for criteria SP42 says nothing about.** Proposed: the appendix
   includes a one-line "not assessed by SP42" list (1a, 2c, 3, 4, 6) so a
   reader cannot mistake silence for endorsement. React if that reads as
   noise.
