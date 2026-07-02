# PRD-0008: Bare-URL repair

**Drafter:** Claude Code (Fable 5)
**Editor:** Luis Villa
**Date:** 2026-06-09
**State:** Accepted
**Discussion:** https://github.com/schiste/SP42/issues/27 (this PRD supersedes
the issue's micro-PRD draft; issue left open to track the frwiki-enablement
follow-on, not this MVP)
**Spawned ADRs:** ADR-0010 (operator-confirmed content proposals), drafted with the closing PR once the citation-verification series (0006–0009) merged (see Resolved question 5). The editing *mechanism* is governed by ADR-0003 (accepted, implemented).

## Changelog

- 2026-07-01: State `Draft` → `Accepted` — all five open questions already
  carried decided answers (see Resolved questions below); the closing PR (#40,
  merged 2026-06-12) implements items 1–7 of the Definition of Done, checked
  off below with their test coverage. **Not** moved to `Implemented`: DoD
  item 8, the live confirmed repair on `test.wikipedia.org`, was left
  unchecked as "Manual testwiki smoke (operator at keyboard)" in PR #40's own
  test plan — Editor to confirm whether that smoke test has since run before
  this moves to `Implemented`. PRD-0014 (browser surface for this flow) spawned.

## Scope boundary

This PRD owns the **propose → review → confirm → repair** flow for bare-URL
references: what the operator is offered, what they see before confirming, and
what lands on the wiki.

**MVP wiki scope: `test.wikipedia.org` only.** The MVP proves the
node-anchored propose/confirm/repair loop end to end on the test wiki, with
the **SP42 command line as the MVP's operator surface**. Production wikis
(`frwiki`, `enwiki`) are **not enabled** by this PRD's closing PR; enabling
them — including the `frwiki` `{{Lien web}}` template mapping, its French
formatting, and the patrol-review-surface affordance — is a follow-on that
starts once the editing mechanism has proven itself live.

It deliberately excludes adjacent concerns:

