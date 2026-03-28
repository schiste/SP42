# SP42 Scoring Constitution

Status: Draft v0.2  
Scope: SP42 scoring, ranking, recommendation, policy, evaluation, and future content-review workflows

## 1. Mission

SP42 scoring exists to prioritize human review attention.

Its purpose is to help reviewers decide what deserves review first, why it deserves review, and what action may be appropriate. Scoring is a triage system. It is not a truth engine and it is not an autonomous editorial authority.

## 2. Long-Term Direction

SP42 begins as a vandalism-patrolling tool, but it must be designed as a broader content-review engine.

The scoring system must therefore be able to grow from:

- vandalism patrol

into later domains such as:

- content quality review
- sourcing and verification workflows
- policy-compliance review
- structured-data validation
- maintenance and cleanup queues
- content CI/CD style review pipelines

The architecture must not trap the project inside a vandalism-only model.

## 3. Primary Objective

The primary objective of SP42 scoring is to maximize useful human attention allocation.

This means the system should optimize for:

- reviewer trust in the queue
- reviewer speed and focus
- understandable and actionable prioritization
- fast surfacing of obviously harmful changes

It must not optimize for raw predictive accuracy alone if that would degrade interpretability, fairness, maintainability, or operator trust.

## 4. Non-Goals

SP42 scoring must not be treated as:

- a truth engine
- a replacement for human judgment
- a legitimacy engine based on user status
- a pure benchmark-accuracy exercise
- justification for irreversible automation based on score alone

## 5. Core Principles

### 5.1 Human Review Supremacy

Scoring may recommend attention and actions. It does not replace editorial judgment.

### 5.2 Explainability First

Every score must be decomposable into readable, human-meaningful reasons.

### 5.3 Policy Over Hidden Tuning

Weights, thresholds, combinations, and rule parameters should live in human-readable policy, not hidden code constants, wherever practical.

### 5.4 Fairness Matters

A more predictive system is not automatically a better system. False positives against newcomers, temporary accounts, and anonymous contributors are a first-class quality concern.

### 5.5 Extensibility Matters

The scoring framework must support multiple content-review domains without redesigning its foundations.

## 6. Signal Philosophy

Scoring must be decomposed into explicit dimensions.

The constitutional dimensions are:

- content
- actor
- subject
- context
- temporal
- policy
- external evaluation

These dimensions are intentionally broad so the system can scale beyond vandalism patrol.

### 6.1 Dimension Meaning

- Content: properties of the change itself
- Actor: properties and recent behavior of the contributor
- Subject: properties of the page, item, or target under change
- Context: workflow state, prior review state, trust, constraints, and public configuration
- Temporal: timing, bursts, repetition, recent attack windows, recency effects
- Policy: explicit rule-set influence, thresholds, mode choice, and local overrides
- External evaluation: auxiliary evaluators such as Lift Wing

No single dimension should silently absorb the work of the others.

## 7. Identity Principle

Identity-based signals may only act as modifiers. They must never be the primary driver of prioritization.

This applies especially to:

- anonymous status
- temporary-account status
- account age
- edit-count surrogates
- group and rights markers

Behavioral, structural, contextual, and policy-based evidence must be able to outweigh identity-based effects.

Identity signals may be used because they are predictive, but they must remain:

- bounded
- visible
- explainable
- independently tunable

### 7.1 Constitutional Default

The scoring architecture should include an explicit cap on identity-driven contribution.

The exact numeric cap may evolve by domain, but the constitutional rule is stable:

- identity contribution must be bounded at policy level
- identity contribution must be inspectable in explanations and evaluation
- no active policy may allow identity to dominate the final score by itself

If a future domain needs stronger actor-based weighting, that policy must still preserve the modifier-only principle.

## 8. Fairness Principle

SP42 scoring must explicitly evaluate false-positive impact on:

- newcomers
- temporary accounts
- anonymous contributors

A scoring improvement that increases raw detection while materially worsening fairness must not become active without explicit human approval.

