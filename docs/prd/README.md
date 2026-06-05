# Product Requirements Documents (PRDs)

A PRD captures the user-facing intent of a change — who it is for, what it lets
them do, and how we know it is done — before code is written. ADRs record *how*
the system is built; PRDs record *what* a change should achieve for operators
and editors.

PRDs and ADRs are complementary, not sequential. A PRD usually spawns one or
more ADRs as structural questions surface, and links them. An ADR can exist with
no PRD (a purely internal change).

## When a PRD is required

A PRD is required when a change:

- introduces or removes an operator- or editor-facing capability
- changes an operator workflow (what a reviewer does, in what order)
- changes what an operator or editor sees or is asked to decide
- changes scoring, ranking, or Wikimedia action *semantics* — the user-facing
  meaning of a result or action, as distinct from its implementation

A PRD is *not* required for purely internal changes (refactors, crate-boundary
moves, storage-format or runtime changes); those remain governed by the ADR
triggers in `GOVERNANCE.md`.

## Overlap with ADR triggers

Two ADR triggers are dual-natured and require **both** artifacts:

- *scoring policy, ranking behavior, or Wikimedia action semantics*
- *public contracts or APIs*

For these, the PRD owns the user-facing intent and the definition of done; the
ADR owns the structural decision. Open the PRD first; it links the ADR(s) it
spawns. The remaining ADR triggers (crate boundaries, runtime/deploy,
auth/token/CSRF, desktop packaging, storage formats) stay ADR-only.

## Lifecycle

`Draft` → `Discussion` → `Accepted` → `Implemented`, plus `Withdrawn`.

- `Draft` / `Discussion` — the document is mutable; intent is being refined.
  Open questions live in the PRD's Open Questions section, each stated with a
  proposed answer to react to.
- `Accepted` freezes Problem, Proposal, and Definition of Done, and requires
  every open question to be resolved — answered in-doc, or, if it is really
  implementation tracking rather than a design question, converted to a tracked
  issue and linked. An `Accepted` PRD carries no unresolved design question.
  Later learnings get an amendment note or a successor PRD, mirroring ADR
  immutability.
- `Implemented` cannot be claimed until every Definition-of-Done item maps to a
  passing test or observable in the closing PR — which CI already enforces under
  Constitution Article 5.

## Files

- `0000-template.md` — the PRD template.
- `NNNN-title.md` — one file per PRD, sequential, zero-padded.

## Proposed `GOVERNANCE.md` addition

Insert under `## Decision Making`, after the ADR trigger list:

```markdown
A change also requires a Product Requirements Document (PRD) when it introduces
or changes an operator- or editor-facing capability, an operator workflow, what
a user sees or decides, or scoring/ranking/Wikimedia action *semantics* (as
distinct from implementation). PRDs record user-facing intent and a definition
of done; they live in `docs/prd/` and link the ADRs they spawn. See
`docs/prd/README.md`.
```
