# PRD-0012: Citation insertion for unsourced claims

**Drafter:** Claude Code (Opus 4.8)
**Editor:** Luis Villa
**Date:** 2026-06-23
**State:** Discussion
**Discussion:** <pending — raise with schiste>
**Spawned ADRs:** prose-anchored insertion (extends ADR-0003 from
replace/modify to *insert-at-position*) — number TBD. Reuses the
operator-confirmed-proposal contract (ADR-0010) and the citation-grounding
contract (ADR-0006–0009). The atomize → ground → roll-up semantics extend the
verdict layer those ADRs govern.

## Changelog

- 2026-07-01: State `Draft` → `Discussion` — committed and circulated (was
  sitting as an uncommitted local draft). Not moved to `Accepted`: the six
  items in Open questions below are still stated as *proposed* answers, not
  resolved ones, and the section's own text says implementation doesn't
  proceed until they settle. PRD-0014 (browser surface for this flow, plus
  PRD-0008's repair flow) spawned, depending on this PRD's insert path
  landing.

## Summary

Wikipedia requires claims to be backed by sources, but when a reviewer spots a
sentence with no citation, fixing it means leaving SP42: find a source, judge
whether it really supports the claim, and hand-place a reference without breaking
the article. This adds a command that does the safe parts of that. Given a
sentence and a candidate source, SP42 checks whether the source actually supports
the claim — splitting a compound sentence into its separate facts and verifying
each one — and, only when the source supports all of them, offers a filled
citation the reviewer can confirm, inserted at exactly the right place. The
reviewer approves every edit; SP42 never invents a source, never searches for one,
and never writes on its own. When it can't confirm support, it says so and leaves
the claim flagged rather than attaching a citation that would mislead.

## Scope boundary

This PRD owns the **propose → review → confirm → insert** flow for adding a
citation to a claim that has **no reference today** — the case PRD-0008 (bare-URL
repair) does not reach, because its mechanism only rewrites a reference that
already exists. Where 0008 fills an empty `<ref>`, this inserts a new one.

**MVP wiki scope: `test.wikipedia.org` only**, driven from the **SP42 command
line**, mirroring PRD-0008. Production wikis are not enabled by this PRD's closing
PR.

It deliberately excludes:

- **Finding the source.** SP42 does **not** retrieve or recommend a source URL.
  The candidate source is supplied to SP42 (operator-typed in the MVP; a skill's
  inner-loop retrieval later), exactly as Citoid supplies metadata in PRD-0008.
  SP42 grounds, validates, places, and commits — it does not author the claim or
  hunt the web for evidence. (See Alternatives: *SP42 as retrieval engine*.)
- **Discovering unsourced claims at scale.** Queue ranking and cross-article
  surfacing of `{{citation needed}}` / unsourced sentences are the citation
  review queue (issue #26, future). This PRD covers claims in the revision the
  operator is already reviewing.
- **How the edit is carried out below the insert point.** Node addressing,
  anti-drift anchoring, and lossless re-serialization are ADR-0003. This PRD
  relies on that mechanism and on its **insertion extension** (spawned ADR); it
  does not re-specify serialization.
- **Clause-level attachment.** The MVP attaches one `<ref>` at the end of the
  anchored sentence. Citing one clause of a multi-claim sentence individually is
  deferred (see Alternatives).
- **Identity and capability gating** — PRD-0005; this disposition follows the
  same gating as the inline-edit and bare-URL dispositions (PRD-0004/0008).

## Problem

A patroller reviewing a revision finds a claim that nothing backs — a bare
sentence, or one tagged `{{citation needed}}`. Verifiability is policy, so the
fix is a citation, but producing one today means leaving SP42: find a source,
confirm it actually supports the claim, hand-write a `<ref>` with a filled
template, and place it at the right sentence without disturbing the rest of the
article. Two costs make reviewers skip it. The placement is fiddly and
error-prone — a literal edit hits the wrong spot when the sentence recurs. And
the judgment is the hard part: *does this source actually support this claim?* is
a question even experienced editors disagree on, and a citation that doesn't
support its claim is worse than none — it launders an unverifiable statement into
an apparently-sourced one.

This is for the **experienced reviewer/patroller** acting under their own account,
one claim at a time, who already has a candidate source in hand (they found the
page, or a skill proposed it) and needs SP42 to check it and place it safely.

## Proposal

For an unsourced claim in the revision under review and a candidate source URL,
SP42 **proposes a grounded citation the operator can confirm** — and refuses to
propose one it cannot ground.

- The operator identifies the sentence to cite and supplies a candidate source
  URL. SP42 anchors to that **sentence**, located by its text and unique **within
  its section** (the anchor's drift guard); if the sentence is absent or appears
  more than once in the section, SP42 declines rather than guess where to place
  the reference.
- SP42 resolves the sentence into **atomic claims** — one stage that both
  resolves co-reference ("it" → its referent) and splits a compound sentence into
  separately-checkable assertions. This stage is the only judgment-bearing step,
  and it is grounded, not trusted: its output is what the next step checks.
- SP42 fetches the candidate source **read-only** and **grounds each atom against
  it** through the citation-verification layer (ADR-0006–0009): does the source
  support this assertion? The per-atom verdicts roll up deterministically.
- SP42 **proposes the insert only when the source supports every atom** of the
  sentence. A single end-of-sentence `<ref>` asserts the source covers the whole
  sentence; if the source supports only some atoms, that reference would
  **over-attribute** — claim support the source does not give — so SP42 declines
  to propose and tells the operator which atoms are unsupported, leaving the claim
  a finding rather than placing a misleading citation.
- SP42 builds the `<ref>` filled **only** from fetched source metadata (the
  PRD-0008 provenance floor: every field traces to the metadata or the fetch
  event; SP42 authors nothing), and **inserts** it at the anchored sentence —
  node-anchored, with the rest of the article byte-identical (ADR-0003 insertion
  extension).
- The operator sees the sentence **before and after**, the per-atom grounding
  result, and the source, then confirms or dismisses. It is a **proposal, not a
  write**: nothing reaches the wiki without the operator confirming that exact
  insert. On confirm, the edit lands under the operator's own session, rights,
  and summary, replaying the proposal verbatim with a `baserevid` guard; if the
  article drifted, the apply **refuses rather than guesses** (ADR-0010 contract).
- When SP42 cannot ground the claim, it returns a **structured decline, not a
  failure**: the candidate source does not support the atoms (`no-grounded-
  support`), supports only some (`partial-support`), the sentence is too compound
  to source with one reference (`over-compound`), co-reference could not be
  resolved (`claim-not-self-contained`), or the source could not be fetched or
  carried no usable evidence (`retrieval-gap`). Each keeps the claim a finding and
  says why.

In the MVP the operator drives this from the CLI against the test wiki; the
patrol review surface gains the affordance with the production-enablement
follow-on, alongside the inline-edit (PRD-0004) and bare-URL-repair (PRD-0008)
dispositions.

## Proposed CLI surface

Two mutually-exclusive flag-modes over the dev bridge (ADR-0002), following the
house pattern and PRD-0008's preview/execute split:

```text
--cite-preview --title <T> --rev <N> --sentence <S> --source <URL> [--wiki <ID>] [--bridge-base-url <URL>] [--format text|json|markdown]
--cite-execute --title <T> --rev <N> --sentence <S> --source <URL> [--wiki <ID>] [--action-note <summary>] [--bridge-base-url <URL>] [--format text|json|markdown]
```

- `--cite-preview` calls the proposal endpoint and renders `{proposal | declined}`
  — the anchored sentence, the atoms and their per-atom verdicts, the rendered
  `<ref>`, and the before/after. Read-only; no session.
- `--cite-execute` re-fetches the proposal, re-anchors, and replays it verbatim
  through the apply endpoint under the operator's bridge session (bootstrap +
  CSRF). The server's anti-drift re-check and `baserevid` guard refuse on any
  race, zero writes.
- `--sentence` selects the claim by its text; `--source` is the candidate URL.
- Declines render with their reason codes so the operator sees why a claim kept
  its finding.

## Definition of Done

*The items below name planned coverage; the closing PR records exact test ids and
moves this PRD to `Implemented`.*

- [ ] A new `<ref>` is inserted at the end of the addressed sentence with **every
      other byte of the article unchanged**, verified by a fixture round-trip test
      over the ADR-0003 insertion extension.
- [ ] The insert targets the correct sentence when the **same sentence text
      recurs in another section**, and **declines** (`anchor-ambiguous`) when it
      recurs within the same section, verified by fixture tests on both.
- [ ] A candidate source that supports **all** atoms yields a proposal; one that
      supports **only some** yields `partial-support` with the unsupported atoms
      named and **no proposal**, verified by replayed-source grounding tests (no
      live network in tests).
- [ ] A sentence resolving to **≥5 atoms** yields `over-compound` and no proposal,
      verified by an atomizer-output test.
- [ ] Confirming applies **exactly the proposed insert** with the reviewed
      revision's `baserevid`; a drifted anchor refuses with **zero** requests to
      the wiki edit endpoint, verified by mock-wiki write-path and refusal tests.
- [ ] Every field of the rendered `<ref>` traces to fetched metadata or the fetch
      event; an unfetchable or evidence-empty source degrades to `retrieval-gap`
      without blocking other dispositions, verified by sparse-/error-fixture tests.
- [ ] The disposition is offered **only on enabled wikis**; the MVP enables only
      the test wiki, verified by a config-gating test on proposal and apply paths.
- [ ] A first grounded citation is inserted on `test.wikipedia.org` through the
      CLI: the closing PR records the sentence, candidate source, before/after
      wikitext, the per-atom verdicts, and the resulting revision id; the session's
      action history returns the confirmed entry (the live-edit acceptance gate).

## Alternatives

- *Insert without grounding (place any operator-supplied source).* Rejected: the
  whole value over a literal edit is that SP42 checks support; an unguarded insert
  is a placement tool, not a verifiability tool, and over-attribution is exactly
  the harm to prevent.
- *Propose on partial support, attaching the ref to the whole sentence.* Rejected:
  it over-attributes — the citation would claim support the source does not give.
  Decline-to-propose is the honest floor (mirrors PRD-0008's no-usable-title
  decline).
- *Clause-level attachment — cite the one supported clause of a multi-claim
  sentence.* Deferred: it needs sub-sentence anchoring the prior art never built,
  and the all-atoms gate is a safe v1. Revisit once the sentence-level loop is
  proven.
- *SP42 as retrieval engine (find the source itself).* Rejected for scope and
  posture: source retrieval is a whole subsystem (cf. SIDE's dense+sparse index
  over web-scale corpora), and finding evidence is generation/search that belongs
  in the skill's inner loop. SP42 stays the validator and committer; the candidate
  arrives, as metadata does in PRD-0008.
- *Auto-insert high-confidence groundings.* Rejected: violates
  operator-confirms-every-edit (PRD-0004); grounding is fallible and the task is
  genuinely subjective (see Risks).
- *A judge LLM pass to refine the per-atom roll-up.* Rejected: prior measurement
  found a deterministic all/any/mix roll-up matched a judge pass for real latency
  and cost. The roll-up is deterministic and needs no extra model call.
- *Literal substring insertion at the matched sentence.* Rejected: hits the wrong
  occurrence of a repeated sentence and cannot scope placement — the ADR-0003
  failure modes node anchoring exists to eliminate.

## Risks

- **Over-attribution (the headline risk).** A single reference implies whole-
  sentence support. Mitigation: the all-atoms gate — propose only when the source
  supports every atom; partial support declines with the gap named.
- **Grounding is fallible and the task is subjective.** Whether a source supports
  a claim is a judgment experienced editors disagree on (the SIDE study measured
  inter-annotator agreement barely above chance on the fine-grained task).
  Mitigation: the grounding result is a **decision aid surfaced to the operator,
  never an autonomous gate** — the operator confirms every insert, seeing the
  per-atom verdicts and the source. Consistent with PRD-0004/0008 posture.
- **Retrieval/coverage bias.** Grounding can only confirm what is fetchable;
  paywalled, offline, non-English, or multimedia sources read as `retrieval-gap`
  even when the claim is true and well-sourced elsewhere. This shifts bias from
  model memory to what-is-fetchable. Mitigation: surface `retrieval-gap` as a
  distinct, visible outcome — "could not verify," never "not true" — so the
  operator is not nudged to delete a verifiable claim.
- **Atomizer error (under-/over-decomposition, unresolved co-reference).**
  Mitigation: under-decomposition is caught downstream (an un-split conjunction
  fails grounding and declines); unresolved co-reference declines
  (`claim-not-self-contained`); over-compound declines. Decomposition is not
  treated as a silver bullet — it gates conservatively.
- **Anchor drift between proposal and confirm (TOCTOU).** Mitigation: anti-drift
  re-check plus the `baserevid` guard (ADR-0003/0010); refuse and re-propose.
- **Operator habituation.** Mitigation: single-sentence diff, per-atom verdicts
  shown, and conservative declines keep proposal quality high enough that reading
  each one stays cheap.

## Open questions

Each carries a proposed answer to react to; implementation does not proceed until
they settle.

1. **Where does the candidate source come from in the MVP?** Proposed:
   operator-typed `--source` URL — proves the ground/place/commit loop without
   coupling to a retrieval skill. The skill-supplied path is the same endpoint
   with a different caller, added once a skill drives it.
2. **What triggers the flow — does SP42 detect unsourced claims, or does the
   operator point at a sentence?** Proposed: operator points at a sentence
   (`--sentence`) for the MVP; detection of unsourced / `{{citation needed}}`
   claims is the review-queue concern (issue #26), out of scope here.
3. **Is repairing a `{{citation needed}}` tag (replacing the tag with a ref) the
   same disposition or a different one?** Proposed: treat it as **this** flow with
   one addition — when the anchored sentence carries a `{{citation needed}}`,
   confirming the insert also removes that tag (a node *replace*, ADR-0003
   existing primitive, composed with the insert). Flag for reviewer: this couples
   an insert and a replace in one confirmed edit.
4. **What grounding threshold counts as "supported" per atom?** Proposed: reuse
   the citation-verification layer's existing `supported` verdict unchanged — do
   not introduce a separate threshold here; the roll-up is all-`supported`.
5. **Attachment position relative to terminal punctuation and any existing
   trailing refs.** Proposed: insert after terminal punctuation and after any
   existing trailing references on the sentence (`AfterTrailingRefs`), matching
   the dominant on-wiki convention; lock with renderer tests.
6. **Does the insertion primitive extend ADR-0003 or warrant a new ADR?**
   Proposed: a thin ADR that **extends ADR-0003** — same anchor/drift/`baserevid`
   discipline, new operation (insert-at-prose-anchor with a uniqueness guard),
   reusing ADR-0010's propose/confirm contract. Number assigned when drafted.
