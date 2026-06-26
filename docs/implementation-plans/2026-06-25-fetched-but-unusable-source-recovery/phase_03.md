# Piece 1 — Phase 3: Nav-chrome / paywall detector (#42)

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** Classify paywall / registration-wall stubs as `Unusable` so the #42 confabulated `partial` can no longer happen — using a layered, deterministic-first detector tuned for *net value*, not zero false positives.

**Architecture:** Add `NavChromePaywall` to `BodyUsabilityReason`. Add the detector to `classify_source_usability` (the paywall slot from Phase 2, between host-rule and the text-shape delegation) — it lives here, not in `classify_body_usability`, because the deterministic markers need the `raw_html` threaded in Phase 1. It fires only when **(a) a paywall marker** (deterministic-first: schema.org `isAccessibleForFree`, `article:content_tier` meta, vendor `<script>` fingerprints — scanned in `raw_html` — then a registration-phrase fallback on the extracted text) **and (b) no substantial readable prose**. A consent-wall guard suppresses cookie/GDPR banners. The prose check (b) is load-bearing: a paywalled page that still ships the text stays verifiable. Reuses the Phase-1 short-circuit + reason plumbing.

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

## Task 2: The layered paywall detector

**Files:**
- Modify: `crates/sp42-core/src/citation/body_classifier.rs` (statics near the top; the paywall slot in `classify_source_usability`, between the host-rule check and the text-shape delegation — comment `// 3. Paywall / nav-chrome` from Phase 2)
- Test: same file, `#[cfg(test)]` module

**Design:** Fire `NavChromePaywall` only when **(a) a paywall marker** and **(b) no substantial readable prose**. The prose check (b) is load-bearing — a paywalled page that still ships the article text reads as high-prose and is **not** flagged (we verify it; coverage win). Marker (a) is deterministic-first: schema.org `isAccessibleForFree`, `article:content_tier` meta, and paywall-vendor `<script>` srcs (all scanned in `raw_html`), then a registration-phrase regex on the extracted text as the weak fallback. A **consent-wall guard** stops cookie/GDPR banners (the top false-positive source) from tripping the weak fallback. The detector lives in `classify_source_usability` because the markers need `raw_html`.

**Step 1: Write the failing tests**

```rust
#[test]
fn schema_org_marker_with_no_prose_is_flagged() {
    let raw = r#"<html><head><script type="application/ld+json">{"@type":"NewsArticle","isAccessibleForFree":false}</script></head><body>Home News Sections Account</body></html>"#;
    let r = classify_source_usability("https://news.example.com/x", "text/html", Some(raw), Some("Home News Sections Account"));
    assert!(!r.usable);
    assert_eq!(r.reason, BodyUsabilityReason::NavChromePaywall);
}

#[test]
fn schema_org_nested_haspart_is_flagged() {
    // isAccessibleForFree:true at top, but :false in a nested hasPart — the recursive
    // serde_json walk must find it (a case a flat regex would get wrong).
    let raw = r#"<html><head><script type="application/ld+json">
        {"@type":"NewsArticle","isAccessibleForFree":true,
         "hasPart":{"@type":"WebPageElement","isAccessibleForFree":false,"cssSelector":".paywall"}}
        </script></head><body>Home News Sections Account</body></html>"#;
    let r = classify_source_usability("https://news.example.com/x", "text/html", Some(raw), Some("Home News Sections Account"));
    assert!(!r.usable);
    assert_eq!(r.reason, BodyUsabilityReason::NavChromePaywall);
}

#[test]
fn vendor_fingerprint_with_no_prose_is_flagged() {
    let raw = r#"<html><head><script src="https://cdn.tinypass.com/api/tinypass.min.js"></script></head><body>Subscribe Menu Account</body></html>"#;
    let r = classify_source_usability("https://news.example.com/x", "text/html", Some(raw), Some("Subscribe Menu Account"));
    assert!(!r.usable);
    assert_eq!(r.reason, BodyUsabilityReason::NavChromePaywall);
}

#[test]
fn registration_phrase_with_no_prose_is_flagged() {
    // Law360-shaped: nav chrome + registration wall, almost no real sentences.
    let body = format!(
        "Home News Sections Account {} Subscribe to read the full article. Sign in to continue.",
        "Companies Pernod Ricard SA Brown-Forman ".repeat(20)
    );
    let r = classify_source_usability("https://www.law360.com/articles/735000/x", "text/html", Some(&body), Some(&body));
    assert!(!r.usable);
    assert_eq!(r.reason, BodyUsabilityReason::NavChromePaywall);
}

#[test]
fn soft_paywall_with_full_text_is_not_flagged() {
    // isAccessibleForFree:false BUT the article text is present → high prose → verify it.
    let article = "The bridge opened in 1998 after a decade of work. Engineers debated the design. \
        Officials praised the result. Traffic doubled within five years. Crews inspect it yearly. "
        .repeat(4);
    let raw = format!(
        r#"<html><head><script type="application/ld+json">{{"isAccessibleForFree":false}}</script></head><body>{article}</body></html>"#
    );
    let r = classify_source_usability("https://news.example.com/x", "text/html", Some(&raw), Some(&article));
    assert!(r.usable, "paywalled page that still ships the text must stay verifiable");
}

#[test]
fn real_article_with_subscribe_link_is_not_flagged() {
    let article = "The treaty was signed in 1815 after long talks. Five nations attended. \
        The terms reshaped the region. Historians debate it still. Some clauses were never enforced. "
        .repeat(4);
    let body = format!("{article} Subscribe to read more of our coverage.");
    let r = classify_source_usability("https://news.example.com/x", "text/html", Some(&body), Some(&body));
    assert!(r.usable, "real article must not be flagged as paywall");
}

#[test]
fn consent_wall_with_registration_phrase_is_suppressed() {
    // A cookie-consent interstitial with a sign-in prompt but NO hard paywall marker.
    // The guard prevents calling a consent UI a paywall.
    let body = "We value your privacy. Accept all cookies. Manage cookies. Sign in to continue. Home Menu Account";
    let r = classify_source_usability("https://news.example.com/x", "text/html", Some(body), Some(body));
    assert_ne!(r.reason, BodyUsabilityReason::NavChromePaywall);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p sp42-core _is_flagged`  (the three `*_is_flagged` tests)
