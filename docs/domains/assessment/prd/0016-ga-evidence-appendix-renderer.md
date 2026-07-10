# PRD-0016: GA evidence appendix renderer

**Drafter:** Claude Code
**Editor:** Luis Villa
**Date:** 2026-07-10
**State:** Discussion
**Discussion:** [PR #129](https://github.com/schiste/SP42/pull/129); design
conversation 2026-07-10.
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
- 2026-07-10 (review, continued): the honesty line inverted to state the
  assessed set positively (the complement enumeration was wrong and would
  re-drift); the stats line derives the grounded/unconfirmed split from
  `findings` with an additive `supported_unlocated` stats counter noted
  upstream; determinism pinned via shell-injected `rendered_at` with an
  additive `verified_at` noted upstream; plain-wikitext rationale restated
  (dependency failure mode, not renderer trust) with the `{{GAList}}`
  native-idiom question routed to alpha; placement corrected to
  `sp42-citation` contracts; both CLI surfaces bound to the DoD. The
  implementation sketch was synced with all of the above. State `Draft` →
  `Discussion` on PR open.
- 2026-07-10 (Codex review, PR #129): two contract assumptions corrected
  against the code. `ref_id` is the stable cite id, not the rendered marker
  (the extractor's `BlockRef::ref_text` never reaches the report) — the MVP
  derives a reader-facing ref label and an additive `ref_text` is the third
  upstream note. And `archive_of` exists only on findings *recovered* through
  an archive fallback — the dead-links sublist split into recovered-via-
  archive (with repair handles) and unrecovered (dead URL only; additive
  candidate-archives field is the fourth upstream note).
- 2026-07-10 (Codex review, round 2): the `Partial`+unlocated double-bucket
  resolved — verdict partitions the sublists, grounding annotates, every
  finding appears in exactly one sublist (disagreements win; hiding a
  disagreement is the worse failure); a residual raw-`ref_id` mention in
  Q2's resolution corrected to the derived ref label; and the sketch's CLI
  phase pins an explicit `ga-appendix` value name against the enum's
  lowercase `rename_all`.
- 2026-07-10 (Codex review, round 3): three more contract corrections. The
  quote requirement is scoped to findings carrying `passage` — `NotSupported`
  is typically a no-quote verdict (`NotApplicable`), so its line states no
  supporting passage was found, and `source_excerpt` only ever renders as
  labeled context. `archive_of` renders on any line that carries it (the
  contract stamps it on archive-backed disagreements too, which stay in the
  disagreements bucket with their repair handle). And `LocatedFuzzy` joins
  `Unlocated` in the non-exact-grounding treatment, matching the contract's
  exact-`Located`-only grounding gate.
- 2026-07-10 (Codex review, round 4): the quote-escaping rule hardened
  against the wrapper's own terminator — a literal `</nowiki>` inside a quote
  must not break out; the helper entity-encodes terminators and the malicious
  fixture asserts the breakout case renders inert.

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
     line carrying the claim, its reader-facing ref label, the reader-facing
     verdict, the located quote **when the finding carries one**, and the
     source link. A `NotSupported` finding is typically a **no-quote verdict**
     (ADR-0007: grounded by the search outcome, never a fabricated passage —
     `passage` absent, grounding `NotApplicable`): its line states that no
     supporting passage was found, rather than pretending to quote.
     `source_excerpt`, when present, may render only as labeled context (what
     the panel read), never presented as grounded evidence. And `archive_of`
     renders on **any** line that carries it, whatever the bucket — an
     archive-backed disagreement keeps its dead-live-URL repair handle here.
     These can sink criterion 2 or reveal text drift; they lead. **This bucket takes every**
     `NotSupported`/`Partial` **finding regardless of grounding status**: an
     unlocated quote on a `Partial` renders as an annotation on its line
     here, never re-buckets it under unconfirmed supports — hiding a
     claim–source disagreement is the worse failure. Verdict partitions the
     sublists; grounding annotates. Every finding appears in exactly one
     sublist.
  2. *Recovered via archive* — findings whose claim was **supported** through
     an archive fallback: the live URL is dead and `archive_of` is the repair
     handle — "update the citation to the archive." Pulled out of the
     supported list because they are actionable. (The contract stamps
     `archive_of` on any non-unavailable archive verdict, including archive-
     backed `Partial`/`NotSupported` — those stay in the disagreements bucket
     and carry the same repair handle there.)
  3. *Dead links (unrecovered)* — unreachable sources no archive rescued.
     The report preserves no archive candidates for these (candidate archive
     URLs live on the extractor's use-site and are not copied into findings),
     so lines carry the dead URL only; an additive candidate-archives field
     on unreachable findings is noted upstream so the appendix could offer
     repair candidates.
  4. *Unreadable sources* — fetched but not machine-readable (PDF, viewer
     shells), honestly framed as a tool limitation: the citation may be fine.
  5. *Unconfirmed supports* — `Supported` verdicts whose quote was not
     re-located **exactly**: `Unlocated` (not found at all) or `LocatedFuzzy`
     (approximate match only — explicitly not groundable; the contract's
     grounding gate accepts exact `Located` alone), each annotated with
     which. Never blended into the supported list. (`Partial` with either
     status stays in the disagreements bucket, annotated — see its precedence
     rule.)
  6. *Supported findings* — a compact one-line-each spot-check record
     (ref label, claim prefix, grounding marker); the reviewing guide
     expects the reviewer to say what they checked, and counts alone are not a
     record. Quotes stay in the CLI/structured rendering.
  7. *Skipped refs and extraction failures* — first-class, never dropped.
  8. *Book citations* — resolve/ground outcomes with scanned-page deep links
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
- **The grounding axis renders honestly.** Non-exact grounding is always
  visible and never presented as grounded: a `Supported` finding whose quote
  is `Unlocated` or only `LocatedFuzzy` renders in the unconfirmed-supports
  sublist (annotated with which), a `Partial` with either status carries the
  annotation on its disagreement line, and a no-quote verdict states that no
  supporting passage was found rather than quoting anything. This is
  precisely the nuance hand-transcription loses.
- **Cold-reader legibility.** No raw contract identifiers in the output —
  verdict and status vocabulary renders through the reader-facing copy module
  ("the source did not support this claim", never `NotSupported`), and refs
  are addressed by a **reader-facing ref label**. The report today carries
  only the stable cite id (`ref_id`, e.g. `cite_ref-smith_3-0`) — not the
  rendered marker, despite ADR-0011's prose gesturing at `[1]`-style
  addressing — so the MVP derives the label (the ref name parsed from the
  cite id when present, else a stable per-report index) and never prints the
  raw cite id. The clean fix is upstream: the extractor already holds the
  visible marker (`BlockRef::ref_text`) and simply does not copy it into the
  report; an additive `ref_text` on `CitationFinding`/`SkippedRef` is noted
  for the references domain.
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
  must never transclude a template or break the page's markup. The escaping
  must survive the wrapper's own terminator: a quote containing a literal
  `</nowiki>` would close a naive wrapper and let everything after it execute,
  so the helper entity-encodes nowiki terminators (and any markup-significant
  angle brackets) inside the quoted content rather than trusting the wrapper
  alone.
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
      reader-facing ref label, reader-facing verdict, the located quote when
      `passage` is present (no-quote verdicts render the no-passage wording;
      `source_excerpt` only ever as labeled context), and source link;
      `archive_of` renders on every line that carries it, in every bucket,
      while unrecovered dead-link lines carry the dead URL only — verified by
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
- [ ] Non-exact grounding is always visible and never double-bucketed: a
      `Supported` finding with `Unlocated` or `LocatedFuzzy` grounding
      renders in the unconfirmed-supports sublist (annotated with which), a
      `Partial` with either stays in the disagreements bucket with the
      annotation, and every finding appears in exactly one sublist — verified
      by renderer tests over all four verdict×status combinations.
- [ ] An archive-backed disagreement (`Partial`/`NotSupported` carrying
      `archive_of`) renders in the disagreements bucket **with** its repair
      handle, verified by a renderer test.
- [ ] A no-quote `NotSupported` finding renders the no-passage wording and
      never a quote, with `source_excerpt` (when present) labeled as context
      rather than evidence, verified by a renderer test.
- [ ] Supported findings render as compact one-line entries (ref label,
      claim prefix, grounding marker) with no quotes, and unconfirmed supports
      render in their own sublist rather than inside the supported list,
      verified by renderer tests.
- [ ] The "assessed by SP42" line states the assessed set positively (2b
      only; plus 5 exactly when a `StabilitySignal` renders) and that all
      other criteria and sub-criteria were not assessed, verified by renderer
      tests over both input shapes.
- [ ] Quoted evidence containing wikitext markup (templates, refs, links,
      **and a literal `</nowiki>` terminator**) is escaped so the appendix
      never transcludes or breaks page markup — the terminator case asserts
      that markup *following* an embedded `</nowiki>` still renders inert —
      verified by a malicious-quote fixture covering all four shapes.
- [ ] The provenance footer (article, `rev_id`, run date, version, framing
      line, what-is-this explainer link) is always present, verified by a
      renderer test.
- [ ] No raw contract identifiers (`NotSupported`, `SourceUnavailable`, enum
      variant names generally, and raw `cite_ref-…` ids) appear in the
      appendix; all verdict/status vocabulary comes from the reader-facing
      copy module and ref labels are derived, verified by a renderer
      assertion scanning output over a fixture exercising every verdict and
      status.
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
- *Put the renderer in `sp42-reporting`.* Rejected: that crate hosts shared
  report primitives (`ReportDocument`, consumed by patrol reports and
  `sp42-citation` alike) — putting process-specific GA policy in a shared-
  primitives crate would invert the layering from the other direction; the
  layering rule puts process policy in its own domain crate.

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
   findings render one line each (derived reader-facing ref label, claim
   prefix, grounding marker),
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
