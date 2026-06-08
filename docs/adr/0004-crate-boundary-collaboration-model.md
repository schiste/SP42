# ADR-0004: Crate boundaries for collaborative feature ownership

**Status:** Accepted
**Date:** 2026-06-06
**Author:** SP42

**Implementation note:** Accepted after PR #15 landed the initial crate-boundary
batch and closed issues #6-#13. Follow-up work should use this ADR as the
contract for future crate splits. The first `sp42-types` slice now owns
transport, storage, and platform dependency contracts.

## Context

SP42 is becoming a multi-maintainer open source project. The workspace already
has separate shell crates for the browser, server, CLI, and desktop targets, but
most domain behavior still lives in `sp42-core`.

That was useful while contracts were changing quickly. It now creates review and
ownership risks:

- unrelated features can collide inside one broad crate
- `sp42-core` can become a god crate
- feature ownership is hard to assign
- CLI, desktop, browser, and server code can drift when shared orchestration has
  no clear home
- PR scope is harder to review when one change crosses many domains

This is an ADR-only change. It defines internal architecture and ownership
boundaries; it does not change operator-facing behavior, so no PRD is required.

## Decision

SP42 will use Rust crates as bounded-context and review boundaries for large
feature areas. A crate split is justified when it improves ownership,
reviewability, dependency direction, and API stability. A split is not justified
when it only renames modules.

Crate boundaries must follow these rules:

- **Single responsibility:** one domain crate owns one coherent feature area and
  public contract.
- **Open/closed:** adding a wiki, shell, runtime adapter, report consumer, or
  deployment target should add config or an implementation, not edits across
  every caller.
- **Substitution:** shared traits need deterministic test doubles that satisfy
  the same contract as production adapters.
- **Small interfaces:** crates expose narrow APIs, not catch-all service objects.
- **Dependency inversion:** domain logic depends on traits and data contracts;
  shell crates provide concrete I/O adapters.
- **DRY:** route contracts, fixtures, filtering, reporting, and cross-target
  construction have one authoritative implementation.

Cargo features are not an ownership boundary. Use them for optional adapters or
platform support; use crates for ownership and review scope.

Dependency flow remains one-way:

```text
shared contracts
  -> domain crates
  -> shell crates and deployment adapters
```

Domain crates must not depend on `sp42-server`, `sp42-app`, `sp42-cli`,
`sp42-desktop`, or Tauri.

## Target Vision

The final architecture separates stable contracts, domain behavior, and shells:

```text
crates/
  sp42-types/         # shared contracts, errors, traits, branding
  sp42-wiki/          # wiki config, registry, capability profiles
  sp42-live/          # EventStreams, backlog runtime, queue filtering
  sp42-coordination/  # messages, codecs, state, room runtime
  sp42-actions/       # patrol/rollback/undo/save action contracts
  sp42-reporting/     # reports, digests, summaries, shell state
  sp42-devtools/      # deterministic fixtures and previews
  sp42-core/          # compatibility facade during migration, then shrink
  sp42-server/        # HTTP server, auth/session, static serving, adapters
  sp42-app/           # browser UI shell
  sp42-cli/           # CLI shell
  sp42-desktop/       # desktop shell
```

This is the target shape. The project should split early when a contract is
clear, but it should not split a domain before the public API is useful to
current callers and likely to survive the next real caller.

`sp42-core` is transitional. It may re-export stable APIs while callers migrate,
but it should stop accumulating new domain ownership once a target crate exists.

## Contract Stabilization Checklist

Before a domain becomes its own crate, check the contract:

1. **Boundary:** define what the crate owns and what it must not own.
2. **Consumers:** name current callers and the likely next caller.
3. **Public contract:** identify request/response structs, errors, traits, and
   feature flags that callers will use.
4. **Tests:** move deterministic fixtures or doubles with the contract.
5. **Dependency direction:** confirm the crate does not depend on shell crates,
   deployment adapters, or UI frameworks.
6. **Behavior:** land extraction before behavior changes.

When a contract is still unclear, keep the code in `sp42-core` behind module
boundaries and stabilize the API there first.

This is a PR-description checklist, not a separate approval process.

## Split Decisions

Initial split decisions:

- **Split now: `sp42-devtools`.** Preview/report fixtures are deterministic,
  shared by CLI and desktop, and not production runtime behavior.
