//! The deterministic body-usability (GIGO) gate (ADR-0007 §4).
//!
//! Before any model sees a fetched source, this pure, I/O-free classifier inspects the
//! body. If it is structurally unusable — an anti-bot interstitial, a CSS/JSON-LD leak,
//! archive-chrome/redirect notices, an Amazon stub, or a body under a length floor —
//! verification short-circuits to `source_unavailable` **without a model call**, so a
//! scrape failure is never mis-attributed as a model accuracy error (ADR-0007 §4).
//!
//! Every detector is bounded (fixed-size head windows) and the engine is Rust's
//! linear-time `regex`, so the classifier is ReDoS-safe by construction. It is tuned to
//! favor false-negatives — let a borderline body through to the model — over discarding
//! real text. The model's own STEP 1 remains the backstop for *semantic* unusability a
//! regex cannot catch (ADR-0007 §1/§4).

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use url::Url;

/// Window (in characters) inspected by the JSON-LD / CSS / Wayback-chrome detectors.
const SIGNATURE_LEN: usize = 500;
/// Bodies shorter than this many characters (after trim) are unusable.
const SHORT_BODY_FLOOR: usize = 300;
/// At or above this length the Wayback-chrome detector stands down (a real article
/// is assumed to follow the banner).
const CHROME_LENGTH_CAP: usize = 600;
/// Window inspected by the anti-bot / Wayback-redirect detectors.
const CHALLENGE_WINDOW: usize = 1500;

/// Why a body was judged unusable (or `Ok` when it is usable).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BodyUsabilityReason {
    /// The body is usable.
    Ok,
    /// A JSON-LD / schema.org metadata blob leaked instead of article text.
    JsonLdLeak,
    /// A CSS stylesheet leaked instead of article text.
    CssLeak,
    /// An anti-bot / "are you human" challenge page.
    AntiBotChallenge,
    /// A Wayback "Got an HTTP NNN response at crawl time" redirect notice.
    WaybackRedirectNotice,
    /// Only Wayback toolbar/banner chrome, no article body.
    WaybackChrome,
    /// An Amazon "Conditions of Use … Privacy Notice … © …, Amazon.com" stub.
    AmazonStub,
    /// The body is shorter than the usable-length floor (also: a failed/empty fetch).
    ShortBody,
    /// The response is a PDF (by content-type or `%PDF-` magic), not extractable
    /// HTML — a tool limitation, not a bad citation.
    PdfBody,
    /// A host-specific viewer/embed shell (e.g. a Google Books JavaScript reader)
    /// returned chrome instead of readable content.
    ViewerShell,
    /// A paywall / registration-wall stub: navigation + a sign-in/subscribe
    /// prompt, but no readable article body. Fetched 2xx, so otherwise invisible.
    NavChromePaywall,
}

impl BodyUsabilityReason {
    /// The fixed `snake_case` token for this reason — usable as the `&'static str`
    /// reason on [`crate::errors::CitationVerificationError::SourceUnavailable`].
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            BodyUsabilityReason::Ok => "ok",
            BodyUsabilityReason::JsonLdLeak => "json_ld_leak",
            BodyUsabilityReason::CssLeak => "css_leak",
            BodyUsabilityReason::AntiBotChallenge => "anti_bot_challenge",
            BodyUsabilityReason::WaybackRedirectNotice => "wayback_redirect_notice",
            BodyUsabilityReason::WaybackChrome => "wayback_chrome",
            BodyUsabilityReason::AmazonStub => "amazon_stub",
            BodyUsabilityReason::ShortBody => "short_body",
            BodyUsabilityReason::PdfBody => "pdf_body",
            BodyUsabilityReason::ViewerShell => "viewer_shell",
            BodyUsabilityReason::NavChromePaywall => "nav_chrome_paywall",
        }
    }
}

/// The result of classifying a fetched body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BodyUsability {
    /// `true` iff the body is usable article content (`reason == Ok`).
    pub usable: bool,
    /// The (first-matching) reason; `Ok` iff `usable`.
    pub reason: BodyUsabilityReason,
}

