# Intake / Lifecycle / Gates Implementation Plan — Phase 4: the config linter and its CI wiring

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** Implement ADR-0026 §7's config linter as **one** `xtask` subcommand
covering the whole config set, actually wired into CI — and prove the point by
extending it twice more in Phases 6 and 7 rather than adding two more checkers.

**Architecture:** The validation logic is a library function in
`sp42-platform` (so it is unit-testable and reusable by the engine's load path);
the `xtask` subcommand is a thin CLI wrapper over it. CI step added to
`.github/workflows/ci.yml`.

**Tech Stack:** `xtask` (std only, no clap), `serde_yaml`, and the existing
`scripts/check-*.sh` idiom for anything that stays in bash.

**Scope:** Phase 4 of 8.

**Codebase verified:** 2026-09-28.

- `xtask/src/main.rs` is 1128 lines, one file, `[dependencies]` empty. Dispatch is a
  single `match command.as_str()` in `run()` at lines 59-77, plus a line in
  `print_help()` at 88-97. Nine existing subcommands, **all build/package/report —
  none is a check**, so this is the first gate-shaped command. Everything returns
  `Result<(), String>`; `main()` (:44) prints and `exit(1)`.
- `scripts/ci-all.sh` is 12 lines: a `trap` to `clean-house.sh`, then
  `sp42_run_xtask "$repo_root" ci-all "$@"`. `ci_all()` at
  `xtask/src/main.rs:345-439` runs cargo build/test/clippy/doc, the wasm build and
  test-check, trunk, and the Tauri contract check — **no `scripts/*.sh`**.
- The real gate list is `.github/workflows/ci.yml` lines 74-106:
  `check-layering.sh` (:92), `generate-architecture-map.sh --check` (:94),
  `check-design-system.sh` (:96), `check-forbidden-patterns.sh --range` (:100),
  `check-links.sh` (:102), `check-supply-chain.sh` (:104), `check-coverage.sh` (:106).
- `scripts/check-scoring-governance.sh` is referenced **only** in ADR-0021 and
  ADR-0026 prose — not in `ci.yml`, not in `.husky/`, not in `ci-all.sh`. It is an
  orphaned gate. Its primitives are `require_file`, `require_line` (literal
  `grep -Fq`, not YAML parsing), and `require_json_valid` (a `python3 -c` heredoc
  that only checks the *schema* parses — nothing validates YAML against it).
- `.husky/` exists but no hooks are installed, so the pre-push mirror is
  theoretical; do not rely on it.
- `scripts/check-forbidden-patterns.sh` scans **added lines only** (lines 4-5) and
  exempts inline `#[cfg(test)]` modules (lines 56-67, 103).

---

## Task 1: Validation as a library function

**Files:**
- Create: `crates/sp42-platform/src/intake_lint.rs`
- Create: `configs/intake/*.yaml`
- Create: `schemas/intake-pipeline.schema.json`

**Step 1: One entry point over the whole config set.** ADR-0026 §7 lists the
checks as a set — schema conformance, every `CapabilityRef` resolving, every
`Custom(String)` field registered, every `field`/`op`/`value` triple type-checking,
every `route_to` and `Reclassify.pipeline` target resolving, no `Reclassify` cycle
in the directed graph across the whole set, and no statically unreachable branch.
Several of those are only decidable **across** files (the reclassify graph spans
the config set), which is why the entry point takes a set, not a single file.

```rust
pub fn lint_intake_config_set(sources: &[(PipelineId, &str)], ctx: &LintContext)
    -> Result<(), Vec<IntakeLintError>>
```

Collect all errors rather than failing on the first — a linter that stops at the
first problem is a linter people run locally instead of in CI.

**Step 2: Reuse Phase 3's per-field table**, not a second copy (CONSTITUTION §6.1).

**Step 3: The two graph checks are the ones with no precedent here — write them
carefully.**
- *Reclassify cycles*: build the directed graph over `Reclassify.pipeline` edges
  **across all pipelines in the set**, then DFS with a colouring. A direct
  self-reference is the degenerate case and must be rejected explicitly
  (ADR-0026 §7 calls it out).
- *Statically unreachable branches*: a branch is unreachable when a `Not(All(…))`
  is already the disjunction of its siblings' polarities, or when an `All([All([])])`
  shadows its following siblings. Implement the trivially-detectable subset
  (empty-`All` short-circuit, direct self-reference) and return a clear
  `not_yet_detected` style non-error for the general case rather than pretending
  to be complete — a linter that silently under-reports is worse than one that
  admits its limit.

**Step 4: Ship the embedded baseline pipeline** per ADR-0026 §2 — deliberately
broad, restricted to intrinsic facts (event type, bot flag) only. See Phase 8 and
D9: **it must not reference a temporal field**, or `on_unknown`'s fail-closed
`Drop` default will drop real traffic the day it ships.

**Step 5: Commit** — `feat(platform): intake config linter over the whole config set`

---

## Task 2: The `xtask` subcommand

**Files:**
- Modify: `xtask/src/main.rs` (one `match` arm, one `print_help()` line, one `fn`)

**Step 1: Add the arm** in the existing dispatch, following the shape of the others:

```rust
"lint-intake" => lint_intake(&root),
```

**Step 2: Implement `lint_intake`**, printing each violation with its pipeline id,
rule path, and offending value — a linter that says "invalid config" is useless.
Exit non-zero on any violation, per the repo's `Result<(), String>` convention.

**Step 3: Commit** — `feat(xtask): lint-intake subcommand`

---

## Task 3: Wire it into CI — the part ADR-0026 §7 assumes and reality lacks

**Files:**
- Modify: `.github/workflows/ci.yml` (the `checks` job)

**Step 1: Add the step** alongside the other `check-*.sh` steps (after
`check-layering.sh` at :92 reads naturally — layering, then intake config):

```yaml
- name: Lint intake config
  run: ./scripts/ci-all.sh --lint-intake   # or: cargo run -p xtask -- lint-intake
```

Match whatever invocation the neighbouring steps use; do not introduce a fourth
convention.

**Step 2: While you are here, adopt the orphaned scoring governance script.**
`scripts/check-scoring-governance.sh` has never run in CI. Wiring it is a two-line
change to the same job and it is a real gate that has been silently off. Flag this
to the owner as a separate concern from the intake linter even though it lands in
the same commit — if it fails on existing configs, that is a pre-existing finding,
not something the intake linter caused.

**Step 3: Mirror in `.husky/pre-push`** for local feedback, following the file's
existing list (lines 65-94). Note the hooks are not installed, so this is for
whenever they are.

**Step 4: Prove the gate bites.** Add at least one deliberately invalid config
under a test-only path and assert the linter rejects it, so a future change cannot
silently make the linter a no-op. This is the test that matters most in this
phase.

**Step 5: Commit** — `ci: run the intake config linter and the orphaned scoring governance check`

---

## Task 4: Prove the "one linter, extended" claim

**Files:**
- Modify: `crates/sp42-platform/src/intake_lint.rs` (tests only)

**Step 1: A test that the rule set is a single pass over a shared context.**
When Phases 6 and 7 add their rules, the test should still show one traversal,
not a second linter. Assert the error enum is shared and that a single call
reports intake, eligibility, and quality violations together.

**Step 2: Commit** — `test(intake): one traversal reports violations across all gate types`

---

## Phase 4 exit criteria

- [ ] `cargo run -p xtask -- lint-intake` exits non-zero on a deliberately invalid config
- [ ] The step is present in `.github/workflows/ci.yml` — not only in `xtask`
- [ ] The reclassify-cycle check rejects a direct self-reference and a 2-cycle
- [ ] The statically-unreachable check is honest about what it does not detect
- [ ] The per-field table is shared with Phase 3, not copied
- [ ] `check-scoring-governance.sh` is wired (or its omission is reported to the owner)
- [ ] The shipped baseline pipeline references no temporal field
