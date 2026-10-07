---
name: repo-onboarding
description: Use when starting work in an unfamiliar repository, when the task asks for repo overview, setup, architecture, entrypoints, test commands, or where to begin. Skip for narrow file-scoped edits once the relevant paths are already known.
---

# Repo Onboarding: SP42

## When to Use

- Load this skill first when the repository is unfamiliar or the request is broad.
- Recommended when: first task in repo, repo overview, setup or run instructions, architecture or entrypoints, where should I start, broad debugging or feature-localization request.
- Skip when: known file-scoped edit, follow-up inside already identified area, task already localized to concrete files.
- Use `.codex/skills/aethyme/SKILL.md` or `.claude/skills/aethyme/SKILL.md` for Aethyme's short operating contract after orientation; load its `references/` files only when needed.

## Repo Identity

- Kind: `repository`
- Languages: `rust`
- Package manager: `cargo`
- Key manifests: `Cargo.toml, Procfile, crates/sp42-app/Cargo.toml, crates/sp42-assessment/Cargo.toml, crates/sp42-citation/Cargo.toml, crates/sp42-cli/Cargo.toml, crates/sp42-coordination/Cargo.toml, crates/sp42-core/Cargo.toml, crates/sp42-desktop/Cargo.toml, crates/sp42-desktop/src-tauri/Cargo.toml, crates/sp42-devtools/Cargo.toml, crates/sp42-fetch/Cargo.toml, crates/sp42-inference/Cargo.toml, crates/sp42-live/Cargo.toml, crates/sp42-mcp/Cargo.toml, crates/sp42-parsoid/Cargo.toml, crates/sp42-patrol/Cargo.toml, crates/sp42-platform/Cargo.toml, crates/sp42-reporting/Cargo.toml, crates/sp42-server/Cargo.toml, crates/sp42-types/Cargo.toml, crates/sp42-ui/Cargo.toml, crates/sp42-wiki/Cargo.toml, xtask/Cargo.toml`

## Workspaces

- `.` (primary; cargo; manifest `Cargo.toml`; high confidence)
- `crates/sp42-desktop/src-tauri` (supporting; cargo; manifest `crates/sp42-desktop/src-tauri/Cargo.toml`; high confidence)

## Start Here

- `dev`: `cargo run`
- `fast_test`: `cargo test --workspace`
- `build`: `cargo build --workspace`

## Supporting Commands

- `cargo run` (dev; medium confidence from `Cargo.toml`)
- `SP42_BIND_ADDR=0.0.0.0:8000 SP42_DEPLOYMENT_MODE=vps SP42_PUBLIC_BASE_URL=https://sp42.toolforge.org ./target/release/sp42-server` (dev; medium confidence from `Procfile:web`)
- `cargo test` (fast_test; high confidence from `Cargo.toml`)
- `cargo test --workspace` (fast_test; high confidence from `Cargo.toml`)
  Workspace: `.`
- `cargo test --manifest-path crates/sp42-desktop/src-tauri/Cargo.toml --workspace` (fast_test; high confidence from `crates/sp42-desktop/src-tauri/Cargo.toml`)
  Workspace: `crates/sp42-desktop/src-tauri`

## Entrypoints

- `app`: `Procfile:web` (Procfile process entrypoint; high confidence)
- `cli`: `crates/sp42-app/src/main.rs` (tracked Rust binary entrypoint in `.`; high confidence)

## Additional Entrypoints

- `Procfile:web` (process; role=app; Procfile process entrypoint; high confidence)
- `crates/sp42-app/src/main.rs` (file; role=cli; tracked Rust binary entrypoint in `.`; high confidence)
  Executable: `sp42-app`
- `crates/sp42-cli/src/main.rs` (file; role=cli; tracked Rust binary entrypoint in `.`; high confidence)
  Executable: `sp42-cli`
- `crates/sp42-desktop/src-tauri/src/main.rs` (file; role=cli; tracked Rust binary entrypoint in `crates/sp42-desktop/src-tauri`; high confidence)
  Executable: `sp42-desktop-tauri`
- `crates/sp42-desktop/src/main.rs` (file; role=cli; tracked Rust binary entrypoint in `.`; high confidence)
  Executable: `sp42-desktop`

## Repo Map

- `.github` (automation; automation and CI configuration; high confidence)
- `docs` (docs; documentation area; high confidence)
- `scripts` (tooling; developer tooling or scripts; high confidence)

## Aethyme Recipes

- `aethyme explore --repo "$PWD" --request "<task>" --format answer-json`
  Purpose: Broad repository orientation for a user request
- `aethyme repo inspect "$PWD" --mode brief --json-output`
  Purpose: Quick deterministic repo summary
- `aethyme graph callers "$PWD" "<symbol-or-file>" --json-output`
  Purpose: Trace likely impact before editing

## Caution Zones

- `fixtures` (likely generated, vendored, fixture, or migration-heavy area)

## Generated and Dangerous Paths

- Generated/vendor `.aethyme/generated`: tracked generated or vendored surface; verify ownership before editing
- Sensitive `.aethyme/gates.toml`: repository validation policy; changes affect every broker submission
- Sensitive `.github/workflows`: repository automation; changes can affect publication or shared CI

## Freshness

- Source digest: `50b057d5f9ff1851a21d4b1ecac1b9454323cb851d29d19edf383d3c51415671`
- Tracked source files: `540`
- Overrides applied: `False`
- Sections generated: `repo, workspaces, primary_workspace, commands, areas, entrypoints, caution_zones, generated_paths, dangerous_paths, navigation_recipes, summon, freshness`
