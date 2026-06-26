# Node-Anchored Wikitext Editing (ADR-0003) — Phase 1: Exactly-One-Occurrence Guard

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** Replace the two blind `replacen(…, 1)` literal content-edit sites with an exactly-one-occurrence guard so an ambiguous needle refuses instead of silently editing the wrong span (ADR-0003 Decision 4 — ship-first hardening).

**Architecture:** A pure guarded-replacement helper lives in `sp42-core::action_executor` (pure logic in core, side effects at the edges — Constitution Art. 2). Both server call sites in `sp42-server::action_routes` route through it.

**Tech Stack:** Rust workspace; `clippy::pedantic` and `warnings` are **deny** workspace-wide — public functions returning `Result` need `# Errors` doc sections, public pure functions need `#[must_use]` where applicable.

**Scope:** Phase 1 of 5 from ADR-0003 (docs/platform/adr/0003-node-anchored-wikitext-editing.md).

**Codebase verified:** 2026-06-09 (worktree `.worktrees/node-anchored-editing`, branch `louie/node-anchored-editing`, baseline `cargo test --workspace` green).

---

## Context for the implementer

- ADR-0003 is at `docs/platform/adr/0003-node-anchored-wikitext-editing.md`. Decision 4: "For any remaining literal-span path, replace blind `replacen(…, 1)` with an exactly-one-occurrence guard: reject the edit unless the needle occurs exactly once."
- The two sites today (verified):
  - **InlineEdit** — `crates/sp42-server/src/action_routes.rs:387-396`: `page_text.replacen(&original, &replacement, 1)` with only a not-found check (`code: "text-not-found"`). Multiple occurrences silently edit the first one.
  - **TagCitationNeeded** — `apply_citation_template` at `crates/sp42-server/src/action_routes.rs:421-443`: same pattern.
- `ActionError` is a single-variant enum (`ActionError::Execution { message, code, http_status, retryable }`) defined at `crates/sp42-core/src/errors.rs:91-99`. `http_status: None` maps to HTTP 502 in `action_error_response` (`action_routes.rs:850-871`); `Some(400..=499)` maps to 400.
- Test commands: fast loop is `cargo test -p sp42-core -p sp42-server`. The repo convention for the full focused check is `scripts/check-focused.sh` (serial tests via `RUST_TEST_THREADS=1`).
- House test style: `#[cfg(test)] mod tests` blocks in the same file; descriptive snake_case test names; `expect("…should…")` messages.

---

## Task 1: Guarded replacement helper in `sp42-core`

**Files:**
- Modify: `crates/sp42-core/src/action_executor.rs` (helper + tests; the existing `#[cfg(test)] mod tests` starts at line 606)
- Modify: `crates/sp42-core/src/lib.rs` (export; the `pub use action_executor::{…}` block is at lines ~60-64)

**Step 1: Write the failing tests**

Append inside the existing `#[cfg(test)] mod tests` module at the bottom of `crates/sp42-core/src/action_executor.rs` (add `use super::replace_exactly_once;` alongside the module's existing imports if it does not glob-import `super::*`; `ActionError` is already in scope in that module via the crate's error imports — if not, add `use crate::errors::ActionError;`):

```rust
    #[test]
    fn replace_exactly_once_replaces_single_occurrence() {
        let result = replace_exactly_once("alpha beta gamma", "beta", "BETA")
            .expect("single occurrence should replace");
        assert_eq!(result, "alpha BETA gamma");
    }

    #[test]
    fn replace_exactly_once_rejects_missing_needle() {
        let error = replace_exactly_once("alpha beta", "delta", "DELTA")
            .expect_err("missing needle should refuse");
        let ActionError::Execution { code, retryable, .. } = error;
        assert_eq!(code.as_deref(), Some("text-not-found"));
        assert!(!retryable);
    }

    #[test]
    fn replace_exactly_once_rejects_ambiguous_needle() {
        let error = replace_exactly_once("ref one ref two", "ref", "REF")
            .expect_err("ambiguous needle should refuse");
        let ActionError::Execution { code, message, .. } = error;
        assert_eq!(code.as_deref(), Some("text-ambiguous"));
        assert!(message.contains("2 times"), "message should report the count: {message}");
    }

    #[test]
    fn replace_exactly_once_rejects_empty_needle() {
        let error = replace_exactly_once("alpha", "", "X")
            .expect_err("empty needle should refuse");
        let ActionError::Execution { code, .. } = error;
        assert_eq!(code.as_deref(), Some("invalid-input"));
    }
```

**Step 2: Run the tests to verify they fail**

Run: `cargo test -p sp42-core replace_exactly_once`
Expected: compile error — `replace_exactly_once` not found. That is the failing state.

**Step 3: Write the implementation**

Add to `crates/sp42-core/src/action_executor.rs`, as a free public function near the other `execute_*`/`build_*` functions (before the `#[cfg(test)]` module). The file already has `ActionError` in scope (it is the error type of `execute_rollback` at line 235):

```rust
/// Replace exactly one occurrence of `needle` within `haystack`.
///
/// ADR-0003 Decision 4: a literal-span content edit must never guess between
/// multiple matches, so zero matches and multiple matches both refuse instead
/// of silently editing the wrong span.
///
/// # Errors
///
/// Returns [`ActionError::Execution`] with code `invalid-input` when `needle`
/// is empty, `text-not-found` when it does not occur in `haystack`, and
/// `text-ambiguous` when it occurs more than once.
pub fn replace_exactly_once(
    haystack: &str,
    needle: &str,
    replacement: &str,
) -> Result<String, ActionError> {
    if needle.is_empty() {
        return Err(ActionError::Execution {
            message: "replacement target text must not be empty".to_string(),
            code: Some("invalid-input".to_string()),
            http_status: None,
            retryable: false,
        });
    }
    match haystack.matches(needle).count() {
        0 => Err(ActionError::Execution {
            message: "selected text not found in page content".to_string(),
            code: Some("text-not-found".to_string()),
            http_status: None,
            retryable: false,
        }),
        1 => Ok(haystack.replacen(needle, replacement, 1)),
        occurrences => Err(ActionError::Execution {
            message: format!(
                "selected text occurs {occurrences} times in page content; refusing ambiguous replacement"
            ),
            code: Some("text-ambiguous".to_string()),
            http_status: None,
            retryable: false,
        }),
    }
}
```

