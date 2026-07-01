# SP42

SP42 is a Rust-first Wikipedia patrolling workbench. It combines shared patrol logic, a browser shell, a CLI, a desktop shell, and a localhost bridge so the same review flow can be exercised across targets while the project moves toward live Wikimedia verification.

SP42 is currently alpha software. The repository is public and buildable, but it is not yet a production-ready moderation tool.

## What SP42 Is

- A shared core for patrol queueing, diffing, scoring, context building, and MediaWiki action preparation
- A browser shell for patrol review, diagnostics, and local single-user Wikimedia testing
- A CLI and desktop shell that reuse the same core contracts
- A localhost server for coordination, debug surfaces, and the local development auth bridge

## Current Status

- Local and offline development is substantially implemented
- Single-user local Wikimedia token testing is supported through a localhost bridge
- Live Wikimedia integration is still the main remaining external milestone
- Multi-user production auth is not implemented yet

Phase summary:

- `Phase 1`: offline patrol core and queueing, effectively complete for local/offline development
- `Phase 2`: coordination and shared room state, effectively complete for local development
- `Phase 3`: browser, CLI, and desktop shells with shared reports, shared shell-state, telemetry, and the interactive patrol rail, effectively complete for local development
- `Phase 4`: live Wikimedia integration, pending real credentials and external verification
- `Phase 5`: PWA/offline packaging and installability, effectively complete for local development

Detailed status lives in [docs/STATUS.md](docs/STATUS.md).

## Repository Layout

SP42 is a platform that owns shared abstraction layers, with domains that consume
them. The crates group along that seam.

Platform layers (shared, domain-agnostic):

- `crates/sp42-types`: transport contracts and storage/HTTP/clock abstractions
- `crates/sp42-fetch`: guarded read-only HTTP fetch edge (SSRF-guarded DNS resolver, redirect/size caps, Retry-After retry loop, Wikimedia UA) shared by the CLI and server source fetches
- `crates/sp42-coordination`: multi-operator collaboration protocol and room state
- `crates/sp42-wiki`: wiki config parsing, registry/default selection, and capability profiles
- `crates/sp42-server`: localhost HTTP/WebSocket server, auth bridge, and routing
- `crates/sp42-devtools`: deterministic fixtures and demo-surface builders
- `crates/sp42-core`: shared contracts, runtime primitives, and the scoring engine (also hosts patrolling action/queue logic pending a future split)
- `xtask`: workspace build tasks

Patrolling domain (the shipped review workflow):

- `crates/sp42-live`: EventStreams ingestion, recentchanges/backlog polling, and live queue filtering
- `crates/sp42-reporting`: patrol scenario, session-digest, and operator-summary reporting
- `crates/sp42-cli`: CLI shell
- `crates/sp42-app`: browser and PWA shell
- `crates/sp42-desktop`: desktop shell

References / citation domain is incoming (no crate yet); see the PRD and ADRs in
`docs/domains/references/`.

Supporting trees:

- `configs/`: per-wiki and scoring configuration
- `schemas/`: config schemas
- `fixtures/`: test fixtures
- `docs/`: platform, domain, and project documentation (see [docs/README.md](docs/README.md))

## Requirements

- Rust `1.96` or newer
- The `wasm32-unknown-unknown` target for browser builds

Optional:

- `trunk` for serving the browser app during development
- `sccache` for faster repeated local Rust builds
- A local Wikimedia testing token in `.env.wikimedia.local` for the single-user auth bridge

## Quick Start

### 1. Clone and build

```sh
./scripts/build-local.sh
```

Builds are incremental by default. Pass `--clean` to any build entrypoint when
you want to purge generated artifacts, including `target/`, before building.

For a deployable web release build:

```sh
./scripts/build-web-release.sh
```

For CI-shaped builds with deterministic caching:

```sh
./scripts/ci-all.sh
```

For focused local checks during iteration:

```sh
./scripts/check-focused.sh
```

### 2. Run the localhost server

```sh
cargo run -p sp42-server
```

Useful local endpoints:

- `http://127.0.0.1:8788/healthz`
- `http://127.0.0.1:8788/debug/summary`
- `http://127.0.0.1:8788/dev/auth/bootstrap/status`

### 3. Run the CLI

```sh
cargo run -p sp42-cli -- --help            # list subcommands
cargo run -p sp42-cli -- preview           # ranked queue from STDIN (built-in sample if empty)
```

Capabilities are subcommands (`verify`, `verify-page`, `locate-probe`,
`bare-url`, `preview`). For the full command-line reference — including
environment variables — see [docs/platform/CLI.md](docs/platform/CLI.md).

### 4. Build the browser app

```sh
rustup target add wasm32-unknown-unknown
./scripts/build-frontend.sh
```

To generate Cargo timings reports:

```sh
./scripts/build-timings.sh
```

For live local development with the server and Trunk proxy running together:

```sh
./scripts/dev-local.sh
./scripts/dev-local.sh --smoke
```

The dev command runs `sp42-server` on `127.0.0.1:8788` and Trunk on
`127.0.0.1:4173`.

For runtime settings, local credentials, and API base URL behavior, see
[docs/platform/RUNTIME_CONFIGURATION.md](docs/platform/RUNTIME_CONFIGURATION.md).
For desktop app packaging, see
[docs/platform/DESKTOP_DISTRIBUTION.md](docs/platform/DESKTOP_DISTRIBUTION.md).
For a Wikimedia Cloud VPS artifact, run `./scripts/package-vps.sh`; the
generated package includes its own deployment README.

## Development Commands

```sh
./scripts/build-local.sh
./scripts/build-local.sh --clean
./scripts/build-server.sh
./scripts/build-frontend.sh
./scripts/build-web-release.sh
./scripts/package-vps.sh
./scripts/build-desktop.sh --platform macos --debug
./scripts/dev-local.sh --smoke
./scripts/check-focused.sh
./scripts/ci-all.sh
```

Contributor validation expectations live in [CONTRIBUTING.md](CONTRIBUTING.md).
Maintainer/full-CI commands live behind `./scripts/ci-all.sh` and the Cargo
aliases in [.cargo/config.toml](.cargo/config.toml).

Optional shared compiler cache:

- Install `sccache` for faster repeated local builds
- Set `SP42_USE_SCCACHE=1` to require `sccache`
- Leave `SP42_USE_SCCACHE` unset to auto-enable `sccache` when available without making it mandatory

## Documentation

Documentation is organized to mirror the platform/domain architecture. Start with
the docs map, then drill into a layer or domain:

- [docs/README.md](docs/README.md): documentation map — platform, domains, and project docs
- [docs/platform/README.md](docs/platform/README.md): platform layers — runtime, desktop, developer surface, design contract, scoring, and ADR-0001–0006
- [docs/domains/README.md](docs/domains/README.md): domains — patrolling (shipped) and references/citation (incoming)

Project and process docs:

- [docs/STATUS.md](docs/STATUS.md): phase-by-phase project status
- [CONTRIBUTING.md](CONTRIBUTING.md): contributor workflow and local checks
- [GOVERNANCE.md](GOVERNANCE.md): maintainer model, protected areas, and release authority
- [CONSTITUTION.md](CONSTITUTION.md): binding engineering laws
- [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md): participation expectations
- [docs/process/prd-protocol.md](docs/process/prd-protocol.md): PRD protocol for user-facing changes

## License

SP42 is licensed under the GNU General Public License version 3 only
(`GPL-3.0-only`). See [LICENSE](LICENSE).
