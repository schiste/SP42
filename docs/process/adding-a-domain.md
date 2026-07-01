# Adding a domain

This is the turnkey path for owning a **domain** in SP42. It assumes the layered
architecture from [ADR-0013](../platform/adr/0013-layered-platform-domain-architecture.md):
a **platform** owns reusable mechanisms/primitives/frameworks/contracts; a
**domain** is a thin crate of one capability's policy/config/workflow that consumes
the platform.

You should be able to own a domain end-to-end without reading or risking the
platform internals or sibling domains. The layer check and `CODEOWNERS` enforce
that boundary mechanically.

## The one rule

**Reuse-by-design ⇒ platform.** Before writing code, ask of each piece: *"would a
second domain genuinely reuse this by design?"*
- **Yes** — it is a mechanism/primitive/framework/contract → it belongs in the
  **platform** (`sp42-platform` or a focused platform crate), not your domain.
  Propose it as a platform addition.
- **No** — it encodes *your* domain's intent (a policy, a config, a workflow, a
  report definition) → it belongs in **your domain crate**.

Shared logic is **promoted to the platform**, never wired domain→domain. Domains may
depend on each other, but if you find yourself reaching into a sibling for a
*mechanism*, that mechanism should be promoted to platform instead.

## Dependency direction (enforced)

```
platform  ◄─  domains  ◄─  shells
```

- Your domain crate may depend on **platform** crates and **other domain** crates.
- Your domain crate must **never** depend on a **shell** (`sp42-cli`, `sp42-app`,
  `sp42-desktop`, `sp42-server`, `sp42-devtools`).
- Never depend on a shell's concrete adapters (reqwest/axum/IndexedDB/Parsoid
  clients). Depend on the **contract traits** in `sp42-types`
  (`HttpClient`, `EventSource`, `Storage`, `Clock`, `Rng`, `WebSocket`,
  `ModelClient`, `WikitextEditor`); shells construct the impls and inject them.

`scripts/check-layering.sh` fails CI on any violating edge.

## Steps

1. **Decide the boundary.** Confirm the capability is a domain (policy/workflow),
   not a platform mechanism. If unsure, open a short note or ADR addendum before
   writing code.
2. **Create the crate** under the domain layer:
   ```
   crates/domains/<domain>/sp42-<domain>/
     Cargo.toml      # depends only on sp42-platform (+ sp42-types) and, if needed,
                     # sibling domain crates — never a shell
     src/lib.rs
   ```
   Add it to the workspace `members` in the root `Cargo.toml`. Inherit
   `[lints] workspace = true` so the pedantic gate applies.
3. **Build on the platform contract.** Import the platform's public surface
   (its `contract` prelude) and the `sp42-types` traits. Keep your crate's own
   internals `pub(crate)`; expose only what a shell needs to compose you.
4. **Wire a shell.** A shell (`sp42-server` and/or `sp42-cli`/`sp42-app`/
   `sp42-desktop`) adds your crate as a dependency, constructs the trait impls,
   and injects them. Shells are the only place `impl Trait` lives.
5. **Claim ownership.** Add a `CODEOWNERS` line:
   ```
   /crates/domains/<domain>/   @your-handle
   ```
6. **Document it.** Add a `docs/domains/<domain>/` entry describing the capability
   and any domain-specific PRDs/ADRs.
7. **Verify green.** Run:
   ```sh
   ./scripts/check-layering.sh
   ./scripts/ci-all.sh
   ```
   Both must pass. Open the PR per [CONTRIBUTING.md](../../CONTRIBUTING.md);
   `CODEOWNERS` routes review to you for your domain and to leads for any platform
   change you propose alongside it.

## What you do NOT need

Per [CONTRIBUTING.md](../../CONTRIBUTING.md), owning a domain does not require
repository secrets, release/signing credentials, or Wikimedia Cloud VPS access —
those stay maintainer-only. A domain builds entirely on the public platform
contract.