Scoring must keep separate the concepts of:

- content-quality suspicion
- actor-risk suspicion

These may interact, but they must not be silently conflated.

## 9. External Evaluation Principle

External evaluators such as Lift Wing may be used when they provide additional value.

They are auxiliary evaluators, not constitutional foundations.

Therefore:

- they may strengthen ranking
- they may refine prioritization
- they may provide additional evidence
- they must remain optional
- they must never be the sole basis of prioritization or action

The system must remain usable and interpretable when external evaluators are unavailable.

## 10. Policy Principle

Scoring behavior should be defined by readable policy files wherever practical.

That includes:

- weights
- thresholds
- rule toggles
- dimension weights
- combinations
- queue cutoffs
- domain-specific overrides
- wiki-specific overrides

Active scoring policy must be:

- versioned
- reviewable
- attributable
- auditable

### 10.1 Required Policy Metadata

Every active scoring policy file should declare at least:

- `domain`
- `wiki_id` or an explicit shared-scope marker
- `policy_version`
- `evaluation_profile`
- `inherits_from` when applicable

This is required because the system is expected to grow into a content CI/CD framework and cannot rely on implicit policy meaning.

## 11. Evaluation Gate

No scoring change may become active without passing evaluation gates.

The minimum constitutional gates are:

- config validation
- invariants
- regression cases
- ranking checks
- fairness checks

Recommended additional gates:

- calibration reports
- explanation completeness checks
- latency and performance checks
- domain-specific acceptance suites

Evaluation failure blocks activation.

### 11.1 Constitutional Evaluation Baseline

At minimum, every active scoring policy must be evaluated against:

- regression fixtures
- ranking fixtures
- invariant checks
- fairness checks for newcomer-like actors

No policy may become active without this baseline.

## 12. Self-Improvement Principle

SP42 may learn from outcomes and generate policy suggestions.

SP42 may not silently promote those suggestions into active policy.

The constitutional policy lifecycle is:

- suggested
- candidate
- active

Suggested changes may be machine-generated. Candidate changes may be evaluated automatically. Active changes require explicit human approval.

### 12.1 Sandbox Rule

Sandbox experimentation may be allowed for suggested and candidate policies.

Sandbox execution may automatically:

- tune parameters
- compare candidate policies
- generate policy suggestions

Sandbox execution may not:

- silently replace the active policy
- mutate production thresholds without approval
- bypass evaluation gates

## 13. Operating Modes

SP42 may support multiple scoring modes, but each mode must remain explicit and inspectable.

Examples:

- high-confidence patrol assistance
- broad human review queue
- content quality triage
- structured-data constraint review
- CI/CD-style policy review

Each mode should declare:

- domain
- threshold policy
- evaluation profile
- action semantics

## 14. Technical Constitution

This section defines the engineering constraints of the scoring system itself.

### 14.1 Rust First

The scoring engine, policy loading, evaluation framework, and core reporting should be implemented in Rust.

Rust is the scoring system’s implementation language unless there is a very strong reason otherwise. This is required to preserve:

- consistency across targets
- type safety
- performance
- testability
- maintainability

### 14.2 Evaluation Everywhere

Every meaningful scoring behavior must be testable and evaluable.

That includes:

- signal extraction
- rule matching
- dimension scoring
- final ranking
- policy parsing
- policy validation
- policy migration
- explanation output

Scoring code without evaluation coverage is constitutionally incomplete.

### 14.3 No Hidden Tuning in Code

Operational weights, thresholds, and tunable rule parameters must not live as silent constants in code when they are part of product behavior.

Code may contain:

- structural defaults
- safe fallback values
- non-policy internal limits

But the following should be policy-driven wherever practical:

- score weights
- score caps
- thresholds
- signal enablement
- combination bonuses
- domain and wiki overrides

The scoring code should express mechanics. Policy should express tuning.

### 14.3.1 Allowed Rust Defaults

