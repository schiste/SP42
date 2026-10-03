---
name: repo-act
description: Use after repo-onboarding or Explore when moving from orientation into execution planning, debugging checklists, and validation steps. Skip when only broad repo orientation is needed.
---

# Repo Act: SP42

## When to Use

- Load this after repo-onboarding when the next step is execution or validation planning.
- Skip it for broad repo overview questions with no action plan yet.

## Debugging Checklist

- Load repo-onboarding first if the repository or area is unfamiliar.
- Run Aethyme Explore on the user task before broad manual search.
- Verify answer candidates before concluding when trust_policy is not answer-safe.
- Identify the nearest fast test or validation command before editing.
- Inspect caution zones before changing generated or vendored areas.

## Validation Checklist

- Run the fastest relevant test command first.
- Check likely entrypoints and impacted callers before finishing.
- Re-run lint/build only after the focused validation passes.
- Document any override-based maintainer note that affected the change plan.

## Useful Commands

- `fast_test`: `cargo test --workspace`
- `build`: `cargo build --workspace`
- `dev`: `cargo run`

## Primary Entrypoints

- `app`: `Procfile:web` (Procfile process entrypoint)
- `cli`: `crates/sp42-app/src/main.rs` (tracked Rust binary entrypoint in `.`)

## Likely Entrypoints

- `Procfile:web` (Procfile process entrypoint)
- `crates/sp42-app/src/main.rs` (tracked Rust binary entrypoint in `.`)
- `crates/sp42-cli/src/main.rs` (tracked Rust binary entrypoint in `.`)

## Caution Zones

- `fixtures` (likely generated, vendored, fixture, or migration-heavy area)