- **Finding bare URLs at scale** — queue ranking, cross-article discovery, and
  prioritization are the citation review queue (issue #26, future). This PRD
  covers only the references of the revision the operator is already reviewing.
- **How the edit is carried out** — node addressing, anti-drift anchors, and
  lossless re-serialization are **ADR-0003** (implemented). This PRD relies on
  that mechanism; it does not re-specify it.
- **Archive enrichment** — attaching archive snapshots is PRD-0010 (issue
  #29). A proposed citation here carries only what the live page's metadata
  supports.
- **Identity and capability gating** — which operators may edit is PRD-0005;
  the repair disposition follows the same gating as the existing inline-edit
  disposition (PRD-0004).

## Problem

A frwiki patroller reviewing a revision often finds references that are nothing
but a URL — `<ref>https://example.org/article</ref>` — with no title, author,
site, or access date. The encyclopedic fix is a filled citation template, but
producing one today means leaving SP42: open the URL, read the page, hand-write
`{{Lien web}}` with its French parameter names and date formats, then come back
having lost queue position and context. That cost means operators skip the fix
even when they have already noticed the problem, and bare URLs accumulate —
each one a citation that will silently rot into an unverifiable claim when the
link dies.

This is for the **experienced reviewer/patroller** acting under their own
account, one revision at a time. The eventual operator is the `frwiki`
patroller; the MVP proves the flow on `test.wikipedia.org` before any
production wiki is enabled.

## Proposal

For a bare-URL reference in the revision under review, SP42 **proposes a filled
citation the operator can confirm**:

- The operator can see which of the revision's references are bare URLs — a
  reference whose content, after whitespace trimming, is exactly one plain
  `http(s)` URL; `<ref>` attributes such as `name=` do not affect this and
  are preserved — and pick one to repair.
- SP42 fetches the linked page's metadata **read-only** through a
  citation-metadata service integrated with the wiki — the same one the wiki's
  visual-editor cite tool uses — and builds a citation template filled from
  that metadata: title, site name, author, publication date when known, the
  access date, and the source's language when it is not the wiki's own.
  **Every emitted field traces to the fetched metadata or to the fetch event
  itself** (the access date) — SP42 authors nothing. That provenance is what
  the operator's confirmation vouches for, and it is the boundary later
  features in this family cross deliberately, under the spawned ADR, when they
  put judgment in the loop.
- The proposal replaces the bare URL **inside that exact reference**, anchored
  to the reference node with an anti-drift re-check (ADR-0003). The `<ref>` tag
  and its attributes are preserved; every other byte of the article is
  untouched.
- The operator sees the reference **before and after** and confirms or
  dismisses. It is a **proposal, not a write**: nothing reaches the wiki
  without the operator confirming that exact proposed edit. On confirm, the
  edit lands under the operator's own session and rights with their summary,
  and the original revision is marked reviewed — the same disposition
  semantics as the existing inline fix (PRD-0004).
- If the article changed under the proposal (the reference moved, was edited,
  or no longer matches), SP42 **refuses rather than guesses** and offers to
  re-propose against the current state (ADR-0003 refusal semantics).
- When the metadata carries no usable title — none at all, or the URL echoed
  back as the title — SP42 **declines to propose**: a title-less citation is a
  flagged template error on the target wikis, worse than the bare URL it
  replaces. The reference stays a finding, not a bad edit.

In the MVP the operator drives this flow from the SP42 command line against
the test wiki; the patrol review surface gains the affordance with the
frwiki-enablement follow-on, alongside the existing inline-edit disposition
(PRD-0004's action-affordance pattern).

Template knowledge is per-wiki: the proposal renders the citation template the
wiki's mapping configures, and a wiki without a configured mapping renders the
default `{{cite web}}` field names so the flow degrades, never errors. **The
MVP ships only the default map** — the test wiki exercises it directly — and
it writes the access date in ISO 8601 (`2026-06-09`). The
`frwiki` `{{Lien web}}` mapping (`url`, `titre`, `site`, `auteur`, `date`,
`consulté le`, `langue` — `url` and `titre` are the template's required
fields) lands with the frwiki-enablement follow-on, where its French date
formats and parameter names get their own locked tests. Fetching each wiki's
own template mappings at runtime is deferred further still (see Alternatives).

**Relation to the spawned ADR.** Every content-edit feature in this family
(issues #27/#28/#29/#32) needs the same write discipline: a proposal the
operator can read, and a confirmation **bound to the exact proposed edit** so
that what the operator approved is what lands — or the write refuses. ADR-0003
already supplies the binding primitives (expected-text anchor + `baserevid`
guard + refuse-on-drift); a thin ADR (ADR-0010) records the
contract built on them — the proposal carries the locator and replacement
verbatim, confirm replays exactly that payload, any divergence refuses — so
that issues #28/#29/#32 reuse it instead of re-deriving it (Resolved
question 5).

## Proposed CLI surface

The MVP's operator surface is the `bare-url` CLI subcommand with two actions
over the dev bridge (ADR-0002). (It was originally shipped as two
mutually-exclusive `--bare-url-preview` / `--bare-url-execute` flag-modes,
2026-06-09; the CLI later moved to `clap` subcommands.)

```text
bare-url preview --title <T> --rev <N> [--wiki <ID>] [--format text|json|markdown]
bare-url execute --title <T> --rev <N> --ordinal <K> [--wiki <ID>] [--action-note <summary>] [--bridge-base-url <URL>] [--format text|json|markdown]
```

- `bare-url preview` calls `POST /dev/citation/bare-url-proposals` and
  renders the revision's `{proposals, declined}`; it is read-only and needs
  no session.
- `bare-url execute` re-fetches the proposals, selects ordinal `<K>`, and
  replays exactly that proposal against `POST /dev/citation/bare-url-apply`
  under the operator's bridge session (bootstrap + CSRF token). The fresh
  fetch re-anchors the locator; the server's anti-drift re-check and
  `baserevid` guard refuse on any race (`node-drift` / `node-out-of-range`,
  zero writes).
- `--wiki` defaults to `testwiki`, the only wiki the MVP enables.
- `--action-note` wins over the default edit summary `SP42: bare-URL repair`.
- Declined references render with their reason codes (`metadata-unavailable`,
  `no-usable-title`) so the operator sees why a bare URL kept its finding.

## Definition of Done

*Items 1–7 verified by the `crates/sp42-citation/src/bare_url_repair.rs` and
`crates/sp42-server/src/citation_routes.rs` test suites as of PR #40
(merged 2026-06-12); test names below. Item 8 is the one DoD item PR #40 did
not close — see Changelog.*

- [x] Given a reference whose content is a bare URL, SP42 proposes a filled
      citation populated **only** from fetched metadata, verified by renderer
      tests over a **replayed metadata-service response** (no live network in
      tests). — `renders_every_field_from_the_basic_fixture`,
      `website_falls_back_and_partial_dates_pass_through`,
      `creators_fallback_formats_authors` (`bare_url_repair.rs`).
- [x] The proposal targets the correct reference when the **same URL occurs
      earlier** in the article (in prose and in another reference), verified by
      a fixture test asserting the addressed node changed and nothing else did.
      — `proposals_target_each_bare_reference_including_duplicates`
      (`citation_routes.rs`).
- [x] Confirming applies **exactly the proposed replacement**: the wiki save
      carries the proposal's wikitext and the reviewed revision's `baserevid`,
      verified by a mock-wiki write-path test. — the scripted `WikitextEditor`
      test double and apply-path tests around `editor_errors_map_to_editor_codes`
      (`citation_routes.rs`).
- [x] No write occurs without a confirmation bound to the exact proposed edit:
      a proposal whose anchor has drifted (or whose ordinal is gone) refuses
      with `node-drift`/`node-out-of-range` and **zero** requests reach the
      wiki's edit endpoint, verified by a write-path refusal test. —
      `editor_errors_map_to_editor_codes` (`citation_routes.rs`).
- [x] Metadata with no usable title (none, or the URL echoed back as the
      title) yields **decline-to-propose** (a structured "no proposal"
      outcome, not an error and not a thin template), verified by
      sparse-fixture tests covering both cases. —
      `declines_without_a_usable_title`, `declines_when_the_title_echoes_the_url`
      (`bare_url_repair.rs`).
- [x] The metadata service being unreachable or rate-limited degrades to "no
      proposal available" without blocking the review flow or the other
      dispositions, verified by an error-path test. —
      `declines_when_metadata_is_unavailable` (`bare_url_repair.rs`),
      `citoid_failure_declines_only_the_affected_reference` (`citation_routes.rs`).
- [x] The repair disposition is offered **only on wikis where it is enabled**,
      and the MVP enables only the test wiki: the production `frwiki` config
      does not offer or accept it, verified by a config-gating test on the
      proposal and apply paths. — `gate_yields_the_configured_template`,
      `gate_refusal_emits_no_citoid_traffic_and_leaves_editor_untouched`,
      `ensure_bare_url_edit_capability_accepts_when_can_edit_true`,
      `ensure_bare_url_edit_capability_refuses_when_can_edit_false`
      (`citation_routes.rs`).
- [ ] A first confirmed repair lands on `test.wikipedia.org`, driven through
      the CLI surface: the closing PR
      records the target reference, its before/after wikitext, and the
      resulting revision id, and the session's action history returns the
      confirmed action entry (the live-edit acceptance gate; endpoint
      availability verified 2026-06-09). — **Unconfirmed.** PR #40's test plan
      lists this as "Manual testwiki smoke (operator at keyboard)," unchecked.
      Not required for `Accepted`, but required before this PRD claims
      `Implemented`.

## Alternatives

- *Auto-apply high-confidence repairs.* Rejected: violates the
  operator-confirms-every-edit posture (PRD-0004); page metadata is too often
  wrong or thin to trust unattended.
- *Literal substring replacement of the bare URL.* Rejected: hits the wrong
  occurrence of a repeated URL and cannot scope an edit to one reference node
  (ADR-0003 failure modes 1 and 3 — the exact modes ADR-0003 was accepted to
  eliminate).
- *Hand off to the browser (open the wiki's visual-editor cite dialog).*
  Rejected: the whole cost being removed is leaving SP42 and losing queue
  context; a hand-off re-imports it.
- *Enable a production wiki (`frwiki` `{{Lien web}}` mapping) in the MVP.*
  Deferred: production-wiki enablement waits until the node-anchored mechanism
  is proven live on the test wiki. The follow-on carries the French template
  mapping, its formatting tests, and the community-classification
  consideration (see Risks) together.
- *Fetch each wiki's citation template mappings at runtime instead of shipping
  built-in mappings.* Deferred: it is the right long-term shape for many-wiki
  support, but it adds a second live dependency and a cache for a feature that
  targets one wiki today. A built-in per-wiki map is a few fields; revisit
  when a second production wiki is configured.
- *Fill `description=` from page metadata when there is no title.* Deferred on
  scope, not posture: `{{Lien web}}` accepts `description` in place of
  `titre`, and a page-summary field (`og:description`-class metadata) is the
  same provenance class as every other relayed field — but it is one more
  mapping with high variance in usefulness, for exactly the cases where the
  rest of the metadata is usually junk. The MVP declines instead (Resolved
  question 2).
- *Enrich the proposal with an archive snapshot in the same edit.* Deferred to
  PRD-0010 (issue #29): archive discovery has its own external-service risk
  and its own frwiki template-practice question; coupling it here would couple
  the failure modes too.

## Risks

- **Garbage propagation.** There is no generative step in this pipeline, so
  nothing can hallucinate — but the metadata service faithfully relays junk:
  anti-bot interstitials ("Just a moment…") or soft-404 boilerplate as the
  title, wrong dates from page metadata. This is the classic failure mode of
  citation-filling bots. Mitigation: the operator reviews the rendered
  before/after for that one reference before confirming — that review is the
  real gate. The no-usable-title decline floor catches the fetch-failed
  cases, and because every field traces to the metadata, thin metadata
  surfaces as a sparse template rather than an invented one.
- **The anchored node moved between proposal and confirm (TOCTOU).**
  Mitigation: the anti-drift re-check refuses and offers re-proposal; the
  `baserevid` guard independently prevents writing over an unseen revision
  (ADR-0003).
- **Metadata-service unavailability, latency, or rate limiting.** Mitigation:
  proposals are operator-paced (one reference at a time), so a slow or failed
  fetch degrades to "no proposal available" and never blocks the other
  dispositions.
- **Community classification of assisted citation-filling.** The MVP makes no
  production-wiki writes, so nothing is exercised yet — but the follow-on
  will be. SP42's posture is unchanged from its existing dispositions
  (PRD-0004): every edit is operator-confirmed, operator-attributed, one at a
  time, with the operator's summary — an assisted edit, not a bot. Mitigation
  for the follow-on: if frwiki consensus turns out to require more for
  citation-filling tools specifically (e.g. a bot-request discussion), the
  wiki stays unenabled until resolved. *(French formatting correctness —
  dates, accented parameter names, non-French source titles and `langue=` —
  is follow-on scope with the `{{Lien web}}` mapping, locked by renderer tests
  there.)*
- **Operator habituation (rubber-stamping proposals).** Mitigation: the diff
  shown is small and single-reference; declining sparse metadata keeps the
  proposal quality high enough that reading it stays cheap.

## Resolved questions

All five carried the Editor's decided answers (2026-06-09), folded into the
body above, and are settled as of `Accepted` (2026-07-01):

1. **What counts as a bare URL for the MVP?** Resolved: a reference whose
   content, after whitespace trimming, is exactly one plain `http(s)` URL —
   `<ref>https://…</ref>`. Bracket-wrapped forms (`<ref>[https://…]</ref>`)
   are **excluded from the MVP**: the wiki renders them as a numbered link, so
   the reference's visible text (which the anchor mechanism and the operator's
   "before" view both use) is not the URL itself; repairing them safely needs
   additional handling that is not worth coupling to the first cut. Labeled
   links and references with any other prose are not bare — they carry
   operator-authored content this feature must not discard. `<ref>` attributes
   such as `name=` do not affect bareness and are preserved.
2. **When is metadata "too sparse"?** Resolved: decline when there is no
   usable title — none at all, or the URL echoed back as the title (the
   service's common "got nothing" fallback). Two reasons: a title-less
   citation is a flagged template error on the target wikis (`{{cite web}}`
   without `|title=` lands in the CS1 missing-title error category; frwiki's
   `Module:Biblio` requires `titre` unless `description` is supplied), and a
   missing title is a strong signal the whole fetch returned junk. The rule
   is a floor, not the gate: a garbage title (anti-bot boilerplate) passes
   it, and the operator's review is what catches that. A missing author or
   date renders sparse-but-honest. (The `description=` fallback is deferred
   on scope — see Alternatives.)
3. **What value goes in the access-date field?** Resolved: the date SP42
   fetched the page during proposal — that is the access date the operator's
   confirmation actually vouches for. The MVP's default map renders it in
   ISO 8601; per-wiki formatting — e.g. `consulté le` with French month
   names — lands with each wiki's template mapping in the enablement
   follow-on.
4. **On what surface does the operator invoke the repair?** Resolved:
   CLI-first — the SP42 command line is the MVP's operator surface, keeping
   the live-edit gate operator-reachable without coupling the MVP to frontend
   work. The patrol review surface gains the affordance with the
   frwiki-enablement follow-on, alongside the existing inline-edit
   disposition (PRD-0004's action-affordance pattern).
5. **Does the propose/confirm contract need its own ADR?** Resolved: yes, but
   thin — ADR-0003's primitives (expected-text anchor, `baserevid` guard,
   refuse-on-drift) already bind a confirmation to an exact edit; the new ADR
   records the contract built on them (proposal payload carries the locator and
   replacement verbatim; confirm replays exactly that payload; any divergence
   refuses) so that issues #28/#29/#32 reuse it instead of re-deriving it. Its
   number is assigned when drafted, after the citation-verification ADR series
   (0006–0009) merges. That condition is now satisfied: the series merged (PR #24) and the ADR is drafted as ADR-0010.
