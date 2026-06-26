# Piece 1 — Phase 3: Nav-chrome / paywall detector (#42)

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** Classify paywall / registration-wall stubs as `Unusable` so the #42 confabulated `partial` can no longer happen — using a conservative two-signal text heuristic tuned for *net value*, not zero false positives.

**Architecture:** Add `NavChromePaywall` to `BodyUsabilityReason`. Add a text-shape detector inside `classify_body_usability` (it must run on long bodies, since paywall stubs are large — placed after the Amazon stub, before the short-body floor). It fires only when **both** a registration/paywall marker is present **and** the body shows little real article prose. Reuses the Phase-1 short-circuit + reason plumbing.

**Tech Stack:** Rust, `regex` with `LazyLock` statics (matching the existing `ANTI_BOT` / `JSON_LD_*` pattern in this file), in-module tests.

**Scope:** Phase 3 of 4 (Piece 1). Depends on Phases 1–2.

**Codebase verified:** 2026-06-25.

**Tuning stance (from design):** Judged on measured net value — net reduction in confabulations without material coverage loss — not on a zero-false-positive corner. The thresholds below are a starting heuristic; if fixture tuning can't hit a good balance, the fallback (design Additional Considerations) is to ship behind a flag and tune against real review traffic. False positives are acceptable: a wrongly-flagged readable source costs one abstention, and grounding is the backstop for misses.

---

## Task 1: Add the `NavChromePaywall` reason

**Files:**
- Modify: `crates/sp42-core/src/citation/body_classifier.rs` (enum ~30–50; `as_str`)

**Step 1: Add the variant**

After `ViewerShell` (added in Phase 2), add:

```rust
    /// A paywall / registration-wall stub: navigation + a sign-in/subscribe
    /// prompt, but no readable article body. Fetched 2xx, so otherwise invisible.
    NavChromePaywall,
```

**Step 2: Add the `as_str` arm**

```rust
            BodyUsabilityReason::NavChromePaywall => "nav_chrome_paywall",
```

**Step 3: Verify it compiles**

Run: `cargo test -p sp42-core --no-run`
Expected: compiles.

**Step 4: Commit**

```bash
git add crates/sp42-core/src/citation/body_classifier.rs
git commit -m "feat(citation): add NavChromePaywall usability reason"
```

---

## Task 2: The two-signal paywall detector

**Files:**
- Modify: `crates/sp42-core/src/citation/body_classifier.rs` (statics near the top; detector in `classify_body_usability` after the Amazon stub `#6`, before short-body `#7`)
- Test: same file, `#[cfg(test)]` module

**Step 1: Write the failing tests**

```rust
#[test]
fn paywall_stub_with_marker_and_no_prose_is_flagged() {
    // Nav chrome + a registration wall, almost no real sentences (Law360-shaped).
    let body = format!(
        "Home News Topics Sections Account {} Subscribe to read the full article. \
         Sign in to continue. Create a free account.",
        "Companies Pernod Ricard SA Brown-Forman ".repeat(20)
    );
    let r = classify_source_usability("https://www.law360.com/articles/735000/x", "text/html", Some(&body));
    assert!(!r.usable);
    assert_eq!(r.reason, BodyUsabilityReason::NavChromePaywall);
}

#[test]
fn real_article_with_a_subscribe_link_is_not_flagged() {
    // A genuine article that happens to contain a "subscribe" prompt must NOT fire.
    let article = "The bridge opened in 1998 after a decade of construction. \
        Engineers had debated the design for years. Local officials praised the result. \
        The span quickly became a regional landmark. Traffic doubled within five years. \
        Maintenance crews inspect the cables annually. "
        .repeat(4);
    let body = format!("{article} Subscribe to read more of our coverage.");
    let r = classify_source_usability("https://news.example.com/bridge", "text/html", Some(&body));
    assert!(r.usable, "real article must not be flagged as paywall");
}

#[test]
fn nav_chrome_without_marker_is_not_paywall() {
    // Little prose but no registration marker → not a paywall (may fall to short/usable).
    let body = "Home News Topics Sections Account Menu Search ".repeat(10);
    let r = classify_source_usability("https://example.com/x", "text/html", Some(&body));
    assert_ne!(r.reason, BodyUsabilityReason::NavChromePaywall);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p sp42-core paywall`  and  `cargo test -p sp42-core real_article_with_a_subscribe`
