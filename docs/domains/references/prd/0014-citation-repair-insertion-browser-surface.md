# PRD-0014: Citation repair and insertion — browser surface

**Drafter:** Claude Code (Sonnet 5)
**Editor:** Luis Villa
**Date:** 2026-07-01
**State:** Accepted
**Discussion:** <PR link TBD>
**Spawned ADRs:** none of its own. Reuses ADR-0003 (node-anchored editing —
specifically `InlineEdit`'s literal-fallback path, since `WikitextNodeKind`
covers `Reference`/`Template` only, not prose) and ADR-0010
(operator-confirmed proposals) unchanged. The `NotSupported` flag path's
insert-after-`<ref>` anchor rides on PRD-0012's spawned ADR-0003 extension
rather than spawning a second one (Resolved question #4). Depends on PRD-0008
(repair, `Accepted`) and PRD-0012 (insert, `Discussion`) reaching the browser
rather than redefining either.

## Changelog

- 2026-07-09: Amend the Re-verify contract to *cached page re-verification*
  (new "Amendment (2026-07-09)" section) — Re-verify refreshes the whole page to
  the current revision instead of folding one finding into a stale-revision
  report, removing the mixed-revision display seam. Depends on **ADR-0019**
  (in-memory verdict cache); the one-finding fold-and-regroup remains the interim
  implementation with the mixed-revision display as a known limitation.
- 2026-07-02: First implementation pass (this branch): the `CitationConcernKind`
  flag-citation mechanism, the Re-verify route, and the browser action row all
  land, with unit tests on the routing/gating logic. Not done: the
  `FailedVerification` insert-after-`<ref>` primitive (refuses informatively,
  as scoped), the live-edit acceptance gate (needs an authenticated
  `test.wikipedia.org` session), and the side-by-side evidence layout (shipped
  as stacked-but-expanded instead). See Definition of Done for the full
  per-item status.
- 2026-07-01: State `Draft` → `Accepted`. All five open questions resolved:
  repair-first sequencing for "Fix citation" (tracked in
  [#108](https://github.com/schiste/SP42/issues/108)); explicit
  operator-triggered Re-verify instead of relying on a full page rescan;
  whole-sentence `InlineEdit` pre-fill; the `NotSupported` flag anchor rides
  on PRD-0012's spawned ADR; no restriction on template override. Flag
  citation and Re-verify were both added to scope during review, alongside
  the original two-action (edit text / fix citation) design.

## Summary

The Citations tab (PR #81) shows verification findings but can't act on them —
every citation write today is CLI-only. This adds a per-finding action row to
that tab for `Partial`/`NotSupported` findings, offering three paths: fix the
citation (PRD-0008/0012's existing propose/confirm flows), fix the article
text (the existing `InlineEdit` action already used by the patrol rail), or
flag the citation as failed verification for a reviewer with more context to
resolve later (a small new action generalizing the existing
`TagCitationNeeded` mechanism). A mismatch between a source and the claim it
backs can mean either side is wrong — a source that says "7%" against article
text that says "6%" is usually a text problem, not a citation problem — and
sometimes the operator genuinely can't tell. SP42 doesn't guess in either
direction: all three options are shown with equal weight, the source's
located passage and the article's claim text rendered side by side, and the
operator picks — including picking "I don't know, flag it."

## Scope boundary

This PRD owns the **browser presentation and routing** layer for citation
repair (PRD-0008) and insertion (PRD-0012), plus **two small new pieces**:
flagging a citation with a maintenance template, suggested by SP42 from
`finding.verdict` and confirmed (or overridden) by the operator; and an
explicit, operator-triggered **re-verify** on any finding, wrapping the
existing single-use-site verify primitive (`verify_citation_use_site`,
already used internally by the whole-page scan and already exposed via the
CLI) in a new browser-reachable route. Re-verify adds no new verification
logic — it's a thin route over what already exists — but it is new surface
area, not pure routing to something already browser-reachable, unlike the
other two actions. That action
generalizes the existing `TagCitationNeeded` mechanism
(`crates/sp42-server/src/action_routes.rs`) from one single-purpose config
field to a small closed set of concern kinds — a new
`CitationConcernKind` enum, a `WikiTemplates.citation_concerns` map replacing
what would otherwise be one struct field per template, and one generic
executor. The two v1 kinds need two different wikitext anchors (span-wrap for
`Partial`, insert-after-`<ref>` for `NotSupported`) — the former reuses
`TagCitationNeeded`'s existing mechanism directly; the latter needs a new
insert-at-position primitive and is sequenced alongside PRD-0012's own
insertion extension (see Proposal).

Beyond that one action, this PRD does not redefine:

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
  questions #5 and Alternatives. This is a hard requirement, not a v1
  simplification: it follows directly from the project's existing
  abstention-biased, anti-fabrication posture (PRD-0010 Risks; PRD-0012
  Alternatives' rejection of auto-insert on high-confidence grounding).
- **A CLI surface.** Both underlying actions already have one (PRD-0008/0012);
  this PRD adds no new CLI surface of its own.
- **Detecting unsourced claims or surfacing findings at scale** — the citation
  review queue (issue #26), unchanged from PRD-0008/0012's own exclusions.
- **Concern kinds beyond the two v1 ships.** `FailedVerification` and
  `PartialSupport` are the only two `CitationConcernKind` variants in this
  PRD's closing PR, chosen because they map 1:1 to SP42's two in-scope
  verdicts. Reliability- or accuracy-flavored templates (`{{Unreliable
  source?}}`, `{{Dubious}}`) are out of scope — SP42 doesn't measure source
  reliability or claim accuracy, only claim-source support (PRD-0010
  Alternatives), so applying either would mislabel what SP42 actually found.

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

And even offering both fixes doesn't cover every case: sometimes the operator
can see something's wrong but doesn't have the context to know which side —
resolving 6% vs. 7% might need domain knowledge the reviewer doesn't have.
Today the only way to preserve that finding for someone who does is informal
(a talk-page note, or just moving on) — SP42's own finding is invisible to
anyone not looking at its report. Wikipedia already has a convention for
this: inline maintenance templates like `{{failed verification}}` that mark a
citation as disputed for any future editor to see, not just SP42 users.

## Proposal

For every `Partial`/`NotSupported` finding, the action row sits beneath the
finding's existing card (`FindingCard`, PR #81) — which already shows the
claim, the located quote, a broader source excerpt, the source URL, and
bibliographic metadata, all `CitationFinding` fields the card already renders
today — with three equal-weight actions:

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
- **Flag citation** → suggests a per-wiki-configured maintenance template
  based on `finding.verdict` — `{{Failed verification span}}` (wraps the
  claim text) for `Partial`, `{{Failed verification}}` (placed after the
  `<ref>`) for `NotSupported` — surfaced in the same propose/preview step the
  other two actions use, with the operator able to accept the suggested
  template or pick a different configured one before confirming. An optional
  free-text field lets the operator add their own explanation, threaded into
  the template's `reason=` parameter (both templates support one) when
  filled; leaving it blank is fine — the templates themselves make `reason=`
  optional. Generalizes `TagCitationNeeded`'s existing mechanism to a small
  closed set of concern kinds (`CitationConcernKind`) rather than one more
  single-purpose field: `WikiTemplates` gains
  `citation_concerns: BTreeMap<CitationConcernKind, String>` alongside the
  existing `citation_needed`/`bare_url_citation` fields, and one generic
  executor replaces what would otherwise be a third near-duplicate of
  `execute_tag_citation_needed_action`. **The two verdicts need genuinely
  different anchors**, matching how the two real templates are used: the
  `Partial` path (span-wrap) is structurally identical to
  `TagCitationNeeded`'s existing literal-fallback mechanism — cheap, ships
  first. The `NotSupported` path (insert-after-the-`<ref>`) needs a new
  insert-at-position primitive this PRD doesn't yet have — worth sequencing
  alongside PRD-0012's own insertion extension, since both are "insert new
  content at an anchored position," just anchored to different node types.
  Declining is always available — "none of these fit" is a normal outcome,
  not a dead end, the same as declining any other proposal in this domain.
  When a wiki has no configured template for the suggested
  `CitationConcernKind` at all, the row says so rather than silently omitting
  the button, and points at Edit article text as the remaining recourse,
  since that action needs no per-wiki template configuration.

All three actions terminate in the existing ADR-0010 propose/preview/confirm
component — before/after diff, explicit confirm, `baserevid`-guarded apply
that refuses on drift. None of the three is pre-selected, defaulted, or shown
with more visual weight than the others. Within Flag citation, the
verdict-appropriate template is pre-filled as a suggestion in that same
preview step, not applied ahead of it — the operator can swap it for a
different configured template or add a reason before confirming, the same
way they can edit `InlineEdit`'s replacement text before confirming that.
`Supported` and `SourceUnavailable` findings are unaffected — they stay
read-only, as today.

Every finding with an action row also gets a fourth, always-available
control: **Re-verify**. It calls a new route wrapping
`verify_citation_use_site` (`crates/sp42-citation/src/citation/verify.rs:944`
— the same single-use-site primitive the whole-page scan already calls
internally, one finding at a time, and the CLI already exposes standalone)
against the finding's current article state, and replaces the card with the
fresh result. Not automatic — the operator triggers it explicitly, so they
control when the inference cost is spent (mirrors PRD-0010's BYO-key
cost-ownership posture) — but it means checking whether an edit actually
resolved a mismatch doesn't require leaving the card to re-run a full-page
scan. It also doubles as a live check on how good the underlying rescan
actually is, independent of whether any edit was made at all.

### Amendment (2026-07-09): Re-verify is *cached page re-verification*

The original design above re-verifies **one finding** and replaces its card in
place. That is what the first implementation ships, but it has a seam that
review surfaced: the report was loaded for revision *N*, and Re-verify checks
the **current** article state (revision *N+1* after the operator's edit). Folding
that *N+1* verdict back into an *N* report leaves the card's verdict fresh while
the report header, the raw-text report, and the card's "show citation in
article" link still read *N* — a **mixed-revision** display. The interim
implementation carries this as a known limitation.

The resolved contract is to make Re-verify **refresh the whole page to the
current revision**, so header, links, per-verdict sections, and every card are
consistently *as of N+1* and the mixed-revision state cannot arise. Re-running
the entire page would normally be too expensive (a full model panel per
citation), so this depends on the in-memory verdict cache specified in
**ADR-0019 (Cached page re-verification)**:

- A verdict is a function of the **source content** and the **claim sentence**
  (the panel is fixed within a session). Cache it, content-addressed, by
  `(snapshot_hash, claim)` — the primitive ADR-0009 already defines but leaves
  dormant.
- Re-verify re-fetches the **article** (that is the thing the operator changed —
  the whole point), reuses **session-cached source bodies** (external sources are
  stable across a few-minute session; no per-re-verify source re-fetch), and so
  reuses cached verdicts for every citation whose source content and claim are
  unchanged. Only the citation(s) the operator actually edited miss the cache and
  spend fresh inference.
- **No force-refresh control.** Every case that warrants fresh inference — an
  edited claim, a changed source — busts the content-addressed cache
  automatically; a case that hits is one where re-inferring could not legitimately
  change the answer (e.g. repairing a bare URL into a cite template does not change
  whether the source supports the claim). "Nothing changed ⇒ the same verdict" is
  the honest outcome. A deliberate *second-opinion / re-roll on identical inputs*
  is a **separate future control**, not Re-verify.

Under this contract the operator-facing behavior of Re-verify becomes "bring this
page up to date with the current article, cheaply," and the per-card verdict, its
links, and the page's counts are always for the same revision. Mechanism, cache
scope, and the fetch model are ADR-0019's to decide; this PRD owns only the
contract.

## Definition of Done

- [x] An action row renders only for `Partial`/`NotSupported` findings;
      `Supported`/`SourceUnavailable` findings remain read-only, verified by a
      component test over all four verdict fixtures.
- [ ] The source's located passage and the article's claim text render side by
      side above the action row whenever it's shown, verified by a rendering
      test.
- [x] "Edit article text" opens `InlineEdit` pre-filled with `finding.claim` as
      `selected_text` and an empty operator-authored `replacement_text`,
      verified end-to-end against the stub editor.
- [x] "Fix citation" routes to the replace action when `finding.ref_id` is
      non-empty and to the insert action when it is empty, verified by tests
      over both finding shapes.
- [x] A claim sentence occurring more than once in the article refuses any
      literal-fallback action anchored on it — `InlineEdit`'s text-edit path
      and Flag citation's `Partial` (span-wrap) path both — rather than
      guessing, verified by fixture tests over both call sites. Same guard,
      exercised from two new call sites.
- [x] "Flag citation" suggests `{{Failed verification span}}` for `Partial`
      findings and `{{Failed verification}}` for `NotSupported` findings in
      the propose/preview step, before any write, verified by tests over both
      verdict shapes.
- [x] The operator can override the suggested `CitationConcernKind` for any
      other wiki-configured one before confirming, verified by a test
      asserting the apply payload reflects the operator's choice, not just
      the suggestion.
- [x] An optional reason field, when filled, lands in the applied template's
      `reason=` parameter; left blank, the template renders without one,
      verified by fixture tests over both cases.
- [ ] A `CitationConcernKind` with no configured template for the wiki
      refuses rather than inserting a wrong-language template, verified by a
      config-gating test — mirrors `citation_needed_template`'s existing
      refusal behavior (`ensure`-style test, `action_routes.rs`). The row
      surfaces this as "no configured template," not a silently missing
      button, verified by a rendering test.
- [ ] Declining a Flag-citation suggestion — the operator not wanting any of
      the offered templates for this finding — results in **zero writes**
      and leaves the other two actions available, verified by a test
      asserting decline is a first-class outcome, not an error. Distinct from
      the no-configured-template case above: this is a proposal the operator
      saw and chose not to apply, not a refusal before one was ever shown.
- [x] All three actions terminate in the existing ADR-0010
      propose/preview/confirm component with no new confirm UI added,
      verified by review (no new confirm-dialog component introduced in the
      diff).
- [x] None of the three actions is pre-selected, defaulted, or rendered with
      more visual weight than the others, and no confidence score is shown
      alongside them, verified by a snapshot/parity test asserting all three
      buttons share the same component and styling. Within Flag citation, the
      suggested template is editable in the same preview step, not applied
      ahead of it.
- [ ] The action row is offered **only on enabled wikis**; the MVP enables
      only `test.wikipedia.org`, verified by the same config-gating tests
      PRD-0008/0012 use.
- [x] Every finding with an action row also offers **Re-verify**, which calls
      the new route, re-runs `verify_citation_use_site` against the
      finding's current state, and replaces the card with the fresh result,
      verified by an end-to-end test against the stub verifier.
- [x] Re-verify never fires automatically — not on mount, not after any of
      the three actions confirms — only on explicit operator click, verified
      by a test asserting no verify call happens without it.
- [x] The new route adds no verification logic of its own — it calls
      `verify_citation_use_site` unchanged — verified by review (no new
      grounding, panel, or verdict logic introduced outside the existing
      module).
- [ ] A first confirmed `Partial`-verdict flag (`{{Failed verification
      span}}`) lands on `test.wikipedia.org`, driven through the browser: the
      closing PR records the finding, the suggested and (if changed) chosen
      template, before/after wikitext, and the resulting revision id — the
      live-edit acceptance gate for this PRD, mirroring PRD-0008/0012's own.
      **The `NotSupported` flag path (insert-after-`<ref>`) is not required
      for this item**: it's blocked on the new insert-at-position primitive
      (see Proposal, Open questions) and may close in a follow-on PR.

### Implementation status (2026-07-01)

The mechanism, routing, and gating logic above are implemented and unit
tested; the checked items reflect that. Three caveats on *how* "verified by"
was satisfied, since this repo has no Leptos DOM/e2e test harness (pre-existing,
not introduced here) and `sp42-app`'s `pages`/`components`/`platform` modules
are wasm32-gated at the crate root, so even pure-logic tests in those modules
don't run under the workspace's plain `cargo test`: "component test" /
"end-to-end test" / "rendering test" / "snapshot test" items are satisfied by
pure-function unit tests (compiled and traced by hand under
`--target wasm32-unknown-unknown`) plus manual code review, not by an
automated DOM harness — genuinely new coverage here, but a different kind
than the checklist item names.

Left unchecked, not just under-verified:

- **Side-by-side layout.** The evidence disclosure now defaults open when the
  action row shows (so the located passage isn't hidden behind a click), but
  claim and passage still render stacked, not side by side.
- **"No configured template" as a distinct, surfaced message.** The
  underlying refusal is real and tested server-side
  (`citation-concern-not-enabled`), but the row doesn't proactively check
  config before rendering the button — the reason only surfaces reactively,
  in the status line, after the operator clicks confirm and it's refused.
- **Decline-is-zero-writes**, implemented (Cancel sends nothing) but
  unverified by an automated test, for the harness reason above.
- **Wiki-level enablement gating in the UI.** The row renders for any wiki
  based on verdict alone; enablement is enforced by the underlying actions
  refusing server-side, not by hiding the row per wiki. There was no prior
  browser UI for PRD-0008 to establish a convention either way.
- **The live-edit acceptance gate itself.** Not done — needs an authenticated
  Wikimedia session against `test.wikipedia.org`, driven through the browser,
  which this implementation pass didn't have credentials for.

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
- *Auto-apply the verdict-appropriate template without operator
  confirmation.* Rejected: raised, then explicitly declined — "the model
  should suggest, but not decide, the template." Every other action in this
  PRD treats confirmation as non-negotiable; silently applying a template
  because SP42 computed the "obviously right" one would be the same
  shortcut already rejected for the top-level text-vs-citation choice, just
  recurring one level down.
- *One struct field + one `SessionActionKind` variant per concern template
  (mirror `TagCitationNeeded` exactly).* Considered, not chosen: works fine
  for exactly one new template, but `TagCitationNeeded` is already matched
  across 5 call sites — a third near-duplicate means touching all 5 again,
  and every concern kind after that means it again. A small closed
  `CitationConcernKind` enum + one config map + one generic executor pays
  that cost once.
- *Reuse the span-wrap mechanism for both concern kinds*, even though
  `{{Failed verification}}` isn't conventionally used that way (real usage
  places it after the `<ref>`, not wrapping the claim). Rejected: ships a
  template in a form real editors don't use it in, to avoid building new
  insert-at-position mechanics — the same "cheap but structurally wrong"
  tradeoff PRD-0008 already rejected for literal substring replacement.
- *Ship a best-guess frwiki mapping now* (e.g. `Source insuffisante`, the
  closest existing template). Rejected: frwiki's own template-talk pages
  show the community has discussed and not settled this gap — picking one
  unilaterally risks the same "community classification" concern PRD-0008
  already flags elsewhere. Raise with schiste instead (#107); frwiki stays
  unconfigured for this concern until that resolves.

## Risks

- **Operator confusion about which of three buttons to press.** Mitigation:
  the action row sits beneath the finding's existing evidence card (claim,
  located quote, source excerpt, source URL, metadata — `FindingCard`, PR
  #81), so the disagreement is visible before choosing, not inferred from a
  label.
- **Literal-fallback refuses more often than expected (non-unique text).**
  Not a new risk — the guard already exists — but two new call sites now
  share it: `InlineEdit`'s text-edit path and Flag citation's `Partial`
  (span-wrap) path. Mitigation: none needed beyond existing behavior;
  refuse-not-guess is correct, consistent with every other write path in the
  domain.
- **PRD-0012 dependency.** The insert half of "Fix citation" has no mechanism
  to route to until PRD-0012's open questions settle and it ships. Mitigation:
  ship the repair half first — it depends only on `Accepted` PRD-0008, which
  is already implemented — mirroring PRD-0013's identical sequencing call for
  the MCP surface (its replace path ships ahead of its add path for the same
  reason).
- **No primitive yet for the `NotSupported` flag path.** Distinct from the
  PRD-0012 dependency above — insert-after-`<ref>` has no spawned ADR or
  existing mechanism at all, unlike PRD-0012's insert-at-sentence extension,
  which is at least drafted. Mitigation: the DoD's live-edit gate already
  scopes to the `Partial` path only; `NotSupported` flag may close in a
  follow-on PR once that primitive exists.
- **Scope creep into repair/insertion mechanics.** Mitigation: explicit
  non-goals in Scope boundary; any change to *how* a repair or insert is
  computed belongs in PRD-0008/0012, not here.
- **Community reception of tool-applied maintenance templates.** Mirrors
  PRD-0008's "community classification of assisted citation-filling" risk —
  publicly asserting "this citation failed verification" is a stronger claim
  than filling in a citation, and may draw more scrutiny than repair does.
  Mitigation: same posture as PRD-0008 — every application is
  operator-confirmed, operator-attributed, one at a time; the confirming
  operator has already seen the full evidence card (claim, located quote,
  source excerpt, source URL, metadata) before reaching the action row, so
  confirmation reflects a reviewed judgment, not a rubber stamp. If a wiki's
  community wants more before tool-assisted tagging at scale, that wiki's
  concern kind stays unconfigured until resolved.
- **Operator habituation on the suggested template.** Mitigation: the
  suggestion isn't really a judgment call in v1 — exactly two concern kinds,
  mapped 1:1 and deterministically from `finding.verdict` — so there's no
  meaningful "wrong suggestion" to rubber-stamp past. Part of why v1 stays to
  exactly two kinds (see Scope boundary).

## Resolved questions

1. **Does "Fix citation" ship for both findings shapes at once, or repair
   first?** Resolved: repair first (see Risks) — PRD-0008 is `Accepted` and
   implemented; insert is blocked on PRD-0012. The action row still renders
   for both finding shapes, but "Fix citation" on an unsourced-claim finding
   errors informatively until PRD-0012 lands, rather than being hidden.
   Tracked in [#108](https://github.com/schiste/SP42/issues/108).
2. **What happens to a finding after the operator commits one action?**
   Resolved: it stays rendered as its original verdict until re-verified —
   not automatically, and not by requiring a full page rescan. Every finding
   with an action row also offers an explicit **Re-verify** control, a thin
   new route over the existing `verify_citation_use_site` primitive; the
   operator triggers it, sees the fresh result, and controls when the
   inference cost is spent.
3. **Does `InlineEdit`'s pre-fill need more than the raw claim sentence** (e.g.
   highlighting the specific span that disagrees with the source, like the
   number in "6%")? Resolved: no — ships with the whole-sentence pre-fill;
   sub-sentence highlighting is a refinement, not a blocker, and the operator
   already sees the source passage to compare against via the existing
   `FindingCard`.
4. **Does the `NotSupported` flag path's insert-after-`<ref>` primitive
   extend ADR-0003 as its own thin extension, or ride on whatever ADR
   PRD-0012 spawns for its sentence-anchored insert?** Resolved: ride on
   PRD-0012's spawned ADR rather than drafting a second, near-duplicate one —
   both are "insert new content at an anchored position, with the same
   anti-drift/`baserevid` discipline," just anchored to different node kinds
   (sentence-end vs. ref-adjacent). Falls back to a separate thin ADR only if
   PRD-0012's actual ADR scope turns out too narrow to extend cleanly.
5. **Should overriding the suggested `CitationConcernKind` be restricted to
   kinds "compatible" with the finding's verdict, or open to any
   wiki-configured kind regardless?** Resolved: no restriction — any
   wiki-configured kind is selectable regardless of verdict. SP42 only
   suggests, never decides; encoding which overrides are "allowed" would be
   SP42 taking a policy position this domain consistently leaves to the
   operator or the wiki community (mirrors PRD-0010's refusal to own
   source-reliability policy).
