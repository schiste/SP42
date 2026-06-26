# Fetched-but-Unusable Source — Piece 1 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** Plumb the "specific unusable reason" through the citation pipeline — add a unified usability entry point, carry content-type, and record the reason on the finding — without adding any new detectors yet (behavior-preserving).

**Architecture:** `sp42-core` citation module. Today `classify_body_usability(text)` is text-only and the short-circuit in `verify_citation_use_site` discards its `reason`. This phase adds `classify_source_usability(url, content_type, raw_html, text)` (delegating to the existing detectors for now), threads `content_type` and `raw_html` through `FetchedSource`, adds `CitationFinding.unusable_reason: Option<BodyUsabilityReason>`, and sets it at the short-circuit. No new detectors, no verdict-enum change.

**Tech Stack:** Rust, `serde`, in-module `#[cfg(test)]` tests, `StubHttpClient`/`StubModelClient` FIFO test doubles.

**Scope:** Phase 1 of 4 (Piece 1). Design: `docs/design-plans/2026-06-25-fetched-but-unusable-source-recovery.md`.

**Codebase verified:** 2026-06-25.

**Deviation from design:** The design folds "carry reason on finding" into its Phase 4. We add the `unusable_reason` field here (Phase 1) so later phases that populate it don't forward-reference a non-existent field. Report rendering remains in Phase 4.

---

## Task 1: Add `unusable_reason` field to `CitationFinding`

**Files:**
- Modify: `crates/sp42-core/src/citation/verify.rs` (struct `CitationFinding` ~lines 171–225; constructors `no_quote_finding` ~672–698 and `assemble_citation_finding`)

**Step 1: Add the field to the struct**

In `CitationFinding`, after `pub source_unavailable_reason: Option<SourceUnavailableReason>,` (line ~185), add:

```rust
    /// When the verdict is `SourceUnavailable` because the body was fetched but
    /// unusable, the specific classifier reason (PDF, viewer shell, paywall, …).
    /// `None` for usable sources and for unreachable (non-2xx) sources.
    #[serde(default)]
    pub unusable_reason: Option<BodyUsabilityReason>,
```

Ensure `BodyUsabilityReason` is in scope. At the top of `verify.rs`, extend the existing `use super::body_classifier::{…}` import so it reads:

```rust
use super::body_classifier::{classify_body_usability, BodyUsabilityReason};
```
Add **only** `BodyUsabilityReason` — do NOT add `BodyUsability` (it is never named in `verify.rs`; the workspace sets `warnings = "deny"` at root Cargo.toml, so an unused import is a hard build error, not a warning). Keep `classify_body_usability` — it is still called at the short-circuit (~825) until Task 4 swaps it.

**Step 2: Initialize the field in every `CitationFinding { … }` literal**

Add `unusable_reason: None,` to each struct literal. Known sites: `no_quote_finding` (~672–698) and `assemble_citation_finding`. Run the build (Step 3) — the compiler lists every literal missing the field; add `unusable_reason: None,` to each, including any in `#[cfg(test)]`.

**Step 3: Verify it compiles**

Run: `cargo test -p sp42-core --no-run`
Expected: compiles; if it fails, it names each `CitationFinding` literal still missing `unusable_reason` — add the field there and re-run until it builds.

**Step 4: Commit**

```bash
git add crates/sp42-core/src/citation/verify.rs
git commit -m "feat(citation): add CitationFinding.unusable_reason (plumbing, default None)"
```

---

## Task 2: Carry `content_type` and `raw_html` on `FetchedSource`

**Files:**
- Modify: `crates/sp42-core/src/citation/verify.rs` (`FetchedSource` ~700–705; `fetch_source` ~708–754)

The gate needs the response content-type (for PDF detection) and the pre-extraction HTML (for the deterministic paywall markers in Phase 3, whose JSON-LD/meta/script-src signals `html_to_text` strips). Both are already computed inside `fetch_source` and discarded; thread them out. `raw_html` is `Some` only for HTML responses.

**Step 1: Add the fields**