static JSON_LD_PREFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*[{\[]").expect("valid regex"));
static JSON_LD_KEYWORD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""@(context|type|graph)"\s*:"#).expect("valid regex"));
static CSS_RULE_LIKE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[\s.#@\w-]+\{[^{}]{10,}").expect("valid regex"));
static ANTI_BOT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(Making sure you('|&#39;)re not a bot|Anubis uses a Proof-of-Work|Just a moment\.\.\.|Verifying you are human|Please enable JavaScript and cookies|Checking your browser before accessing)",
    )
    .expect("valid regex")
});
static WAYBACK_REDIRECT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"Got an HTTP \d{3} response at crawl time").expect("valid regex"));
static WAYBACK_BANNER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^The Wayback Machine - https?://").expect("valid regex"));
static WAYBACK_CAPTURES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\d{1,9} captures\s{1,5}\d{1,2} \w{1,30} \d{4}").expect("valid regex")
});
static WAYBACK_COLLECTED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bCOLLECTED BY\s+Collection:").expect("valid regex"));
static AMAZON_STUB: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)Conditions of Use(?: & Sale)?\s{0,20}Privacy Notice\s{0,20}©\s{0,20}\d{4}-\d{4},?\s{0,20}Amazon\.com",
    )
    .expect("valid regex")
});

/// Minimum count of sentence-like spans for a body to read as real article prose.
/// Starting threshold — tune against the fixture sample (see plan tuning stance).
/// Replaced by a `dom_smoothie` content-quality signal in a later (deferred) pass.
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

/// Paywall-platform (`PaaS`) hosts that appear in `<script>`/`<link>` srcs. Domains are
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

/// Hosts whose pages are viewer/embed shells under generic extraction (no readable
/// article body). Matched as a case-insensitive substring of the URL host. Extension point: add hosts here.
const SPECIAL_CASE_HOSTS: &[&str] = &["books.google."];

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

/// The first `n` characters of `text` (a char-boundary-safe prefix window).
fn head(text: &str, n: usize) -> String {
    text.chars().take(n).collect()
}

