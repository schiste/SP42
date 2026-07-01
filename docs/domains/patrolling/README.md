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

## Related platform decisions

The patrol workflow relies on platform ADRs rather than owning its own: scoring
([SCORING_CONSTITUTION](../../platform/scoring/SCORING_CONSTITUTION.md)), the
content-edit mechanism ([ADR-0003](../../platform/adr/0003-node-anchored-wikitext-editing.md)),
identity ([ADR-0002](../../platform/adr/0002-local-dev-auth-bridge.md)), and
coordination (the `sp42-coordination` layer).

## Landscape research

- [patrol-tool-landscape.md](patrol-tool-landscape.md) — feature inventory of existing Wikipedia patrol and anti-vandalism tools, with a comparison matrix
