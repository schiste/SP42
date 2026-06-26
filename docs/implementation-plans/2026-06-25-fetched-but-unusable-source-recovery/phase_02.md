# Piece 1 — Phase 2: PDF detector + special-case host-rule table

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** Deterministically classify two unusable shapes so they short-circuit before the model panel: PDF responses (by content-type or `%PDF-` magic) and host-specific viewer-shell sources (an extensible host-rule table seeded with Google Books).

**Architecture:** Add `PdfBody` and `ViewerShell` to `BodyUsabilityReason`. In `classify_source_usability` (added in Phase 1), run the new URL/content-type detectors *before* delegating to the text-shape detectors. The host-rule table is a `&[&str]` of host substrings so adding hosts is data, not code. No verdict-enum change; these reuse the Phase-1 short-circuit and reason plumbing.

**Tech Stack:** Rust, `url::Url` for host parsing, in-module `#[cfg(test)]` tests.

**Scope:** Phase 2 of 4 (Piece 1). Depends on Phase 1 (`classify_source_usability`, `FetchedSource.content_type`, `unusable_reason`).

**Codebase verified:** 2026-06-25.

---

## Task 1: Add `PdfBody` and `ViewerShell` reasons

**Files:**
- Modify: `crates/sp42-core/src/citation/body_classifier.rs` (enum ~30–50; `as_str` ~52–68)

**Step 1: Add the variants**

In `BodyUsabilityReason`, after `AmazonStub`, add:

```rust
    /// The response is a PDF (by content-type or `%PDF-` magic), not extractable
    /// HTML — a tool limitation, not a bad citation.
    PdfBody,
    /// A host-specific viewer/embed shell (e.g. a Google Books JavaScript reader)
    /// returned chrome instead of readable content.
    ViewerShell,
```

**Step 2: Add the `as_str` arms**

In `as_str`, add:

```rust
            BodyUsabilityReason::PdfBody => "pdf_body",
            BodyUsabilityReason::ViewerShell => "viewer_shell",
```

**Step 3: Verify it compiles**

Run: `cargo test -p sp42-core --no-run`
Expected: compiles (match arms exhaustive).

**Step 4: Commit**

```bash
git add crates/sp42-core/src/citation/body_classifier.rs
git commit -m "feat(citation): add PdfBody and ViewerShell usability reasons"
```

---

## Task 2: PDF detector

**Files:**
- Modify: `crates/sp42-core/src/citation/body_classifier.rs` (`classify_source_usability`)
- Test: same file, `#[cfg(test)]` module

**Step 1: Write the failing tests**

```rust
#[test]
fn pdf_by_content_type_is_flagged() {
    // Correctly-typed PDF: not HTML, so raw_html is None and the %PDF text is `text`.
    let r = classify_source_usability(
        "https://example.com/report",
        "application/pdf",
        None,
        Some("%PDF-1.7 ...binary..."),
    );
    assert!(!r.usable);
    assert_eq!(r.reason, BodyUsabilityReason::PdfBody);
}

#[test]
fn pdf_by_magic_when_mislabeled_html() {
    // Server lies with text/html, so fetch treats it as HTML: raw_html holds the
    // %PDF bytes and `text` is whatever html_to_text made of them. Magic must be
    // checked against the RAW body.
    let r = classify_source_usability(
        "https://example.com/x",
        "text/html",
        Some("%PDF-1.4\n%âãÏÓ\n1 0 obj"),
        Some("garbage extracted text"),
    );
    assert!(!r.usable);
    assert_eq!(r.reason, BodyUsabilityReason::PdfBody);
}

#[test]
fn non_pdf_html_is_not_pdf_flagged() {
    let prose = "The history of the bridge spans more than a century. ".repeat(10);
    let r = classify_source_usability("https://example.com/x", "text/html", Some(&prose), Some(&prose));
    assert!(r.usable);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p sp42-core pdf_by_`
Expected: FAIL — PDFs currently fall through to the text detectors (and a long PDF string may even pass as usable).

**Step 3: Implement the PDF detector**

In `classify_source_usability`, before delegating, add the PDF check. Replace the Phase-1 body with:

```rust
pub fn classify_source_usability(
    _source_url: &str, // unused until Task 3 (host-rule); `_`-prefixed to satisfy -D warnings
    content_type: &str,
    raw_html: Option<&str>,
    text: Option<&str>,
) -> BodyUsability {
    // 1. PDF: content-type or `%PDF-` magic. Check the RAW body (a PDF mislabeled
    //    text/html keeps its magic in raw_html, not the html_to_text output).
    let has_pdf_magic = |s: Option<&str>| s.is_some_and(|t| t.trim_start().starts_with("%PDF-"));
    let is_pdf = content_type.to_ascii_lowercase().contains("application/pdf")
        || has_pdf_magic(raw_html)
        || has_pdf_magic(text);
    if is_pdf {
        return unusable(BodyUsabilityReason::PdfBody);
    }

    // 2. Special-case hosts (host-rule table) — added in Task 3.

    // 3. Paywall / nav-chrome — added in Phase 3.

    // 4. Text-shape detectors.
    classify_body_usability(text)
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p sp42-core pdf_by_`
Expected: PASS.

**Step 5: Commit**

```bash
git add crates/sp42-core/src/citation/body_classifier.rs
git commit -m "feat(citation): detect PDF sources by content-type and %PDF magic"
```

---

## Task 3: Special-case host-rule table (seeded with Google Books)

**Files:**
- Modify: `crates/sp42-core/src/citation/body_classifier.rs` (`classify_source_usability` + a host-rule const)
- Test: same file, `#[cfg(test)]` module

