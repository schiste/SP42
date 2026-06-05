# PRD-0003: Edit scoring and queue ranking

**Author:** Luis Villa
**Date:** 2026-06-05
**State:** Implemented
**As-built:** retroactive characterization of an already-shipped feature (no forward "closing PR").
**Related ADRs:** 0001 (foundational decisions — §9 LiftWing as supplementary ML scoring provider)
**Discussion:** (PR link added on filing)

## Problem

A reviewer working a recent-changes patrol queue faces far more incoming
revisions than they can read. Without a score and a rank, every edit looks
equally urgent: the operator has no principled way to decide *what to look at
first*, *why it surfaced*, or *what action might fit*. They are forced to triage
by hand, in arrival order, and the genuinely harmful changes compete for
attention with routine maintenance edits.

This PRD characterizes what SP42 already ships to solve that: a **deterministic
local scoring engine** that turns each edit event into a single composite score
with a fully decomposed list of reasons, an **optional LiftWing damage-probability
signal** folded in as one supporting input among many, and a **priority-ranked
queue** that orders the highest-scoring (most review-worthy) edits first. It is
built for an experienced patroller who needs the queue to be *interpretable and
trustworthy*, not a black box.

A second, governance-level concern is baked into the shipped semantics: editor
identity (anonymous IP, temporary account) is predictive of risk but must never
*be* the verdict. The operator should understand that identity acts only as a
**bounded, visible, explainable modifier** — capped so it can never dominate a
rank by itself.

## Proposal

What the shipped feature lets an operator do:

- **Read one composite score per edit.** Each queued revision carries an integer
  score, clamped to a configured `0..max_score` band
  (`scoring_engine.rs:45`, default band `0..100`). The score starts from a base
  and accumulates weighted signal contributions
  (`score_edit_with_context`, `scoring_engine.rs:28`).
- **See *why* it scored that way.** The score is never opaque: it ships with an
  itemized list of `SignalContribution`s — each a named signal, its weight, and a
  human-readable note (`CompositeScore`, `types.rs:286`; `push_signal`,
  `scoring_engine.rs:459`). Signals span edit-shape cues (new page, large content
  removal, bot-like edit), comment/diff cues (profanity, link spam, mass blanking,
  inserted profanity, repeated-character noise), reverted-before and trusted-user
  tags, warning history, obvious-vandalism fast-lane, duplicate patterns, and
  combination rules (`scoring_engine.rs:88`–`379`).
- **Work the queue top-down.** Scored edits are ordered highest-score-first; ties
  break deterministically by insertion order so the queue is stable across runs
  (`build_ranked_queue`, `queue_builder.rs:14`; `PriorityQueue`,
  `priority_queue.rs:27`).
- **Have a damage-probability ML signal folded in when available.** If a LiftWing
  `revertrisk`/`damaging` probability is fetched for the revision, it is
  normalized to `0.0..=1.0`, scaled by its policy weight, and contributed as one
  `LiftWing risk` signal — *supplementary, never required*
  (`apply_liftwing_signal`, `scoring_engine.rs:249`; ADR-0001 §9). A missing or
  non-finite probability simply contributes nothing
  (`scoring_engine.rs:255`, `482`).
- **Understand identity as a capped modifier, not a verdict.** Anonymous editors
  (`Anonymous user`, weight +25) and temporary-account editors
  (`Temporary account`, weight +20) add a risk modifier *only when the policy
  enables those modifiers* (`apply_editor_signal`, `scoring_engine.rs:403`). The
  combined identity contribution is then clamped to a policy cap (±25 in the
  active French-Wikipedia policy); when the cap bites, the operator sees an
  explicit `Identity cap adjustment` line in the reasons
  (`apply_identity_contribution_cap`, `scoring_engine.rs:434`;
  `configs/scoring/active/frwiki-vandalism.yaml:17`). The same edit assessed
  identically regardless of identity once that cap is reached — identity can
  never solely dominate a rank.
- **Trust that constructive maintenance sinks, not rises.** Adding only
  references, categories, interwiki links, or wikilink wrappers contributes
  *negative* weight, lowering patrol priority; trusted-user and bot-like edits
  carry strong negative weights (`apply_boolean_context_signals`,
  `scoring_engine.rs:278`; active policy weights `trusted_user: -40`,
  `bot_like_edit: -50`, `reference_addition: -14`).
