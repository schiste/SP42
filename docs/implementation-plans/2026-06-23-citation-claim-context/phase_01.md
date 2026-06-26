# Citation Claim Context — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** Add a `ClaimContext` type and render it into the verification prompt as a context-only block, with empty/absent context preserving today's exact prompt.

**Architecture:** `ClaimContext` is a small contract type in `prompts.rs` (the module that renders it; `verify.rs` already depends on `prompts.rs`, so this stays acyclic). `build_verify_prompt` gains an `Option<&ClaimContext>` parameter and a `context_section` helper that mirrors the existing `metadata_section` (labeled context-only text, empty string when there is nothing to render).

**Tech Stack:** Rust, `sp42_types::ChatMessage`, the existing two-step verifier prompt.

**Scope:** Phase 1 of 3.

**Codebase verified:** 2026-06-23. `build_verify_prompt(claim, source_text, source_url, metadata: Option<&CitoidMetadata>) -> [ChatMessage; 2]` at `prompts.rs:82`; `metadata_section` at `prompts.rs:113`; sole caller is `build_verify_completion_request` at `verify.rs:298`. `lib.rs` re-exports citation prompt/verify items around `lib.rs:98-103`.

---

## Task 1: `ClaimContext` type

**Files:**
- Modify: `crates/sp42-core/src/citation/prompts.rs` (add type near top, after imports)
- Modify: `crates/sp42-core/src/lib.rs` (re-export)

**Step 1: Add the type** in `prompts.rs`:

```rust
/// The SIDE-style co-reference context window for a claim (interpreting material only).
///
/// Rendered into the verification prompt as a **context-only** block — the model may use it
/// to interpret the claim (resolve pronouns / elliptical references) but may never quote it
/// as support. The grounding gate only ever locates quotes in the fetched source body, so
/// this can never become groundable (refines ADR-0007 Alt (e)). Carries the new contextual
/// material only; the claim itself stays single-source on `CitationVerificationRequest`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClaimContext {
    /// The article title.
    pub article_title: String,
    /// The section title, when known.
    pub section_title: Option<String>,
    /// Preceding sentences, in document order (most useful for co-reference).
    pub preceding_sentences: Vec<String>,
}

impl ClaimContext {
    /// `true` when there is no contextual material to render (renders nothing, keeping the
    /// prompt byte-identical to the no-context form).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.article_title.trim().is_empty()
            && self.section_title.as_ref().map_or(true, |s| s.trim().is_empty())
            && self.preceding_sentences.iter().all(|s| s.trim().is_empty())
    }
}
```

**Step 2:** Re-export from `lib.rs` — add `ClaimContext` to the existing `pub use citation::...prompts`/`verify` group (place beside the other citation prompt exports).

**Step 3: Verify it compiles**

