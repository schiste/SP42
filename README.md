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
- A local Wikimedia testing token in `.env.wikimedia.local` for the single-user auth bridge

## Quick Start

### 1. Clone and build

```sh
cargo build --workspace
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
cargo build -p sp42-app --target wasm32-unknown-unknown
```

If you use `trunk`:

```sh
trunk serve
```

## Local Wikimedia Development Auth

SP42 supports a localhost-only single-user development auth path. This is for local testing only, not for public or multi-user deployment.

Create a local file named `.env.wikimedia.local` at the repository root with:

```env
WIKIMEDIA_CLIENT_APPLICATION_KEY=...
WIKIMEDIA_CLIENT_APPLICATION_SECRET=...
WIKIMEDIA_ACCESS_TOKEN=...
WIKIMEDIA_OAUTH_CALLBACK_URL=http://localhost:4173/oauth/callback
```

Rules:

- Keep this file local only
- Never commit it
- Tokens must not be exposed to browser storage or client-side code
- The browser should interact with Wikimedia only through the localhost bridge in development

The file is ignored by `.gitignore`.

## Development Commands

```sh
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo doc --workspace --no-deps
```

Selected utility scripts:

- `./scripts/local-operator-smoke.sh`
- `./scripts/check-doc-consistency.sh`

## Documentation

- [docs/STATUS.md](docs/STATUS.md): phase-by-phase project status
- [docs/DEVELOPER_SURFACE.md](docs/DEVELOPER_SURFACE.md): developer-oriented surface summary
- [docs/FRONTEND_DESIGN_CONTRACT.md](docs/FRONTEND_DESIGN_CONTRACT.md): frontend contract
- [docs/adr/0001-foundational-decisions.md](docs/adr/0001-foundational-decisions.md): foundational architecture decisions
- [docs/adr/0002-local-dev-auth-bridge.md](docs/adr/0002-local-dev-auth-bridge.md): local auth bridge decision

## License

SP42 is licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE).

