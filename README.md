# SP42

SP42 is a Rust-first workbench for Wikimedia maintenance work. It began as a
recent-changes patrolling tool and has grown into a **platform with domains**:
shared, domain-agnostic layers (scoring, fetching, inference, live ingestion,
coordination, wiki registry, UI) with focused capabilities built on top —
patrolling, LLM-assisted citation verification, article-quality assessment, and
Wikidata as a first-class review target. Every shell (browser, CLI, desktop,
localhost server, MCP agent surface) drives the same core contracts.

SP42 is currently alpha software. The repository is public and buildable, but it
is not yet a production-ready moderation tool.

## What SP42 Is

- A **platform** of shared, domain-agnostic layers: transport contracts, a
  guarded read-only fetch edge, the scoring engine and policy framework, live
  EventStreams ingestion, multi-operator coordination, wiki
  registry/capability profiles, the provider-agnostic LLM interface
  (ADR-0006), read-only Parsoid page access, shared reporting, and a shared
  design-system UI layer
- The **patrolling** domain (shipped): patrol queueing, scoring, diffing,
  context building, reviewer actions, and multi-operator coordination
- The **references / citation verification** domain (shipped): LLM-assisted
  verification of whether a cited source actually supports the claim it backs —
  article-level `verify-page` reports, bare-URL reference repair, and
  book-citation grounding through Open Library / Internet Archive
- The **assessment** domain (first crate landed): grounded evidence for
  per-article quality assessments, starting with the Good-article review
  appendix
- The **Wikidata** capability (in progress): entity read/diff and
  content-model-routed patrol wired through the platform's `wikibase` module
- Shells that reuse the same core contracts: a browser/PWA shell, a CLI, a
  desktop shell, a localhost server (coordination, debug surfaces, and the
  local dev-auth bridge), and an MCP surface that exposes citation
  verification to agents

<img width="1920" height="991" alt="Screenshot 2026-07-02 at 09 44 41" src="https://github.com/user-attachments/assets/411286b2-d41c-44c4-a67b-19f93be8eebc" />


## Current Status

- Patrolling and citation verification are substantially implemented for local development
- Single-user local Wikimedia token testing is supported through a localhost bridge
- Live Wikimedia integration is still gated by external credentials and verification
- Multi-user production auth is not implemented yet

The phase-by-phase implementation timeline lives in
[docs/STATUS.md](docs/STATUS.md) — the README deliberately does not duplicate
it.

## Repository Layout

SP42 is a platform that owns shared abstraction layers, with domains that
consume them and shells on top (`platform ◄─ domains ◄─ shells`, ADR-0013).
The dependency direction is mechanically enforced by
[scripts/check-layering.sh](scripts/check-layering.sh) and drawn in
[docs/platform/architecture.md](docs/platform/architecture.md).

Platform layers (shared, domain-agnostic):

- `crates/sp42-types`: transport contracts and storage/HTTP/clock abstractions
- `crates/sp42-platform`: reusable mechanisms and contracts — the scoring
  engine and policy framework, action contracts/executor, wikitext editing,
  OAuth/dev-auth, and the Wikidata entity read model (`wikibase`)
- `crates/sp42-fetch`: guarded read-only HTTP fetch edge (SSRF-guarded DNS
  resolver, redirect/size caps, Retry-After retry loop, Wikimedia UA)
- `crates/sp42-inference`: provider-agnostic LLM edge — model client, endpoint
  config, and the multi-model panel (ADR-0006)
- `crates/sp42-live`: EventStreams ingestion, recentchanges/backlog polling,
  and live queue filtering
- `crates/sp42-coordination`: multi-operator collaboration protocol and room state
- `crates/sp42-wiki`: wiki config parsing, registry/default selection, and
  capability profiles
- `crates/sp42-parsoid`: read-only Parsoid page access — fetch a revision and
  decompose it into prose-bearing blocks
- `crates/sp42-reporting`: shared reporting framework and renderers
- `crates/sp42-ui`: shared Leptos presentation layer — Codex-backed tokens,
  theming, and typed UI primitives