- **Split early: `sp42-reporting`.** Reports, digests, summaries, and shell
  state already serve multiple shells and should not drift.
- **Split early: `sp42-wiki`.** Multiwiki support is a production requirement;
  wiki config, registry, defaults, and capability profiles need an Open/Closed
  boundary.
- **Stabilize then split: `sp42-coordination`.** The room/state/message contract
  should be stable before extraction because it affects multi-user behavior.
- **Stabilize then split: `sp42-live`.** EventStreams, backlog runtime, queue
  filtering, and multiwiki defaults should settle together.
- **Stabilize then split: `sp42-actions`.** MediaWiki action contracts should
  wait for authenticated validation and the content-editing ADR outcome.
- **Split narrowly: `sp42-types`.** The first slice owns transport, storage, and
  platform dependency contracts. Broad shared-type extraction remains deferred.

## `sp42-types` Strategy

`sp42-types` is a narrow contract crate, not a catch-all replacement for
`sp42-core`.

The first extraction is slice-based and behavior-preserving:

1. **Transport and storage contracts first:** `HttpMethod`, `HttpRequest`,
   `HttpResponse`, `ServerSentEvent`, `WebSocketFrame`, the I/O traits, and the
   matching transport/storage errors. These are stable, shared by server,
   browser, live, actions, and dev tooling, and have no product policy.
2. **Edit and queue contracts second:** `FlagState`, `EditorIdentity`,
   `EditEvent`, `QueuedEdit`, `CompositeScore`, `SignalContribution`,
   `ScoringSignal`, and `ScoringContext`, once scoring defaults no longer call
   back into `sp42-core`.
3. **Wiki config later:** `WikiConfig` and `WikiTemplates` only after scoring
   policy defaults are injectable or owned by `sp42-wiki`/scoring policy code.
4. **Branding last:** branding constants should move only if multiple crates
   need them without also needing `sp42-core`.

Current guardrails:

- several scoring structs in `sp42-core::types` get defaults from
  `sp42-core::scoring_policy`, so moving them as-is would make `sp42-types`
  depend back on core policy behavior
- domain-specific errors should move with their owning crate, not into a global
  error bucket, unless they are transport/storage primitives

`sp42-core` remains the compatibility facade while callers migrate. New code
should depend on `sp42-types` for moved contracts and on the owning domain crate
for everything else.

## Extraction Rules

Create a new domain crate only when most of these are true:

- its public API is stable, or an ADR/PRD records the intended contract
- the code has current callers and a credible next caller
- the split removes duplication or reduces review blast radius
- tests move with the crate and remain deterministic
- the new crate does not create dependency cycles
- the extraction can land as small commits with no behavior change

If those conditions are not met, improve module boundaries inside `sp42-core`
first.

Preferred extraction order:

1. `sp42-devtools`
2. `sp42-reporting`
3. `sp42-wiki`
4. `sp42-coordination`
5. `sp42-live`
6. `sp42-actions`
7. further `sp42-types` slices only when they satisfy the strategy above

`sp42-core` may re-export stable APIs during migration so callers can move in
small steps. New code should depend on the extracted crate once it exists.

## Pull Request Notes

Crate-boundary PR descriptions should include only what applies:

- an ADR/PRD link when one records the contract
- dependency-direction notes
- validation notes for the extracted crate and affected shell crates
- a compatibility note when public APIs or re-exports move

Keep unrelated refactors in separate commits. Review should push back on splits
that only create a new place for duplicated logic.

## Alternatives Considered

- **Keep one broad `sp42-core`:** simpler today, but weaker for ownership,
  review scope, and long-term Open/Closed design.
- **Split everything now:** mechanically cheap while the repo is small, but it
  freezes unstable public APIs and risks creating a shared-types dumping ground.
- **Use Cargo features as the boundary:** useful for optional adapters, but too
  weak for ownership, review, and dependency-direction control.

## Consequences

- Maintainers get clearer ownership boundaries and smaller review surfaces.
- Contributors can work in a feature area without needing full-system context.
- Shared logic has a stronger home, reducing drift between targets.
- `sp42-core` can shrink over time instead of accumulating every domain.
- Migration will temporarily add re-export boilerplate.

## Non-Goals

- No all-at-once workspace split.
- No permanent maintainer assignment for every future crate.
- No change to deployment targets, licensing, auth/session rules, or public API
  contracts.
