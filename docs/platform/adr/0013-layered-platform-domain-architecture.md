# ADR-0013: Layered platform/domain architecture with mechanical enforcement

**Status:** Accepted
**Date:** 2026-06-26
**Author:** Christophe Henner (drafted with Claude)

**Implementation note (2026-07-10):** Implemented, with one divergence from
Decision §1. The extraction landed (`sp42-platform`/`sp42-patrol`/`sp42-citation`
split out; `sp42-core` reduced to a re-export facade awaiting retirement) and
the layer check is enforced in CI and pre-push. The physical
`crates/{platform,domains,shells}/` folders have **not** landed — crates remain
flat under `crates/` and the layer tag lives in the map inside
`scripts/check-layering.sh` until the relocation slice (migration phase 5).

## Context

SP42's documentation describes a **platform that owns shared abstraction layers**
and **domains that consume them** (patrolling shipped; references/citation
emerging; more coming). The **code does not match**: `sp42-core` is a hybrid that
tangles platform abstractions, patrolling logic, and citation logic, and nothing
enforces dependency direction — any crate can reach any other. ADR-0004 names
crates as ownership boundaries but enforces nothing.

The goal: make the platform/domain split **real in the workspace and mechanically
enforced**, so a contributor can own a domain end-to-end without understanding or
risking the platform or sibling domains — including external contributors who may
own platform crates too.

## Decision

### 1. Three layers, expressed as workspace-crate folders
Boundaries are **workspace crates** grouped into `crates/{platform,domains,shells}/`
— a monorepo, nothing published. The folder is the layer tag.
- **Platform** — reusable mechanisms, primitives, frameworks, contracts.
- **Domains** — thin policy/config/workflow/definitions for one capability
  (`domains/<domain>/<crate>`), e.g. patrolling, references.
- **Shells** — composition roots / entrypoints (cli, app, desktop, server, devtools).

### 2. The classification rule (reuse-by-design ⇒ platform)
A reusable **mechanism / primitive / framework / contract** is platform; a domain's
**policy / config / workflow / definition** is domain. Test: *"would a second domain
genuinely reuse this by design?"* This applies **fractally** — a hybrid crate splits
into a platform part and a domain part. Genuinely shared logic discovered in a domain
is **promoted to platform**, not coupled domain→domain.

### 3. Dependency direction (the invariant)
`platform ◄─ domains ◄─ shells`:
- Platform crates **never** depend on a domain or shell crate.
- Domain crates depend on platform and on **each other** (acyclic — Cargo forbids
  cycles); never on shells.
- Shells compose platform + any domains.

### 4. Mechanical enforcement
- **Layer check** (`scripts/check-layering.sh`): reads `cargo metadata`, maps each
  crate to its layer, and **fails** on any forbidden edge. Runs in CI (`checks`) and
  pre-push.
- **Visibility:** platform exposes a deliberate `pub` contract (a `contract` prelude);
  internals are `pub(crate)` so domains cannot reach platform guts.
- `cargo-deny` continues to cover advisories/licenses.
- **No `cargo-semver-checks`/version machinery** — in a monorepo the unified build is
  the contract enforcer (a contract break fails the one build).

### 5. Ownership
`CODEOWNERS` routes review by layer/domain: platform → leads; each domain → its
owner; shells → leads. Platform changes get lead review; a domain owner owns their
domain folder.

### 6. Open ownership
External contributors may own platform crates as well as domains. The platform's
public API is therefore a real contract, kept explicit (the `contract` prelude) and
enforced by the build + layer check rather than by trust.

## Target taxonomy (summary; full mapping in the migration plan)
- **Platform:** `sp42-types`, `sp42-platform` (ex-`sp42-core` mechanisms + reusable
  primitives), `sp42-coordination`, `sp42-wiki`, `sp42-live` (live ingestion),
  `sp42-reporting` (reporting framework), `sp42-inference` (LLM adapter, ADR-0006),
  `sp42-fetch` (guarded read-only HTTP fetch edge, ADR-0015).
- **Domains:** `patrolling/sp42-patrol` (scoring policy, evaluation, review workbench,
  patrol reports, queue policy); `references/sp42-citation` (all citation kept
  together; reusable citation primitives flagged for later promotion — #69–#73).
- **Shells:** `sp42-cli`, `sp42-app`, `sp42-desktop` (+ `src-tauri`), `sp42-server`
  (composition root), `sp42-devtools`.

## Alternatives considered
- **Module boundaries inside fewer crates** — rejected: the Rust compiler cannot
  isolate modules within a crate (`pub(crate)` leaks), so it could not be
  mechanically enforced.
- **Separate repos / published packages** — rejected: out of scope; the monorepo
  build is a stronger, cheaper enforcer than version contracts.
- **Convention/review-only enforcement** — rejected: an unenforced boundary is a
  suggestion; open ownership requires mechanical guarantees.

## Consequences
- A multi-slice migration extracts `sp42-platform`/`sp42-patrol`/`sp42-citation` from
  `sp42-core` and relocates crates into the layer folders, each slice landing green.
- During migration `sp42-core` is a documented **hybrid exemption** in the layer check
  until it is split and retired.
- The platform grows (reusable primitives pool into it) and domains become thin — a
  new domain owner builds on a rich platform and writes only their policy/workflow.
