# SP42 in the Good-article review process — convergence sketch

**Date:** 2026-07-09
**Status:** Sketch (pre-implementation), composing existing contracts into one
operator workflow
**Governs the *how* for:** no new ADR yet; composes ADR-0010 (propose/confirm),
ADR-0011 (article-level verification), PRD-0009 (book grounding / Open Library
enrichment), PRD-0011 + ADR-0016/0017 (Wikidata), PRD-0015 (stability signal).
User-facing intent for the stability piece is PRD-0015; this sketch names the
end-to-end GA workflow the pieces converge on.

## Why this exists

English Wikipedia's Good-article process is a per-article human review against
[six criteria](https://en.wikipedia.org/wiki/Wikipedia:Good_article_criteria),
run through
[a nomination queue and a review subpage](https://en.wikipedia.org/wiki/Wikipedia:Good_article_instructions)
(`Talk:Article/GAn`), with a required
[spot-check of sources](https://en.wikipedia.org/wiki/Wikipedia:Spot_checking_sources).
Wikipedia's own documentation frames the spot-check as a cost compromise:
checking every citation is "best quality, but prohibitively expensive" for a
human, so reviewers sample. SP42's article-level verification path exists to
invert exactly that trade — check everything, hand the reviewer grounded
evidence, keep judgment human.

Nothing in this sketch is a new mechanism. It is a map of how the shipped and
specified pieces compose into "SP42 assists a GA review," what order to build
the missing connective tissue in, and the posture invariants that bound the
whole thing. The assessment domain (`docs/domains/assessment/`) is its home.

## Criterion coverage map

| GA criterion | SP42 role | Substrate |
| --- | --- | --- |
| 1a prose quality | none (human) | — |
| 1b MoS (5 named pages) | lint-style *candidate flags* only | future; `article_inventory` is adjacent |
| 2a references section/layout | deterministic structure checks | future, same lint lane |
| 2b inline support | **core**: per-citation verdicts, grounded quotes | ADR-0011 `verify-page`; PRD-0009 Layers 1–2 for books |
| 2b source reliability | none yet (roadmap `assess_reliability`) | PRD-0010 roadmap |
| 2c original research | none (human) | — |
| 2d copyvio / plagiarism | *extension*: claim↔source overlap signal | future; verifier already holds both texts |
| 3 broad coverage | none (human); Wikidata/sitelink *context* at most, later | ADR-0016 read module, speculative |
| 4 neutrality | none (human; PRD-0010 records the no-NPOV-verdict posture) | — |
| 5 stability | evidence + gated interpretation | **PRD-0015** |
| 6a media licensing | deterministic license/rationale relay | future PRD; `media_diff` extraction + an `imageinfo` fetch edge |
| 6b media relevance/captions | caption *presence* only; suitability human | same future PRD |

The human-only rows are load-bearing: SP42 emits no prose, OR, breadth, or
neutrality verdicts, and no criterion-level pass/fail anywhere. The report is
evidence *for* a review, not a review.

## The workflow

1. **Intake.** The operator points SP42 at a nomination (article + the pinned
   revision under review; the `{{GA nominee}}` timestamp comes from the talk
   page). Ingesting the GAN backlog as a browsable queue — the patrol queue
   machinery minus scoring — is a later convenience, not MVP.
2. **Evidence run** (read-only, one report):
   - *Citations:* `verify-page` over the pinned revision (ADR-0011) — verdict +
     located quote per use-site, dead-vs-unusable split, archive fallbacks,
     skips first-class. Book citations flow through PRD-0009 resolve/ground.
   - *Stability:* the PRD-0015 two-layer signal, keyed to the nomination
     timestamp.
   - *(as they land)* media-licensing relay (6a) and the MoS/layout lint flags.
3. **GA-shaped rendering.** The report renders as a wikitext evidence appendix
   organized by criterion number — the shape a reviewer pastes into
   `Talk:Article/GAn`, findings addressable by `ref_id` — alongside the
   existing structured/CLI rendering. Wording stays evidential ("12 of 14 URL
   citations verified supported; 2 dead links: …"), never "criterion 2: pass."
4. **Posting (optional), via ADR-0010.** The review subpage is an ordinary
   MediaWiki page, so posting the appendix is a propose/confirm apply under the
   operator's own account — preview shown, exact-text confirm, refuse-on-drift.
   SP42 never posts autonomously and never signs an assessment.
5. **The hold loop.** GA reviews usually hold ~7 days for fixes. The re-run
   against the post-fix revision should be incremental: ADR-0011's committed
   "re-verify one use-site" fast-follow means only touched citations re-verify,
   and the second report diffs against the first ("3 previously-dead links now
   archived; 1 not_supported unchanged"). Design the report contract with
   run-to-run comparability in mind from the start.
6. **Harvest lanes** — the generative half, at the moment the sources are
   freshest:
   - *Open Library:* thin records met during book resolution yield PRD-0009
     Layer 3 field-level enrichment proposals (operator-confirmed,
     proposal-only until the OL apply-contract ADR lands).
   - *Wikidata, citation→facts:* claims the run just verified `supported` are
     the best-qualified feed for referenced-statement proposals on the subject's
     item (ADR-0017 discipline; FRBR edition guard; operator-confirmed).
   - *Wikidata, on promotion:* a passed GA is recorded as a "good article"
     badge on the enwiki sitelink — a small, well-defined entity edit SP42 can
     propose (ADR-0016/0017 machinery) as the closing step. Proposal-only, like
     everything else.

## Posture invariants (bounding the whole feature)

- **Evidence, not verdicts.** No criterion pass/fail, no NPOV/OR/breadth
  judgment, categorical vocabularies only where interpretation runs (PRD-0015).
- **Grounded or absent.** Anything interpretive cites verbatim-locatable
  evidence (ADR-0007 discipline) or abstains.
- **Honesty arms are first-class.** Skips, extraction failures, truncation, and
  "Layer B ran here" markers render in the report, never a footnote.
- **Every write is ADR-0010-shaped.** Talk-page post, OL enrichment, Wikidata
  statement, badge: propose → operator confirms exact change → refuse-on-drift,
  under the operator's own identity.
- **Quiet is free.** Deterministic layers gate inference; a clean nomination
  spends near-zero model budget outside citation verification itself.

## Sequencing

1. **GA report renderer** over the existing `PageVerificationReport` — pure
   composition, no new fetches; immediately useful pasted by hand. Smallest
   honest MVP.
2. **PRD-0015 stability signal** (Layer A first, Layer B behind it).
3. **Media-licensing relay (6a)** — one `imageinfo`/file-page fetch edge plus
   deterministic checks; its own thin PRD.
4. **Hold-loop incremental re-run** — rides ADR-0011's single-use-site
   fast-follow.
5. **ADR-0010 posting path for the appendix**, then the harvest-lane hooks as
   PRD-0009 Layer 3 and ADR-0017 land.
6. **MoS/layout lint lane and claim↔source overlap (2d) signal** — opportunistic,
   each trivially bounded.

Deliberately unsequenced: `assess_reliability` (PRD-0010 roadmap), breadth
context via sitelinks (speculative), GAN queue ingestion (convenience).

## Non-goals

- Assessing prose quality, original research, breadth, or neutrality.
- Auto-posting reviews, auto-passing/failing nominations, or acting as the
  reviewer of record. The GA process requires a human reviewer; SP42 is the
  reviewer's workbench, and the one place that must stay true even as coverage
  grows.
- DYK/FA processes. The FA criteria are stricter and the DYK checks different;
  both are plausible later consumers of the same report arms, and nothing here
  may hard-code GA specifics into the platform layers to foreclose that.