Expected: FAIL — no paywall detector yet. The `*_not_flagged` / `_suppressed` tests may already pass (nothing fires yet) and act as regression guards.

**Step 3: Add the statics**

Add near the other `LazyLock<Regex>` statics at the top of `body_classifier.rs`:

```rust
/// Minimum count of sentence-like spans for a body to read as real article prose.
/// Starting threshold — tune against the fixture sample (see plan tuning stance).
/// Replaced by a dom_smoothie content-quality signal in a later (deferred) pass.
const PROSE_SENTENCE_FLOOR: usize = 5;

/// Locates JSON-LD blocks. Their *contents* are parsed with `serde_json`
/// (see `json_ld_marks_paywalled`), never regex-matched — this only finds the block.
static LD_JSON_BLOCK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<script[^>]*type=["']application/ld\+json["'][^>]*>(.*?)</script>"#)
        .expect("valid regex")
});

/// Open Graph content tier marking gated content.
static CONTENT_TIER_GATED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)property=["']article:content_tier["']\s+content=["'](locked|metered)["']"#)
        .expect("valid regex")
});

/// Registration / paywall prompt in the extracted text — the weakest marker.
static PAYWALL_PHRASE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(subscribe to (read|continue)|sign in to continue|create a free account|register to (read|continue)|to continue reading|subscribers? only|this (article|content) is for subscribers)",
    )
    .expect("valid regex")
});

/// Cookie / GDPR consent-banner markers — the top paywall false-positive source.
static CONSENT_MARKER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(accept all cookies|manage cookies|cookie (policy|consent|preferences|settings)|we value your privacy)",
    )
    .expect("valid regex")
});

/// A sentence-like span (proxy for real article prose); chrome has few, articles many.
static SENTENCE_END: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z]{3,}[.!?](\s|$)").expect("valid regex"));

/// Paywall-platform (PaaS) hosts that appear in `<script>`/`<link>` srcs. Domains are
/// facts (no license entanglement); cross-referenced against miscfilters/antipaywall.txt
/// (GPL-3.0, compatible) for maintenance.
const PAYWALL_VENDOR_HOSTS: &[&str] = &[
    "piano.io",
    "tinypass.com",
    "npttech.com",
    "poool.fr",
    "poool.tech",
    "zephr.com",
    "arcpublishing.com",
    "pelcro.com",
    "evolok.com",
    "wallkit.net",
];
```

