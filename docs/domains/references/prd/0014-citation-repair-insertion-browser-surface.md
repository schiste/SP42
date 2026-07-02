# PRD-0014: Citation repair and insertion — browser surface

**Drafter:** Claude Code (Sonnet 5)
**Editor:** Luis Villa
**Date:** 2026-07-01
**State:** Draft
**Discussion:** <PR link TBD>
**Spawned ADRs:** none. Reuses ADR-0003 (node-anchored editing — specifically
`InlineEdit`'s literal-fallback path, since `WikitextNodeKind` covers
`Reference`/`Template` only, not prose) and ADR-0010 (operator-confirmed
proposals) unchanged. Depends on PRD-0008 (repair, `Accepted`) and PRD-0012
(insert, `Discussion`) reaching the browser rather than redefining either.

## Summary

The Citations tab (PR #81) shows verification findings but can't act on them —
every citation write today is CLI-only. This adds a per-finding action row to
that tab for `Partial`/`NotSupported` findings, offering two paths: fix the
citation (PRD-0008/0012's existing propose/confirm flows) or fix the article
text (the existing `InlineEdit` action already used by the patrol rail). A
mismatch between a source and the claim it backs can mean either side is wrong
— a source that says "7%" against article text that says "6%" is usually a
text problem, not a citation problem — and SP42 has no way to know which. So
it doesn't guess: both options are shown with equal weight, the source's
located passage and the article's claim text rendered side by side, and the
operator picks.

## Scope boundary

This PRD owns the **browser presentation and routing** layer only: which
action(s) the Citations tab offers per finding, and how the operator chooses
between them. It does not redefine:

- **Repair mechanics** (PRD-0008) or **insertion mechanics** (PRD-0012) — this
  PRD is a new caller of both flows' existing propose/confirm endpoints, the
  same way their CLI surfaces are, not a new implementation of either.
- **The verification engine or its verdicts** (ADR-0006/0007) — findings,
  `claim` text, and `ref_id` are consumed as `CitationFinding` already
  produces them today.
- **Text-edit mechanics** — `InlineEdit` (`action_routes.rs`) already does
  arbitrary article-text replacement with an anti-drift literal-fallback
  guard; this PRD is a new *call site* for it (the Citations tab), not a
  change to it.
- **The confirm/apply UX** — both paths terminate in the existing ADR-0010
  propose/preview/confirm component. No new confirmation UI is built.

**MVP wiki scope: `test.wikipedia.org` only**, mirroring PRD-0008/0012.
Production wikis are not enabled by this PRD's closing PR.

It deliberately excludes:

- **Any model judgment about which side is wrong.** SP42 never classifies a
  mismatch as "probably the text" or "probably the citation" — see Resolved
  questions and Alternatives. This is a hard requirement, not a v1
  simplification: it follows directly from the project's existing
  abstention-biased, anti-fabrication posture (PRD-0010 Risks; PRD-0012
  Alternatives' rejection of auto-insert on high-confidence grounding).
- **A CLI surface.** Both underlying actions already have one (PRD-0008/0012);
  this PRD adds no new CLI surface of its own.
- **Detecting unsourced claims or surfacing findings at scale** — the citation
  review queue (issue #26), unchanged from PRD-0008/0012's own exclusions.

## Problem

A patroller reviewing the Citations tab sees a `Partial` or `NotSupported`
finding and cannot act on it — every write path (bare-URL repair, and once it
lands, insertion) exists only via CLI (`crates/sp42-server/src/citation_routes.rs`),
never wired to the tab that renders the report (`crates/sp42-app/src/pages/citation.rs`,
which is pure report-rendering — `CitationSurface`, `PageReportView`,
`FindingCard` — no propose/confirm/apply component exists there today).

Worse, even a citation-repair affordance alone would be the wrong tool some of
the time. A `Partial`/`NotSupported` verdict can mean the citation is stale or
wrong (PRD-0008/0012's territory), or it can mean the *article text* drifted
from a citation that's actually fine — a number was vandalized, mistyped, or
updated in the source but not the article. SP42's verdict tells you the two
disagree; it says nothing about which one to trust. A UI that only offers
"fix the citation" would actively steer the operator toward editing the
wrong thing in exactly the cases where the citation was right all along.

## Proposal

For every `Partial`/`NotSupported` finding, the Citations tab shows the
source's located passage and the article's claim text (`finding.passage`,
`finding.claim` — both already present on `CitationFinding`,
`crates/sp42-citation/src/citation/verify.rs`) side by side, with two
equal-weight actions beneath:

- **Edit article text** → opens the existing `InlineEdit` propose flow,
  pre-filled with `selected_text = finding.claim`. The operator authors the
  replacement text themselves; SP42 does not suggest one (consistent with SP42
  never authoring claim content anywhere else in the citation domain).
  Re-anchoring at apply time goes through `InlineEdit`'s literal-fallback path
  (`WikitextNodeKind` has no prose variant — `Reference`/`Template` only,
  ADR-0003) with its existing exactly-one-occurrence guard: if
  `finding.claim` isn't unique in the article, the edit refuses rather than
  guessing which occurrence to change, the same behavior every other
  literal-fallback edit already has.
- **Fix citation** → routes to PRD-0008's replace flow when `finding.ref_id`
  is non-empty (an existing-but-wrong citation), or to PRD-0012's insert flow
  when it's empty (the finding came from an unsourced-claim scan with no ref
  to replace).

Both terminate in the existing ADR-0010 propose/preview/confirm component —
before/after diff, explicit confirm, `baserevid`-guarded apply that refuses on
drift. Neither action is pre-selected, defaulted, or shown with more visual
weight than the other; there is no confidence score, hint, or badge
suggesting which one to pick. `Supported` and `SourceUnavailable` findings are
unaffected — they stay read-only, as today.

## Definition of Done

- [ ] An action row renders only for `Partial`/`NotSupported` findings;
      `Supported`/`SourceUnavailable` findings remain read-only, verified by a
      component test over all four verdict fixtures.
- [ ] The source's located passage and the article's claim text render side by
      side above the action row whenever it's shown, verified by a rendering
      test.
- [ ] "Edit article text" opens `InlineEdit` pre-filled with `finding.claim` as
      `selected_text` and an empty operator-authored `replacement_text`,
      verified end-to-end against the stub editor.
- [ ] "Fix citation" routes to the replace action when `finding.ref_id` is
      non-empty and to the insert action when it is empty, verified by tests
      over both finding shapes.
- [ ] A claim sentence occurring more than once in the article refuses the
      text-edit path rather than guessing, verified by a fixture test — the
      same guard `InlineEdit`'s literal fallback already enforces elsewhere,
      exercised from this new call site.
- [ ] Both actions terminate in the existing ADR-0010 propose/preview/confirm
      component with no new confirm UI added, verified by review (no new
      confirm-dialog component introduced in the diff).
- [ ] Neither action is pre-selected, defaulted, or rendered with more visual
      weight than the other, and no confidence/hint is shown alongside them,
      verified by a snapshot/parity test asserting both buttons share the same
      component and styling.
- [ ] The action row is offered **only on enabled wikis**; the MVP enables
      only `test.wikipedia.org`, verified by the same config-gating tests
      PRD-0008/0012 use.

## Alternatives

- *Model-assisted triage hint (suggest which side is likely wrong).*
  Rejected: raised and explicitly declined during design — "for now we always
  make the human pick." It would also introduce a genuinely new and much
  squishier judgment than the existing support/contradict verdict (who's
  wrong, the encyclopedia or the source), cutting against the project's
  abstention-biased, anti-fabrication posture (PRD-0010 Risks; PRD-0012
  Alternatives). Revisit only as a deliberate, separately-scoped PRD with its
  own Risks treatment, not folded into this one.
- *Citation-only v1, defer the text-edit path.* Considered during design, not
  chosen: `InlineEdit` and its browser call-site pattern already exist
  (patrol rail, `revision_artifacts.rs`), so exposing it here is routing, not
  new engineering — there's no real cost saved by deferring it, and shipping
  citation-only would repeat the "actively wrong tool sometimes" problem this
  PRD exists to avoid.
- *A single "fix" button that internally decides which flow to invoke.*
  Rejected: this reintroduces triage by the back door — whichever action the
  button chooses to fire *is* the triage judgment, just hidden. Two explicit,
  equal-weight affordances are the only way to keep the choice with the
  operator.

## Risks

- **Operator confusion about which button to press.** Mitigation: the source
  passage and article claim render side by side above the action row, so the
  disagreement (and which side looks wrong) is visible before choosing, not
  inferred from a label.
- **Text-edit refuses more often than expected (non-unique sentences).** Not a
  new risk — the same guard exists everywhere `InlineEdit`'s literal fallback
  runs — but this is a new, higher-traffic call site for it. Mitigation: none
  needed beyond existing behavior; refuse-not-guess is the correct outcome,
  consistent with every other write path in the domain.
- **PRD-0012 dependency.** The insert half of "Fix citation" has no mechanism
  to route to until PRD-0012's open questions settle and it ships. Mitigation:
  ship the repair half first — it depends only on `Accepted` PRD-0008, which
  is already implemented — mirroring PRD-0013's identical sequencing call for
  the MCP surface (its replace path ships ahead of its add path for the same
  reason).
- **Scope creep into repair/insertion mechanics.** Mitigation: explicit
  non-goals in Scope boundary; any change to *how* a repair or insert is
  computed belongs in PRD-0008/0012, not here.

## Open questions

1. **Does "Fix citation" ship for both findings shapes at once, or repair
   first?** Proposed: repair first (see Risks) — PRD-0008 is `Accepted` and
   implemented; insert is blocked on PRD-0012. The action row still renders
   for both finding shapes, but "Fix citation" on an unsourced-claim finding
   errors informatively until PRD-0012 lands, rather than being hidden.
2. **What happens to a finding after the operator commits one action?**
   Proposed: it stays rendered as its original verdict until the next
   `verify-page` run — this PRD does not add optimistic client-side
   verdict-clearing. Revisit if that reads as stale in practice.
3. **Does `InlineEdit`'s pre-fill need more than the raw claim sentence** (e.g.
   highlighting the specific span that disagrees with the source, like the
   number in "6%")? Proposed: no — ship with the whole-sentence pre-fill;
   sub-sentence highlighting is a refinement, not a blocker, and the operator
   already sees the source passage to compare against.