**Step 1: Write the failing tests**

```rust
#[test]
fn google_books_host_is_viewer_shell() {
    for url in [
        "https://books.google.com/books?id=abc123",
        "https://books.google.es/books?id=xyz",
    ] {
        let r = classify_source_usability(url, "text/html", None, Some("irrelevant body text"));
        assert!(!r.usable, "{url}");
        assert_eq!(r.reason, BodyUsabilityReason::ViewerShell, "{url}");
    }
}

#[test]
fn non_special_host_is_not_viewer_shell() {
    let prose = "The history of the bridge spans more than a century. ".repeat(10);
    let r = classify_source_usability(
        "https://en.wikipedia.org/wiki/Bridge",
        "text/html",
        None,
        Some(&prose),
    );
    assert!(r.usable);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p sp42-core viewer_shell`
Expected: FAIL — host not yet recognized.

**Step 3: Implement the host-rule table**

Add a const near the other statics at the top of `body_classifier.rs`:

```rust
/// Hosts whose pages are viewer/embed shells under generic extraction (no readable
/// article body). Matched as a suffix on the URL host. Extension point: add hosts here.
const SPECIAL_CASE_HOSTS: &[&str] = &["books.google."];
```

Add a helper:

```rust
/// `true` if the URL's host matches a special-case viewer-shell host.
fn is_special_case_host(source_url: &str) -> bool {
    let Ok(parsed) = Url::parse(source_url) else {
        return false;
    };
    let Some(host) = parsed.host_str() else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    SPECIAL_CASE_HOSTS
        .iter()
        .any(|needle| host.contains(needle))
}
```

Ensure `use url::Url;` is present at the top of `body_classifier.rs` (add if absent).

Fill in the "Special-case hosts" slot in `classify_source_usability` (between the PDF check and the text delegation). This is the first use of the URL param, so **rename `_source_url` to `source_url`** in the signature now (it is no longer unused):

```rust
    // 2. Special-case hosts (viewer/embed shells).
    if is_special_case_host(source_url) {
        return unusable(BodyUsabilityReason::ViewerShell);
    }
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p sp42-core viewer_shell`
Expected: PASS.

**Step 5: Commit**

```bash
git add crates/sp42-core/src/citation/body_classifier.rs
git commit -m "feat(citation): special-case host-rule table for viewer shells (Google Books seed)"
```

---

## Task 4: End-to-end — PDF and viewer-shell skip the panel

**Files:**
- Test: `crates/sp42-core/src/citation/verify.rs` `#[cfg(test)]` module

**Step 1: Write the failing tests**

```rust
#[test]
fn pdf_source_is_unusable_with_no_model_call() {
    let fetch = StubHttpClient::new([Ok(HttpResponse {
        status: 200,
        headers: BTreeMap::from([("content-type".to_string(), "application/pdf".to_string())]),
        body: b"%PDF-1.7\n...binary report body...".to_vec(),
    })]);
    let model_client = StubModelClient::new([]); // empty → any model call errors
    let outcome = block_on(verify_citation_use_site(
        &fetch,
        &model_client,
        &FixedClock::new(1000),
        &[model()],
        &request("Claim from a PDF", "https://example.com/report.pdf"),
        None,
        3,
        VerifyOptions::default(),
    ))
    .expect("verifies");
    assert_eq!(outcome.finding.verdict, CitationVerdict::SourceUnavailable);
    assert_eq!(outcome.finding.unusable_reason, Some(BodyUsabilityReason::PdfBody));
    assert!(outcome.votes.is_empty());
}

#[test]
fn google_books_source_is_unusable_with_no_model_call() {
    let fetch = StubHttpClient::new([Ok(HttpResponse {
        status: 200,
        headers: BTreeMap::from([("content-type".to_string(), "text/html".to_string())]),
        body: long_html_with("Pernod Ricard"), // chrome with the entity name present
    })]);
    let model_client = StubModelClient::new([]);
    let outcome = block_on(verify_citation_use_site(
        &fetch,
        &model_client,
        &FixedClock::new(1000),
        &[model()],
        &request("Some claim about a book", "https://books.google.com/books?id=abc"),
        None,
        3,
        VerifyOptions::default(),
    ))
    .expect("verifies");
    assert_eq!(outcome.finding.verdict, CitationVerdict::SourceUnavailable);
    assert_eq!(outcome.finding.unusable_reason, Some(BodyUsabilityReason::ViewerShell));
    assert!(outcome.votes.is_empty());
}
```

**Step 2: Run tests to verify they fail / pass**

Run: `cargo test -p sp42-core source_is_unusable_with_no_model_call`
Expected: PASS (Phase-1 wiring + Phase-2 detectors already make this work). If either FAILs, the detector or wiring is wrong — fix before committing.

**Step 3: Run the full suite**

Run: `cargo test -p sp42-core`
Expected: all pass.

**Step 4: Commit**

```bash
git add crates/sp42-core/src/citation/verify.rs
git commit -m "test(citation): PDF and Google Books sources short-circuit before the panel"
```

---

## Phase 2 Done When

- `cargo test -p sp42-core` passes.
- PDF (by content-type and by `%PDF-` magic, incl. `text/html`-mislabeled) → `Unusable` / `PdfBody`, zero model calls.
- `books.google.*` (the seed host) → `Unusable` / `ViewerShell`, zero model calls; non-special hosts unaffected.
- Adding a host is a one-line edit to `SPECIAL_CASE_HOSTS`.
