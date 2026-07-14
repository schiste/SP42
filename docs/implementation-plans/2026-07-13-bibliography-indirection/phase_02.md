# Bibliography Indirection Implementation Plan — Phase 2: refined skip reason, docs, gate, measurement

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** The unresolved-short-cite case gets its honest, distinct skip reason on THIS branch's surface (extract → dev report); docs and the full workspace gate close the branch; the ESC 1973 conversion measurement is produced for the PR description.

**Architecture:** `SkippedReason` gains a **third** additive variant; `sp42-parsoid`'s `short_cite_unresolved` flag (Phase 1) drives it in `extract_use_sites`; the dev page report renders it automatically (Debug-formatted). Everything else is docs/verification.

**Cross-branch note (explicit):** the GA-appendix renderer (`crates/sp42-assessment`, its `copy.rs` skip vocabulary, `what-is-this-appendix.md`, and the `page_report_ga_demo.json` fixture) lives on the **unmerged** `claude/ga-appendix-renderer` branch and is NOT in this tree. Its skipped-section renderer and explainer must gain the new reason's reader-facing copy **when the two branches meet** (whichever merges second). Record this as a follow-up in the design doc's Done-when (Task 2) — do NOT attempt those edits here.

**Scope:** Phase 2 of 2.

**Codebase verified:** 2026-07-13 (validator-corrected). `SkippedReason` at `crates/sp42-citation/src/citation/extract.rs:39-49` has **two** variants — `NonUrlSource` and `BookSource` (the merged book lane's refined reason) — serde snake_case. Producers/matchers that must keep compiling when a variant is added: `extract.rs` (skip branch ~137-145 and test ~370), `citation/page.rs:739` and `:1448`, `citation_page_report.rs:424` (tests), plus the `lib.rs` re-export. The dev report renders skip reasons via `{:?}` Debug at `citation_page_report.rs:127-128` — a new variant renders with **zero code change** there (CamelCase Debug form; that surface is dev-facing, raw tokens acceptable); the assertion at `citation_page_report.rs:518` may need extending.

---

## Task 1: `SkippedReason::UnresolvedShortCite`

**Files:**
- Modify: `crates/sp42-citation/src/citation/extract.rs`

**Step 1: Failing tests** (in `extract.rs`'s test module, mirroring the existing `bref`/`block` helpers):

```rust
#[test]
fn unresolved_short_cite_ref_gets_the_refined_skip_reason() {
    let mut r = bref(10, &[]);
    r.short_cite_unresolved = true;
    let b = block("Cats purr.", vec![r]);
    let out = extract_use_sites(&[b], &page());
    assert_eq!(out.skipped.len(), 1);
    assert_eq!(out.skipped[0].reason, SkippedReason::UnresolvedShortCite);
}

#[test]
fn resolved_short_cite_ref_is_a_book_use_site_not_a_skip() {
    // A short-cite that resolved carries book_sources (Phase 1 sets the flag
    // false); belt-and-braces: with both set, book_sources win.
    let mut r = bref(10, &[]);
    r.short_cite_unresolved = true;
    r.book_sources = vec![/* one valid BookSource, as the existing book tests build */];
    let b = block("Cats purr.", vec![r]);
    let out = extract_use_sites(&[b], &page());
    assert_eq!(out.book_use_sites.len(), 1);
    assert!(out.skipped.is_empty());
}

#[test]
fn skip_reason_round_trips_serde() {
    let json = serde_json::to_string(&SkippedReason::UnresolvedShortCite).expect("serializes");
    assert_eq!(json, "\"unresolved_short_cite\"");
    assert_eq!(
        serde_json::from_str::<SkippedReason>(&json).expect("parses"),
        SkippedReason::UnresolvedShortCite
    );
}
```

**Step 2: Run to verify failure.**

**Step 3: Implement** — ADD the variant, preserving both existing ones (this enum has TWO today; do not drop `BookSource`):

```rust
    /// The ref is a shortened footnote whose bibliography target could not
    /// be resolved to a book identifier (PRD-0009 Layer-1 amendment,
    /// 2026-07-13): the anchor matched nothing, or the matched entry carried
    /// no validated identifier. Never guessed — disclosed.
    UnresolvedShortCite,
```

In the skip branch (`extract.rs` ~137-145): the reason becomes `UnresolvedShortCite` when `r.short_cite_unresolved` (else the existing logic — read the current branch, which already distinguishes `BookSource` skips, and slot the new check so book-source-bearing refs keep their current behavior). Verify every match site the header lists still compiles; extend the `citation_page_report.rs:518`-area assertion if it enumerates reasons.

**Step 4: Run** — `cargo test -p sp42-citation` → pass. **Step 5: Commit** — `feat(citation): refined skip reason for unresolved shortened footnotes`

---

## Task 2: Docs

**Files:**
- Modify: `docs/STATUS.md` (book-citation grounding bullet: one added clause — identifiers now also resolve via bibliography indirection and ref-local magiclinks; unresolved short-cites disclosed with a refined skip reason)
- Modify: `docs/design-plans/2026-07-13-bibliography-indirection.md` — "Status: Sketch" → "Status: Implemented (this branch)"; tick the Done-when items with their test names; ADD a follow-up line: "GA appendix copy for `unresolved_short_cite` (reader-facing vocabulary + explainer entry) lands when this branch and `claude/ga-appendix-renderer` meet — tracked in whichever merges second."
- Regenerate `docs/platform/architecture.md` via `scripts/generate-architecture-map.sh` only if `--check` says stale.

**Verify:** `bash scripts/check-doc-consistency.sh && bash scripts/check-links.sh`. **Commit** — `docs(references): record bibliography-indirection implementation`

---

## Task 3: Full gate (the complete list — every one has bitten)

```sh
cargo build --workspace --all-targets --profile ci
cargo test --workspace --profile ci
cargo clippy --workspace --all-targets --all-features --profile ci -- -D warnings
cargo doc --workspace --no-deps --profile ci
cargo check -p sp42-app --target wasm32-unknown-unknown --tests
./scripts/check-layering.sh
./scripts/generate-architecture-map.sh --check
./scripts/check-design-system.sh          # no "#NNN" in comments — write "PR 147"
./scripts/check-links.sh
./scripts/check-doc-consistency.sh
./scripts/check-forbidden-patterns.sh --range origin/main..HEAD   # AFTER final commit
```
Wasm note: if the size ceiling trips, stop and surface — do not ratchet without measured numbers.

Commit fallout only. Do NOT push in this task.

---

## Task 4: Live measurement + wrap-up (no PR)

- Start the branch server: build `sp42-server`/`sp42-cli`, run with `SP42_BIND_ADDR=127.0.0.1:8790`, env from `../../.env.wikimedia.local` + `alex-cite-checker/.env`, `SP42_FETCH_ALLOW_PRIVATE=1`.
- Run `verify-page --title "Eurovision Song Contest 1973" --wiki enwiki --rev 1362468105 --format json` against it; record `books_resolved` / `books_not_found` / `book_lookups_failed` and the skipped-reason split vs. the 2026-07-12 baseline (73 skipped, 0 lookups).
- Push the branch (`SP42_SKIP_GIT_HOOKS=1 git push -u origin claude/bibliography-indirection`) only after Task 3 ran green.
- **Do NOT open a PR.** Report to the Editor: the measurement delta, fixture provenance (live probes), the cross-branch follow-up (appendix copy), and the two deliberate non-goals (dewiki unlinked free-text short refs; lane-B page parsing).
