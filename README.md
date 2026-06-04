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

- `crates/sp42-core`: pure domain logic, contracts, parsing, scoring, and action helpers
- `crates/sp42-app`: browser and PWA shell
- `crates/sp42-cli`: CLI shell
- `crates/sp42-desktop`: desktop shell
- `crates/sp42-server`: localhost coordination and auth bridge server
- `configs/`: per-wiki configuration
- `schemas/`: config schemas
- `fixtures/`: test fixtures
- `docs/`: status, ADRs, and design documents

## Requirements

- Rust `1.92` or newer
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
cargo run -p sp42-cli
```

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
[docs/RUNTIME_CONFIGURATION.md](docs/RUNTIME_CONFIGURATION.md). For desktop app
packaging, see [docs/DESKTOP_DISTRIBUTION.md](docs/DESKTOP_DISTRIBUTION.md).
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

- [docs/STATUS.md](docs/STATUS.md): phase-by-phase project status
- [docs/DEVELOPER_SURFACE.md](docs/DEVELOPER_SURFACE.md): developer-oriented surface summary
- [docs/RUNTIME_CONFIGURATION.md](docs/RUNTIME_CONFIGURATION.md): runtime modes, API base URL config, and local auth
- [docs/DESKTOP_DISTRIBUTION.md](docs/DESKTOP_DISTRIBUTION.md): macOS, Windows, and Linux desktop packaging
- [CONTRIBUTING.md](CONTRIBUTING.md): contributor workflow and local checks
- [GOVERNANCE.md](GOVERNANCE.md): maintainer model, protected areas, and release authority
- [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md): participation expectations
- [docs/scoring/SCORING_CONSTITUTION.md](docs/scoring/SCORING_CONSTITUTION.md): scoring system principles and technical rules
- [docs/scoring/POLICY_LAYOUT.md](docs/scoring/POLICY_LAYOUT.md): scoring policy and evaluation directory layout
- [docs/FRONTEND_DESIGN_CONTRACT.md](docs/FRONTEND_DESIGN_CONTRACT.md): frontend contract
- [docs/adr/0001-foundational-decisions.md](docs/adr/0001-foundational-decisions.md): foundational architecture decisions
- [docs/adr/0002-local-dev-auth-bridge.md](docs/adr/0002-local-dev-auth-bridge.md): local auth bridge decision

## License

SP42 is licensed under the GNU General Public License version 3 only
(`GPL-3.0-only`). See [LICENSE](LICENSE).