/// `true` if any JSON-LD block marks the content paywalled
/// (`isAccessibleForFree: false`), checked recursively (incl. nested `hasPart`).
fn json_ld_marks_paywalled(raw_html: &str) -> bool {
    LD_JSON_BLOCK.captures_iter(raw_html).any(|caps| {
        serde_json::from_str::<serde_json::Value>(caps[1].trim())
            .is_ok_and(|value| value_marks_paywalled(&value))
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

/// Classify whether a fetched body is usable article content (ADR-0007 §4).
///
/// `None` (a failed/empty fetch) classifies as [`BodyUsabilityReason::ShortBody`].
/// Detectors run in a fixed order; the first match wins. Never panics.
#[must_use]
pub fn classify_body_usability(text: Option<&str>) -> BodyUsability {
    let Some(text) = text else {
        return unusable(BodyUsabilityReason::ShortBody);
    };
    let trimmed = text.trim();
    let length = trimmed.chars().count();

    let signature = head(trimmed, SIGNATURE_LEN);

    // 1. JSON-LD leak: the head starts with `{`/`[` and carries a schema.org keyword.
    if JSON_LD_PREFIX.is_match(&signature) && JSON_LD_KEYWORD.is_match(&signature) {
        return unusable(BodyUsabilityReason::JsonLdLeak);
    }

    // 2. CSS leak: rule-like head plus high CSS-glyph density (> 5%).
    if CSS_RULE_LIKE.is_match(&signature) {
        let glyphs = signature
            .chars()
            .filter(|c| matches!(c, '{' | '}' | ';' | ':'))
            .count();
        let window = signature.chars().count();
        // glyphs / window > 0.05  ⟺  glyphs * 20 > window  (integer, no float).
        if glyphs * 20 > window {
            return unusable(BodyUsabilityReason::CssLeak);
        }
    }

    let challenge_window = head(trimmed, CHALLENGE_WINDOW);

    // 3. Anti-bot challenge.
    if ANTI_BOT.is_match(&challenge_window) {
        return unusable(BodyUsabilityReason::AntiBotChallenge);
    }

    // 4. Wayback redirect notice.
    if WAYBACK_REDIRECT.is_match(&challenge_window) {
        return unusable(BodyUsabilityReason::WaybackRedirectNotice);
    }

    // 5. Wayback chrome — only for short bodies (a long body has a real article after it).
    if length < CHROME_LENGTH_CAP
        && (WAYBACK_BANNER.is_match(&signature)
            || WAYBACK_CAPTURES.is_match(&signature)
            || WAYBACK_COLLECTED.is_match(&signature))
    {
        return unusable(BodyUsabilityReason::WaybackChrome);
    }

    // 6. Amazon stub (whole trimmed body).
    if AMAZON_STUB.is_match(trimmed) {
        return unusable(BodyUsabilityReason::AmazonStub);
    }

    // 7. Short body (catch-all).
    if length < SHORT_BODY_FLOOR {
        return unusable(BodyUsabilityReason::ShortBody);
    }

    BodyUsability {
        usable: true,
        reason: BodyUsabilityReason::Ok,
    }
}

const fn unusable(reason: BodyUsabilityReason) -> BodyUsability {
    BodyUsability {
        usable: false,
        reason,
    }
}

/// Usability gate with full context: the source URL, the response content-type,
/// the pre-extraction HTML (for structured paywall markers), and the extracted
/// text. This function runs the PDF detector and special-case-host detector ahead
/// of delegating to the text-shape detectors; only the paywall slot remains as a
/// Phase-3 placeholder.
#[must_use]
pub fn classify_source_usability(
    source_url: &str,
    content_type: &str,
    raw_html: Option<&str>,
    text: Option<&str>,
) -> BodyUsability {
    // 1. PDF: content-type or `%PDF-` magic. Check the RAW body (a PDF mislabeled
    //    text/html keeps its magic in raw_html, not the html_to_text output).
    let has_pdf_magic = |s: Option<&str>| s.is_some_and(|t| t.trim_start().starts_with("%PDF-"));
    let is_pdf = content_type
        .to_ascii_lowercase()
        .contains("application/pdf")
        || has_pdf_magic(raw_html)
        || has_pdf_magic(text);
    if is_pdf {
        return unusable(BodyUsabilityReason::PdfBody);
    }

    // 2. Special-case hosts (viewer/embed shells).
    if is_special_case_host(source_url) {
        return unusable(BodyUsabilityReason::ViewerShell);
    }

    // 3. Paywall / nav-chrome: a paywall marker AND no substantial readable prose.
    //    Markers are deterministic-first (raw-HTML structured signals), then a
    //    registration-phrase fallback guarded against consent banners. The prose
    //    check is load-bearing: a paywalled page that still ships the article text
    //    reads as high-prose and is NOT flagged.
    let trimmed = text.map_or("", str::trim);
    let raw = raw_html.unwrap_or("");
    let hard_marker = json_ld_marks_paywalled(raw)
        || CONTENT_TIER_GATED.is_match(raw)
        || PAYWALL_VENDOR_HOSTS.iter().any(|host| raw.contains(host));
    let soft_marker = PAYWALL_PHRASE.is_match(trimmed);
    let consent_wall = CONSENT_MARKER.is_match(raw) || CONSENT_MARKER.is_match(trimmed);
    // A hard marker fires through anything; the weak registration phrase is suppressed
    // when consent markers dominate and there is no hard signal (don't call a consent
    // UI a paywall). Single combined `if` — `collapsible_if` is denied under -D warnings.
    let marker = hard_marker || (soft_marker && !consent_wall);
    if marker && SENTENCE_END.find_iter(trimmed).count() < PROSE_SENTENCE_FLOOR {
        return unusable(BodyUsabilityReason::NavChromePaywall);
    }

    // 4. Text-shape detectors.
    classify_body_usability(text)
}

#[cfg(test)]
mod tests {
    use super::{BodyUsabilityReason, classify_body_usability, classify_source_usability};

    fn reason_of(text: &str) -> BodyUsabilityReason {
        classify_body_usability(Some(text)).reason
    }

    #[test]
    fn none_is_short_body() {
        let result = classify_body_usability(None);
        assert!(!result.usable);
        assert_eq!(result.reason, BodyUsabilityReason::ShortBody);
    }

    #[test]
    fn json_ld_blob_is_flagged() {
        assert_eq!(
            reason_of(r#"{"@context":"https://schema.org","@type":"Article","headline":"x"}"#),
            BodyUsabilityReason::JsonLdLeak
        );
    }

    #[test]
    fn json_ld_with_nested_object_before_context_is_flagged() {
        assert_eq!(
            reason_of(r#"{"foo":{"bar":1},"@context":"https://schema.org"}"#),
            BodyUsabilityReason::JsonLdLeak
        );
    }

    #[test]
    fn json_ld_array_form_is_flagged() {
        assert_eq!(
            reason_of(r#"[{"@type":"NewsArticle","headline":"y"}]"#),
            BodyUsabilityReason::JsonLdLeak
        );
    }

    #[test]
    fn css_stylesheet_is_flagged() {
        let css = ".header{color:red;font-size:12px;margin:0}.footer{display:none;padding:4px}";
        assert_eq!(reason_of(css), BodyUsabilityReason::CssLeak);
    }

    #[test]
    fn anti_bot_challenge_is_flagged() {
        assert_eq!(
            reason_of("Just a moment... Checking your browser before accessing."),
            BodyUsabilityReason::AntiBotChallenge
        );
    }

    #[test]
    fn wayback_redirect_notice_is_flagged() {
        assert_eq!(
            reason_of("Got an HTTP 302 response at crawl time (redirecting)"),
            BodyUsabilityReason::WaybackRedirectNotice
        );
    }

    #[test]
    fn wayback_banner_short_body_is_chrome() {
        assert_eq!(
            reason_of("The Wayback Machine - https://web.archive.org/web/2020/x"),
            BodyUsabilityReason::WaybackChrome
        );
    }

    #[test]
    fn wayback_captures_toolbar_is_chrome() {
        assert_eq!(
            reason_of("123 captures  7 January 2015 snapshot toolbar"),
            BodyUsabilityReason::WaybackChrome
        );
    }

    #[test]
    fn wayback_prefix_on_a_long_body_is_usable() {
        // A real article follows the banner: favor false-negatives (ADR-0007 §4).
        let mut body = String::from("The Wayback Machine - https://web.archive.org/web/2020/x\n");
        body.push_str(&"Real article prose follows here. ".repeat(40));
        let result = classify_body_usability(Some(&body));
        assert!(result.usable, "long body after banner should be usable");
    }

    #[test]
    fn amazon_stub_is_flagged() {
        assert_eq!(
            reason_of("Conditions of Use & Sale\nPrivacy Notice\n© 2010-2024, Amazon.com, Inc."),
            BodyUsabilityReason::AmazonStub
        );
    }

    #[test]
    fn short_snippet_is_short_body() {
        assert_eq!(
            reason_of("A one-line snippet."),
            BodyUsabilityReason::ShortBody
        );
    }

    #[test]
    fn long_real_prose_is_usable() {
        let prose = "The history of the bridge spans more than a century. ".repeat(10);
        let result = classify_body_usability(Some(&prose));
        assert!(result.usable);
        assert_eq!(result.reason, BodyUsabilityReason::Ok);
    }

    #[test]
    fn pathological_input_does_not_panic() {
        // ReDoS safety: bounded windows + linear-time regex engine.
        let mut input = "{".repeat(5000);
        input.push_str(&"\"@context".repeat(2000));
        let _ = classify_body_usability(Some(&input));
    }

    #[test]
    fn classify_source_delegates_to_text_detectors() {
        // No URL/content-type/raw-HTML signal → behaves exactly like classify_body_usability.
        let prose = "The history of the bridge spans more than a century. ".repeat(10);
        let usable =
            classify_source_usability("https://example.com/a", "text/html", None, Some(&prose));
        assert!(usable.usable);
        assert_eq!(usable.reason, BodyUsabilityReason::Ok);

        let short =
            classify_source_usability("https://example.com/a", "text/html", None, Some("tiny"));
        assert!(!short.usable);
        assert_eq!(short.reason, BodyUsabilityReason::ShortBody);
    }

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
        let r = classify_source_usability(
            "https://example.com/x",
            "text/html",
            Some(&prose),
            Some(&prose),
        );
        assert!(r.usable);
    }

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
            article_prose.repeat(8),
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
}