The following may remain in Rust code as structural defaults:

- schema defaults needed for deserialization safety
- internal safety bounds that prevent invalid execution
- fallback values used only when no active policy is available
- non-product technical limits such as parser or allocator safeguards

These defaults must not silently define real operational scoring behavior once an active policy exists.

### 14.3.2 Forbidden Hidden Tuning

The following must not remain implicit in Rust code if they affect active scoring behavior:

- domain-level thresholds
- live queue cutoffs
- scoring weights
- rule enablement
- combination bonuses
- fairness-sensitive identity multipliers
- domain- or wiki-specific overrides

### 14.4 No Unnecessary Repetition

The scoring system must avoid duplicated feature extraction, duplicated rule logic, duplicated threshold logic, and duplicated explanation paths.

There should be one clear implementation path for:

- feature extraction
- rule application
- dimension aggregation
- policy loading
- evaluation execution
- explanation generation

If two domains need similar logic, they should share abstractions rather than copy logic.

### 14.5 Optimization Is A Requirement

Scoring must be efficient enough for real-time review workflows.

Optimization is not optional polish. It is part of correctness.

The system should prefer:

- cheap local signals first
- bounded allocations
- reuse of parsed/configured state
- cache-friendly data flow
- predictable latency
- incremental enrichment over wasteful recomputation

The scoring system must not become too slow to support live patrol or future content CI/CD workflows.

### 14.6 Optimization Must Remain Explainable

Performance work must not destroy clarity.

The code should aim for:

- optimized hot paths
- readable policy
- testable modular logic
- explicit data flow

The right target is efficient and understandable code, not clever unreadable code.

### 14.7 Stable Abstraction Layers

The scoring architecture should be separated into at least the following layers:

- features
- rules
- policy
- evaluation
- explanation
- ranking

This separation is constitutional because it supports:

- extensibility
- performance tuning
- testability
- domain growth beyond vandalism patrol

### 14.8 Regression Safety

Any scoring change must be evaluated not only for correctness, but for accidental degradation caused by:

- syntax mistakes
- malformed policy
- missing punctuation or formatting mistakes
- unintended threshold shifts
- ranking inversions
- fairness regressions

This is why the evaluation gate exists.

### 14.9 Local-First Design

The scoring engine should assume local execution and local evaluation first.

External services may enrich the score, but core ranking must remain operable locally with local policy and local tests.

### 14.10 Public, Human-Readable Control

Scoring policy must remain understandable to a technically literate human without reading Rust source first.

This is especially important because scoring is intended to evolve into a content CI/CD framework, not only a vandalism heuristic engine.

## 15. Implementation Consequences

This constitution implies the following technical direction:

- explicit feature extraction
- explicit rule evaluation
- dimension-level scoring
- readable policy files
- evaluation corpora and ranking tests
- fairness-aware CI gates
- suggestion reports for policy tuning
- provenance-aware explanations

## 16. Default Constitutional Decisions

Until replaced by an explicit future revision, the constitution adopts the following defaults:

1. Identity contribution must be capped at policy level.
2. Every active policy file must declare a `domain`.
3. Every active policy file must declare an evaluation profile.
4. Sandbox auto-tuning is allowed only for suggested and candidate policies, never active policy.
5. External evaluators remain optional and non-authoritative.
6. Human-readable policy is the source of operational scoring behavior.

## 17. Questions Reserved For Future Revision

These questions remain open, but they do not block v0.2:

1. What exact fairness regression thresholds should block activation by domain?
2. Should action recommendation thresholds be constitutionalized separately from queue-priority thresholds?
3. What exact numeric form should the identity-contribution cap take?
4. Which evaluation profiles should be mandatory for each future domain?

## 18. Summary

SP42 scoring must be:

- human-centered
- policy-driven
- explanation-first
- evaluation-gated
- fairness-aware
- domain-extensible
- Rust-first
- optimized
- low-repetition
- self-improving only through reviewable policy change

That is the constitutional baseline.