Expected: the paywall test FAILs (not yet detected); the real-article test may already pass (nothing flags it yet) — it is the regression guard for Step 3.

**Step 3: Add the statics and the detector**

Add near the other `LazyLock<Regex>` statics at the top of `body_classifier.rs`:

```rust
/// Minimum count of sentence-like spans for a body to read as real article prose.
/// Starting threshold — tune against the fixture sample (see plan tuning stance).
const PROSE_SENTENCE_FLOOR: usize = 5;

/// A registration / paywall prompt. Signal (a) of the two-signal detector.
static PAYWALL_MARKER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(subscribe to (read|continue)|sign in to continue|create a free account|register to (read|continue)|to continue reading|subscribers? only|this (article|content) is for subscribers)",
    )
    .expect("valid paywall marker regex")
});

/// A sentence-like span: a word ending in terminal punctuation. Signal (b) proxy —
/// chrome/nav has few; real articles have many.
static SENTENCE_END: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z]{3,}[.!?](\s|$)").expect("valid sentence-end regex"));
```

In `classify_body_usability`, between detector `#6` (Amazon stub) and `#7` (short body), add:

```rust
    // 6b. Nav-chrome / paywall stub: a registration marker AND little real prose.
    //     Two-signal, conservative, tuned for balance (not zero false positives).
    if PAYWALL_MARKER.is_match(trimmed) {
        let prose_sentences = SENTENCE_END.find_iter(trimmed).count();
        if prose_sentences < PROSE_SENTENCE_FLOOR {
            return unusable(BodyUsabilityReason::NavChromePaywall);
        }
    }
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p sp42-core paywall`  and  `cargo test -p sp42-core real_article_with_a_subscribe`  and  `cargo test -p sp42-core nav_chrome_without_marker`
Expected: all PASS. If the real-article test fails, the heuristic is over-firing — raise `PROSE_SENTENCE_FLOOR` or tighten `PAYWALL_MARKER` until both the paywall and the real-article cases are correct (balance, per the tuning stance).

**Step 5: Commit**

```bash
git add crates/sp42-core/src/citation/body_classifier.rs
git commit -m "feat(citation): two-signal nav-chrome/paywall detector (#42)"
```

---

## Task 3: Representative fixture sample (measured net value)

**Files:**
- Test: `crates/sp42-core/src/citation/body_classifier.rs` `#[cfg(test)]` module

**Step 1: Add a small representative sample and assert the balance**

