# Citation Claim Context — Phase 2

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** Thread a `ClaimContext` from `verify_citation_use_site` down to every panel member's prompt, with verdict/grounding behavior unchanged.

**Architecture:** Context rides on `VerifyModelInputs` (the per-model prompt input) as `context: Option<&'a ClaimContext>`, consumed by `build_verify_completion_request` → `build_verify_prompt`. The orchestration entry point `verify_citation_use_site` gains a `context` parameter. The repair turn is unchanged (transcription-only; needs no context).

**Scope:** Phase 2 of 3. Depends on Phase 1.

**Codebase verified:** 2026-06-23. `VerifyModelInputs<'a>` at `verify.rs:234` (fields claim/source_text/source_url/metadata). `build_verify_completion_request` at `verify.rs:282` (calls `build_verify_prompt` at 298 — Phase 1 left a literal `None` there). `verify_citation_use_site` at `verify.rs:691` builds `VerifyModelInputs` at `verify.rs:741`. Sole production caller is `run_verify` at `sp42-cli/src/main.rs:874`. Test helper `inputs()` at `verify.rs:~838` builds `VerifyModelInputs { metadata: None }`. Test doubles: `StubModelClient`, `StubHttpClient`, `FixedClock`.

---

## Task 1: add `context` to `VerifyModelInputs` and pass it to the prompt

**Files:**
- Modify: `crates/sp42-core/src/citation/verify.rs:234` (struct), `:298` (caller), `:838` (test helper)

**Step 1: Add the field** to `VerifyModelInputs<'a>` (after `metadata`):

```rust
    /// The optional co-reference context window (context only — never grounded).
    pub context: Option<&'a ClaimContext>,
```

Import `ClaimContext`: add to the existing `use super::prompts::{...}` line in `verify.rs`.

**Step 2: Pass it through** in `build_verify_completion_request` (`verify.rs:298`) — replace the literal `None` left by Phase 1 with `inputs.context`:

```rust
    let messages = build_verify_prompt(
        inputs.claim,
        inputs.source_text,
        inputs.source_url,
        inputs.metadata,
        inputs.context,
    )
    .to_vec();
```

**Step 3: Fix construction sites** so the crate compiles:
- Test helper `inputs()` (`verify.rs:~838`): add `context: None,`.
- `verify_citation_use_site` build site (`verify.rs:741`): add `context,` (the new param from Task 2 — do Task 2's signature change together so the crate compiles in one step).

**Step 4: Verify** — combined with Task 2, run `cargo build -p sp42-core`. Expected: builds.

## Task 2: `verify_citation_use_site` gains a `context` parameter

**Files:**
- Modify: `crates/sp42-core/src/citation/verify.rs:691` (signature) and `:741` (VerifyModelInputs build)
- Modify: `crates/sp42-cli/src/main.rs:874` (sole caller — pass `None` for now; Phase 3 supplies real context)

**Step 1: Add the parameter** to `verify_citation_use_site` (after `request`, before `use_site_ordinal`):

```rust
    request: &CitationVerificationRequest,
    context: Option<&ClaimContext>,
    use_site_ordinal: u32,
```

**Step 2: Thread it** into the `VerifyModelInputs` built at `verify.rs:741`:

```rust
    let inputs = VerifyModelInputs {
        claim: &request.claim,
        source_text: &fetched.text,
        source_url: request.source_url.as_str(),
        metadata: metadata.as_ref(),
        context,
    };
```

**Step 3: Update the CLI caller** (`sp42-cli/src/main.rs:874`) — add `None,` in the new argument position (between `&request,` and `0,`).

**Step 4: Run**

Run: `cargo build -p sp42-core -p sp42-cli`
Expected: builds.

Run: `cargo test -p sp42-core`
Expected: existing tests pass (new param is `None` everywhere so far).

**Step 5: Commit**

```bash
git add crates/sp42-core/src/citation/verify.rs crates/sp42-cli/src/main.rs
git commit -m "feat(citation): thread ClaimContext through verify_citation_use_site"
```

## Task 3: characterization + not-groundable tests (TDD)

**Files:**
- Modify: `crates/sp42-core/src/citation/verify.rs` tests module

**Step 1: Write the tests.**

(a) **Characterization** — empty context yields the same outcome as `None`. Drive `verify_citation_use_site` twice with a `StubModelClient`/`StubHttpClient` (follow the existing orchestration test in this module for harness setup), once with `None` and once with `Some(&ClaimContext::default())`, and assert the resulting `finding` is equal:

```rust
#[test]
fn empty_context_matches_no_context_finding() {
    // ... set up StubHttpClient returning a usable body, StubModelClient returning a
    // supported verdict whose quote is in the body, FixedClock, single-model panel ...
    let none = block_on(verify_citation_use_site(
        &http, &models, &clock, &panel, &request, None, 0, VerifyOptions::default(),
    )).expect("verify");
    let empty = block_on(verify_citation_use_site(
        &http, &models, &clock, &panel, &request, Some(&ClaimContext::default()), 0, VerifyOptions::default(),
    )).expect("verify");
    assert_eq!(none.finding, empty.finding);
}
```

(b) **Context is not groundable** — a quote present only in the context window, not the source body, must not ground. Test at the gate (`assemble_citation_finding`), which only sees `source_text`:

```rust
#[test]
fn quote_only_in_context_does_not_ground() {
    let source = "The bridge opened to traffic in 1998.";
    let context_only_quote = "She joined the club in 1985."; // appears in context, NOT in source
    let votes = vec![model_verdict(Verdict::Supported, Some(context_only_quote))];
    let finding = assemble_citation_finding(source, &provenance(), &votes, 0);
    assert_eq!(finding.grounding_status, GroundingStatus::Unlocated);
    assert!(finding.passage.is_none());
}
```

**Step 2: Run to verify failure first** (write the assertions before the Task 1/2 wiring if iterating strictly; otherwise confirm they pass after wiring):

Run: `cargo test -p sp42-core context`
Expected: the not-groundable test passes on existing gate behavior (documents the guarantee); the characterization test passes once Tasks 1–2 land.

**Step 3: Run full suite + clippy**

Run: `cargo test -p sp42-core`
Run: `cargo clippy -p sp42-core -- -D warnings`
Expected: PASS / clean.

**Step 4: Commit**

```bash
git add crates/sp42-core/src/citation/verify.rs
git commit -m "test(citation): pin empty-context equivalence and context non-groundability"
```

**Phase 2 done when:** context supplied to `verify_citation_use_site` reaches the prompt, empty context yields a finding identical to today's, a context-only quote does not ground, full suite passes, clippy clean.