Domains (capabilities built on the platform):

- `crates/sp42-patrol`: patrolling policy, workflow, and report definitions
- `crates/sp42-citation`: citation verification and bare-URL repair
- `crates/sp42-assessment`: Good-article evidence-appendix rendering over
  verification reports
- Wikidata has no crate yet; its entity read model lives in `sp42-platform`
  (see [docs/domains/wikidata/](docs/domains/wikidata/README.md))

Shells (drive the platform and domains):

- `crates/sp42-app`: browser and PWA shell
- `crates/sp42-cli`: CLI shell
- `crates/sp42-desktop`: desktop shell
- `crates/sp42-server`: localhost HTTP/WebSocket server, auth bridge, and routing
- `crates/sp42-devtools`: deterministic fixtures and demo-surface builders
- `crates/sp42-mcp`: agent-facing MCP surface for citation verification

Transitional and tooling:

- `crates/sp42-core`: retiring re-export facade (its code has been extracted
  into `sp42-platform` and the domain crates)
- `xtask`: workspace build tasks

Supporting trees:

- `configs/`: per-wiki and scoring configuration
- `schemas/`: config schemas
- `fixtures/`: test fixtures
- `evals/`: scoring evaluation fixture sets and profiles
- `docs/`: platform, domain, and project documentation (see [docs/README.md](docs/README.md))

## Requirements

- Rust `1.96` or newer
- The `wasm32-unknown-unknown` target for browser builds

Optional:

- `trunk` for serving the browser app during development
- `sccache` for faster repeated local Rust builds
- A local Wikimedia testing token in `.env.wikimedia.local` for the single-user auth bridge

## Quick Start

### Local Development

#### 1. Clone and build

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

#### 2. Run the localhost server

```sh
cargo run -p sp42-server
```

Useful local endpoints:

- `http://127.0.0.1:8788/healthz`
- `http://127.0.0.1:8788/debug/summary`
- `http://127.0.0.1:8788/dev/auth/bootstrap/status`

#### 3. Run the CLI

```sh
cargo run -p sp42-cli -- --help            # list subcommands
cargo run -p sp42-cli -- preview           # ranked queue from STDIN (built-in sample if empty)
```

Capabilities are subcommands (`verify`, `verify-page`, `locate-probe`,
`bare-url`, `preview`). For the full command-line reference — including
environment variables — see [docs/platform/CLI.md](docs/platform/CLI.md).

#### 4. Build the browser app

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

### Using Docker

#### 1. Build the Docker Image

```sh
docker build -t sp42:latest .
```

#### 2. Run SP42 in a Docker Container

You need to create a file `.env.wikimedia.local` with your access token in the current directory.
```sh
docker run --rm \
    --name sp42 \
    -p 8788:8788 \
    -v "$PWD"/.env.wikimedia.local:/var/lib/sp42/.env.wikimedia.local \
    -v sp42-data:/var/lib/sp42/ \
    sp42:latest
```

####

#### 1. Build the Docker Development Image

Clone the repo and build the `sp42-dev` image:
```sh
docker build -f Dockerfile.dev -t sp42-dev:latest .
```

#### 2. Build the Docker Development Image

Assuming the repository is in the current directory:
```sh
docker run --rm \
    -it \
    --name sp42-dev \
    --user "$(id -u):$(id -g)" \
    -e HOME=/tmp \
    -e SSH_AUTH_SOCK=/ssh-agent \
    -v "$PWD:/src" \
    -v "$HOME/.gitconfig:/tmp/.gitconfig:ro" \
    -v "$SSH_AUTH_SOCK:/ssh-agent" \
    -w /src \
    sp42-dev:latest
```

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
- [docs/platform/README.md](docs/platform/README.md): platform layers — runtime, desktop, developer surface, design contract, scoring, and the platform ADRs
- [docs/domains/README.md](docs/domains/README.md): domains — patrolling and references/citation (shipped), assessment and Wikidata (incoming)

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
