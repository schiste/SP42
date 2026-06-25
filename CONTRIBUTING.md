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
- Review relevant ADRs in [docs/adr](docs/adr)

## Development Expectations

- Keep secrets and personal Wikimedia credentials out of the repository
- Prefer small, focused pull requests
- Do not introduce new warnings
- Keep shared logic in `sp42-core` when it is genuinely cross-target
- Avoid architecture drift unless the change is explicitly justified
- Open an ADR for changes that affect crate boundaries, auth/session behavior,
  runtime deployment behavior, scoring policy, desktop packaging, or public
  contracts
- Open a PRD for changes that alter operator- or editor-facing behavior
- For crate extraction and ownership boundaries, follow
  [ADR-0004](docs/platform/adr/0004-crate-boundary-collaboration-model.md)

## Local Checks

Install the project Git hooks once per clone:

```sh
./scripts/install-git-hooks.sh
```

The repo uses a Husky-compatible local hook layout in `.husky/`. The installer
sets `core.hooksPath=.husky`, so the hooks run automatically:

- `pre-commit`: staged/working-tree whitespace checks, `cargo fmt --all -- --check`,
  docs consistency, release-tree audit, and `./scripts/check-focused.sh`.
- `pre-push`: release-tree audit and `./scripts/ci-all.sh`.

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
