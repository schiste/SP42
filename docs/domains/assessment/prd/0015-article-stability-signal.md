# PRD-0015: Article stability signal

**Drafter:** Claude Code
**Editor:** Luis Villa
**Date:** 2026-07-09
**State:** Draft
**Discussion:** design conversation 2026-07-09 (extending SP42 toward Good-article
assessment); no tracking issue yet.
**Spawned ADRs:** none yet. If accepted, expect one thin ADR covering the
page-history read contract (revision list + tags + summaries as a platform fetch
edge) and the `StabilitySignal` report contract.

## Scope boundary

This PRD owns **an article-level stability evidence signal**: given an article and
a pinned revision, SP42 gathers and interprets the evidence a reviewer needs to
judge whether the article is *stable* — the shape of
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
- **Pre- vs post-nomination split.** Activity after the `{{GA nominee}}`
  timestamp is reported separately: the GA footnote exempts changes made in
  response to the review, so post-nomination churn must not silently count
  against stability.

Layer A triages the article into three outcomes:

1. **Quiet** — no reverts, no dispute markers, no protection in the window. The
   report says "no instability indicators found" and **no inference runs**. This
   is the common case and it is free.
2. **Unambiguous** — overwhelming mechanical evidence (e.g. full protection over
   an active dispute banner plus a live revert chain). Reported directly from the
   deterministic facts.
3. **Ambiguous middle** — reverts or markers exist but their meaning is unclear.
   Only this slice pays for Layer B.

### Layer B — panel interpretation (inference, gated to the ambiguous middle)

The existing model panel (ADR-0006) reads the evidence Layer A gathered — edit
summaries, the diffs of the revert-chain revisions, the relevant talk-page
threads — and classifies into a small categorical vocabulary (final set is an
open question, initial proposal):

- `EditWarPattern` — sustained back-and-forth between committed participants
- `VandalismCleanup` — churn is vandalism plus its reversion (GA-exempt)
- `StaleDisputeBanner` — a dispute marker whose discussion has gone dormant
- `ReviewDrivenChurn` — post-nomination edits responding to the review (GA-exempt)
- `Unclear` — the panel declines to characterize

Two disciplines transfer from the citation path unchanged:

- **Grounding (ADR-0007 spirit).** Every characterization must cite its evidence:
  specific `rev_id`s and verbatim excerpts (edit summaries, talk-page sentences)
  that are locatable in the fetched evidence pool. A characterization whose
  excerpt cannot be located verbatim is rejected. "The editors seem to be
  fighting," without a quote, is not an output this system can produce.
- **Abstention.** `Unclear` is always acceptable; low panel agreement surfaces to
  the operator rather than being averaged away.

### The report

A versioned `StabilitySignal` aggregate (Constitution Art. 9.1 serde-contract
discipline) rendering both layers, labeled: the raw timeline and marker inventory
(Layer A, facts), then — only when it ran — the panel characterization with its
cited evidence (Layer B, interpretation). The rendering never uses pass/fail
wording for criterion 5. When the evidence pool was truncated (very large talk
archives), the truncation is disclosed in the report, not silent.

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

- [ ] A quiet article (no reverts/markers/protection in the window) produces a
      "no instability indicators" report and **zero** model-inference calls,
      verified by a fixture test asserting the mock `ModelClient` is never
      invoked.
- [ ] Revert chains are reduced correctly from a replayed history fixture
      (tags + summaries → chains with participants and intervals), including
      self-revert recognition, verified by unit tests over the reducer.
- [ ] Dispute banners, split/merge proposals, and protection status surface from
      replayed talk-page and page-info fixtures, verified by parser tests.
- [ ] Post-nomination activity is split from pre-nomination activity in the
      report, keyed on the `{{GA nominee}}` timestamp, verified by a fixture
      whose churn is entirely post-nomination and asserts it is reported in the
      exempt bucket.
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
  punishes ancient history. Mitigation: configurable window, split
  pre/post-nomination reporting, and an open question below with a proposed
  default.
- **Cost surprise.** Layer B spends real inference. Mitigation: the gate keeps
  quiet pages free; the ambiguous-middle share is tracked (improvement loop), and
  the CLI reports when and why Layer B ran.

## Open questions

1. **Evidence window.** Proposed: 90 days of history before the nomination
   timestamp, with post-nomination activity reported separately (exempt bucket).
   React to the 90-day default.
2. **Placement of the mechanisms.** Proposed: the page-history fetch edge and the
   revert-chain reducer are platform (reuse-by-design — patrol plausibly wants
   both); the triage policy, panel prompting, and `StabilitySignal` report are
   assessment-domain. To be pinned by the spawned ADR.
3. **Categorical vocabulary.** Proposed: the five classes above. Is
   `ReviewDrivenChurn` worth a class of its own, or is the pre/post-nomination
   split in Layer A sufficient?
4. **Panel evidence packaging.** Proposed: summaries + chain diffs + matched talk
   threads as one evidence pool per article. Does the panel see one bundle
   (cheaper, more context) or one call per revert chain (more focused, more
   calls)? Benchmark under PRD-0007 before freezing.
