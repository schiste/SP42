# Architecture Decision Records (ADRs)

An ADR records *how* the system is built — a structural decision and the forces
behind it — so a future reader can recover the reasoning without archaeology.
PRDs record *what* a change should achieve for operators and editors; ADRs
record the decision that makes it so.

PRDs and ADRs are complementary, not sequential. A PRD usually spawns one or
more ADRs as structural questions surface, and links them. An ADR can exist with
no PRD (a purely internal change).

## When an ADR is required

The triggers live in `GOVERNANCE.md` (§ Decision Making) — crate boundaries or
shared contracts, runtime/deploy behavior, auth/CSRF/token handling,
scoring/ranking/Wikimedia-action semantics, desktop packaging, and persistent
storage formats or public APIs. This document does not restate that list; it
covers *how to write* an ADR once one is required.

ADRs live under `docs/platform/adr/` (platform-layer decisions) or
`docs/domains/<domain>/adr/` (domain-specific). ADR numbers are globally unique
across folders. A contract that straddles a domain and the platform lives with
its *owner*: a domain aggregation is a domain ADR even if the mechanism it leans
on is platform (cf. ADR-0020); a reusable mechanism is a platform ADR even if
its only consumer today is one domain (cf. ADR-0021). State which and why.

## Header

Every ADR opens with the same field block, in this order:

```
# ADR-NNNN: <decision title>

**Status:** Proposed | Accepted | Superseded
**Date:** YYYY-MM-DD
**Amended:** YYYY-MM-DD — <reason> (#N)   (optional, append-only)
**Author:** <name / @handle>
**Summary:** <one sentence — the decision, not the topic>
```

`**Date:**` is the acceptance date and never changes. `**Amended:**` is optional
and append-only: one dated pointer per later clarification (`date — reason (#N)`),
newest appended after a `;`. It records *that* the ADR was touched since
acceptance and why; the change itself is a dated note in the body (see
Lifecycle). Status is not one of these — an amended ADR is still `Accepted`.

**Summary is required.** It is the gist a reader — or an LLM, or a tool — should
get from the header alone, without opening the body. Rules:

- State *what was decided*, not the area the title already names. Title:
  "Scoring and ranking contract." Summary: "scores are itemized explanations,
  not bare numbers; the queue ranks deterministically on the integer total."
- One sentence. A semicolon-joined compound is fine; a paragraph is not.
- It is **normative**: if the Decision section changes, the Summary changes with
  it. A Summary that no longer matches the Decision is a bug.
- It is distinct from the `**As-built:**` marker (below), which records
  *provenance*, not the decision.
- A genuinely multi-part or foundational ADR (e.g. a bundle of independent
  decisions) may use its Summary to say so and point the reader into the body,
  rather than compress faithfully-impossible content into one misleading line.
  (ADR-0001 is the example.)

The architecture map surfaces the Summary in its ADR index
(`scripts/generate-architecture-map.sh`), so it is the one-line description of
the ADR everywhere the map is read.

## Sections

- **Context** *(required)* — the forces: the problem, constraint, or ambiguity
  that forces a decision. Not background for its own sake.
- **Decision** *(required)* — what was decided. Number the sub-decisions when the
  ADR records a multi-part contract.
- **Alternatives** *(required for a forward ADR; omitted for as-built)* — what
  else was considered and why not.
- **Consequences** *(required)* — what the decision makes easy, hard, or
  forecloses, and how it is enforced or pinned (the tests/checks that hold it).
- **Non-goals** *(optional)* — what the ADR deliberately does not decide, and
  where those concerns live instead. Use it to bound a contract against its
  neighbours.

## Lifecycle and immutability

`Proposed` → `Accepted`, plus `Superseded`.

- `Proposed` — the decision is still under discussion; the document is mutable.
- `Accepted` — the decision holds and the document is **immutable in substance**.
  A later divergence that *reverses* the decision is corrected by a *new* ADR that
  supersedes this one (mark the old `**Status:** Superseded` and link forward). A
  *clarification* that does not reverse the decision — a wording reconciliation
  with shipped semantics, an added layer note (e.g. ADR-0005's) — stays in place
  as a dated note in the body **and** is recorded in the `**Amended:**` header
  field, never as a silent edit. Status stays `Accepted` through any number of
  amendments; only a reversal changes standing.
- `Superseded` — replaced by a later ADR; keep the file with a forward link so
  the history stays legible.

## Retroactive (as-built) ADRs

A contract that shipped without an ADR can be documented retroactively, so future
changes to it amend an intent rather than reconstruct one. An as-built ADR
characterizes the *existing* contract — verified against the code and its tests,
not against the prose of the spawning PRD — with these adjustments:

- **Status** is `Accepted` from the start, with an `**As-built:**` marker line
  noting it is a retroactive characterization with no forward "closing PR", and
  the issue it was spawned from.
- **Alternatives** is omitted — there is no honest record of options weighed
  before implementation, and inventing one would be fiction. Put any needed
  justification in Decision or Consequences.
- **Consequences** and **Non-goals** characterize what exists, including where
  reality diverged from the spawning PRD (say so plainly — e.g. "six variants,
  the as-built count, vs the PRD's five") and any wired-but-gated gaps.
- The **Summary** still states the decision as built.

An as-built ADR is `Accepted` but, like an as-built PRD, its characterization can
still be amended or superseded as the contract evolves.

## Files

- `adr-template.md` — the ADR template.
- `NNNN-title.md` — one file per ADR, under the owning `adr/` folder, sequential
  and zero-padded, numbers globally unique across folders.