Run: `cargo build -p sp42-core`
Expected: builds (type unused so far is fine; it's `pub`).

**Step 4: Commit**

```bash
git add crates/sp42-core/src/citation/prompts.rs crates/sp42-core/src/lib.rs
git commit -m "feat(citation): add ClaimContext type"
```

## Task 2: `context_section` renderer (TDD)

**Files:**
- Modify: `crates/sp42-core/src/citation/prompts.rs`

**Step 1: Write failing tests** (in the existing `#[cfg(test)] mod tests`):

```rust
#[test]
fn context_section_is_empty_for_empty_context() {
    let ctx = ClaimContext::default();
    assert_eq!(context_section(&ctx), String::new());
}

#[test]
fn context_section_renders_labeled_context_only_block() {
    let ctx = ClaimContext {
        article_title: "Ann Jansson".to_string(),
        section_title: Some("Career".to_string()),
        preceding_sentences: vec!["She joined the club in 1985.".to_string()],
    };
    let rendered = context_section(&ctx);
    assert!(rendered.contains("Ann Jansson"));
    assert!(rendered.contains("Career"));
    assert!(rendered.contains("She joined the club in 1985."));
    // Context-only discipline: must tell the model the supporting quote comes from the SOURCE.
    assert!(rendered.contains("DO NOT quote"));
    assert!(rendered.to_uppercase().contains("SOURCE"));
}
```

**Step 2: Run to verify failure**

Run: `cargo test -p sp42-core context_section`
Expected: FAIL (no `context_section`).

**Step 3: Implement** `context_section` (sibling to `metadata_section`):

```rust
/// Render the co-reference context window as a context-only block (empty string when the
/// context has nothing to show, so the prompt is byte-identical to the no-context form).
fn context_section(context: &ClaimContext) -> String {
    if context.is_empty() {
        return String::new();
    }
    let mut lines = Vec::new();
    if !context.article_title.trim().is_empty() {
        lines.push(format!("- article: {}", context.article_title));
    }
    if let Some(section) = &context.section_title {
        if !section.trim().is_empty() {
            lines.push(format!("- section: {section}"));
        }
    }
    let preceding: Vec<&String> = context
        .preceding_sentences
        .iter()
        .filter(|s| !s.trim().is_empty())
        .collect();
    if !preceding.is_empty() {
        lines.push("- preceding text:".to_string());
        for sentence in preceding {
            lines.push(format!("    {sentence}"));
        }
    }
    format!(
        "CLAIM CONTEXT (for interpreting the claim only — DO NOT quote from here; your supporting quote MUST come verbatim from the SOURCE text below):\n{}\n\n",
        lines.join("\n")
    )
}
```

**Step 4: Run to verify pass**

Run: `cargo test -p sp42-core context_section`
Expected: PASS.

**Step 5: Commit**

```bash
git add crates/sp42-core/src/citation/prompts.rs
git commit -m "feat(citation): render ClaimContext as a context-only prompt block"
```

## Task 3: thread context into `build_verify_prompt` (TDD)

**Files:**
- Modify: `crates/sp42-core/src/citation/prompts.rs` (signature + body)
- Modify: `crates/sp42-core/src/citation/verify.rs:298` (sole caller — pass `None` for now; Phase 2 supplies real context)

**Step 1: Write failing tests** in `prompts.rs` tests:

```rust
#[test]
fn empty_context_is_byte_identical_to_no_context() {
    let with_none = build_verify_prompt("c", "body", "https://example.com", None, None);
    let with_empty =
        build_verify_prompt("c", "body", "https://example.com", None, Some(&ClaimContext::default()));
    assert_eq!(with_none[1].content, with_empty[1].content);
}

#[test]
fn context_block_precedes_the_source_block() {
    let ctx = ClaimContext { article_title: "Ann Jansson".to_string(), ..Default::default() };
    let prompt = build_verify_prompt("c", "body", "https://example.com", None, Some(&ctx));
    let user = &prompt[1].content;
    let ctx_at = user.find("CLAIM CONTEXT").expect("context block present");
    let source_at = user.find("SOURCE (").expect("source block present");
    assert!(ctx_at < source_at);
}
```

Also update the existing `build_verify_prompt` test call sites in this module to pass the new trailing `None` argument (there are several — add `, None` before the closing paren).

**Step 2: Run to verify failure**

Run: `cargo test -p sp42-core --lib citation::prompts`
Expected: FAIL (arity mismatch / new tests fail to compile until signature changes).

**Step 3: Implement** — change the signature and body of `build_verify_prompt`:

```rust
#[must_use]
pub fn build_verify_prompt(
    claim: &str,
    source_text: &str,
    source_url: &str,
    metadata: Option<&CitoidMetadata>,
    context: Option<&ClaimContext>,
) -> [ChatMessage; 2] {
    let context_block = context.map(context_section).unwrap_or_default();
    let section = metadata.map(metadata_section).unwrap_or_default();
    let user = format!(
        "CLAIM:\n{claim}\n\n{context_block}{section}SOURCE ({source_url}):\n\"\"\"\n{source_text}\n\"\"\"\n\nRespond with the JSON object described in the instructions."
    );
    [ChatMessage::system(SYSTEM), ChatMessage::user(user)]
}
```

Then in `verify.rs:298`, update the call to pass the new trailing argument as `None` for now:

```rust
let messages = build_verify_prompt(
    inputs.claim,
    inputs.source_text,
    inputs.source_url,
    inputs.metadata,
    None,
)
.to_vec();
```

**Step 4: Run to verify pass**

Run: `cargo test -p sp42-core` and `cargo build -p sp42-core`
Expected: PASS / builds.

**Step 5: Commit**

```bash
git add crates/sp42-core/src/citation/prompts.rs crates/sp42-core/src/citation/verify.rs
git commit -m "feat(citation): build_verify_prompt accepts an optional ClaimContext"
```

**Phase 1 done when:** `cargo test -p sp42-core` passes, `cargo clippy -p sp42-core -- -D warnings` clean, and empty/absent context produces a byte-identical prompt to today's.
