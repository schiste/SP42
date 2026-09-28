# ADR-0030: Reviewable item identity — one opaque, revision-independent, lane-agnostic identity for every subject under review

**Status:** Accepted
**Date:** 2026-09-28
**Author:** Christophe Henner (drafted by Claude Code)
**Summary:** Everything a gate records a verdict about is addressed by one `ReviewableItemId` — opaque, globally unique, minted at intake admission, unchanged by edits, and carrying no review lane — so a page, draft, nomination, or investigation case has a single durable identity that its whole transition history accumulates under.

## Context

ADR-0027's `ContentLifecycleRecord` is keyed by `ReviewableItemId`, and both
ADR-0028's `EligibilityVerdict` and ADR-0029's `QualityVerdict` carry an
`item_id`. Each of those three ADRs deferred the type itself to "a separate
future ADR", which left the entire chain unbuildable: a lifecycle record cannot
be keyed on a type nobody has defined, and the gate verdicts written to it cannot
be constructed either.

The informal sketch that the deferrals kept gesturing at was "page | draft |
nomination | investigation case" — the queueable unit. Three properties of that
identity are not free choices, though, and each is forced by something already
decided elsewhere in this chain:

- **It cannot be revision-scoped.** ADR-0027 §4 orders a gate's prior-outcome
  query most-recent-first precisely so that a caller asking about a *lineage's*
  current standing gets the answer rather than a stale entry, and ADR-0028 §2
  reads only that first transition. WP:CSD's G4 asks whether a prior deletion
  discussion resolved a page to Delete; ADR-0027 §3's redirect-reopening watch
  fires on a `redirect`→`article` transition. Every one of those is a question
  about a subject across time. An identity that changed on each edit would
  scatter one item's history across one record per revision, and G4 would become
  unanswerable in the only form the policy states it.
- **It cannot encode a review lane.** ADR-0027 §1 keys `states` by `track`
  precisely because NPP, AfD, and CCI can be concurrently active on one page, and
  it makes each domain own only its own track's entry. A lane-bearing identity
  would mint three identities for that one page, and the canonical per-track map
  would then be describing three unrelated items.
- **It cannot be wiki-scoped.** ADR-0027 §1 already carries `wiki_id` as its own
  field. If the id were wiki-scoped, that field would be redundant, and every
  cross-wiki read — a coordination view spanning projects, a tooling query over
  several wikis at once — would need to know which wiki to ask before it could
  ask.

And one boundary is forced by ADR-0026: intake observes raw events for subjects
it has not admitted, at `(wiki_id, page_id, revision_id)` granularity. Minting
identity there would assert that a subject is a reviewable item before anything
has established that it is one, which is the same category of mistake ADR-0028
§1 refuses when it declines to record a scoping decision as a verdict.

## Decision

### 1. `ReviewableItemId` is an opaque, globally unique, revision-independent identity
```
ReviewableItemId { kind: ReviewableItemKind, wiki_id: WikiId, local_id: String, v: u8 }
ReviewableItemKind = Page | Draft | Nomination | InvestigationCase | Custom(String)
```
Opaque means callers compare and store it, never pattern-match its internals to
infer meaning — the same discipline every other identifier in the codebase
follows, and what lets the serialized form change without a flag-day migration.
Globally unique across wikis: `wiki_id` is a component of the value *and* a
component of the encoding, so a record written on one wiki is unambiguous when
read on another, and no lookup has to guess a project first. Revision-independent:
the identity names the subject, never a particular state of it, so the whole
history accumulates under one key.

The canonical encoding is versioned by `v` and must round-trip losslessly,
because ADR-0027 requires a durable record and a persisted identity that cannot
be read back is not an identity. Only the engine constructs and decodes it.

### 2. The kind is a closed core with an open extension, and it never becomes a platform-wide enum per domain
`Page`, `Draft`, `Nomination`, and `InvestigationCase` cover the queueable units
this design was sketched against, but `Custom(String)` keeps a new lane from
requiring a platform-type edit — the same typed-but-open shape ADR-0026 §4 chose
for `IntakeField` and ADR-0027 §1 chose for `LifecycleState.key`, for the same
reason: a closed enum edited per new domain is the seam ADR-0021 §5 already
flagged as a real cost. `local_id` is the kind's own namespace-local identifier
(a `page_id` for `Page`, a draft id for `Draft`, a nomination id for
`Nomination`), opaque to everything above this contract.

`kind` is descriptive, not load-bearing. It exists so a caller holding an id can
resolve *how* to address the subject (§4) and so operator-facing surfaces can
label it, not so the engine can branch eligibility policy on it — a rule that
needed to know the kind should say so through a domain-owned field, exactly as
ADR-0026 §4 routes domain-specific facts through a resolver registry.

### 3. Identity is minted at admission, and only there
An `Admit` outcome from an intake pipeline (ADR-0026 §6) is the single point at
which a subject becomes a reviewable item and an id is minted for it. Minting is
idempotent: re-admitting a subject already carrying an id resolves to the
existing one rather than minting a second, because a duplicate identity would
split one item's history across two records and quietly break every
`transitions_matching` query ADR-0028 §2 depends on.

Nothing downstream mints. A gate that needs to reference an item it was handed
uses the id it was given; a workflow that enqueues a new subject goes back
through intake. Intake itself never mints (Non-goals).