Change `FetchedSource` (~700–705) to:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedSource {
    pub text: String,
    pub status: u16,
    pub content_type: String,
    /// Pre-extraction HTML body, present only for HTML responses. Needed at the
    /// usability gate for structured paywall markers; consumed there and not
    /// retained downstream (grounding uses `text`).
    pub raw_html: Option<String>,
}
```

**Step 2: Populate them in `fetch_source`**

`fetch_source` currently does (~744–752):
```rust
    let body = String::from_utf8_lossy(&response.body).into_owned();
    let text = if looks_like_html(&content_type, &body) {
        html_to_text(&body)
    } else {
        body
    };
    Ok(FetchedSource {
        text: recover_wayback_body(&text),
        status: response.status,
    })
```
Replace that with (keeps the raw HTML for the HTML case without cloning — `html_to_text` borrows `body`, then `body` moves into `raw_html`):
```rust
    let body = String::from_utf8_lossy(&response.body).into_owned();
    let is_html = looks_like_html(&content_type, &body);
    let (extracted, raw_html) = if is_html {
        (html_to_text(&body), Some(body))
    } else {
        (body, None)
    };
    Ok(FetchedSource {
        text: recover_wayback_body(&extracted),
        status: response.status,
        content_type,
        raw_html,
    })
```
The non-2xx early return (~734) becomes:
```rust
        return Ok(FetchedSource {
            text: String::new(),
            status: response.status,
            content_type: String::new(),
            raw_html: None,
        });
```

**Step 3: Fix other `FetchedSource` literals**

Run: `cargo test -p sp42-core --no-run`
Expected: the compiler flags any other `FetchedSource { … }` literals (e.g. in tests) missing the new fields. Add `content_type: String::new(), raw_html: None` (or the appropriate values in tests) to each until it builds.

**Step 4: Commit**

```bash
git add crates/sp42-core/src/citation/verify.rs
git commit -m "feat(citation): thread content-type and raw HTML through FetchedSource"
```

---

## Task 3: Add the unified `classify_source_usability` entry point

**Files:**
- Modify: `crates/sp42-core/src/citation/body_classifier.rs` (after `classify_body_usability`, ~line 180)
- Test: same file, `#[cfg(test)]` module

**Step 1: Write the failing test**

The `#[cfg(test)]` module in `body_classifier.rs` imports its helpers via a `use super::{…}` line (~190) that currently lists `classify_body_usability` but not the new function. Add `classify_source_usability` to that `use super::{…}` list (`BodyUsabilityReason` is already imported there — the existing detector tests use it). Then add:

```rust
#[test]
fn classify_source_delegates_to_text_detectors() {
    // No URL/content-type/raw-HTML signal → behaves exactly like classify_body_usability.
    let prose = "The history of the bridge spans more than a century. ".repeat(10);
    let usable = classify_source_usability("https://example.com/a", "text/html", None, Some(&prose));
    assert!(usable.usable);
    assert_eq!(usable.reason, BodyUsabilityReason::Ok);

    let short = classify_source_usability("https://example.com/a", "text/html", None, Some("tiny"));
    assert!(!short.usable);
    assert_eq!(short.reason, BodyUsabilityReason::ShortBody);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p sp42-core classify_source_delegates_to_text_detectors`
Expected: FAIL — `classify_source_usability` not found.

**Step 3: Implement the entry point (delegating only — detectors arrive in Phase 2/3)**

Add after `classify_body_usability`:

```rust
/// Usability gate with full context: the source URL, the response content-type,
/// the pre-extraction HTML (for structured paywall markers), and the extracted
/// text. Phase 1 delegates to the text-only detectors; later phases add the
/// URL/content-type/raw-HTML detectors (PDF, special-case hosts, paywall) ahead
/// of this delegation. The leading params are unused for now.
#[must_use]
pub fn classify_source_usability(
    _source_url: &str,
    _content_type: &str,
    _raw_html: Option<&str>,
    text: Option<&str>,
) -> BodyUsability {
    classify_body_usability(text)
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p sp42-core classify_source_delegates_to_text_detectors`
Expected: PASS.

