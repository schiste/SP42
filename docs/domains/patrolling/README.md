# Patrolling

The shipped SP42 domain: a Wikipedia patrol review workflow built on the
[platform](../../platform/README.md) layers. An operator opens a live wiki view,
the queue auto-advances, the diff is inspected in place, and a disposition is
chosen — with scoring, action preflight, and identity gating handled by the
platform underneath.

## Product Requirements

- [PRD-0002 — Patrol review workflow](prd/0002-patrol-review-workflow.md) — the operator's main loop
- [PRD-0003 — Edit scoring and queue ranking](prd/0003-edit-scoring-and-queue-ranking.md) — composite score, itemized reasons, queue order
- [PRD-0004 — Reviewer actions on Wikimedia](prd/0004-reviewer-actions-on-wikimedia.md) — rollback, undo, patrol, tag, inline edit
- [PRD-0005 — Operator identity and session](prd/0005-operator-identity-and-session.md) — server-derived identity and capability gating
- [PRD-0006 — Multi-operator coordination](prd/0006-multi-operator-coordination.md) — presence, claims, and room state

## Architecture Decision Records

- [ADR-0020 — Live-operator-view contract (the server-assembled patrol payload)](adr/0020-live-operator-view-contract.md)

## Related platform decisions

The patrol workflow builds mostly on platform ADRs: scoring
([ADR-0021](../../platform/adr/0021-scoring-and-ranking-contract.md),
[SCORING_CONSTITUTION](../../platform/scoring/SCORING_CONSTITUTION.md)), the
reviewer-action contract ([ADR-0022](../../platform/adr/0022-reviewer-action-contract.md))
and its content-edit mechanism ([ADR-0003](../../platform/adr/0003-node-anchored-wikitext-editing.md)),
identity ([ADR-0002](../../platform/adr/0002-local-dev-auth-bridge.md)), and
coordination ([ADR-0023](../../platform/adr/0023-coordination-contract.md),
the `sp42-coordination` layer). Its one domain-owned decision is the
live-operator-view contract above (ADR-0020), which aggregates those platform
mechanisms into the patrol-review payload.

## Landscape research

- [patrol-tool-landscape.md](patrol-tool-landscape.md) — feature inventory of existing Wikipedia patrol and anti-vandalism tools, with a comparison matrix
