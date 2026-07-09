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
   PRD-0014's framing carries over verbatim: a `NotSupported` finding means the
   claim and source *disagree*, not that the citation is the wrong side — the
   appendix must not phrase mismatches as citation failures, or it steers
   nominators toward fixing citations when the article text is what drifted.
4. **Posting (optional), under ADR-0010's discipline.** The review subpage is
   an ordinary MediaWiki page, but appending an evidence section is an
   **insert-at-position**, not a node replacement — ADR-0010's shipped
   mechanism does replace/modify only. Posting therefore depends on the
   ADR-0003 insertion extension (spawned by PRD-0012; generalized as the
   guarded-edit pipeline's `Insert` op in the PRD-0013 design plan), and — for
   the CLI/MCP shells this sketch assumes — on ADR-0018's `WikimediaTokenSource`
   seam for edit authority outside a browser session. Same confirm posture
   regardless: preview shown, exact-text confirm, refuse-on-drift; SP42 never
   posts autonomously and never signs an assessment.
5. **The hold loop.** GA reviews usually hold ~7 days for fixes. The re-run
   against the post-fix revision should be incremental: PRD-0014's Re-verify
   route (the single-use-site re-check ADR-0011 committed to as a fast-follow;
   implemented on the PR #109 branch) means only touched citations re-verify,
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

## In-flight dependencies (as of 2026-07-09)

Three citation PRDs and two Wikidata PRs are open but not merged; each unlocks
a piece above when it lands, and none blocks the first sequencing step:

- **PRD-0014 / PR #109** (browser action row + Re-verify) — satisfies the hold
  loop's re-verify dependency, and makes GA findings actionable in the
  Citations tab (fix citation / fix text / flag with `{{Failed verification}}`
  — the flag templates are a second, in-article channel for GA findings that
  outlives the review page).
- **PRD-0012** (citation insertion, Discussion) — the repair path for the
  unsourced-claim class of criterion-2b findings during the hold; its spawned
  ADR-0003 insertion extension is what the appendix-posting step anchors on.
  Its atomizer (compound sentence → co-reference-resolved atoms) is the
  "Molecular Facts" decomposition ADR-0011 deferred — a future quality
  tailwind for verify-page on GA-caliber prose, not a dependency.
- **PRD-0013 + ADR-0018 / PR #106** (MCP write surface + token seam) — the
  guarded-edit pipeline and non-server edit authority the posting step needs
  from CLI/MCP shells; also makes the whole GA evidence run drivable by an
  external agent over MCP.
- **ADR-0016 / PRs #119–#120** (Wikidata entity read path) — the read module
  the harvest lanes and the promotion-badge proposal build on.

## External-ecosystem notes (fuzheado/Wikipedia-AI-Skills, surveyed 2026-07-09)

The 47-skill [Wikipedia-AI-Skills](https://github.com/fuzheado/Wikipedia-AI-Skills)
repo PRD-0010 cites was surveyed for GA prior art. Findings:

- **No GA/DYK/FA review skill exists** — the assessment domain is greenfield.
  The likeliest first external consumer of a GA-assist MCP surface now has a
  concrete shape: a fuzheado-style skill encoding the GA *process* (review-page
  mechanics, templates, hold etiquette) that calls SP42 for the grounded
  *judgment* — their `wikipedia-reference-verifiability` skill checks only
  citation URL-presence, exactly the support-verification gap PRD-0010 names.
- **LiftWing `articlequality` is a candidate informational pre-screen.** Their
  `wikimedia-ml-services` skill points at the article-quality model family
  (predicts Stub→FA class; a reference-quality sibling exists). Unlike
  revertrisk-on-Wikidata (gated off as unfit, PRD-0011), this model is
  Wikipedia-trained *for* article-level quality, so surfacing its class
  estimate as nomination triage — a score, never a verdict — fits the shipped
  LiftWing posture in patrol. Candidate report arm, unsequenced.
- **Their `wikipedia-en-article-audit` skill was studied in depth**; takeaways:
  - *The PRD-0010 verdict mapping lands on a real schema.* Their sentence
    verdicts are `confirmed / contradicted / npov_or / unverifiable / mixed` —
    PRD-0010's documented mapping (`Supported→confirmed`,
    `NotSupported→contradicted|unverifiable`, `Partial→mixed`) fits cleanly,
    and `npov_or` is precisely the verdict SP42 refuses to emit. Any
    machine-readable GA-report arm should ship SP42's own vocabulary plus that
    mapping, never a fabricated NPOV verdict.
  - *A hold is a work order; their task-graph pattern fits it.* They compile
    findings into a prioritized, dependency-aware task DAG (p0 factual, p1
    structural/citation, p2 polish; citation tasks depend on their sentence's
    rewrite; assessment updates only after substantive fixes — the same
    insight as our re-verify-after-fixes loop). A future machine-readable
    appendix arm could emit findings as such a work order for the nominator —
    with the SP42 twist that tasks reference findings and PRD-0014 repair
    affordances, never SP42-authored `newText` (their tasks carry authored
    replacement text; ours must not).
  - *BLP spotlight at intake.* Their pipeline screens BLP articles first and
    applies a stricter regime. A cheap BLP-applicability flag on the GA report
    (P31/P570 via the ADR-0016 read module — the `is_blp` check PRD-0010 keeps
    on its roadmap) would let the appendix mark BLP nominations for the
    reviewer's heightened sourcing scrutiny.
  - *Derived artifacts, one direction.* Their human-readable `analysis.md` is
    generated from the structured outputs and never hand-edited — independent
    convergence on the report-is-the-contract discipline: the wikitext
    appendix is a pure renderer output over `PageVerificationReport` +
    `StabilitySignal`, never a source of truth.
  - *Independent convergence on audit/edit separation.* Their hard rule — "one
    agent audits, a different agent edits… with explicit user confirmation" —
    is ADR-0010/ADR-0011's read-only-report + operator-confirm split arrived
    at independently; useful external validation that the posture matches how
    the agent-builder ecosystem already wants to consume this.

## Non-goals

- Assessing prose quality, original research, breadth, or neutrality.
- Auto-posting reviews, auto-passing/failing nominations, or acting as the
  reviewer of record. The GA process requires a human reviewer; SP42 is the
  reviewer's workbench, and the one place that must stay true even as coverage
  grows.
- DYK/FA processes. The FA criteria are stricter and the DYK checks different;
  both are plausible later consumers of the same report arms, and nothing here
  may hard-code GA specifics into the platform layers to foreclose that.