Then add these module-level helpers (near `classify_source_usability`, outside `#[cfg(test)]`). They parse JSON-LD with `serde_json` rather than regex-matching inside it, so nested `hasPart` / multiple blocks / whitespace are handled correctly (`serde_json` is already a dependency; reference it fully-qualified — no `use` needed):

```rust
/// `true` if any JSON-LD block marks the content paywalled
/// (`isAccessibleForFree: false`), checked recursively (incl. nested `hasPart`).
fn json_ld_marks_paywalled(raw_html: &str) -> bool {
    LD_JSON_BLOCK.captures_iter(raw_html).any(|caps| {
        serde_json::from_str::<serde_json::Value>(caps[1].trim())
            .map(|value| value_marks_paywalled(&value))
            .unwrap_or(false)
    })
}

fn value_marks_paywalled(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(map) => {
            let flagged = map.get("isAccessibleForFree").is_some_and(|v| {
                matches!(v, serde_json::Value::Bool(false))
                    || matches!(v.as_str(), Some(s) if s.eq_ignore_ascii_case("false"))
            });
            flagged || map.values().any(value_marks_paywalled)
        }
        serde_json::Value::Array(items) => items.iter().any(value_marks_paywalled),
        _ => false,
    }
}
```

**Step 4: Add the detector to `classify_source_usability`**

Replace the `// 3. Paywall / nav-chrome — added in Phase 3.` placeholder (from Phase 2) with:

```rust
    // 3. Paywall / nav-chrome: a paywall marker AND no substantial readable prose.
    //    Markers are deterministic-first (raw-HTML structured signals), then a
    //    registration-phrase fallback guarded against consent banners. The prose
    //    check is load-bearing: a paywalled page that still ships the article text
    //    reads as high-prose and is NOT flagged.
    let trimmed = text.map(str::trim).unwrap_or("");
    let raw = raw_html.unwrap_or("");
    let hard_marker = json_ld_marks_paywalled(raw)
        || CONTENT_TIER_GATED.is_match(raw)
        || PAYWALL_VENDOR_HOSTS.iter().any(|host| raw.contains(host));
    let soft_marker = PAYWALL_PHRASE.is_match(trimmed);
    let consent_wall = CONSENT_MARKER.is_match(raw) || CONSENT_MARKER.is_match(trimmed);
    // A hard marker fires through anything; the weak registration phrase is suppressed
    // when consent markers dominate and there is no hard signal (don't call a consent
    // UI a paywall).
    if hard_marker || (soft_marker && !consent_wall) {
        if SENTENCE_END.find_iter(trimmed).count() < PROSE_SENTENCE_FLOOR {
            return unusable(BodyUsabilityReason::NavChromePaywall);
        }
    }
```

**Step 5: Run tests to verify they pass**

Run: `cargo test -p sp42-core _is_flagged`  and  `cargo test -p sp42-core _not_flagged`  and  `cargo test -p sp42-core _suppressed`
Expected: all PASS. If a `*_not_flagged` test fails, the heuristic is over-firing (the worse error) — raise `PROSE_SENTENCE_FLOOR` or tighten markers toward balance per the tuning stance.

**Step 6: Commit**

```bash
git add crates/sp42-core/src/citation/body_classifier.rs
git commit -m "feat(citation): layered paywall detector — deterministic markers + prose + consent guard (#42)"
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
        if classify_source_usability("https://x/", "text/html", Some(w), Some(w)).reason
            != BodyUsabilityReason::NavChromePaywall
        {
            false_neg += 1;
        }
    }
    let mut false_pos = 0;
    for a in &articles {
        if classify_source_usability("https://x/", "text/html", Some(a), Some(a)).reason
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
- Deterministic markers fire: schema.org `isAccessibleForFree:false` (parsed with `serde_json`, incl. nested `hasPart`) and a vendor `<script>` fingerprint each classify `NavChromePaywall` (with low prose); a soft paywall that still ships the full text stays usable; a consent wall with a sign-in phrase is suppressed.
- Fixture-sample balance holds: zero false positives on real-article controls; catches most paywall stubs (both rates asserted/reported).
- `PROSE_SENTENCE_FLOOR`, `json_ld_marks_paywalled`, and the marker statics (`CONTENT_TIER_GATED`, `PAYWALL_VENDOR_HOSTS`, `PAYWALL_PHRASE`, `CONSENT_MARKER`) are the documented tuning knobs; the `dom_smoothie` content-quality swap + a real HTML/DOM parser, and flag-and-tune-on-traffic, are the deferred escalations.
