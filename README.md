# SP42

SP42 is a platform for Wikimedia quality gates.

It is built to host checks that help an operator decide whether a page, edit,
source, or proposed change is ready to trust. Today that means patrol review,
edit scoring, citation verification, source/readability checks, and
operator-confirmed citation repair. The long-term shape is broader: shared
platform layers underneath, domain-specific gates on top, and browser/CLI/desktop
shells that expose the same contracts.

SP42 is **alpha software**. It is public and buildable, and the local development
stack is the main working surface. It is not yet a production moderation service
and it does not autonomously edit Wikimedia projects.

## SP42 Challenges

Wikimedia quality work does not fit one workflow, one wiki, or one language
edition. A gate that works for recent-changes patrol may not fit citation review;
a policy that is correct on one project may be wrong on another; and local
practices often live in templates, norms, queues, and reviewer habits rather than
in one central API.

SP42's core challenge is to make those differences portable without turning every
new gate into a one-off application. The project needs shared components and
shared functions that can be reused across patrol, references, scoring, editing,
coordination, reporting, and future domains. It also needs abstraction layers
that can accept project-specific policies and practices for any Wikimedia project
or language edition: scoring rules, source expectations, citation templates,
review actions, capability checks, model prompts, and operator workflows.

That is why SP42 is structured as platform plus domains. The platform provides
stable mechanisms; domains describe the quality gate; per-project configuration
and policy decide how that gate behaves in context.

## What SP42 Does

- Runs a browser-based operator workbench backed by an SP42 server.
- Ingests live or replayable Wikimedia edit queues and builds review context.
- Scores edits and ranks work through configurable policy documents.
- Presents diffs, action previews, identity/capability state, and coordination
  state for patrol workflows.
- Verifies whether cited sources support claims and reports the result with
  locatable evidence when support is claimed.
- Proposes citation repairs through an operator-confirmed propose/apply flow.
- Keeps model access, Wikimedia credentials, storage, fetch policy, and runtime
  configuration behind shared platform boundaries.

The design rule is simple: domains own Wikimedia quality gates; the platform owns
the reusable mechanisms those gates need.

## Current Domains

**Patrolling**

The patrol domain covers queueing, scoring, diff review, action preflight,
operator identity, and multi-operator coordination.

Main crates: `sp42-patrol`, `sp42-live`, `sp42-reporting`.

**References / Citation**

The references domain covers citation verification, article-level citation
reports, source snapshots, body-usability classification, Citoid-backed bare-URL
repair, and model-panel voting.

Main crate: `sp42-citation`.

More domains should follow the same pattern: platform contracts first, a narrow
domain crate second, shell integration last.

## Quick Start: Local Browser App

Requirements:

- Rust `1.96` or newer
- the `wasm32-unknown-unknown` Rust target
- `trunk` for serving the browser app during development

Setup:

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk
```

Run the local development stack:

```sh
./scripts/dev-local.sh --smoke
```

This starts:

- browser app: `http://127.0.0.1:4173`
- local server: `http://127.0.0.1:8788`

The script writes logs under `.tmp/` and stops both processes on `Ctrl-C`.

Useful local server endpoints:

- `http://127.0.0.1:8788/healthz`
- `http://127.0.0.1:8788/debug/runtime`
- `http://127.0.0.1:8788/debug/summary`
- `http://127.0.0.1:8788/dev/auth/bootstrap/status`

Most local development does not require committing credentials. Authenticated
Wikimedia actions use the localhost development auth bridge; configure that in a
local `.env.wikimedia.local` file when needed. See
[docs/platform/RUNTIME_CONFIGURATION.md](docs/platform/RUNTIME_CONFIGURATION.md).

LLM-backed citation verification uses the `SP42_INFERENCE_*` environment
variables documented in the inference/platform notes. Keep provider tokens on the
server side; the browser shell should never receive them.

## Common Commands

Focused local checks while iterating:

```sh
./scripts/check-focused.sh
```

Build the Rust workspace:

```sh
./scripts/build-local.sh
```

Build the browser bundle:

```sh
./scripts/build-frontend.sh
```

Run CI-shaped local validation:

```sh
./scripts/ci-all.sh
```

Build deployable artifacts:

```sh
./scripts/build-web-release.sh
./scripts/package-vps.sh
./scripts/build-desktop.sh --platform macos --debug
```

Optional compiler cache:

- leave `SP42_USE_SCCACHE` unset to auto-use `sccache` when available
- set `SP42_USE_SCCACHE=1` to require it