Then add `replace_exactly_once` to the `pub use action_executor::{…}` list in `crates/sp42-core/src/lib.rs` (keep the list's alphabetical order: it goes after `parse_token_response`).

**Step 4: Run the tests to verify they pass**

Run: `cargo test -p sp42-core replace_exactly_once`
Expected: `test result: ok. 4 passed`

**Step 5: Commit**

```bash
git add crates/sp42-core/src/action_executor.rs crates/sp42-core/src/lib.rs
git commit -m "feat(actions): add exactly-one-occurrence replacement guard (ADR-0003 D4)"
```

---

## Task 2: Route both server literal-edit sites through the guard

**Files:**
- Modify: `crates/sp42-server/src/action_routes.rs:9-16` (import), `:387-396` (InlineEdit), `:421-443` (`apply_citation_template`), tests module at `:893-922`

**Step 1: Write the failing tests**

`apply_citation_template` is a pure function — test it directly. In the `#[cfg(test)] mod tests` at the bottom of `action_routes.rs`, extend the imports (currently `use super::action_feedback_for_entry;` and `use sp42_core::{ActionExecutionLogEntry, SessionActionKind};`) with `use super::apply_citation_template;` and `use sp42_core::ActionError;`, then add:

```rust
    #[test]
    fn apply_citation_template_tags_unique_selected_text() {
        let updated = apply_citation_template(
            "Une phrase sans source.",
            "phrase sans source",
            "Référence nécessaire",
            "juin 2026",
        )
        .expect("unique selected text should tag");
        assert_eq!(
            updated,
            "Une {{Référence nécessaire|phrase sans source|date=juin 2026}}."
        );
    }

    #[test]
    fn apply_citation_template_refuses_ambiguous_selected_text() {
        let error = apply_citation_template(
            "mot répété, mot répété.",
            "mot répété",
            "Référence nécessaire",
            "juin 2026",
        )
        .expect_err("ambiguous selected text should refuse");
        let ActionError::Execution { code, .. } = error;
        assert_eq!(code.as_deref(), Some("text-ambiguous"));
    }

    #[test]
    fn apply_citation_template_refuses_missing_selected_text() {
        let error = apply_citation_template(
            "Une phrase sans source.",
            "texte absent",
            "Référence nécessaire",
            "juin 2026",
        )
        .expect_err("missing selected text should refuse");
        let ActionError::Execution { code, .. } = error;
        assert_eq!(code.as_deref(), Some("text-not-found"));
    }
```

**Step 2: Run the tests to verify the ambiguity test fails**

Run: `cargo test -p sp42-server apply_citation_template`
Expected: `apply_citation_template_refuses_ambiguous_selected_text` FAILS (current code happily tags the first occurrence — that is the bug being closed). The other two pass.

**Step 3: Rewire both call sites**

1. In the `use sp42_core::{…}` import block (`action_routes.rs:9-16`), add `replace_exactly_once` (alphabetically, after `parse_action_response_summary`).

2. **InlineEdit** — replace lines 387-396:

```rust
    let page_text = crate::fetch_page_wikitext(client, config, &title).await?;
    let updated_text = page_text.replacen(&original, &replacement, 1);
    if updated_text == page_text {
        return Err(ActionError::Execution {
            message: "original text not found in page content".to_string(),
            code: Some("text-not-found".to_string()),
            http_status: None,
            retryable: false,
        });
    }
```

with:

```rust
    let page_text = crate::fetch_page_wikitext(client, config, &title).await?;
    let updated_text = replace_exactly_once(&page_text, &original, &replacement)?;
```

3. **`apply_citation_template`** — replace its body's tail (lines 433-442):

```rust
    let updated_text = page_text.replacen(selected_text, &tagged, 1);
    if updated_text == page_text {
        return Err(ActionError::Execution {
            message: "selected text not found in page content".to_string(),
            code: Some("text-not-found".to_string()),
            http_status: None,
            retryable: false,
        });
    }
    Ok(updated_text)
```

with:

```rust
    replace_exactly_once(page_text, selected_text, &tagged)
```

If `ActionError` is now unused in this file's imports, the deny-warnings build will say so — remove it from the import list only if the compiler flags it (it is still used elsewhere in the file, e.g. `execute_session_action`, so it almost certainly stays).

**Step 4: Run the tests to verify they pass**

Run: `cargo test -p sp42-server apply_citation_template`
Expected: `test result: ok. 3 passed`

Run: `cargo test -p sp42-core -p sp42-server`
Expected: all green.

**Step 5: Commit**

```bash
git add crates/sp42-server/src/action_routes.rs
git commit -m "feat(server): refuse ambiguous literal content edits (ADR-0003 D4)"
```

---

## Phase verification

Run: `cargo test -p sp42-core -p sp42-server && cargo clippy -p sp42-core -p sp42-server --all-targets -- -D warnings`
Expected: tests pass, clippy clean.

**Done when:** both `replacen` sites are gone from `action_routes.rs` (grep `replacen` in `crates/sp42-server/` returns nothing), ambiguous needles refuse with `text-ambiguous`, and the workspace tests pass.
