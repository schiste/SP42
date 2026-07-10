# PRD-0009: Mobile-first dataset-validation UI

**Author:** Luis Villa (drafted with Claude)
**Date:** 2026-07-10
**State:** Draft
**Discussion:** (pending)
**Spawned ADRs:** none yet — the review-queue service seam (where candidate cases
are served from and where human verdicts are written, kept off the read-only
corpus loader), the reviewer-identity binding (dev-auth bridge now, Wikimedia
OAuth later), and the evidence-tier authorization boundary (who may see the
fair-use full body vs. a link only) are structural decisions to spawn before
implementation, not questions this PRD answers.

## Problem

PRD-0007 gives SP42 a benchmarking corpus and the machinery to grow it cheaply:
Featured Articles are a distant-supervision mine of presumed positives,
`{{Failed verification}}` and friends a mine of candidate negatives. But PRD-0007
is emphatic that distant-supervision labels are **heuristic-assessed candidates,
not ground truth** — "heuristic-assessed (model-labeled) facets may never feed a
gate," and the corpus loader is read-only by construction. A harvested label only
becomes gate-eligible ground truth when a **human** signs off on it.

Today there is no mechanism for that sign-off. The corpus can be *grown*
automatically and *measured against* deterministically, but the audit step in the
middle — a person looking at a `(claim, source)` pair and confirming or
correcting its label — has no home. Doing it in a spreadsheet throws away
everything the citation backend already computed (the located quote, the excerpt,
the panel's read) and puts the reviewer at a desktop, which is the wrong place to
reach the volunteer editors who are the natural labelers.

## Proposal

A **mobile-first review surface** that turns each harvested case into a
one-decision-per-screen card and records a human verdict against it. It is
deliberately the **inverse of the existing citation surface**: `verify_page` /
`execute_citation_verify` already emit, per use-site, a `CitationFinding` that
bundles exactly what an auditor needs — the `claim` and `preceding_context`, the
panel `verdict` and *measured* `agreement`, the located `passage` and bounded
`source_excerpt` ("what the panel read"), `provenance`, and Citoid `metadata`.
The app already renders this as `FindingCard` inside `CitationSurface`. The review
surface re-skins that card for a phone and adds a verdict-capture tap.

The cases are pre-run through the pipeline in **frozen/replay mode** (PRD-0007
source mode 1), so the reviewer adjudicates a finished analysis rather than
waiting on a live fetch — which is what makes the task a ten-second tap on a
phone instead of ten minutes of research. The human verdict is written to a store
**separate from the read-only corpus**, appended with provenance
(`label_method = human_audit`, `label_as_of`, reviewer id) — never mutating the
harvested label (PRD-0007 "GT audit flag, never GT mutation").

### The interaction

One card, one question. Claim and context up top; below it the source excerpt
with the located quote highlighted; then the candidate label and the panel
verdict + agreement. The reviewer answers *does the highlighted passage support
this claim?* with one of the task's four outcomes — Supported / Partial / Not
supported / Can't-tell (`source_unavailable`) — swipe to skip. The tap targets
**are** the `Verdict` enum; there is no free-text and no confidence slider (the
no-confidence-anywhere rule of PRD-0007 applies to this surface too — the only
signals shown are the categorical verdict and the *measured* agreement).

Triage puts the highest-value cases first: the panel-vs-candidate **disagreement
list** (already an emitted PRD-0007 audit artifact) is the front of the queue;
high-agreement model-confirms-the-harvest cases are sampled rather than fully
reviewed.

### Evidence tiers (the fair-use line drives the design)

The bounded `source_excerpt` confirms an obvious match, but the cases that need a
human are exactly the ones where the excerpt is not enough — a clipped qualifying
clause, or a suspected wrong-page scrape. "Show more" has two forms separated by
a licensing line: the frozen source bytes are a **fair-use**-bounded payload, so
republishing the full body to an anonymous crowd is not available. Evidence
therefore escalates in three tiers:

1. **Located quote + bounded excerpt** — already on the finding; the default card.
2. **The full extracted body SP42 read** (via the existing `html_to_text`
   extraction) — the right thing when the excerpt clipped context, but the
   fair-use-bounded payload, so gated to **authenticated / trusted** reviewers.
3. **A link out to the original source** — republishes nothing (a pointer, not a
   copy), so it is the clean tier for a **public** reviewer, and the whole point
   for `source_unavailable` cases (a reviewer with institutional access can open
   the paywalled PDF the scraper could not).

The link tier is specified by `evidence_link::build_evidence_links` (this PRD's
first landed slice): its **primary** link points at the exact bytes the panel
read — the raw archive snapshot when the panel read an archive, otherwise the
fetched URL — so human and model adjudicate the **same text** (the parity that
frozen mode exists to preserve); linking to *live* would silently let the human
judge changed text. When the panel read an archive, the original live URL is
offered only as an explicitly-labelled secondary ("may have changed / may be
dead"). "Mobile-friendly" means the primary link carries a `#:~:text=` fragment
built from the located quote, so the browser scrolls straight to the sentence
instead of dropping the reviewer at the top of a long article.

One honesty flag: if a reviewer forms their verdict against a Tier-3 link (more
than the panel read), that is recorded on the audit label — a stronger label, but
no longer judging the identical input, which matters when reconciling human vs.
model.

### Promotion to ground truth

The human tap is what converts a candidate into gate-eligible GT, so a single tap
is not enough: a label is trusted only after **N-of-M reviewer agreement**, and
inter-rater disagreement is itself a re-audit signal rather than being
auto-resolved. Anti-fabrication parity (ADR-0007 §5) carries to the human: a
Supported / Partial tap should confirm or select the locating span — the human
may not assert support without pointing at the quote any more than the panel may
— with the pre-located `passage` offered as the one-tap default. `source_unavailable`
is a distinct lane: the reviewer is asked "is this genuinely unfetchable, or did
the harvest grab the wrong thing?", not to judge support on a source they also
cannot read.

## Definition of Done

- [ ] A review-queue endpoint serves pre-computed frozen-mode findings as cards,
      seeded from the panel-vs-candidate disagreement list first — verified by a
      queue-ordering test.
- [ ] A capture endpoint records a human verdict as one of the four task outcomes
      with `label_method = human_audit`, `label_as_of`, and reviewer id, to a
      store **disjoint from the corpus**; no code path writes the corpus —
      verified by a test plus the loader being read-only by construction.
- [ ] A Supported / Partial capture requires an evidence locator (parity with
      ADR-0007 §5) — verified by a capture-validation test rejecting a
      support tap with no span.
- [ ] No confidence value appears in the card payload or the capture record —
      verified by a structural test (consistent with PRD-0007).
- [ ] A label is promoted to gate-eligible GT only on N-of-M reviewer agreement;
      disagreement is surfaced for re-audit, never auto-resolved — verified by a
      promotion-logic unit test.
- [ ] `build_evidence_links` returns a primary link matching the bytes the panel
      read (raw snapshot for archives), deep-linked to the quote, with the live
      original offered only as a labelled secondary — **done** (landed with this
      PRD; unit-tested in `evidence_link`).
- [ ] The full extracted body (Tier 2) is reachable only by an authenticated
      reviewer; a public reviewer gets Tiers 1 and 3 only — verified by an
      authorization test.
- [ ] A verdict formed against a Tier-3 link is flagged as such on the audit
      record — verified by a capture-record test.
- [ ] The surface renders and captures on a phone through the existing PWA shell
      (Phase 5) reusing the `FindingCard` presentation — verified by the app
      crate's wasm-gated view test.

## Alternatives

- **Spreadsheet / off-repo labeling.** The status quo for the alex corpus:
  discards the located quote, excerpt, and panel read the backend already
  computed, and reaches no volunteers. Rejected — it rebuilds the reviewer's
  context by hand for every row.
- **Desktop-only review in the existing `CitationSurface`.** Reuses the most
  code, but the labelers we want are on phones; a desktop-only surface caps the
  throughput the whole corpus-growth plan depends on.
- **Serve the full source body to everyone.** Simplest evidence model, but
  republishes fair-use-bounded bytes to an anonymous crowd. Rejected in favor of
  the tiered model, where the link tier carries the public path and the full body
  stays gated.
- **Let the human overwrite the corpus label directly.** Rejected by PRD-0007's
  standing rule: models (and now humans) flag for audit; the corpus is written
  only through its adoption gate, and the human verdict lives in a separate audit
  store that promotion logic reconciles.

## Risks

- **Crowd quality.** Drive-by or adversarial taps. Mitigation: honeypot/gold
  cases with known labels seeded into the queue to score reviewers, and the
  N-of-M promotion gate bounds single-rater error.
- **Anchoring bias.** Showing the panel verdict biases the human toward agreeing.
  Mitigation: consider withholding the panel verdict until after the human
  commits, and at minimum record whether the reviewer saw it so the anchoring is
  measurable.
- **Parity leak.** A reviewer judging the live page instead of the frozen bytes
  grades different text. Mitigation: the primary link is the panel's bytes by
  construction; the live original is a labelled secondary; a Tier-3 verdict is
  flagged.
- **Fair-use exposure.** Tier 2 is bounded and authenticated-only; a public
  deployment must not fall back to it. Mitigation: the authorization boundary is
  a spawned-ADR decision, not an implementation detail.
- **Misreading validated labels as faithfulness.** A human-audited label is
  better ground truth, but still grades verdict-vs-label and evidence existence,
  not the model's reasoning (ADR-0007 §5). Mitigation: the epistemic note rides
  the report, as in PRD-0007.

## Open questions

1. **Reviewer identity venue** — dev-auth bridge suffices for a closed pilot;
   public volunteer review wants Wikimedia OAuth (Phase 4). Sequencing tracked
   with the spawned ADR, not resolved here.
2. **N and M** — the promotion threshold (how many concurring reviewers, and the
   disagreement-escalation rule) is a tuning judgment to set against pilot data,
   not a design constant.