- **Suppress trusted editors and boost repeat-offender bursts at queue level.**
  A `QueueHeuristicPolicy` can name trusted usernames (registered editors then
  receive the trusted-user reduction) and can boost newcomer-like duplicate
  clusters — many similar edits from different fresh accounts/IPs hitting the same
  page raise the rank of the whole cluster (`build_ranked_queue_with_policy`,
  `queue_builder.rs:58`).
- **Read the rank at a glance and trim the queue.** The patrol UI maps a score to
  a colour tier — red at ≥70, amber at ≥30, green below — so the operator scans
  severity visually (`score_tier`, `components/style.rs:5`). An operator-set
  minimum-score filter and a result limit trim the visible queue
  (`filter_edits`, `queue_controller.rs:129`; limit at `queue_controller.rs:35`).
- **Get a score-gated action suggestion.** The live-operator preflight suggests an
  action from the score and its signals: rollback when obvious-vandalism /
  duplicate-pattern signals fire or score ≥70; undo when a duplicate pattern fires
  or score ≥40 (unless trusted-user suppression is present); mark-patrolled only
  for clean, low-score, unpatrolled edits (score <60). The suggestion is advisory;
  the operator still chooses (`is_recommended`, `live_operator.rs:373`).
- **Rely on score semantics living in reviewable policy, not hidden constants.**
  Weights, thresholds, the identity cap, queue limits, the LiftWing contribution
  ceiling, and combination rules are all declared in a human-readable, lifecycle-
  staged policy file (`active` / `candidate` / `suggested`) that compiles into the
  runtime config (`configs/scoring/active/frwiki-vandalism.yaml`;
  `compile_scoring_policy`, `scoring_policy.rs:260`).

## Definition of Done

Re-framed as characterization. Each item is already true and bound to an existing
test.

- [x] An edit accumulates multiple weighted signals into one composite score with
  itemized reasons — verified by
  `scoring_engine.rs::scores_multiple_positive_signals`.
- [x] A bot-like edit applies a strong negative weight that reduces the total —
  verified by `scoring_engine.rs::bot_signal_reduces_total`.
- [x] The final score is clamped into the configured `base_score..max_score` band
  and never escapes it, even under extreme weights — verified by
  `scoring_engine.rs::extreme_weights_do_not_overflow_total_or_scaling` and the
  property test `scoring_engine.rs::property_score_stays_within_config_bounds`.
- [x] The total always equals the clamped sum of its emitted contributions
  (explainability is faithful) — verified by
  `scoring_engine.rs::property_total_matches_clamped_contribution_sum`.
- [x] Each signal is emitted at most once in the reasons list — verified by
  `scoring_engine.rs::property_signals_are_emitted_at_most_once`.
- [x] Anonymous and temporary-account editors produce distinct, identity-specific
  signals — verified by `scoring_engine.rs::scores_multiple_positive_signals`
  (anonymous) and `scoring_engine.rs::temporary_accounts_get_distinct_identity_signal`.
- [x] Combined identity contribution is clamped to the policy cap, emitting an
  explicit `Identity cap adjustment` reason when it bites — verified by
  `scoring_engine.rs::applies_identity_cap_adjustment`.
- [x] A LiftWing probability is folded in as a supporting `LiftWing risk` signal
  when present — verified by
  `scoring_engine.rs::applies_warning_history_and_liftwing_context`.
- [x] A non-finite LiftWing probability is ignored and contributes no signal —
  verified by `scoring_engine.rs::ignores_non_finite_liftwing_risk_values`; a
  LiftWing probability is also normalized to `0.0..=1.0` before use — verified by
  `context_builder.rs::clamps_liftwing_probability_into_unit_interval`.
- [x] A LiftWing response is parsed into a unit-interval damage probability across
  the supported response shapes, rejecting out-of-range values — verified by
  `liftwing.rs::parses_direct_probability_shape`,
  `liftwing.rs::parses_scores_revertrisk_shape`, and
  `liftwing.rs::rejects_probability_outside_unit_interval`.