### 4. Resolution is a read, never an identity mutation
```
fn resolve(item_id: ReviewableItemId) -> Option<ResolvedSubject>
ResolvedSubject { kind, wiki_id, locator: SubjectLocator, title: String }
```
Resolving turns an id into something displayable and gate-feedable — a page's
current title, a nomination's target. It is a read that can fail (the subject may
have been deleted, merged, or moved) and whose failure is *not* an identity
change. A moved or merged page keeps the id it was admitted under, because the
alternative — re-minting on move — would restart the history ADR-0027 exists to
accumulate, and WP:CSD's redirect and merge criteria are exactly the cases where
a subject's address changes while its review history must not.

### 5. `ReviewableItem` is not decided here
This contract fixes the identity of the reviewable unit, not its contents. What a
gate reads off an item — the diff, the extracted claims, the rendered preview —
is ADR-0003's and the domain contracts' business, and inventing a payload here
would front-run them. The two are deliberately separable: identity is what makes
a history durable, and it is needed before any payload is agreed.

## Alternatives

- **Key the lifecycle record on `(wiki_id, page_id)`.** Rejected — drafts,
  nominations, and investigation cases are not pages, so this either cannot
  represent them at all or forces each to be faked as a page with a synthetic id,
  and it re-opens the "what is the identity of a reviewable item" question every
  time a new queueable kind appears.
- **Make the identity revision-scoped.** Rejected — G4, redirect re-opening, and
  GA delisting all read a prior verdict about the same subject across edits; a
  revision-scoped id makes each of them unanswerable and fragments the
  append-only log ADR-0027 §2 defines into one record per edit.
- **Encode the review lane in the identity.** Rejected — NPP, AfD, and CCI are
  legitimately concurrent on one page (ADR-0027 §1), so this mints three
  identities for one subject and leaves the per-track `states` map describing
  unrelated records instead of one item's concurrent tracks.
- **Make the identity wiki-scoped, dropping `wiki_id` from the record.**
  Rejected — ADR-0027 §1 already carries `wiki_id` as its own field, and a
  cross-wiki read would have to know the wiki before it could even look up the
  id.
- **A free-form string identity, with the kind carried alongside as
  configuration.** Rejected — gives up every compile-time and lint-time check
  this contract exists to provide, and re-opens the malformed-identity class
  ADR-0026 §7 and ADR-0027 §3 both reject for their own vocabularies.
- **Let intake mint an identity for every observed event.** Rejected — intake
  runs on raw events for subjects no workflow has admitted (ADR-0026 §2's
  always-on baseline sees every edit on every registered wiki), so minting there
  asserts reviewable-item status that nothing has established, and would create
  identities for content nobody ever reviewed.
- **Mint on first gate verdict rather than at admission.** Rejected — an item
  admitted and then dropped by the first gate's rules would leave no record that
  it was ever a candidate, which is the same missing-history gap ADR-0027 exists
  to close, just one stage earlier.
- **Re-mint on move or merge.** Rejected — WP:CSD's redirect and merge criteria
  are precisely the cases where a subject's address changes while its review
  history must survive; re-minting restarts the history at the moment the policy
  most needs it intact.
- **Fix the payload in this ADR too.** Rejected — identity is needed first and is
  independently decidable; bundling a payload would block the lifecycle record on
  a diff/claims design that ADR-0003 and the domain contracts already own.

## Consequences

- Closes the last undefined type in the ADR-0027 → 0028 → 0029 chain: the
  lifecycle record, both gate verdicts, and the linter that validates them all
  become constructible.
- Admission becomes a real, single responsibility. Exactly one place decides when
  a subject becomes a reviewable item, and that decision is idempotent.
- Address changes stop being identity changes. Move, merge, and redirect — all
  common, all covered by policy the platform must support — leave a subject's
  history intact under its original id.
- Non-page queueable units (drafts, nominations, cases) get a first-class identity
  instead of being modelled as pages with invented ids.
- The engine gains a decode step on every id it reads, and a resolve path that
  can legitimately fail. Pinned by: round-trip property tests over the versioned
  encoding, an idempotency test that re-admitting a subject yields one id, and a
  test that a move preserves the id and the accumulated history.
- `kind` stays descriptive. A rule that needs to branch on subject kind must go
  through a domain-owned field, so this contract cannot become a back door for
  platform-type edits per domain.

## Non-goals

- `ReviewableItem`'s payload — the diff, claims, or rendered content a gate reads
  off the item. That is ADR-0003's and the domain contracts' business; only the
  identity is decided here (§5).
- The state and transition mechanism the identity keys — ADR-0027.
- The reason payload attached to a verdict — ADR-0031.
- Admission policy: which pipelines admit which subjects, and on what rules —
  ADR-0026. This ADR fixes only when an identity comes into existence, not what
  earns one.
- Ranking or prioritization of reviewable items — scoring is ADR-0021, and
  nothing here implies an ordering over identities.
- Cross-wiki federation semantics for a subject tracked on several projects.
  Identities are globally unique, so a cross-wiki *read* is unambiguous; how a
  subject spanning projects should be tracked as one is a separate question.
- Minting identity inside intake — ADR-0026's Non-goals, restated here as the
  boundary this contract depends on.
