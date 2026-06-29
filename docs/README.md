# SP42 Documentation

SP42 is a **platform** that owns shared abstraction layers, with **domains** that
consume them. The documentation is organized along that seam.

## Platform

Shared, domain-agnostic layers: runtime configuration, desktop distribution, the
developer surface, the frontend design contract, the scoring engine, and the
foundational architecture decisions (ADR-0001–0006).

→ [docs/platform/](platform/README.md)

## Domains

Capabilities built on top of the platform.

- **Patrolling** — the shipped review workflow (queueing, scoring, diffing,
  reviewer actions, multi-operator coordination). → [docs/domains/patrolling/](domains/patrolling/README.md)
- **References / citation verification** — incoming; no crate yet. Lives as a PRD
  plus ADR-0007–0009. → [docs/domains/references/](domains/references/README.md)

→ [docs/domains/](domains/README.md)

## Project & process

- [STATUS.md](STATUS.md) — phase-by-phase implementation status
- [../CONSTITUTION.md](../CONSTITUTION.md) — binding engineering laws
- [../GOVERNANCE.md](../GOVERNANCE.md) — maintainer model and decision process
- [../CONTRIBUTING.md](../CONTRIBUTING.md) — contributor workflow and local checks
- [process/prd-protocol.md](process/prd-protocol.md) — when a PRD is required and its lifecycle
- [process/prd-template.md](process/prd-template.md) — PRD template
- [process/adding-a-domain.md](process/adding-a-domain.md) — turnkey path for owning a domain
- [platform/adr/0013-layered-platform-domain-architecture.md](platform/adr/0013-layered-platform-domain-architecture.md) — the layered platform/domain architecture and how it is mechanically enforced
- [platform/adr/0014-wikimedia-oauth-and-any-project.md](platform/adr/0014-wikimedia-oauth-and-any-project.md) — required Wikimedia OAuth login + resolving any Wikimedia project from an embedded SiteMatrix snapshot

## How ADRs and PRDs are filed

Architecture Decision Records (ADRs) and Product Requirements Documents (PRDs) are
filed by the platform layer or domain they govern:

- Platform ADRs live in [platform/adr/](platform/adr/).
- Domain ADRs and PRDs live under `domains/<domain>/adr/` and `domains/<domain>/prd/`.

**Numbering is global.** ADR numbers are unique across all folders, and so are PRD
numbers — a new record takes the next free number regardless of which folder it
lands in. ADRs and PRDs cross-reference each other by textual ID (e.g. `ADR-0006`,
`PRD-0001`), not by file path, so records can be refiled without breaking links.