**Step 5: Commit**

```bash
git add crates/sp42-core/src/citation/body_classifier.rs
git commit -m "feat(citation): add classify_source_usability entry point (delegating)"
```

---

## Task 4: Wire the short-circuit to record the reason

**Files:**
- Modify: `crates/sp42-core/src/citation/verify.rs` (short-circuit in `verify_citation_use_site` ~820–836)
- Test: same file, `#[cfg(test)]` module

**Step 1: Write the failing test**

The `verify.rs` `#[cfg(test)]` module has a `use super::{…}` block (~921–926) that does not yet list `BodyUsabilityReason`. Add `BodyUsabilityReason` to it (this test and the Phase 2/3 tests reference it). Then add to the test module (mirror the existing end-to-end tests; an empty model queue asserts zero model calls):

```rust
#[test]
fn short_body_records_unusable_reason_and_skips_panel() {
    let fetch = StubHttpClient::new([Ok(HttpResponse {
        status: 200,
        headers: BTreeMap::from([("content-type".to_string(), "text/html".to_string())]),
        body: b"<html><body>tiny</body></html>".to_vec(),
    })]);
    let model_client = StubModelClient::new([]); // empty → any model call errors
    let outcome = block_on(verify_citation_use_site(
        &fetch,
        &model_client,
        &FixedClock::new(1000),
        &[model()],
        &request("Some claim", "https://example.com/tiny"),
        None,
        3,
        VerifyOptions::default(),
    ))
    .expect("verifies");
    assert_eq!(outcome.finding.verdict, CitationVerdict::SourceUnavailable);
    assert_eq!(
        outcome.finding.unusable_reason,
        Some(BodyUsabilityReason::ShortBody)
    );
    assert!(outcome.votes.is_empty());
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p sp42-core short_body_records_unusable_reason_and_skips_panel`
Expected: FAIL — `unusable_reason` is `None` (short-circuit doesn't set it yet).

**Step 3: Update the short-circuit**

Replace the short-circuit block (~820–836) so it calls the unified classifier and records the reason:

```rust
    let body = if fetched.text.is_empty() {
        None
    } else {
        Some(fetched.text.as_str())
    };
    let usability = classify_source_usability(
        request.source_url.as_str(),
        &fetched.content_type,
        fetched.raw_html.as_deref(),
        body,
    );
    if !usability.usable {
        let mut finding = no_quote_finding(
            CitationVerdict::SourceUnavailable,
            GroundingStatus::NotApplicable,
            PanelAgreement::new(0, 0),
            &provenance,
            use_site_ordinal,
        );
        finding.unusable_reason = Some(usability.reason);
        return Ok(VerificationOutcome {
            finding,
            votes: Vec::new(),
        });
    }
```

Update the module-level `body_classifier` import at the top of `verify.rs`: this swap removes the only remaining call to `classify_body_usability`, so it is now unused (a hard error under `warnings = "deny"`). The import must become exactly:

```rust
use super::body_classifier::{classify_source_usability, BodyUsabilityReason};
```
(Drop `classify_body_usability`; add `classify_source_usability`; keep `BodyUsabilityReason`.)

**Step 4: Run tests to verify they pass**

Run: `cargo test -p sp42-core short_body_records_unusable_reason_and_skips_panel`
Expected: PASS.

Run: `cargo test -p sp42-core`
Expected: the full suite passes (behavior unchanged for usable bodies; existing SU tests still pass — `source_unavailable_reason` derivation is untouched).

**Step 5: Commit**

```bash
git add crates/sp42-core/src/citation/verify.rs
git commit -m "feat(citation): record specific unusable_reason at the usability short-circuit"
```

---

## Phase 1 Done When

- `cargo test -p sp42-core` passes.
- A short/empty body yields `SourceUnavailable` with `unusable_reason == Some(ShortBody)` and zero model-panel calls.
- `FetchedSource` carries `content_type`; `classify_source_usability` exists and delegates.
- No behavior change for usable bodies; `source_unavailable_reason` (Unreachable/Unusable) derivation unchanged.
