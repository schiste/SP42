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

/// The first `n` characters of `text` (a char-boundary-safe prefix window).
fn head(text: &str, n: usize) -> String {
    text.chars().take(n).collect()
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

#[cfg(test)]
mod tests {
    use super::{BodyUsabilityReason, classify_body_usability};

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
}
