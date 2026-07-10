# PRD-0001: Citation verification — initial implementation

**Drafter:** Claude Code Opus 4.8
**Editor:** Luis Villa
**Date:** 2026-06-04
**State:** Draft
**Discussion:** <PR link>
**Spawned ADRs:** ADR-0006, ADR-0007, ADR-0008, ADR-0009 (see below)

**Implementation note (2026-07-10):** The capability shipped (the
`sp42-citation` verification tree: verify, panel voting/agreement, snapshot
storage, CLI surface; ADR-0006–0009 merged in PR #24). The DoD has been
audited against the test suite — see *DoD bindings* below: 3 of 8 items fully
bound, 5 partial. Remaining before `Implemented`: the coverage gaps in #134
and the anti-fabrication wording decision in #133.

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

## Surface: the CLI (first cut)

The first cut is delivered **CLI-first**, as a read-only command in `sp42-cli` (a web
surface can follow). What matters at this altitude is the interaction model — the
exact flag spelling is an implementation detail:

- **What it checks — one of:**
  - **a whole article** — every citation it contains;
  - **a revision** — the citation(s) that revision adds or changes;
  - **a single citation** within an article or revision — selected by a **snippet of
    the claim it backs** (the cold path: copy a few words of the sentence) or by the
    **index the article report assigns** (the drill-down after a full run); selecting
    a named source checks every place that source is used; or
  - **an ad-hoc claim + source URL** supplied directly — for a source not yet on a
    wiki, and for smoke-testing the verifier in isolation.
- **The unit is a (claim, source) pair, not a footnote.** A source cited in several
  places backs a *different* claim at each use, so each **use-site** is checked
  independently and may receive a different verdict; a whole-article run reports one
  result per use-site, in document order, and assigns each the index the
  single-citation drill-down refers to. (Verdict semantics → ADR-0007.)
- **What it returns — per use-site checked:** the **verdict** (*supported / partial /
  not supported / source unavailable*), the **located supporting passage** (or a note
  that none was found), the **source** checked, and — when a panel is used — the
  **measured agreement**. Output is human-readable by default, with a
  **machine-readable JSON** option in the first cut and a terse **verdict-only** mode
  for a quick scan.

The CLI changes none of the verdict semantics above; it is only how an operator runs
the check standalone. Where it sits in the workflow — standalone, off-queue, and
off-score for now — is covered under *Scope decisions* below.

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
- [ ] The `sp42-cli` citation-verification command accepts a whole article, a
      revision, a single selected citation, or an ad-hoc claim + source URL, and
      prints **one result per citation use-site** — verdict, located passage (or its
      absence), and source — in the default human format, a machine-readable JSON
      format, and a terse verdict-only format — verified by a CLI integration test
      against a recorded source snapshot.

### DoD bindings (2026-07-10)

Audit of each DoD item against the test suite (all cited tests pass;
`cargo test -p sp42-citation -p sp42-mcp`, 282 tests, plus `sp42-server`/
`sp42-cli` suites in CI). Boxes above stay unchecked until every clause of an
item is bound; remaining gaps are tracked in #134, and item 3's wording
decision in #133.

| # | Item | Verdict | Binding / gap |
|---|------|---------|---------------|
| 1 | Categorical verdict, no numeric confidence | PARTIAL | Categorical set fully bound: `citation/verdict.rs` (`verdict_wire_round_trips_all_four_values`, `unknown_wire_string_is_rejected`, `abstention_never_serializes_as_a_support_level`). Gap: "no numeric confidence surfaced" is structural (`PanelAgreement::fraction` is computed, never serialized) but unasserted (#134). |
| 2 | Panel vote + measured agreement | BOUND | `citation/voting.rs` (`unanimous_panel_has_full_agreement`, `clear_plurality_wins_with_measured_fraction`, `tie_never_resolves_up_to_supported`); surfaced via `sp42-cli` `renders_human_verdict_block`; e2e `verify.rs::end_to_end_supported_outcome_with_votes`. Agreement is computed from votes by construction. |
| 3 | Never *supported* without verbatim passage | PARTIAL | Property tests bind the shipped invariant: `verify.rs` (`fabricated_support_is_never_groundable` proptest, `fabricated_multi_token_quote_never_grounds_fuzzily`, `end_to_end_fabricated_quote_is_unverified_not_groundable`). Gap: they assert never-*groundable*-support (ADR-0007 two-axis), not the literal "never yields supported" — the verdict is surfaced and gated, not suppressed. Wording decision: #133. |
| 4 | Unfetchable source → *source unavailable* | BOUND | `verify.rs::end_to_end_unreachable_source_is_source_unavailable_with_no_model_call` (404 → `SourceUnavailable`, no model call); plus PDF/paywall/short-body variants (`pdf_source_is_unusable_with_no_model_call`, `law360_paywall_stub_short_circuits_no_partial`, `end_to_end_all_model_failures_surface_source_unavailable`). |
| 5 | No wiki writes from verification | PARTIAL | Zero-write assertions exist only on the propose/apply path (`sp42-server` `bare_url_apply_*_refuses_with_zero_writes`). The verify path has no write capability wired in, but no test asserts a verification run issues zero writes (#134). |
| 6 | Deterministic replay over same snapshot | BOUND | `citation/storage.rs::replay_is_deterministic_over_the_same_snapshot_and_votes` (identical finding, verdict, agreement); `verify.rs::prefetched_source_skips_http_fetch`. |
| 7 | Observable: source + passage (or absence) + verdict | PARTIAL | Each piece tested: `build_source_excerpt` windows (`long_body_windows_around_the_located_quote`), passage+verdict renders (`citation_page_report.rs::renders_stats_findings_skipped_and_failures`, CLI `renders_human_verdict_block`). Gap: no single end-to-end assertion that one observable carries all three, and no surface shows the fetched-source excerpt (#134). |
| 8 | CLI accepts article/revision/citation/ad-hoc | PARTIAL | Ad-hoc mode bound (`parses_ad_hoc_verify_flags`, `verdict_only_flag_is_recognized`, human/JSON/verdict-only renders). Gap: article/revision/single-citation CLI modes are unimplemented ("await the article parser" — whole-article verification shipped server-side via `post_verify_page` instead), and no CLI integration test against a recorded snapshot exists. Amend-or-implement decision + test: #134. |

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

This PRD spawned the four ADRs below, drafted alongside it. **ADR-0006 — whether
and how SP42 uses LLMs at all — is the foundational one and is meant to be reviewed
first**: it settles the platform model posture before the citation-specific
mechanics. The other three cover the dual-natured ADR triggers PRD-0001 names — with
ADR-0008 covering two (the public-API contract and the crate boundary).

- **Using LLMs** — open-weight models are best ensembled, so multi-model voting is
  the **default** (with **measured agreement** as the honest signal), while a single
  open or SOTA model is **also first-class**; reached through a config-driven
  inference endpoint (local, direct, or a sponsor/hosted proxy) whose keys and budget
  may be a third party's (e.g. WMF via HuggingFace); the browser shell holds no
  provider key. SP42's platform posture for model use → **ADR-0006**.
- **Verdict & action semantics** — the categorical verdict set and the
  "no support without a verbatim, in-session locatable passage" rule
  (*Wikimedia action semantics*) → **ADR-0007**.
- **Verification contract & crate placement** — the request/response surface a
  verification result is exposed through, and where the verification logic lives
  (`sp42-core` modules, not a new crate) (*public contracts or APIs*; *crate
  boundaries*) → **ADR-0008** (the crate placement is its Decision 7).
- **Source-snapshot storage** — how fetched source snapshots and verdict records
  are persisted for reproducibility and audit (*persistent storage formats*) →
  **ADR-0009**.

## Scope decisions

The questions raised in drafting and design are now resolved; they are recorded
here as the agreed scope of the first cut.

- **Source types — resolved.** The first cut covers **HTML pages and existing
  archived snapshots (archive.org) only**. **PDFs** and **The Wikipedia Library**
  (paywalled / credentialed sources) are **out of scope for now** — each deferred
  to a follow-up. *Rationale:* HTML plus archived snapshots covers the large
  majority of citations, while page-level PDF text extraction and credentialed
  source access are separate costs that stay out of the first cut.
- **Workflow placement — resolved.** Verification is **built standalone first**
  and wired into revision review only **after it is tested**. The first cut is
  invoked on demand against a specified target — a revision that adds or changes a
  citation, or a whole article (see *Surface* above) — not a separate queue, and not
  yet in the revision-review flow. Being standalone, it does not
  feed SP42's scoring at all; whether an integrated version ever would is a later,
  post-testing step. *(This subsumes the earlier scoring-coupling question.)*
- **Wiki scope — resolved.** The first cut targets **English Wikipedia (enwiki)
  only** — the wiki the underlying approach has been systematically tested on.
  Other-language wikis (e.g. French Wikipedia) are **deferred**: extending there is
  gated on a testing strategy for citations in another language, tracked in
  [issue #23](https://github.com/schiste/SP42/issues/23).