- [x] The queue orders the highest-scoring edit first — verified by
  `queue_builder.rs::ranks_highest_score_first` and
  `priority_queue.rs::dequeues_highest_priority_first`.
- [x] Edits with equal scores keep a deterministic (insertion) order — verified by
  `priority_queue.rs::preserves_insertion_order_for_equal_priorities`.
- [x] A named trusted user is suppressed via the trusted-user reduction at queue
  build time, dropping below a risky anonymous edit — verified by
  `queue_builder.rs::trusted_user_policy_suppresses_registered_editor`.
- [x] Repeated newcomer-like edits to the same page raise rank via a
  duplicate-pattern boost — verified by
  `queue_builder.rs::duplicate_cluster_boost_applies_to_newcomer_patterns`.
- [x] Contextual risk inputs (warning history, LiftWing) change the rank order —
  verified by `queue_builder.rs::applies_contextual_scores_when_ranking`.
- [x] The active French-Wikipedia policy compiles, carrying the identity
  contribution cap (25), the queue default limit (25), and its combination
  rules into the runtime config — verified by
  `scoring_policy.rs::parses_and_compiles_scoring_policy` and
  `scoring_policy.rs::loads_default_embedded_active_policy`.
- [x] The score maps to the operator-visible colour tiers at the 70 / 30 / 0
  thresholds — verified by `components/style.rs::score_tier_maps_thresholds`.
- [x] A high-scoring edit yields a rollback action recommendation in the
  live-operator preflight — verified by
  `live_operator.rs::preflight_recommends_rollback_for_high_score_edit`.

## Alternatives

The shipped shape implies several considered-and-rejected directions:

- **A single opaque ML score (LiftWing alone).** Rejected in favour of a
  deterministic local engine with LiftWing folded in only as one *supporting*
  signal that is never required (ADR-0001 §9; `apply_liftwing_signal`,
  `scoring_engine.rs:249`). This keeps the queue interpretable and resilient when
  the ML service is unavailable, and keeps the operator's trust anchored in
  readable reasons rather than a probability.
- **Letting identity drive rank directly.** Rejected: identity is admitted as a
  bounded modifier only, behind per-modifier enable flags and a hard contribution
  cap that emits its own audit line (`apply_identity_contribution_cap`,
  `scoring_engine.rs:434`). The design deliberately separates actor-risk suspicion
  from content suspicion so a fresh account making a good edit isn't buried.
- **A floating-point or model-emitted "confidence" total.** Rejected in favour of
  an integer composite of named, individually-tunable weights, so every point in
  the total is attributable to a reason the operator can read.
- **Re-sorting the queue in the view layer.** Rejected: ordering is produced once,
  deterministically, by a priority queue with a stable tie-break
  (`priority_queue.rs:27`); the UI only filters and limits an already-ranked list
  (`queue_controller.rs:91`).
- **Hard-coding weights and thresholds in Rust.** Rejected in favour of
  human-readable, lifecycle-staged policy files compiled at load
  (`scoring_policy.rs:260`), so scoring/ranking semantics can be reviewed and
  evolved as policy rather than code.

## Risks

- **The operator over-trusts the rank and skips reading the diff.** Mitigation:
  the score is advisory and always decomposed into reasons the operator is meant
  to read; action recommendations are suggestions, not auto-actions
  (`is_recommended`, `live_operator.rs:373`). There is no autonomous action path
  gated on score alone.
- **Identity weighting unfairly buries good newcomer edits.** Mitigation: identity
  contributes only behind enable flags and a hard ±cap with a visible adjustment
  line; constructive-maintenance signals carry negative weight to pull good edits
  down the queue (`scoring_engine.rs:403`, `:434`, `:278`). Residual risk: the cap
  is a *policy* number — a misconfigured policy could raise it (see Known gaps on
  the missing dedicated fairness/ranking regression test wired to the live engine).
- **A LiftWing outage or malformed response degrades ranking quality silently.**
  Mitigation: LiftWing is optional and a missing/invalid probability contributes
  nothing rather than erroring the score
  (`scoring_engine.rs:255`; `liftwing.rs:159`). The local deterministic score
  still ranks the queue.
