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
  as SP42's first impression. Same review: criterion numbering keyed to the
  GA criteria at first use, appendix headings carry number and name, and the
  criterion-2 sublists reordered by consequence for the review
  (disagreements lead; link rot follows).

## Scope boundary

This PRD owns **one renderer**: a pure function from SP42's existing article
evidence reports to a **GA-review-shaped wikitext evidence appendix** — the
artifact a Good-article reviewer pastes into `Talk:Article/GAn`. It is the
first build step of the GA review-assist design sketch
(`docs/design-plans/2026-07-09-ga-review-assist.md`) and deliberately its
smallest: no new verification, no new fetches, no writes, no inference.

Criterion numbering throughout refers to the
[Good article criteria](https://en.wikipedia.org/wiki/Wikipedia:Good_article_criteria):
1 well-written (1a prose, 1b MoS), 2 verifiable (2b inline support, 2c no
original research, 2d no copyvio), 3 broad, 4 neutral, 5 stable, 6 media.
SP42's evidence feeds 2b (and 5, once PRD-0015 lands); everything else is
covered only by the "assessed by SP42" honesty line.

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

One renderer, `reports → wikitext appendix`, organized by GA criterion.
Section headings carry the criterion number **and name** — "Criterion 2
(verifiable)" — with the criteria page linked once, because the default reader
may not know the numbering:

- **Criterion 2 (verifiable) section.** A stats summary line — preferring the
  report's `stats` arm, deriving deterministically from `findings` only what
  `stats` lacks: today, the grounded/unconfirmed split within supported
  verdicts, which the summary must state ("10 supported, 2 of them
  unconfirmed") because blending them would violate the grounding invariant
  below. (The gap is really ADR-0011's stats arm lagging its own grounding
  axis; an additive `supported_unlocated`-style counter on
  `PageVerificationStats` in `sp42-citation` is the clean upstream fix, noted
  here for the references domain — this PRD does not gate on it.) Then
  sublists **in order of consequence for the review** — the substantive
  spot-check events before the mechanical repairs:
  1. *Claim–source disagreements* — `NotSupported`/`Partial` findings, each
     line carrying the claim, its citation marker (`[1]`, named refs — what
     `ref_id` already holds), the reader-facing verdict, the verbatim located
     quote, and the source link. These can sink criterion 2 or reveal text
     drift; they lead.
  2. *Dead links* — unreachable sources, with their `archive_of` repair
     handles. Mechanical and actionable.
  3. *Unreadable sources* — fetched but not machine-readable (PDF, viewer
     shells), honestly framed as a tool limitation: the citation may be fine.
  4. *Unconfirmed supports* — supported/partial verdicts whose quote could not
     be re-located: the panel's judgment without evidence in hand, never
     blended into the supported list.
  5. *Supported findings* — a compact one-line-each spot-check record
     (citation marker, claim prefix, grounding marker); the reviewing guide
     expects the reviewer to say what they checked, and counts alone are not a
     record. Quotes stay in the CLI/structured rendering.
  6. *Skipped refs and extraction failures* — first-class, never dropped.
  7. *Book citations* — resolve/ground outcomes with scanned-page deep links
     when grounded, as PRD-0009 lands them in the report contract.
- **Criterion 5 (stable) section** (when a `StabilitySignal` is supplied): Layer A facts
  first (timeline, phase markers, marker inventory, triage outcome and knob
  disclosure), then the labeled Layer B characterizations with their cited
  evidence. The PRD-0015 conduct posture binds the rendering: participants
  counted, never named outside evidence/diff links.
- **An "assessed by SP42" line**, stated positively because the assessed set
  is tiny and the complement drifts: "this appendix carries evidence for
  criterion 2b (inline support) only" — plus criterion 5 exactly when a
  stability signal renders — "all other criteria and sub-criteria were not
  assessed." One line; the appendix's own honesty arm, so silence cannot be
  read as endorsement, and drift-proof as evidence lanes land.
- **Provenance footer**, always: article and `rev_id`, run date, SP42 version,
  an explicit framing line — this is a tool-generated evidence appendix; the
  criteria judgments and the pass/hold/fail are the reviewer's — and a
  **"what is this?" link** to a stable explainer of the tool and its verdict
  vocabulary, because the default reader has never heard of SP42. The
  explainer is a repo-hosted docs page for the MVP — the platform's
  `public_documents` surface is the natural host — with an on-wiki essay page
  as the eventual home once SP42 has a community presence, since wiki editors
  trust on-wiki links more than GitHub URLs. The footer's date is the
  shell-injected render date, labeled as such, until the report contract
  carries a verification timestamp (see the Definition of Done's upstream
  note).

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

- **Plain wikitext for the MVP** — headings, lists, links, no transclusions.
  The reason is the dependency failure mode, not renderer trust (the wiki is
  the renderer): a transclusion executes wiki-side logic SP42 does not
  control — a missing template renders as redlink garbage, and a later
  template revision silently changes what an already-posted appendix
  displays. Plain markup displays as emitted, forever, on any wiki. The known
  counterargument is recorded: enwiki GA reviews have a native template idiom
  (`{{GAList}}` and friends), and to the on-ramp reader a plain appendix may
  read as less native, not more trustworthy — so whether to adopt the native
  idiom is explicitly routed to the alpha copy review with real GA reviewers,
  and adopting it would be a copy-module change, not an architecture change.
  Independent of that decision, verbatim quoted evidence is `<nowiki>`-escaped
  as a hard safety rule: a grounded quote is arbitrary text, and pasting it
  must never transclude a template or break the page's markup.
- **Stable ordering and addressing.** Sublist *categories* order by
  consequence (as specified above); *within* each sublist, findings keep
  report order, keyed by `ref_id`/`use_site_ordinal` — so two runs over the
  same article produce line-comparable appendices (the hold-loop
  comparability the design sketch commits to) while the disagreements still
  lead.
- **Placement per the layering rule**: the renderer is assessment-domain
  *policy* (GA-shaped wording is process-specific) over the references
  domain's report contracts — `PageVerificationReport` lives in
  `sp42-citation` (the implementation sketch verified this; domain→domain
  dependency is allowed), with `StabilitySignal` slated for `sp42-core` per
  PRD-0015. The crate is flat `crates/sp42-assessment`, matching the actual
  workspace layout rather than `adding-a-domain.md`'s nested illustration,
  and is otherwise wired per that doc. The criterion copy lives in one module
  for later localization; MVP copy is English/enwiki-GA.
- **Surface**: CLI-first — a `ga-appendix` output format on the page-verify
  path, and equally a render of a *saved* report (the replay-friendly core: a
  stored `PageVerificationReport` + optional `StabilitySignal` render with no
  network and no inference).

## Definition of Done

*Names planned coverage; the closing PR records exact test ids. All renderer
tests run over fixture reports — no live network, no inference (the renderer
is pure).*

- [ ] A fixture `PageVerificationReport` renders to an appendix with the
      criterion-2 structure above: number-and-name section headings, stats
      line (stating the grounded/unconfirmed split within supported verdicts,
      derived from `findings` since `stats` lacks it), sublists in the
      consequence order specified, each disagreement line carrying claim,
      citation marker, reader-facing verdict, verbatim quote, and source
      link; dead-link lines carry their `archive_of` URLs — verified by
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
      then — and whenever the signal is absent — criterion 5 stays out of the
      "assessed by SP42" line, verified by an absent-signal fixture. The
      citations-only appendix is this PRD's MVP acceptance gate.
- [ ] An unlocated-support finding (`Supported`/`Partial` with
      `grounding_status` unlocated) renders as unconfirmed support, distinct
      from grounded findings, verified by a renderer test.
- [ ] Supported findings render as compact one-line entries (citation marker,
      claim prefix, grounding marker) with no quotes, and unconfirmed supports
      render in their own sublist rather than inside the supported list,
      verified by renderer tests.
- [ ] The "assessed by SP42" line states the assessed set positively (2b
      only; plus 5 exactly when a `StabilitySignal` renders) and that all
      other criteria and sub-criteria were not assessed, verified by renderer
      tests over both input shapes.
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
- [ ] Rendering is deterministic: the same inputs — reports plus the
      shell-injected `rendered_at` timestamp (`Clock` trait; the report
      contract carries no run timestamp today) — produce a byte-identical
      appendix, verified by a replay test with a pinned timestamp. (Upstream
      note, alongside the stats-split one: an additive `verified_at` on
      `PageVerificationReport` would let the footer state when the
      *verification* ran — the honest timestamp for a re-rendered saved
      report; until then the footer labels its date as the render date.)
- [ ] Both MVP surfaces produce the appendix: `render-report` over a saved
      fixture report, and the `ga-appendix` format on the page-verify path,
      with the saved-report render byte-identical to the fixture render,
      verified by CLI tests.
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
- *Use GA review templates (`{{GAList}}` and friends) for native layout.*
  Deferred, not rejected: the MVP is plain wikitext for the dependency
  failure mode (missing/revised templates change or break a posted appendix),
  but the native idiom may read as more trustworthy to the on-ramp reader —
  the decision is routed to the alpha copy review with real GA reviewers, and
  adopting it is a copy-module change only.
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
   honesty line.** One line, the appendix's own honesty arm; it protects the
   reviewer (no one can later claim the tool vouched for neutrality or
   breadth), which outweighs any reads-as-noise concern. During the
   paragraph review the line was inverted to state the *assessed* set
   positively ("evidence for criterion 2b only" — plus 5 exactly when a
   `StabilitySignal` renders; everything else not assessed): the original
   complement enumeration (1a, 2c, 3, 4, 6) was simply wrong — it omitted
   1b, 2a, and 2d — and a complement list re-drifts every time an evidence
   lane lands, while the assessed set stays tiny and self-correcting.
