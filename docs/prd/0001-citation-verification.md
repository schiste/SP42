# PRD-0001: Citation verification for revision review

**Author:** Luis Villa
**Date:** 2026-06-04
**State:** Draft
**Discussion:** <PR link>
**Spawned ADRs:** none yet (see *Spawned ADRs* below)

## Problem

When an operator reviews a revision that adds or changes a citation, there is no
fast way to tell whether the cited source actually supports the claim it is
attached to. Checking by hand means leaving SP42, opening the source, and
reading it. In practice that cost means citation-quality problems — a source
that does not say what the article claims — go effectively unpatrolled, even
though they are exactly the kind of low-visibility error that erodes article
trust.

## Proposal

Add an operator-facing capability: for a claim and its cited source, SP42
fetches the source read-only and reports a **categorical verdict** on whether
the source supports the claim, with the supporting passage shown inline so the
operator can confirm at a glance.

- The verdict is one of a fixed set (e.g. *supported*, *partially supported*,
  *not supported*, *source unusable*, *unclear*). No numeric confidence is ever
  shown — a fabricated percentage is false precision.
- The tool **abstains** (*unclear* / *source unusable*) rather than guess when
  the source cannot be fetched or read.
- Verification is **read-only and never writes**. If review leads to an edit,
  that edit goes through SP42's existing operator-confirmed action path
  unchanged.

The capability is informational: it helps an operator decide, it does not decide
for them.

## Definition of Done

The Constitution already guarantees these are tested, deterministic, and
CI-green. The criteria below are specific to this feature:

- [ ] A verdict is exactly one value from the fixed categorical set, and no
      numeric confidence is ever surfaced — verified by unit tests on the
      verdict type and a surface/contract test.
- [ ] The tool never reports *supported* unless the supporting passage is
      locatable **verbatim** in a source SP42 actually fetched this session —
      verified by a property test: a claim with no matching source text never
      yields *supported*. (This is the load-bearing anti-fabrication invariant.)
- [ ] When the source cannot be fetched or read, the verdict is *source
      unusable* / *unclear*, never a support judgment — verified by an
      integration test against an unreachable / unusable source.
- [ ] Verification performs **no wiki writes**; any resulting edit flows only
      through the existing operator-confirmed action path — verified by an
      integration test asserting zero autonomous writes on the verification
      path.
- [ ] Re-running verification on the same claim and the same fetched source
      snapshot yields the same verdict category (Constitution Art. 2) — verified
      by a recorded-source replay test.
- [ ] Each verification emits an observable showing the fetched source, the
      located passage (or its absence), and the verdict (Constitution Art. 3) —
      checkable in the operator/debug surface.

## Alternatives

- **Score the citation numerically instead of a categorical verdict.** Rejected:
  a number invites the operator to trust precision the system does not have, and
  obscures the one thing that matters — can the claim be located in the source.
- **Let the tool fix bad citations automatically.** Rejected: it would put
  unreviewed writes on the wiki, violating the operator-confirmed action model.
- **Do nothing; rely on manual source-checking.** Rejected: the manual cost is
  exactly why these errors go unpatrolled today.

## Risks

- **A confident-but-wrong verdict.** Mitigated by the verbatim-locatability
  invariant: *supported* is unreachable without a real, quotable passage from a
  really-fetched source, and the passage is shown for operator confirmation.
- **Source fetch etiquette / rate limits.** Mitigated by read-only fetching with
  standard backoff; covered at ADR/implementation altitude.
- **Operator over-trust.** Mitigated by abstention on uncertainty and by keeping
  the capability informational, never an action.

## Spawned ADRs

This PRD touches two dual-natured ADR triggers and so will spawn ADRs before
implementation:

- **Verdict & action semantics** — the categorical verdict set and the
  "no support without a verbatim, in-session locatable passage" rule
  (*Wikimedia action semantics*).
- **Verification contract** — the request/response surface a verification result
  is exposed through (*public contracts or APIs*).
- **Crate boundary** — where verification logic lives (`sp42-core` vs. a new
  crate) (*crate boundaries*).
- **Source-snapshot storage** — how fetched source snapshots and verdict records
  are persisted for reproducibility and audit (*persistent storage formats*).

## Open questions

Each carries a proposed answer to react to, not a commitment.

- **Which source types are in scope for the first cut?**
  *Proposed:* HTML pages and existing archived snapshots only; PDFs deferred to a
  follow-up PRD. *Rationale:* covers the large majority of citations while
  page-level PDF text extraction — a separate cost — stays out of the first cut.
- **Does a verdict feed SP42's existing scoring?**
  *Proposed:* no — strictly informational for the first cut. *Rationale:* coupling
  an unproven signal into scoring would put the scoring policy at risk before the
  verdict's reliability is established, and keeps this PRD off the scoring-policy
  ADR surface for now.
- **Where does verification sit in the operator workflow?**
  *Proposed:* inline in revision review, triggered on demand by the operator
  against a citation — not a separate queue. *Rationale:* it lands at the moment
  the operator is already deciding, and avoids building new queue infrastructure.
