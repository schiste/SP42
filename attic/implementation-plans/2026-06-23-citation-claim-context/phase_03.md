# Citation Claim Context — Phase 3

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** Let an operator pass a context window to the verifier from the CLI; default (no flags) preserves the control arm.

**Architecture:** Extend the hand-rolled arg parser with `--section-title <T>` and a repeatable `--preceding-sentence <S>`. Assemble an `Option<ClaimContext>` in `run_verify` (only `Some` when non-empty) and pass it to `verify_citation_use_site`. Article title is left empty for the CLI MVP — the library API is the eval-corpus driver; the CLI is a manual convenience surface, and section title + preceding sentences carry the co-reference signal.

**Scope:** Phase 3 of 3. Depends on Phase 2.

**Codebase verified:** 2026-06-23. `VerifyCliOptions` at `main.rs:87`; `build_verify_options` at `main.rs:339`; `CliParseState` at `main.rs:359`; `apply_cli_argument` match at `main.rs:400`; parse setup (state locals + construction) spans `main.rs:~258-336`; `run_verify` builds the request and calls `verify_citation_use_site` at `main.rs:874` (Phase 2 left a `None` in the context arg). Parser is hand-rolled (no clap); repeatable flags push onto a `Vec`. Existing tests at `main.rs:~1026`.

---

## Task 1: parse the new flags (TDD)

**Files:**
- Modify: `crates/sp42-cli/src/main.rs` — `VerifyCliOptions` (`:87`), `build_verify_options` (`:339`), `CliParseState` (`:359`), the parse-setup block (`:~258-336`), `apply_cli_argument` (`:400`)

**Step 1: Write a failing parse test** (in the CLI tests module, mirroring `verify_requires_both_claim_and_source_url`):

```rust
#[test]
fn verify_collects_section_and_preceding_context() {
    let args = vec![
        "--claim".to_string(), "c".to_string(),
        "--source-url".to_string(), "https://example.com".to_string(),
        "--section-title".to_string(), "Career".to_string(),
        "--preceding-sentence".to_string(), "She joined in 1985.".to_string(),
        "--preceding-sentence".to_string(), "She scored twice.".to_string(),
    ];
    let parsed = parse_cli_options(args.into_iter()).expect("parse").verify.expect("verify mode");
    assert_eq!(parsed.section_title.as_deref(), Some("Career"));
    assert_eq!(parsed.preceding_sentences, vec!["She joined in 1985.", "She scored twice."]);
}
```

(Use whatever the existing parse entry point is named — confirm against `verify_requires_both_claim_and_source_url` in the tests module and match its call style.)

**Step 2: Run to verify failure**

Run: `cargo test -p sp42-cli verify_collects_section`
Expected: FAIL (fields/flags absent).

**Step 3: Implement.**

- Add fields to `VerifyCliOptions`:
  ```rust
      section_title: Option<String>,
      preceding_sentences: Vec<String>,
  ```
- Add to `CliParseState`:
  ```rust
      verify_section_title: &'a mut Option<String>,
      verify_preceding: &'a mut Vec<String>,
  ```
- Add match arms in `apply_cli_argument` (beside `--source-url`):
  ```rust
      "--section-title" => {
          *state.verify_section_title = Some(next_option_value(args, "--section-title")?);
      }
      "--preceding-sentence" => {
          state.verify_preceding.push(next_option_value(args, "--preceding-sentence")?);
      }
  ```
- In the parse-setup block (`~258-336`): declare `let mut verify_section_title = None;` and `let mut verify_preceding: Vec<String> = Vec::new();`, wire them into the `CliParseState { .. }` construction, and pass them into `build_verify_options`.
- Extend `build_verify_options` to accept and forward them:
  ```rust
  fn build_verify_options(
      claim: Option<String>,
      source_url: Option<String>,
      include_metadata: bool,
      debug_votes: bool,
      repair: bool,
      section_title: Option<String>,
      preceding_sentences: Vec<String>,
  ) -> Result<Option<VerifyCliOptions>, String> {
      match (claim, source_url) {
          (Some(claim), Some(source_url)) => Ok(Some(VerifyCliOptions {
              claim, source_url, include_metadata, debug_votes, repair,
              section_title, preceding_sentences,
          })),
          (None, None) => Ok(None),
          _ => Err("citation verification requires both --claim and --source-url".to_string()),
      }
  }
  ```
- Update the existing `VerifyCliOptions { .. }` literals in the tests module (`~:1026`) to include the two new fields (`section_title: None, preceding_sentences: Vec::new()`), so the crate compiles.

**Step 4: Run to verify pass**

Run: `cargo test -p sp42-cli verify_collects_section`
Expected: PASS.

**Step 5: Commit**

```bash
git add crates/sp42-cli/src/main.rs
git commit -m "feat(cli): parse --section-title and --preceding-sentence for verify"
```

## Task 2: assemble `ClaimContext` and pass it to the verifier

**Files:**
- Modify: `crates/sp42-cli/src/main.rs` — `run_verify` (`:815`), call site (`:874`)

**Step 1: Build the optional context** in `run_verify`, before the `verify_citation_use_site` call:

```rust
    let claim_context = {
        let ctx = ClaimContext {
            article_title: String::new(),
            section_title: options.section_title.clone(),
            preceding_sentences: options.preceding_sentences.clone(),
        };
        if ctx.is_empty() { None } else { Some(ctx) }
    };
```

Import `ClaimContext` from `sp42_core` in the CLI's use list.

**Step 2: Pass it** — replace the `None` Phase 2 left in the context argument at `main.rs:874` with `claim_context.as_ref()`.

**Step 3: Verify**

Run: `cargo build -p sp42-cli`
Expected: builds.

Run: `cargo test -p sp42-cli`
Expected: all pass (no network; the new path is exercised by the parse test, not a live call).

**Step 4: Commit**

```bash
git add crates/sp42-cli/src/main.rs
git commit -m "feat(cli): pass assembled ClaimContext to the verifier (empty = control)"
```

**Phase 3 done when:** omitting the flags reproduces today's behavior (empty context → `None`), the flags assemble a `ClaimContext`, parse tests pass, `cargo test -p sp42-cli` passes, `cargo clippy -p sp42-cli -- -D warnings` clean.

## Final verification (whole feature)

Run: `cargo test -p sp42-core -p sp42-cli`
Run: `cargo clippy -p sp42-core -p sp42-cli -- -D warnings`
Expected: all pass, clean. (Full `ci-all` with the Tauri build can't pass in this environment — GTK libs absent; run core+cli checks here and leave the full gate to the operator per the SP42 pre-push note.)
