# ADR-0021: Scoring and ranking contract — composite score, policy schema, compile step

**Status:** Accepted
**Date:** 2026-07-11
**Author:** Luis Villa (drafted by Claude Code)
**Summary:** Edit scores are itemized explanations (a signed contribution per signal, integer total, no model confidence); all tuning lives in versioned per-wiki policy files compiled to a hot-path struct, and the queue ranks deterministically on the total — the mechanism is platform, the signal catalogue is patrolling-specific today.

**As-built:** retroactive characterization of a shipped contract (PRD-0003).
Records the composite-score shape, the scoring-policy file schema, and the
compile-to-runtime step as they exist, pinned by the scoring tests, the eval
fixtures, and `scripts/check-scoring-governance.sh`. Spawned from issue #19.

## Context

PRD-0003 (edit scoring and queue ranking) owns the user-facing meaning of scores
and ranks. ADR-0001 §9 fixes only that LiftWing is a supplementary, cached,
never-required signal, and `docs/platform/scoring/SCORING_CONSTITUTION.md` is a
governance charter — neither records the *structural* contract: what a score is,
how a policy file is shaped, and how a file becomes a runtime scorer. That
contract lives in code (`sp42-platform`) and is mechanically governed, so it
warrants an ADR. The *mechanism* is domain-neutral, but the signals it scores are
currently patrolling-specific — see Consequences for where that seam sits.

## Decision

### 1. The composite score is an explanation, not a number
`CompositeScore { total: i32, contributions: Vec<SignalContribution> }`. Each
`SignalContribution { signal: ScoringSignal, weight: i32, note: Option<String> }`
carries the **signal identity** (a closed 22-variant `ScoringSignal` enum, never
a free string), the **signed weight** it added (negative for trust/constructive
signals), and an optional human rationale. Scores are integers; there is no
model-reported confidence. The applied weight *is* the contribution; raw inputs
(byte delta, LiftWing probability) survive in the note.

### 2. Policy lives in versioned files; mechanics live in code
All tuning — weights, thresholds, caps, enablement, LiftWing role — lives in
per-wiki YAML under `configs/scoring/{active,candidate,suggested}/`, described by
`schemas/scoring-policy.schema.json` and mirrored by `ScoringPolicyDocument`
(`sp42-platform::scoring_policy`). Rust holds only structural defaults, and those
are sourced from the embedded active policy (a `OnceLock` baseline every
`Default` delegates to) — the "safe fallback in code, tuning in policy" rule
(Constitution §14.3). A policy carries `dimensions`, `identity`
(`contribution_cap`), `queue` limits, `signal_parameters`, `rules`
(slug → `{enabled, weight, …}`), `combination_rules`, `external_evaluation`
(LiftWing), and a `fairness` budget.

### 3. Compile once to a hot-path struct
YAML → `parse_scoring_policy` (deserialize + `validate_scoring_policy`) →
`compile_scoring_policy` → `CompiledScoringPolicy`. Validation requires all 20
tunable rules present, non-blank metadata, a non-negative cap, sane queue limits,
and non-empty marker lists. Compilation flattens the human document into the
hot-path `ScoringConfig` (`ScoreWeights` etc.), resolving disabled rules to
weight 0 and clamping the LiftWing weight to its role and `max_contribution`.
A `WikiConfig` compiles its policy on materialization (default ref
`active/default-language-agnostic`).

### 4. Ranking keys on the total, deterministically
`build_ranked_queue*` scores each `EditEvent` and orders by `CompositeScore.total`
via a binary heap, breaking ties FIFO by insertion sequence. Queue-level
heuristics (trusted-user suppression, newcomer duplicate-cluster boosting) enrich
the `ScoringContext` before scoring. Presentation severity tiers (Low/Medium/High
at 30/70) are a separate mapping, not the ranking key.

### 5. LiftWing is one clamped signal among many (honors ADR-0001 §9)
LiftWing risk enters as `ScoringContext.liftwing_risk: Option<f32>`; absent, it
is a no-op. Present, it is normalized, scaled by the configured weight, clamped to
`±max_contribution`, and pushed as a single `LiftWingRisk` contribution. Local
ranking never depends on it.

## Consequences

- Scoring is a reusable platform *mechanism* — the accumulation types, engine,
  policy compiler, and deterministic ranking live in `sp42-platform` (reached
  today through the `sp42-core` re-export facade pending the ADR-0013
  relocation), and per-wiki policy *files* are domain tuning. But the mechanism
  is the only domain-neutral part today. The `ScoringSignal` catalogue is
  entirely patrolling / anti-vandalism vocabulary (`AnonymousUser`,
  `MassBlanking`, `BotLikeEdit`, `ObviousVandalism`, `LiftWingRisk`, …), and
  every consumer is the patrol stack (`sp42-patrol`, the live queue, the patrol
  surfaces) — there is no references- or assessment-domain consumer.
  Cross-domain reuse is therefore forward-looking, not current; and because
  `ScoringSignal` is a *closed* enum, a new domain cannot add a signal without
  editing this platform type. That closed enum is the domain seam any future
  reuse must cross. The ADR lives in platform (not patrolling — cf. ADR-0020,
  which kept a straddling contract in its domain because `LiveOperatorView` is
  an *aggregation*) because here the *mechanism* is what warrants the
  reusable-contract treatment; the patrolling-specific catalogue is merely what
  that mechanism currently carries.
- `scripts/check-scoring-governance.sh` mechanically enforces the doc/schema/
  config/eval triad: it pins the existence and key lines of the two docs, both
  JSON schemas, the active/candidate policies, the eval profile, and the four
  per-domain fixtures (`regression`/`ranking`/`invariants`/`fairness`). This is
  the enforcement of "no hidden tuning in code."
- Pinned by unit tests in `scoring_policy.rs` (parse/compile/embedded-load),
  `scoring_engine.rs` (signal application), and `queue_builder.rs`/
  `priority_queue.rs` (ranking order, contextual boosts), plus the `evals/scoring/`
  fixtures.

## Non-goals

- The user-facing *meaning* of scores and the review workflow — PRD-0003.
- Raw LiftWing fetch/caching mechanics — `sp42-platform::liftwing`, framed by
  ADR-0001 §9.
- `min_score_cutoff` / limit trimming is carried in the queue policy but applied
  by the caller (`live_queue.rs`), not by `build_ranked_queue` itself.
- `ExternalEvaluatorRole::TieBreaker` is defined but not yet special-cased (the
  compiler distinguishes only disabled vs enabled).
