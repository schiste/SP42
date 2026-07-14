# GA Evidence Appendix Renderer Implementation Plan — Phase 3: fixture realism + docs

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** A real-article-shaped saved-report fixture rendered through the actual CLI path, the "what is this?" explainer the footer links to, and the documentation the crate's birth obligates (assessment READMEs, architecture map, PRD state).

**Architecture:** Fixture is a checked-in JSON `PageVerificationReport` under `fixtures/` (synthetic content, realistic shape — bundled refs, every bucket populated; NOT copied from live benchmark runs, which stay local by project policy). The sandbox talk-page paste check and the `{{GAList}}` idiom question are operator (Editor) steps — the plan prepares the artifact and flags them, it does not post anywhere.

**Scope:** Phase 3 of 3.

**Codebase verified:** 2026-07-12. `fixtures/` exists (citoid/, frwiki JSON, testwiki.yaml) with no page-report fixture. Stale "No crate yet" lines: `docs/domains/assessment/README.md:11` and `docs/domains/README.md:39` (assessment entry; lines 20/29 are *other* domains — leave them). Architecture map is generated — never hand-edit `docs/platform/architecture.md`; run `scripts/generate-architecture-map.sh` after the crate exists.

---

## Task 1: Real-article-shaped fixture + round-trip test

**Files:**
- Create: `fixtures/page_report_ga_demo.json`
- Modify: `crates/sp42-assessment/src/ga_appendix.rs` (one test)

**Step 1: Create the fixture.** A `PageVerificationReport` JSON (the exact serde shape — generate it by writing a small fixture-builder test first, or serialize the Phase-1 `fixtures::full_report()` with `serde_json::to_string_pretty` and hand-adjust) modeling a plausible mid-size article, all synthetic:

- `wiki_id: "testwiki"`, `title: "Fixture Bridge (demo)"`, `rev_id: 100200300`
- ~12 findings covering: 2 disagreements (one `not_supported` with `source_excerpt` and no passage at 1-of-3 agreement; one `partial` with a passage, fuzzy grounding, and `archive_of`), 1 recovered-via-archive supported, 2 dead links, 2 unreadable (`pdf_body`, `nav_chrome_paywall`), 2 unconfirmed supports (`unlocated`, `located_fuzzy`), 3 supported including a bundled ref (same `ref_id`, two URLs)
- 2 skipped refs (`non_url_source`), 1 extraction failure with a raw `cite_ref-…` in its `reason`
- `stats` arithmetically consistent with the findings/skips/failures
- One hostile field: a claim containing `{{Infobox}}` and a `</nowiki>` (safety regression stays exercised at the fixture level, not only in unit fixtures)

Serde spellings to honor (verified): verdicts serialize flat (`"supported" | "partial" | "not_supported" | "source_unavailable"`), `grounding_status` snake_case (`"located" | "located_fuzzy" | "unlocated" | "not_applicable"`), `unusable_reason` snake_case (`"pdf_body"` …), `source_unavailable_reason` snake_case. Confirm any doubt by serializing the in-code fixture and diffing.

**Step 2: Write the test** (in `ga_appendix.rs` tests; `include_str!` keeps the renderer pure):

```rust
#[test]
fn demo_fixture_renders_the_full_appendix_shape() {
    let raw = include_str!("../../../fixtures/page_report_ga_demo.json");
    let report: sp42_citation::PageVerificationReport =
        serde_json::from_str(raw).expect("fixture parses as a saved report");
    let out = super::render_ga_appendix(&report, 1_752_300_000_000, "0.1.0");
    for heading in [
        copy::BUCKET_DISAGREEMENTS, copy::BUCKET_RECOVERED, copy::BUCKET_DEAD_LINKS,
        copy::BUCKET_UNREADABLE, copy::BUCKET_UNCONFIRMED, copy::BUCKET_SUPPORTED,
        copy::BUCKET_SKIPPED, copy::BUCKET_EXTRACTION_FAILURES,
    ] {
        assert!(out.contains(heading), "missing bucket: {heading}");
    }
    assert!(!out.contains("cite_ref-"), "raw id leaked from fixture");
}
```

**Step 3: Run** — `cargo test -p sp42-assessment demo_fixture` → PASS (iterate the fixture until stats consistency and every bucket assertion holds).

**Step 4: Render the paste artifact for the Editor's sandbox hand-check** (do NOT post it anywhere):

```sh
cargo run -p sp42-cli -- render-report fixtures/page_report_ga_demo.json --format ga-appendix > /tmp/ga-appendix-demo.wikitext
```

Report the file path in the phase summary; the sandbox talk-page paste and the `{{GAList}}` native-idiom question to real GA reviewers are the Editor's steps (PRD-0016 Alternatives; copy-module change if adopted).

**Step 5: Commit**

```sh
git add fixtures/page_report_ga_demo.json crates/sp42-assessment
git commit -m "test(assessment): real-article-shaped saved-report fixture for the appendix"
```