## Repository Layout

Platform and shared contracts:

- `crates/sp42-types`: neutral transport/model/storage contracts and DTOs
- `crates/sp42-platform`: scoring, diffing, action contracts, OAuth/dev-auth,
  wikitext editing, queueing, policy loading, and other reusable mechanisms
- `crates/sp42-coordination`: room state, presence, claims, and collaboration
  protocol
- `crates/sp42-wiki`: Wikimedia project registry, configs, SiteMatrix snapshot,
  and capability profiles
- `crates/sp42-reporting`: shared report document/rendering framework
- `crates/sp42-inference`: server-side model client construction
- `crates/sp42-server`: localhost/server runtime, HTTP routes, WebSockets, auth
  bridge, static app serving, and Wikimedia-facing adapters
- `crates/sp42-devtools`: deterministic fixtures and demo builders
- `crates/sp42-core`: temporary migration facade that re-exports platform and
  domain crates while callers move to the new boundaries

Domains:

- `crates/sp42-patrol`: patrol workflow, reports, shell-state models, and scoring
  evaluation fixtures
- `crates/sp42-live`: EventStreams/recentchanges ingestion and live queue support
- `crates/sp42-citation`: citation verification, citation reports, source fetch
  helpers, and bare-URL repair

Shells:

- `crates/sp42-app`: Leptos browser/PWA shell
- `crates/sp42-cli`: command-line shell
- `crates/sp42-desktop`: desktop shell and Tauri packaging

Supporting trees:

- `configs/`: per-wiki configuration
- `schemas/`: configuration schemas
- `fixtures/`: test fixtures
- `docs/`: architecture, domain, PRD, ADR, and process documentation
- `scripts/`: build, CI, dev, packaging, and repository hygiene entrypoints
- `xtask/`: workspace build tasks used by scripts

The current architecture is recorded in
[ADR-0013](docs/platform/adr/0013-layered-platform-domain-architecture.md).

## Runtime Configuration

The server defaults to local mode and binds to `127.0.0.1:8788`.

Important environment variables include:

- `SP42_DEPLOYMENT_MODE=local|vps|desktop`
- `SP42_BIND_ADDR`
- `SP42_PUBLIC_BASE_URL`
- `SP42_APP_DIST_DIR`
- `SP42_RUNTIME_DIR`
- `SP42_ALLOWED_ORIGINS`
- `SP42_WIKI_CONFIG_DIR`
- `SP42_DEFAULT_WIKI_ID`
- `SP42_SUPERVISOR_WIKIS`

The browser app uses same-origin API paths by default. Split frontend/API setups
can configure the API base URL through runtime config, metadata, or build-time
environment variables. See
[docs/platform/RUNTIME_CONFIGURATION.md](docs/platform/RUNTIME_CONFIGURATION.md)
for the full contract.

## Documentation

Start here:

- [docs/README.md](docs/README.md): documentation map
- [docs/STATUS.md](docs/STATUS.md): implementation status
- [docs/platform/README.md](docs/platform/README.md): platform docs and ADRs
- [docs/domains/README.md](docs/domains/README.md): domain docs
- [CONTRIBUTING.md](CONTRIBUTING.md): contributor checks and workflow
- [GOVERNANCE.md](GOVERNANCE.md): maintainer model and decision process
- [CONSTITUTION.md](CONSTITUTION.md): binding engineering rules

Key architecture records:

- [ADR-0004](docs/platform/adr/0004-crate-boundary-collaboration-model.md):
  crate boundaries and collaboration model
- [ADR-0005](docs/platform/adr/0005-design-system-shared-component-layer.md):
  design-system/shared-component direction
- [ADR-0006](docs/platform/adr/0006-using-llms.md): model-panel and inference
  boundary
- [ADR-0013](docs/platform/adr/0013-layered-platform-domain-architecture.md):
  platform/domain layering
- [ADR-0014](docs/platform/adr/0014-wikimedia-oauth-and-any-project.md):
  Wikimedia OAuth and any-project support

## Project Status

SP42 is actively changing. Treat the root README as the operator/developer entry
point, not the full project ledger. The detailed moving status lives in
[docs/STATUS.md](docs/STATUS.md), and product/architecture changes should be
captured as PRDs or ADRs under `docs/`.

## License

SP42 is licensed under the GNU General Public License version 3 only
(`GPL-3.0-only`). See [LICENSE](LICENSE).
