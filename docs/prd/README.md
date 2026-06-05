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

Some ADR triggers can also need a PRD:

- *scoring policy, ranking behavior, or Wikimedia action semantics*
- *public contracts or APIs* that change operator/editor behavior or external
  integration behavior

For these, the PRD owns the user-facing intent and definition of done; the ADR
owns the structural decision. Purely internal crate boundaries, runtime/deploy
mechanics, auth/token/CSRF implementation details, desktop packaging mechanics,
storage formats, and Rust API reshaping stay ADR-only unless they change
operator/editor behavior or external integration behavior.

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

## Retroactive (as-built) PRDs

Most PRDs are written before code. A feature that already shipped without one can
be documented retroactively, so future changes to it have an intent to amend
rather than reconstruct. An as-built PRD characterizes the *existing* user-facing
surface and follows the template with four adjustments:

- **State** is `Implemented` from the start, with an `**As-built:**` marker line
  noting it is a retroactive characterization — there is no forward "closing PR".
- **Related ADRs** replaces *Spawned ADRs*: an as-built PRD links the ADRs that
  already govern the feature; it did not spawn them.
- **Definition of Done** is a *characterization* — each item is a behavior that is
  already true, bound to an existing test or observable. These existing bindings
  stand in for the closing-PR check the lifecycle otherwise requires of
  `Implemented`. A behavior with no test is not a DoD item; it is recorded under
  Known gaps.
- **Known gaps / drift** replaces *Open questions*: factual observations of what is
  undocumented, inconsistent, or untested, surfaced while reverse-engineering — not
  design questions to resolve before acceptance.

An as-built PRD is `Implemented` but not frozen the way a forward PRD's acceptance
freezes it: later changes still amend or supersede it, and its Known gaps are the
backlog a forward PRD would later pick up.

## Files

- `0000-template.md` — the PRD template.
- `NNNN-title.md` — one file per PRD, sequential, zero-padded.