- **Weight tuning produces a runaway or negative total.** Mitigation: the total is
  always clamped to the configured band and proven bounded under adversarial
  weights (`property_extreme_config_weights_never_escape_score_bounds`,
  `scoring_engine.rs`).
- **The operator misreads a low score as "safe."** Mitigation: the colour tier and
  the obvious-vandalism / duplicate-pattern fast-lane recommendations surface high
  risk independently of a raw threshold (`is_recommended`, `live_operator.rs:390`).

## Known gaps / drift

Factual observations noticed while reverse-engineering; not design proposals.

- **`min_score_cutoff` policy field is not enforced at runtime.** The active
  policy and the compiled `QueuePolicyConfig` carry `min_score_cutoff`
  (`scoring_policy.rs:85`; `configs/scoring/active/frwiki-vandalism.yaml:26`), but
  no scoring or queue-build code consumes it. The only score-based queue trimming
  is the *operator-set* `min_score` UI filter (`queue_controller.rs:129`), which is
  a separate, unrelated value. The policy cutoff is effectively documentation.
- **`account_age_modifier_enabled` is plumbed but unused.** It exists in policy and
  config (`ScoringIdentityConfig`, `types.rs:166`;
  `configs/scoring/active/frwiki-vandalism.yaml:20`, set `false`) but no scoring
  code reads it — there is no account-age signal in `scoring_engine.rs`. It is a
  reserved knob with no behaviour.
- **The LiftWing contribution clamp has no dedicated unit test.**
  `compile_liftwing_weight` clamps the configured weight to
  `±max_contribution` (`scoring_policy.rs:546`), and the active policy's weight
  (35) equals its `max_contribution` (35) so the clamp is a no-op in practice. No
  test exercises a configured weight *exceeding* the ceiling, so the clamp itself
  is uncovered.
- **Ranking and fairness eval fixtures parse but are not run against the live
  engine.** `evals/scoring/fixtures/vandalism_patrol/frwiki/ranking.yaml` and
  `fairness.yaml` declare ordering comparisons and per-cohort false-positive-
  regression checks, and the governance script asserts the files exist
  (`scripts/check-scoring-governance.sh:45`–`47`), but the only tests are
  *parse* tests (`scoring_evaluation.rs::parses_ranking_fixtures`,
  `::parses_fairness_fixtures`). No test feeds the declared ranking comparisons or
  fairness cohorts through `score_edit`/`build_ranked_queue` to assert the live
  engine actually satisfies them. The fairness invariant ("identity can never
  dominate") is enforced structurally by the cap, but not regression-tested
  against the corpus.
- **The diff-derived risk hints are computed but not wired into live scoring
  context.** `analyze_diff_for_scoring` (`diff_engine.rs:234`) produces
  `link_addition_only` / `reference_addition_only` / `mass_blanking_detected` /
  etc. hints, and `score_edit_with_context` consumes the corresponding
  `ScoringContext` flags (`scoring_engine.rs:278`). But the live context builder
  (`context_builder.rs:15`) only populates `user_risk` and `liftwing_risk` — it
  hard-codes every diff-derived flag to `Disabled` (`context_builder.rs:24`–`31`).
  So in the live operator path these diff-aware signals never fire unless a caller
  constructs the context manually; they are exercised only in unit tests
  (`scoring_engine.rs::applies_new_diff_aware_signals_from_context`).
- **Queue-level limits (`default_limit` / `max_limit`) are governance numbers, not
  runtime enforcement.** They compile into `queue_defaults`
  (`scoring_policy.rs:612`) and are validated for sanity
  (`scoring_policy.rs:402`), but the live queue's result cap comes from the UI
  filter's `limit` (`queue_controller.rs:35`), not from the compiled policy
  default. The two are not connected in code.
- **The trusted-user *tag* path and the trusted-user *username* path both feed the
  same signal but from different layers.** Tag-based trust is a per-edit signal in
  the scoring engine (`scoring_engine.rs:171`); username-based trust is a
  queue-policy override applied at ranking time (`queue_builder.rs:72`). An
  operator reading "Trusted user" in the reasons cannot tell from the score alone
  which path fired without reading the note. This is a transparency nuance, not a
  correctness bug.
