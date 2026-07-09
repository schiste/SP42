# Domains

Capabilities built on the [platform](../platform/README.md) layers. Each domain
owns its product requirements (PRDs) and any domain-specific architecture
decisions (ADRs); both consume shared platform abstractions rather than
reimplementing them.

## Patrolling — shipped

The review workflow: live ingestion, scoring and queue ranking, in-place diff
review, reviewer actions on Wikimedia, operator identity, and multi-operator
coordination. Implemented across `sp42-live`, `sp42-reporting`, and the
`sp42-cli` / `sp42-app` / `sp42-desktop` shells, on top of `sp42-core`.

→ [patrolling/](patrolling/README.md)

## References / citation verification — incoming

LLM-assisted verification of whether a cited source supports a claim, reported as
an informational verdict (never an autonomous edit). No crate yet; specified as a
PRD plus ADR-0007–0009, building on the platform LLM interface (ADR-0006).

→ [references/](references/README.md)

## Wikidata — incoming

Making Wikidata a first-class target: reviewable in the patrol workflow via a shared
platform entity read/diff mechanism (entity JSON, not wikitext), and a structured,
referenced source/sink of facts for the citation work. No crate yet; specified as
PRD-0011, spawning platform ADR-0015 and continuing ADR-0014.

→ [wikidata/](wikidata/README.md)

## Assessment — incoming

Assisting per-article quality assessments, starting with Good-article review:
grounded evidence against the assessable criteria (citation support, stability,
media licensing, structural lint), rendered for the reviewer, with judgments and
pass/fail staying human. No crate yet; specified as PRD-0015 plus the GA-workflow
design sketch, consuming the references domain's verification path (ADR-0011).

→ [assessment/](assessment/README.md)

More domains are anticipated; each will follow the same platform-consumes pattern.
