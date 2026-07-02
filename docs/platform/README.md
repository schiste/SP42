# Platform

Shared abstraction layers that domains consume. Platform code lives in
`crates/sp42-types`, `sp42-coordination`, `sp42-wiki`, `sp42-server`,
`sp42-devtools`, the shared half of `sp42-core` (contracts, runtime, scoring
engine), and `xtask`.

## Operational docs

- [RUNTIME_CONFIGURATION.md](RUNTIME_CONFIGURATION.md) — server environment, API base URL, and local dev auth
- [DESKTOP_DISTRIBUTION.md](DESKTOP_DISTRIBUTION.md) — desktop shell backends, build prerequisites, signing/notarization
- [DEVELOPER_SURFACE.md](DEVELOPER_SURFACE.md) — localhost reporting, action/live boundaries, telemetry, milestone boundaries
- [CLI.md](CLI.md) — `sp42-cli` command-line reference: flags, modes, environment, exit codes
- [FRONTEND_DESIGN_CONTRACT.md](FRONTEND_DESIGN_CONTRACT.md) — binding frontend design spec
- [CODEX_DESIGN_SYSTEM.md](CODEX_DESIGN_SYSTEM.md) — how SP42 aligns to Wikimedia Codex (tokens, not Vue components) and where it deliberately diverges

## Scoring

The scoring engine is a platform layer: patrolling consumes it today, and other
domains may consume it as the system expands toward sourcing and quality signals.

- [scoring/SCORING_CONSTITUTION.md](scoring/SCORING_CONSTITUTION.md) — scoring mission, principles, and signal philosophy
- [scoring/POLICY_LAYOUT.md](scoring/POLICY_LAYOUT.md) — policy/evaluation directory layout and lifecycles

## Architecture Decision Records

- [ADR-0001 — Foundational architectural decisions](adr/0001-foundational-decisions.md)
- [ADR-0002 — Local dev-auth bridge contract](adr/0002-local-dev-auth-bridge.md)
- [ADR-0003 — Node-anchored wikitext editing](adr/0003-node-anchored-wikitext-editing.md)
- [ADR-0004 — Crate boundaries for collaborative ownership](adr/0004-crate-boundary-collaboration-model.md)
- [ADR-0005 — Design system and shared component layer (`sp42-ui`)](adr/0005-design-system-shared-component-layer.md)
- [ADR-0006 — Using LLMs: model panel, measured agreement, inference endpoint](adr/0006-using-llms.md)
- [ADR-0010 — Operator-confirmed content proposals (propose/confirm)](adr/0010-operator-confirmed-content-proposals.md)
- [ADR-0012 — Frontend end-to-end testing approach](adr/0012-frontend-e2e-testing-approach.md) (Proposed)
- [ADR-0013 — Layered platform/domain architecture with mechanical enforcement](adr/0013-layered-platform-domain-architecture.md)
- [ADR-0014 — Required Wikimedia OAuth login + any-Wikimedia-project resolution](adr/0014-wikimedia-oauth-and-any-project.md)
- [ADR-0016 — Wikidata entity content-model: revision read, `EntityDiff`, and content-model routing](adr/0016-wikidata-entity-content-model.md) (Proposed)
- [ADR-0017 — Wikidata statement-proposal write contract](adr/0017-wikidata-statement-proposal-write-contract.md) (Proposed)

ADR-0006 defines the provider-agnostic LLM interface every capability reaches a
model through, and ADR-0010 defines the propose/confirm editing pattern domains
reuse — both are platform layers even though they landed alongside the citation
work. The citation-specific ADRs that build on them live in
[domains/references/adr/](../domains/references/adr/).
