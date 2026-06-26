# Contributing to SP42

SP42 is still early-stage software. Contributions are welcome, but changes
should stay aligned with the current architecture, security model, and project
constraints.

## Before You Start

- Read [README.md](README.md)
- Read [docs/STATUS.md](docs/STATUS.md)
- Read [CONSTITUTION.md](CONSTITUTION.md)
- Read [GOVERNANCE.md](GOVERNANCE.md)
- Read [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)
- Review relevant ADRs — see the [docs map](docs/README.md) for their platform vs. domain locations

## Development Expectations

- Keep secrets and personal Wikimedia credentials out of the repository
- Prefer small, focused pull requests
- Do not introduce new warnings
- Respect the layered architecture: **platform** owns reusable
  mechanisms/primitives/frameworks/contracts; **domains** consume them; **shells**
  compose. Reusable-by-design code goes to the platform, not domain→domain — see
  [ADR-0013](docs/platform/adr/0013-layered-platform-domain-architecture.md). The
  layer check (`./scripts/check-layering.sh`) enforces the dependency direction.
- Avoid architecture drift unless the change is explicitly justified
- Open an ADR for changes that affect crate boundaries, auth/session behavior,
  runtime deployment behavior, scoring policy, desktop packaging, or public
  contracts
- Open a PRD for changes that alter operator- or editor-facing behavior
- For crate extraction and ownership boundaries, follow
  [ADR-0004](docs/platform/adr/0004-crate-boundary-collaboration-model.md) and
  [ADR-0013](docs/platform/adr/0013-layered-platform-domain-architecture.md); to
  stand up a new domain, follow
  [docs/process/adding-a-domain.md](docs/process/adding-a-domain.md)

## Local Checks

Install the project Git hooks once per clone:

```sh
./scripts/install-git-hooks.sh
```

The repo uses a Husky-compatible local hook layout in `.husky/`. The installer
sets `core.hooksPath=.husky`, so the hooks run automatically:

- `pre-commit`: staged/working-tree whitespace checks, `cargo fmt --all -- --check`,
  the forbidden-pattern guard on added lines (§5.3), pedantic `clippy` on the
  changed crates, docs consistency, the markdown link check, release-tree audit,
  and `./scripts/check-focused.sh`.
- `commit-msg`: enforces Conventional Commits (§8.1).
- `pre-push`: release-tree audit, the markdown link check, the layer check
  (ADR-0013 dependency direction), `./scripts/ci-all.sh`, the supply-chain gate
  (`cargo deny` + `cargo audit`), the coverage gate (`sp42-core` ≥90% lines and a
  workspace floor, excl. `xtask`), and the forbidden-pattern guard over the pushed
  range.

These gates need a few extra tools installed once (all Rust):

```sh
cargo install --locked cargo-deny cargo-audit cargo-llvm-cov
```

The markdown link check is a deterministic, internal-links-only scan (Python 3,
no extra install; external URL liveness is intentionally out of scope). CI also
runs the optimized wasm bundle + size ceiling on every PR (its inputs can't be
path-enumerated), a path-filtered desktop (Tauri) build-check, and a weekly
`cargo-mutants` mutation-testing report.

The same gates run in CI on every non-draft pull request (see
`.github/workflows/ci.yml`), which additionally enforces the wasm bundle-size
ceiling against the optimized build. The wasm-size gate is CI/release-only — it
is not in `pre-push`, which would otherwise force an optimized rebuild on every
push.

> The supply-chain gate is currently red on `main` due to transitive advisories
> with no available fix (`paste`/`proc-macro-error2` are Leptos build-time
> proc-macros; `rand` RUSTSEC-2026-0097 has no patched 0.9.x). Until those clear
> upstream, pushing requires `SP42_SKIP_GIT_HOOKS=1` and PR CI will show the
> supply-chain step red. New advisories are still caught — this is deliberate.

Documentation-only changes (every modified file is `.md`, `.log`, or `.txt`)
skip the compile-heavy steps: `pre-commit` still runs the whitespace and docs
consistency checks and `pre-push` runs the docs consistency check, but the
focused build, release-tree audit, and `ci-all` are skipped. Any non-docs file
in the change runs the full checks.

For an emergency one-off bypass, set `SP42_SKIP_GIT_HOOKS=1`.

Run the focused local check before opening a pull request:

```sh
cargo fmt --all
./scripts/check-focused.sh
```

If your change touches the browser app or runtime config, also run:

```sh
rustup target add wasm32-unknown-unknown
./scripts/build-frontend.sh
```

If your change touches the server or local dev flow, run:

```sh
./scripts/dev-local.sh --smoke
```

If your change touches deployment packaging, run the relevant package build:

```sh
./scripts/build-web-release.sh
./scripts/package-vps.sh
```

If your change touches desktop packaging, run the fastest native check for your
platform:

```sh
./scripts/build-desktop.sh --platform macos --debug
```

Maintainers may run the full CI-shaped check before merge:

```sh
./scripts/ci-all.sh
```

## Credentials and Local Auth

If you use the local Wikimedia development bridge:

- keep credentials in `.env.wikimedia.local`
- never commit that file
- never paste raw access tokens into code, docs, issues, or PRs
- redact cookies, CSRF tokens, and authorization headers from logs

## Pull Requests

A good PR should include:

- a clear problem statement
- the smallest reasonable implementation
- validation notes listing the checks run, or a clear reason a check was not run
- any documentation updates required by the change
- an ADR link when the change affects architecture or protected behavior

External contributors should not need repository secrets, release credentials,
Wikimedia Cloud VPS access, or code-signing credentials for normal development.
Those remain maintainer-only.

## Issue Selection

Good first issues should be self-contained and should not require production
credentials, Wikimedia Cloud VPS access, signing certificates, or architectural
authority. Ask in the issue before starting larger work.
