# Piece 1 — Phase 4: Finding surface, serde contract & cross-crate verification

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** Lock the `unusable_reason` data contract (serialized on the finding, back-compatible with legacy records), confirm it rides through the page report, surface it in any existing human-facing findings output, and verify the whole of Piece 1 builds across the workspace and for wasm32.

**Architecture:** The field was added in Phase 1 with `#[serde(default)]`, so it already serializes in `CitationFinding` and therefore in `PageVerificationReport.findings`. This phase is mostly tests plus a small, grep-located human-facing surfacing. Per the design DoD, the finding-field/serde behavior is the hard requirement; report formatting follows it. Investigation (2026-06-25) found `source_unavailable_reason` is consumed only as page stats counters in `page.rs` (~200–211) and is *not* rendered as prose in `sp42-reporting` — so there is little prose rendering to extend, and no new `PageVerificationStats` fields are added.

**Tech Stack:** Rust, `serde`/`serde_json`, in-module tests, `xtask` wasm build.

**Scope:** Phase 4 of 4 (Piece 1). Depends on Phases 1–3.

**Codebase verified:** 2026-06-25.

---

## Task 1: Serde round-trip + legacy back-compat

**Files:**
- Test: `crates/sp42-core/src/citation/verify.rs` `#[cfg(test)]` module

**Step 1: Write the test**

(`serde_json` is a normal dependency of `sp42-core`, always available in tests — no Cargo.toml change needed. `BodyUsabilityReason` is already in the `verify.rs` test-module imports from Phase 1.)

```rust
#[test]
fn unusable_reason_round_trips_and_legacy_defaults_to_none() {
    let fetch = StubHttpClient::new([Ok(HttpResponse {
        status: 200,
        headers: BTreeMap::from([("content-type".to_string(), "application/pdf".to_string())]),
        body: b"%PDF-1.7 body bytes".to_vec(),
    })]);
    let model_client = StubModelClient::new([]);
    let finding = block_on(verify_citation_use_site(
        &fetch,
        &model_client,
        &FixedClock::new(1000),
        &[model()],
        &request("A claim", "https://example.com/x.pdf"),
        None,
        3,
        VerifyOptions::default(),
    ))
    .expect("verifies")
    .finding;
    assert_eq!(finding.unusable_reason, Some(BodyUsabilityReason::PdfBody));

    // Round-trip preserves the reason.
    let json = serde_json::to_string(&finding).expect("serialize");
    let back: CitationFinding = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.unusable_reason, Some(BodyUsabilityReason::PdfBody));

    // Legacy record (field absent) deserializes to None via #[serde(default)].
    let mut value: serde_json::Value = serde_json::from_str(&json).expect("to value");
    value
        .as_object_mut()
        .expect("object")
        .remove("unusable_reason");
    let legacy: CitationFinding = serde_json::from_value(value).expect("legacy deserialize");
    assert_eq!(legacy.unusable_reason, None);
}
```

**Step 2: Run it**

Run: `cargo test -p sp42-core unusable_reason_round_trips`
Expected: PASS.

**Step 3: Commit**

```bash
git add crates/sp42-core/src/citation/verify.rs
git commit -m "test(citation): unusable_reason serde round-trip + legacy back-compat"
```

---

## Task 2: Page-level integration — the reason rides through the report

**Files:**
- Test: `crates/sp42-core/src/citation/page.rs` `#[cfg(test)]` module

**Step 1: Write the test**

Mirror the existing page-level tests (which already exercise `verify_page` with `StubHttpClient`/`StubModelClient`; see the `source_unavailable_reason == Unusable` assertion around page.rs:738). Add `BodyUsabilityReason` to the page.rs `#[cfg(test)]` module's `use super::{…}` imports (the existing page tests reference `SourceUnavailableReason` but not `BodyUsabilityReason`). Assert that a PDF citation's finding in the assembled `PageVerificationReport` carries `unusable_reason == Some(PdfBody)` and that the report serde round-trips with the field intact. Follow the existing page-test setup for constructing the page input and panel.

Key assertions (adapt to the existing page-test harness in this module):