Add inline fixtures — a few paywall-shaped bodies and a few real-article-shaped bodies — and assert the detector fires on the former and not the latter, reporting the split. (This is the unit-scale proxy for the design's "both rates measured" criterion; broader tuning against live traffic is the flag-and-tune fallback.)

```rust
#[test]
fn paywall_detector_fixture_sample_balance() {
    // (label, body, expect_paywall)
    let nav = "Home News Topics Sections Account Menu Search Subscribe Login ";
    let walls = [
        format!("{}{}Subscribe to read the full article.", nav, "Related coverage ".repeat(30)),
        format!("{}This content is for subscribers. Sign in to continue.", nav.repeat(8)),
        format!("{}Create a free account to continue reading.", "Topic Tag ".repeat(40)),
    ];
    let article_prose = "The treaty was signed in 1815 after long negotiation. \
        Delegates from five nations attended. The terms reshaped the region for a generation. \
        Historians still debate its consequences. Several clauses were never enforced. ";
    let articles = [
        format!("{}{}", article_prose.repeat(5), "Subscribe to our newsletter."),
        format!("{}", article_prose.repeat(8)),
        format!("News flash. {}", article_prose.repeat(6)),
    ];

    let mut false_neg = 0;
    for w in &walls {
        if classify_source_usability("https://x/", "text/html", Some(w)).reason
            != BodyUsabilityReason::NavChromePaywall
        {
            false_neg += 1;
        }
    }
    let mut false_pos = 0;
    for a in &articles {
        if classify_source_usability("https://x/", "text/html", Some(a)).reason
            == BodyUsabilityReason::NavChromePaywall
        {
            false_pos += 1;
        }
    }
    // Hard requirement: no real article flagged. Catches the bulk of walls.
    assert_eq!(false_pos, 0, "real articles must not be flagged ({false_pos} were)");
    assert!(false_neg <= 1, "should catch most paywall stubs ({false_neg}/3 missed)");
}
```

**Step 2: Run it**

Run: `cargo test -p sp42-core paywall_detector_fixture_sample_balance`
Expected: PASS. If `false_pos > 0`, loosen the trigger (this is the worse error); if `false_neg` is high, the markers/floor are too strict — adjust toward balance.

**Step 3: Commit**

```bash
git add crates/sp42-core/src/citation/body_classifier.rs
git commit -m "test(citation): paywall detector fixture-sample balance (net-value proxy)"
```

---

## Task 4: #42 end-to-end regression (zero model calls)

**Files:**
- Test: `crates/sp42-core/src/citation/verify.rs` `#[cfg(test)]` module

**Step 1: Write the regression test**

```rust
#[test]
fn law360_paywall_stub_short_circuits_no_partial() {
    // The #42 case: a large nav-chrome/paywall body with the claim's entity in a
    // sidebar. Must classify Unusable and never reach the panel (no confabulated partial).
    let body = format!(
        "Home News Sections Account {} Subscribe to read the full article. Sign in to continue.",
        "Companies Pernod Ricard SA Gosling's Brown-Forman ".repeat(25)
    )
    .into_bytes();
    let fetch = StubHttpClient::new([Ok(HttpResponse {
        status: 200,
        headers: BTreeMap::from([("content-type".to_string(), "text/html".to_string())]),
        body,
    })]);
    let model_client = StubModelClient::new([]); // empty → any model call errors the test
    let outcome = block_on(verify_citation_use_site(
        &fetch,
        &model_client,
        &FixedClock::new(1000),
        &[model()],
        &request(
            "Gosling's has litigated over the mark against Pernod Ricard",
            "https://www.law360.com/articles/735000/x",
        ),
        None,
        3,
        VerifyOptions::default(),
    ))
    .expect("verifies");
    assert_eq!(outcome.finding.verdict, CitationVerdict::SourceUnavailable);
    assert_eq!(outcome.finding.unusable_reason, Some(BodyUsabilityReason::NavChromePaywall));
    assert!(outcome.votes.is_empty(), "paywall stub must not reach the panel");
}
```

**Step 2: Run it**

Run: `cargo test -p sp42-core law360_paywall_stub`
Expected: PASS.

**Step 3: Run the full suite**

Run: `cargo test -p sp42-core`
Expected: all pass.

**Step 4: Commit**

```bash
git add crates/sp42-core/src/citation/verify.rs
git commit -m "test(citation): #42 Law360 paywall stub short-circuits before the panel"
```

---

## Phase 3 Done When

- `cargo test -p sp42-core` passes.
- The #42 Law360-shaped fixture → `Unusable` / `NavChromePaywall`, zero model calls.
- Fixture-sample balance holds: zero false positives on real-article controls; catches most paywall stubs (both rates asserted/reported).
- `PROSE_SENTENCE_FLOOR` / `PAYWALL_MARKER` are the documented tuning knobs; flag-and-tune-on-traffic is the fallback if fixtures prove insufficient.
