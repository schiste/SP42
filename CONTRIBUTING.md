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

## Local Checks

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
- tests or validation notes
- any documentation updates required by the change
- an ADR link when the change affects architecture or protected behavior

External contributors should not need repository secrets, release credentials,
Wikimedia Cloud VPS access, or code-signing credentials for normal development.
Those remain maintainer-only.

## Issue Selection

Good first issues should be self-contained and should not require production
credentials, Wikimedia Cloud VPS access, signing certificates, or architectural
authority. Ask in the issue before starting larger work.