```rust
    // ... build a page whose single citation points at a PDF, run verify_page ...
    let finding = &report.findings[0];
    assert_eq!(finding.verdict, CitationVerdict::SourceUnavailable);
    assert_eq!(finding.unusable_reason, Some(BodyUsabilityReason::PdfBody));

    // The report serializes the per-finding reason (the reviewer-facing surface).
    let json = serde_json::to_string(&report).expect("serialize report");
    let back: PageVerificationReport = serde_json::from_str(&json).expect("deserialize report");
    assert_eq!(back.findings[0].unusable_reason, Some(BodyUsabilityReason::PdfBody));
```

**Step 2: Run it**

Run: `cargo test -p sp42-core` (run the page tests)
Expected: PASS. Confirm the page `source_unavailable_unusable` stat still increments (unchanged — no new stats fields).

**Step 3: Commit**

```bash
git add crates/sp42-core/src/citation/page.rs
git commit -m "test(citation): unusable_reason survives the page report round-trip"
```

---

## Task 3: Surface the reason in existing human-facing output (if any)

**Files:**
- Investigate then (conditionally) modify: `crates/sp42-cli/` and/or `crates/sp42-reporting/`

**Step 1: Locate the human-facing render site**

The reviewer sees a citation result through whatever turns a finding into text. Find it:

Run: `grep -rn "source_unavailable_reason\|SourceUnavailableReason\|\.as_str()" crates/sp42-cli/src crates/sp42-reporting/src`

- **If a site renders the SU reason / verdict as human text** (e.g. the CLI verify command printing `source_unavailable_reason.as_str()`): add `unusable_reason` alongside it, e.g. append `finding.unusable_reason.map(BodyUsabilityReason::as_str)` to the same line/section. Keep the format consistent with the surrounding output.
- **If no such site exists** (investigation found none in `sp42-reporting` as of 2026-06-25): no code change — the serialized `unusable_reason` on each finding is the reviewer-facing surface (consumed by the structured report). Note this in the commit message and move on. Do **not** invent a new renderer; richer prose rendering is a separate follow-on.

**Step 2: Verify**

If you changed a render site: add/extend its test to assert the reason string appears in the output for a PDF/paywall finding, and run it. If no change: skip.

Run (if changed): `cargo test -p sp42-cli` (or `-p sp42-reporting`)
Expected: PASS.

**Step 3: Commit**

```bash
git add -A
git commit -m "feat(reporting): surface unusable_reason in verify output (or: note: serialized finding is the surface)"
```

---

## Task 4: Piece-1 acceptance — workspace + wasm build

**Files:** none (verification only)

**Step 1: Full core suite + clippy**

Run: `cargo test -p sp42-core`
Expected: all pass.

Run: `cargo clippy -p sp42-core --all-targets -- -D warnings`
Expected: no warnings. (The Phase-1 `_source_url`/`_content_type` params became used in Phase 2, so there are no unused-variable warnings.)

**Step 2: Verify the wasm build still succeeds**

`sp42-core` is pulled into the wasm crate `sp42-app`, and all Piece-1 code is pure text/URL/byte inspection (no native deps), so the wasm build must still pass.

Run: `cargo build -p sp42-app --target wasm32-unknown-unknown`
Expected: builds. (Requires the `wasm32-unknown-unknown` target — the repo pins it; if missing, `rustup target add wasm32-unknown-unknown`. Non-login shells may need `PATH="$HOME/.cargo/bin:$PATH"`.)

**Step 3: Commit (if any incidental fixups were needed)**

```bash
git add -A
git commit -m "chore(citation): Piece 1 acceptance — core tests, clippy, wasm build green" --allow-empty
```

---

## Phase 4 Done When

- `cargo test -p sp42-core` and `cargo clippy -p sp42-core --all-targets -- -D warnings` pass.
- `unusable_reason` round-trips and legacy findings (field absent) deserialize to `None`.
- The reason rides through `PageVerificationReport`; page `source_unavailable_unusable` stats unchanged (no new fields).
- Any existing human-facing render of the SU reason also shows `unusable_reason`; if none exists, the serialized finding field is the documented surface.
- `cargo build -p sp42-app --target wasm32-unknown-unknown` succeeds.

---

## Piece 1 Complete

With Phases 1–4 merged: #42's confabulated `partial` is fixed (paywall stubs short-circuit), PDFs and Google-Books shells classify deterministically with precise reasons, and every unusable finding carries a specific `unusable_reason` for the reviewer. Piece 2 (Wayback fallback #46, PDF→text #52, per-host adapters #53/arXiv) follows in a separate plan gated on **ADR-0012** (the fetch-edge policy, #34).
