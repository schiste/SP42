# PRD-0001: Citation verification — initial implementation

**Drafter:** Claude Code Opus 4.8
**Editor:** Luis Villa
**Date:** 2026-06-04
**State:** Draft
**Discussion:** <PR link>
**Spawned ADRs:** ADR-0006, ADR-0007, ADR-0008, ADR-0009, ADR-0010 (see below)

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

- The verdict is one of a fixed categorical set (defined in ADR-0007:
  *supported*, *partial*, *not supported*, *source unavailable*). No
  model-reported confidence number is ever shown — a fabricated percentage is
  false precision.
- The **default** is a **panel of independent models** combined by vote, with the
  operator shown a **measured agreement** signal — how much of the panel backed the
  verdict (an observed vote count, not a model's self-assessment: the one honest
  quantitative signal). Voting is the default because open-weight models are best
  ensembled, but a **single model is also first-class** — a single open model where
  it is good enough, or a single SOTA model an operator chooses to use or test.
- The tool **abstains** (*source unavailable*) only when the source cannot be
  fetched or read; a usable source always yields a support judgment. There is no
  "couldn't determine" verdict — model uncertainty instead surfaces as **low
  panel agreement**, a *borderline — review* signal rather than false certainty
  (ADR-0006).
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
- [ ] When a panel is used (the default), the verdict is its voted result and a
      **measured agreement** signal (computed from independent model votes, never a
      model-reported number) is surfaced with it — verified by unit tests on the
      vote aggregation and a surface test.
- [ ] The tool never reports *supported* unless the supporting passage is
      locatable **verbatim** in a source SP42 actually fetched this session —
      verified by a property test: a claim with no matching source text never
      yields *supported*. (This is the load-bearing anti-fabrication invariant.)
- [ ] When the source cannot be fetched or read, the verdict is *source
      unavailable*, never a support judgment — verified by an integration test
      against an unreachable / unusable source.
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
- **Operator over-trust.** Mitigated by abstaining when the source cannot be used
  (never guessing), by the verbatim-locatability invariant, by surfacing low
  panel agreement as a *borderline — review* signal, and by keeping the
  capability informational, never an action.

## Spawned ADRs

This PRD spawned the five ADRs below, drafted alongside it. **ADR-0006 — whether
and how SP42 uses LLMs at all — is the foundational one and is meant to be reviewed
first**: it settles the platform model posture before the citation-specific
mechanics. The other four are the dual-natured ADR triggers PRD-0001 names.

- **Using LLMs** — open-weight models are best ensembled, so multi-model voting is
  the **default** (with **measured agreement** as the honest signal), while a single
  open or SOTA model is **also first-class**; reached through a config-driven
  inference endpoint (local, direct, or a sponsor/hosted proxy) whose keys and budget
  may be a third party's (e.g. WMF via HuggingFace); the browser shell holds no
  provider key. SP42's platform posture for model use → **ADR-0006**.
- **Verdict & action semantics** — the categorical verdict set and the
  "no support without a verbatim, in-session locatable passage" rule
  (*Wikimedia action semantics*) → **ADR-0007**.
- **Verification contract** — the request/response surface a verification result
  is exposed through (*public contracts or APIs*) → **ADR-0008**.
- **Crate boundary** — where verification logic lives (`sp42-core` vs. a new
  crate) (*crate boundaries*) → **ADR-0009**.
- **Source-snapshot storage** — how fetched source snapshots and verdict records
  are persisted for reproducibility and audit (*persistent storage formats*) →
  **ADR-0010**.

## Open questions

Each carries a proposed answer to react to, not a commitment.

- **Which source types are in scope for the first cut?**
  *Proposed:* HTML pages and existing archived snapshots only; PDFs deferred to a
  follow-up PRD. *Rationale:* covers the large majority of citations while
  page-level PDF text extraction — a separate cost — stays out of the first cut.
- **Where does verification sit in the operator workflow?**
  *Proposed:* build it **outside the revision cycle first**, and wire it into
  revision review only **after it is tested**. The first cut is invoked on demand
  against a specified target — a particular citation, or an article for which the
  operator requests a whole-article report — not necessarily a separate queue, and
  not yet in the revision-review flow. *Rationale:* the capability can be built and
  validated standalone before it touches the live patrol path; revision-cycle
  integration is a later step, once the verdict's reliability is established. Being
  standalone, the first cut does not feed SP42's scoring at all — and whether an
  integrated version ever would is part of that later, post-testing step. (This
  subsumes the earlier open question on scoring coupling.)