---

## Task 2: The "what is this?" explainer page

**Files:**
- Create: `docs/domains/assessment/what-is-this-appendix.md`

**Step 1: Write the page.** The footer's `copy::EXPLAINER_URL` (Phase 1) points here; audience is the on-ramp reader (zero SP42 context) landing from a talk page. Required content, in reader order, ~1 page:

1. What the appendix is: a tool-generated evidence report about whether an article's inline citations support its text; generated by SP42, an open-source citation-verification tool; the reviewer pastes it — the tool never edits the wiki.
2. What it is not: not a verdict on the article, not a GA pass/fail, not a complete check (evidence for criterion 2b only; everything else unassessed).
3. The vocabulary, one line each, in the copy module's exact phrasing: claim–source disagreement / supported via archive copy / dead link / source the tool could not read (tool limitation) / unconfirmed support (judged supported, quote not re-located) / supported spot-check / not machine-verified (books, offline) / panel split (low-confidence).
4. How to read a line: ref label, claim, quote or its absence, source link.
5. Where to report problems: the GitHub repo link.

Cross-check against `copy.rs` after writing: every heading and vocabulary phrase in the explainer must match the copy module verbatim (copy-drift is PRD-0016's named first-impression risk). Add a comment at the top of `copy.rs`: `// Vocabulary here renders on-wiki; keep docs/domains/assessment/what-is-this-appendix.md in sync.`

**Step 2: Verify** — every `BUCKET_*` phrase in `copy.rs` appears in the explainer (`grep` each), and `copy::EXPLAINER_URL`'s path segment matches the created filename.

**Step 3: Commit**

```sh
git add docs/domains/assessment/what-is-this-appendix.md crates/sp42-assessment/src/copy.rs
git commit -m "docs(assessment): what-is-this explainer for the GA appendix footer"
```

---

## Task 3: Domain docs, architecture map, PRD state

**Files:**
- Modify: `docs/domains/assessment/README.md` (line 11 block)
- Modify: `docs/domains/README.md` (line ~39 assessment entry only)
- Modify: `docs/domains/assessment/prd/0016-ga-evidence-appendix-renderer.md` (Changelog + State)
- Regenerate: `docs/platform/architecture.md` (generated — no hand edits)

**Step 1: Update the assessment README.** Replace the "No crate yet." sentence (line 11) with: the domain's first crate is `crates/sp42-assessment` (PRD-0016 renderer: pure `PageVerificationReport → wikitext` appendix; CLI surfaces `verify-page --format ga-appendix` and `render-report`); workflow sequencing still lives in the design sketch.

**Step 2: Update the domain index.** In `docs/domains/README.md`, the assessment entry's "No crate yet; specified as PRD-0015 plus the GA-workflow…" — rewrite to name `crates/sp42-assessment` as the domain's crate (PRD-0016 implemented; PRD-0015 still specification-only). Do not touch the other domains' "No crate yet" lines (20/29).

**Step 3: PRD-0016 bookkeeping.** Append a Changelog entry (dated, implementation PR): MVP implemented as `crates/sp42-assessment` + CLI surfaces; record the exact test ids covering each DoD item (enumerate from `cargo test -p sp42-assessment -- --list`); note the two staged non-MVP items (criterion 5 / PRD-0015) remain open; note the panel-split annotation addition and its session provenance for Editor review — including its deliberate scoping: `PanelAgreement` is carried on every finding, but the annotation renders only on disagreement lines (where a minority verdict is an accusation the reviewer may act on); supported/unavailable lines stay unannotated to keep the spot-check record compact. Strike-or-keep, and widen-or-narrow, are both one-const copy decisions. Move `State: Discussion` → `State: Implemented (MVP; criterion-5 arm staged on PRD-0015)`.

**Step 4: Regenerate the architecture map**

```sh
bash scripts/generate-architecture-map.sh
```
Expected output line reports 20 crates (was 19) and picks up sp42-assessment's ADR/PRD references.

**Step 5: Full gate + commit**

```sh
cargo build --workspace --all-targets --profile ci && cargo test --workspace --profile ci \
  && cargo clippy --workspace --all-targets --all-features --profile ci -- -D warnings \
  && cargo doc --workspace --no-deps --profile ci && ./scripts/check-layering.sh
git add docs/domains docs/platform/architecture.md
git commit -m "docs(assessment): record the crate's birth (PRD-0016 MVP implemented)"
```

---

## Task 4: Branch wrap-up (no PR)

- Push the branch: `SP42_SKIP_GIT_HOOKS=1 git push -u origin claude/ga-appendix-renderer` **only after** the full gate above ran green manually (the pre-push hook's final Tauri build cannot pass on this host — known GTK gap; the manual gate substitutes for it).
- **Do NOT open a PR.** Summarize for the Editor: what landed, the demo appendix artifact path, the DoD checklist state, the panel-split addition to strike-or-keep, and the two operator steps (sandbox paste check, `{{GAList}}` question).
