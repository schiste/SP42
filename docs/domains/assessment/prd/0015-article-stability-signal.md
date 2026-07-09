# PRD-0015: Article stability signal

**Drafter:** Claude Code
**Editor:** Luis Villa
**Date:** 2026-07-09
**State:** Draft
**Discussion:** design conversation 2026-07-09 (extending SP42 toward Good-article
assessment); no tracking issue yet.
**Spawned ADRs:** none yet. If accepted, expect one thin **platform** ADR
pinning the resolved-Q2 placement: the page-history fetch edge (revision list +
tags + summaries), the revert-chain reducer, and the talk-activity sensor as
platform mechanisms (pure builders/parsers over the `HttpClient` trait), with
the `StabilitySignal` contract in `sp42-core` (Constitution Art. 9.1, the
`PageVerificationReport` precedent) and triage/vocabulary policy left to the
assessment domain.

## Scope boundary

This PRD owns **an article-level stability evidence signal**: given an article and
the head revision observed at run time (the `rev_id` that anchors the evidence
window and makes the run reproducible), SP42 gathers and interprets the evidence
a reviewer needs to judge whether the article is *stable* — the shape of
[Good article criterion 5](https://en.wikipedia.org/wiki/Wikipedia:Good_article_criteria)
("does not change significantly from day to day because of an ongoing edit war or
content dispute"). It is the first assessment-domain capability; how it slots into
a full GA review is the companion design doc
(`docs/design-plans/2026-07-09-ga-review-assist.md`).

It deliberately excludes:

- **Any pass/fail on criterion 5.** The output is evidence plus a categorical
  characterization; the criterion judgment stays with the human reviewer, matching
  the informational-verdict posture of the references domain (ADR-0007).
- **Patrol queue ranking.** The revert/dispute machinery here may later inform
  queue ranking, but scoring policy is PRD-0003 territory and untouched.
- **Per-edit damage scoring.** LiftWing revertrisk answers "is this one edit
  likely vandalism"; this signal answers "is this article's recent history a
  fight." Different question, different consumer (see Alternatives).
- **Automatic on-nomination runs.** The eventual goal includes running this
  signal automatically when an article is nominated, which needs a GAN-feed
  watcher (patrolling/live-domain machinery). That comes much later, after this
  design is proven extensively in operator-attended use. Two forward
  constraints are recorded now so the design does not foreclose it: the
  three-phase window (resolved Q1) must degrade gracefully when run at
  nomination time (phases 2–3 empty), and an unattended run executes **Layer A
  only** — Layer B inference spend with no operator present is deferred to the
  first attended run or an explicit budget opt-in (the ADR-0011 Decision 5
  budget concern).

## Problem

A GA reviewer must assess stability, and today that means manually reading the
page history and talk page. The evidence-gathering is mechanical drudgery; the
judgment is not. Crucially, the GA criteria's own footnote exempts several kinds
of churn from counting as instability — vandalism reversion, split/merge
proposals, good-faith copyediting, and changes made in response to the review
itself. That exemption is why a purely deterministic checker cannot be
comprehensive, likely ever: three tagged reverts in a week could be an edit war
between two editors *or* one patroller cleaning up a drive-by vandal, and the
revision tags look identical. Telling them apart means reading the edit
summaries, the reverted diffs, and the talk-page thread — interpretation, not
counting.

This is for the same experienced reviewer as the references domain: they want the
evidence assembled and honestly characterized, at zero cost for the common quiet
case, with the interpretive layer clearly labeled as such.

## Proposal

Two layers over one evidence pool, with the deterministic layer acting as both
sensor and cost gate.

### Layer A — deterministic sensor and gate (no inference, always runs)

For the nominated article, SP42 fetches and reduces:

- **Revert timeline** from page history (revision list with tags and edit
  summaries): revisions tagged `mw-undo` / `mw-rollback` / `mw-reverted`, reduced
  to revert *chains* (who reverted whom, how many rounds, over what interval),
  with self-reverts recognized as such. Revert density and distinct-participant
  counts over the evidence window.
- **Dispute markers** from the article talk page: active dispute banners
  (`{{POV}}` and family), open split/merge proposals, RfC templates — the same
  template-presence parsing discipline `user_analyzer` already applies to warning
  templates.
- **Protection status** of the article (full/semi, expiry).
- **Talk-page activity volume** (resolved Q3): talk-edit counts and thread
  recency over the window — a deterministic sensor, so a banner-less active
  dispute (a hot talk page with no reverts) still reaches triage.
- **Three review phases** (resolved Q1). The evidence window is anchored to
  **run time** — 90 days back — not the nomination timestamp: GAN backlog waits
  run to months, and criterion 5 is about *current* behavior, so a
  nomination-anchored window would hand the reviewer a stale picture. The
  `{{GA nominee}}` and review-start timestamps render as phase markers on the
  timeline: **pre-nomination** (the baseline record), **nomination →
  review-start** (counts fully toward stability — backlog-wait editing is
  ordinary editing; no exemption applies yet), and **review-start → now**
  (presumptively exempt under the GA footnote's changes-based-on-the-review
  carve-out, but *classified rather than blanket-exempted* — an edit war that
  erupts during a hold still counts). The report records its window bounds and
  phase timestamps, restoring the reproducibility a fixed anchor would have
  given. Dispute markers and protection status are **windowless** — a live
  banner is current state regardless of when it was placed.

Layer A triages the article into three outcomes:

1. **Quiet** — no reverts, no dispute markers, no protection, and no unusual
   talk activity in the window. The report says "no instability indicators
   found" and **no inference runs**. This is the common case and it is free.
2. **Unambiguous** — overwhelming mechanical evidence (e.g. full protection over
   an active dispute banner plus a live revert chain). Reported directly from the
   deterministic facts.
3. **Ambiguous middle** — reverts or markers exist but their meaning is unclear.
   Only this slice pays for Layer B.

### Layer B — panel interpretation (inference, gated to the ambiguous middle)

The existing model panel (ADR-0006) reads the evidence Layer A gathered — edit
summaries, the diffs of the revert-chain revisions, the relevant talk-page
threads — and classifies into a small categorical vocabulary (resolved Q3):

- `EditWarPattern` — sustained back-and-forth between committed participants
- `ActiveContentDispute` — an unresolved dispute conducted on the talk page,
  without reverts or banners (criterion 5 names "content dispute" as a distinct
  thing from an edit war; the talk-activity sensor is what routes this case to
  Layer B at all)
- `VandalismCleanup` — churn is vandalism plus its reversion (GA-exempt)
- `StaleDisputeBanner` — a dispute marker whose discussion has gone dormant
- `ReviewDrivenChurn` — review-phase edits responding to the review (GA-exempt).
  Kept as a class rather than folded into the phase split: the phase marker says
  *when* an edit happened, classification says *why* — an edit war erupting
  mid-hold is phase-3 activity that is **not** exempt, and only classification
  (summaries referencing the review, edits landing in review-flagged sections)
  separates the two.
- `Unclear` — the panel declines to characterize

This vocabulary is **explicitly provisional**: it is the part of the design most
exposed to alpha evidence, and the improvement loop below is its revision
mechanism — alpha-era Layer B runs *are* the PRD-0007 benchmark corpus. The
contract therefore treats class evolution as additive under the ADR-0009 replay
discipline (new variants never break deserialization of old snapshots), so
reshaping the vocabulary is a planned amendment path, not a broken freeze.

Two disciplines transfer from the citation path unchanged:

- **Grounding (ADR-0007 spirit).** Every characterization must cite its evidence:
  specific `rev_id`s and verbatim excerpts (edit summaries, talk-page sentences)
  that are locatable in the fetched evidence pool. A characterization whose
  excerpt cannot be located verbatim is rejected. "The editors seem to be
  fighting," without a quote, is not an output this system can produce.
- **Abstention.** `Unclear` is always acceptable; low panel agreement surfaces to
  the operator rather than being averaged away.

**Evidence packaging (resolved Q4): one bundle per article.** The panel sees the
whole evidence pool — all chains, summaries, and matched talk threads — in a
single call, because the question is article-level and cross-chain context is
itself evidence: the same two editors recurring across three chains over six
weeks is the signature of a sustained dispute, and per-chain calls structurally
cannot see that pattern (nor cleanly partition talk threads that relate to
several chains). Cost also scales with nominations rather than with churn. The
bundle is size-capped with **disclosed truncation**; per-chain sharding is not a
rival design but the overflow strategy when even the truncated bundle exceeds
the cap — used with its cross-chain blindness stated in the report. Whether
attention dilution on large bundles is real at our evidence sizes is exactly
what the alpha-era PRD-0007 fixtures measure; the choice is benchmark-revisable.

### The report

A versioned `StabilitySignal` aggregate — a Constitution Art. 9.1 serde contract
living in `sp42-core`, following the `PageVerificationReport` precedent
(ADR-0011), so shells and renderers consume it without depending on the
assessment crate (resolved Q2) — rendering both layers, labeled: the raw
timeline with phase markers and the marker inventory (Layer A, facts), then —
only when it ran — the panel characterization with its cited evidence (Layer B,
interpretation). The report records its window bounds and phase timestamps for
replay. The rendering never uses pass/fail wording for criterion 5. When the
evidence pool was truncated (very large talk archives), the truncation is
disclosed in the report, not silent.

### The improvement loop

Layer B is not a permanent crutch; it is the measurement instrument that tells us
which deterministic rules are worth writing:

- Every Layer B run is recorded as a replayable case under the ADR-0009
  snapshot/replay discipline and becomes a candidate fixture for the PRD-0007
  benchmarking harness.
- When a *class* of cases proves consistently classifiable (self-reverts;
  reverts whose summaries cite vandalism policy), that class graduates into
  Layer A as a deterministic rule, and a regression fixture pins it.
- The share of nominations that need Layer B at all is the tracked health metric:
  a shrinking inference share is what "the algorithm is improving" means here.

Posture precedent: the patrol queue already consumes a model signal (LiftWing
revertrisk) informationally, and PRD-0011 gated scoring *off* for Wikidata rather
than fabricate signal from an unfit model. Same principle, both directions: use a
model where interpretation is genuinely required, grounded and categorical; spend
nothing and claim nothing where it is not.

### Surface

CLI-first, matching the references-domain pattern: a `stability` report for
`{wiki_id, title, rev_id}`, foldable into the GA assist report (design doc) and
eventually the browser shell. Read-only; no apply lane exists or is planned for
this signal.

## Definition of Done

*Names planned coverage; the closing PR records exact test ids. All tests replay
recorded MediaWiki responses — no live network (ADR-0009 discipline).*

- [ ] A quiet article (no reverts/markers/protection/unusual talk activity in
      the window) produces a "no instability indicators" report and **zero**
      model-inference calls, verified by a fixture test asserting the mock
      `ModelClient` is never invoked.
- [ ] Revert chains are reduced correctly from a replayed history fixture
      (tags + summaries → chains with participants and intervals), including
      self-revert recognition, verified by unit tests over the reducer.
- [ ] Dispute banners, split/merge proposals, and protection status surface from
      replayed talk-page and page-info fixtures, verified by parser tests.
- [ ] The report splits activity across the three phases (pre-nomination /
      nomination→review-start / review-start→run), keyed on the `{{GA nominee}}`
      and review-start timestamps, records its window bounds, and degrades to
      fewer phases when the later timestamps are absent — verified by fixtures
      with churn in each phase, asserting in particular that
      nomination→review-start activity is **never** reported as exempt.
- [ ] A banner-less, revert-free article with heavy recent talk activity reaches
      Layer B via the talk-activity sensor and may classify
      `ActiveContentDispute`, verified by a replayed talk-history fixture.
- [ ] An ambiguous fixture (reverts present, meaning unclear) triggers Layer B,
      and the resulting characterization carries at least one cited `rev_id` and
      one verbatim excerpt locatable in the fetched evidence; a fabricated
      (non-locatable) excerpt is rejected by the grounding gate, verified by unit
      tests on the gate.
- [ ] A vandalism-cleanup fixture (reverts whose context is vandalism reversion)
      classifies as `VandalismCleanup` with cited evidence and is **not**
      reported as an instability indicator, verified by a replayed-panel fixture
      test.
- [ ] `Unclear` is an accepted panel outcome and low agreement is surfaced, not
      averaged away, verified by a disagreeing-panel fixture.
- [ ] The rendered report labels Layer A facts and Layer B interpretation
      distinctly and contains no criterion-5 pass/fail wording, verified by a
      renderer test.
- [ ] Every Layer B run persists a replayable snapshot suitable for the PRD-0007
      harness, verified by asserting the snapshot round-trips byte-identically.
- [ ] Evidence-pool truncation (oversized talk archives) is disclosed in the
      report, verified by an oversized fixture.

## Alternatives

- *Pure deterministic checker.* Rejected: the GA footnote's exemptions
  (vandalism reversion, review-driven changes, good-faith copyediting) are
  meaning distinctions that identical revision tags cannot express. A
  deterministic-only signal would either over-report (counting vandalism cleanup
  as instability) or require the very interpretation it forgoes.
- *Always-on LLM assessment.* Rejected: the common case is a quiet page where
  there is nothing to interpret; spending panel inference there is waste and
  invites fabricated signal — the exact failure PRD-0011 avoided by gating
  scoring off for Wikidata.
- *Reuse LiftWing revertrisk aggregated over recent edits.* Rejected: it scores
  per-edit damage probability for patrol triage; summing it does not answer
  "is this a sustained dispute," and it is Wikipedia-trained per-edit, not
  validated for article-level aggregation.
- *Numeric thresholds ("unstable if >N reverts/week").* Rejected: false
  precision the GA documentation itself does not define; thresholds would encode
  a criterion judgment this signal deliberately refuses to make.
- *Status quo (reviewer reads the history manually).* This remains the fallback
  whenever inference is unavailable — Layer A alone still assembles the evidence,
  which is most of the drudgery being removed.

## Risks

- **Panel misreads discussion tone** (sarcasm, heated-but-resolving threads).
  Mitigation: grounded excerpts the operator can check, the categorical (not
  free-text) vocabulary, `Unclear` as a first-class outcome, and the human owning
  the criterion judgment.
- **Large talk archives blow the evidence budget.** Mitigation: scope to the
  current talk page plus recent archives, sample beyond that, and disclose
  truncation in the report.
- **Operator habituation / over-trust.** Mitigation: evidence-forward rendering
  (facts before interpretation), no pass/fail wording, low-agreement surfacing.
- **Wrong evidence window.** Too short misses slow-burn disputes; too long
  punishes ancient history. Mitigation: configurable window (90-day default,
  resolved Q1), run-time anchoring with recorded bounds, and the three-phase
  split so exemption never silently expands.
- **Cost surprise.** Layer B spends real inference. Mitigation: the gate keeps
  quiet pages free; the ambiguous-middle share is tracked (improvement loop), and
  the CLI reports when and why Layer B ran.

## Resolved questions

All four carry the Editor's decided answers (design discussion 2026-07-09),
folded into the body above; they remain open to reviewer reaction until
acceptance.

1. **Evidence window.** Resolved: **90 days anchored to run time, three
   phases.** The original nomination-timestamp anchor was wrong — GAN backlog
   waits run to months and criterion 5 is about current behavior — so the
   window ends at the evidence run, with `{{GA nominee}}` and review-start as
   phase markers: pre-nomination (baseline), nomination→review-start (counts
   fully), review-start→run (presumptively exempt but classified, never
   blanket-exempted). Reproducibility is restored by recording the window
   bounds in the report rather than by anchoring to a stale event. Dispute
   markers and protection are windowless. The phase design degrades gracefully
   for a future automatic nomination-time run (see Scope boundary), whose feed
   watcher is deliberately deferred until the design is proven in
   operator-attended use.
2. **Placement of the mechanisms.** Resolved: **mechanisms platform, contract
   in `sp42-core`, policy in the domain.** The page-history fetch edge and
   revert-chain reducer are platform — pure builders/parsers over the
   `HttpClient` trait (the Citoid/recentchanges precedent) — with the
   reuse-by-design case strengthened since drafting: the history fetch is
   content-model agnostic (`prop=revisions` serves entity revisions
   identically), making the Wikidata domain a credible second consumer beyond
   patrol. The `StabilitySignal` report follows the `PageVerificationReport`
   precedent (Constitution Art. 9.1, ADR-0011): a versioned serde contract in
   `sp42-core`, not the domain crate. Triage thresholds, panel prompts, the
   vocabulary, and GA phase semantics stay assessment-domain. Pinned by the
   spawned ADR.
3. **Categorical vocabulary.** Resolved: **six classes, explicitly
   provisional.** `ReviewDrivenChurn` stays — the phase split says *when*,
   classification says *why*, and an edit war erupting mid-hold must not
   inherit the phase's exemption. `ActiveContentDispute` is added: criterion 5
   names "content dispute" as distinct from edit war, and a hot talk page with
   no reverts and no banner was inexpressible in the five-class draft — with
   the Layer A consequence that talk-activity volume joins the deterministic
   triage sensors. The vocabulary is expected to be reshaped by alpha evidence
   (the Editor's stated expectation); the additive-evolution rule in the body
   is what makes that a planned amendment rather than a contract break.
4. **Panel evidence packaging.** Resolved: **one bundle per article**, size-
   capped with disclosed truncation; per-chain sharding demoted to the overflow
   strategy (cross-chain blindness stated in the report when used). Cross-chain
   patterns — the same participants recurring across chains — are themselves
   evidence an article-level question needs, and per-chain calls cannot
   recover them. Benchmark-revisable via the alpha-era PRD-0007 corpus.
