//! Parse a model's free-text response into a graded [`Verdict`] plus an optional
//! supporting quote (ported from wikiharness `parsing.ts`).
//!
//! The parser recovers a verdict from a JSON object first (fenced block, then the
//! outermost brace span), falling back to a prose scan. It returns `None` when no
//! verdict can be recovered — it does **not** default to `NotSupported`. The
//! "no verbatim span ⇒ not-supported" rule lives in the prompt (model side) and the
//! grounding gate ([`crate::citation::verify`]); the caller decides what a `None` parse
//! means. All regexes are bounded (ReDoS-safe).

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use super::verdict::Verdict;

/// A parsed model verdict with its (optional, ungrounded) candidate quote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedVerdict {
    /// The recovered verdict.
    pub verdict: Verdict,
    /// The candidate supporting quote, if the response offered a non-empty one. This is
    /// **not yet grounded** — the gate re-locates it in the fetched bytes (ADR-0007 §5).
    pub quote: Option<String>,
}

static UNAVAILABLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\b(unavailable|inaccessible)\b|(could ?n.?t|cannot|can.?t|unable to|failed to|no) (access|retrieve|reach|load|fetch)",
    )
    .expect("valid regex")
});
static NOT_SUPPORTED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bunsupported\b|\bnot supported\b|\bcontradict|\brefut").expect("valid regex")
});
static PARTIAL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bpartial|\bpartly\b").expect("valid regex"));
static SUPPORTED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bsupported\b|\bsupports\b|\bconfirm").expect("valid regex"));
// Lazy fenced-block capture. Rust's `regex` is linear-time, so the wikiharness
// JS backtracking ReDoS-guard bound ({0,50000}) is unnecessary here and a bounded
// repetition that large only bloats the compiled program — use unbounded lazy.
static FENCE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)```(?:json)?\s*([\s\S]*?)```").expect("valid regex"));
// Straight or curly double quotes as literal characters (U+201C / U+201D).
static QUOTED_SPAN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"["“]([^"”]{1,2000})["”]"#).expect("valid regex"));

/// Normalize raw verdict text: lowercase, `_`→space, whitespace-run collapse, trim.
fn normalize(raw: &str) -> String {
    raw.to_lowercase()
        .replace('_', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Canonicalize free verdict text into a [`Verdict`], or `None` if unrecognized.
///
/// Branch order is load-bearing: `SourceUnavailable` and `NotSupported` are tested
/// before `Supported` so "not supported" / "could not access" never fall through.
#[must_use]
pub fn canonicalize_verdict(raw: &str) -> Option<Verdict> {
    let text = normalize(raw);
    if text.is_empty() {
        return None;
    }
    if UNAVAILABLE.is_match(&text) {
        return Some(Verdict::SourceUnavailable);
    }
    if NOT_SUPPORTED.is_match(&text) {
        return Some(Verdict::NotSupported);
    }
    if PARTIAL.is_match(&text) {
        return Some(Verdict::Partial);
    }
    if SUPPORTED.is_match(&text) {
        return Some(Verdict::Supported);
    }
    None
}

/// Parse a model response into a [`ParsedVerdict`], or `None` if no verdict is
/// recoverable. JSON candidates are tried first, then a prose fallback.
#[must_use]
pub fn parse_verdict_response(text: &str) -> Option<ParsedVerdict> {
    for candidate in json_candidates(text) {
        let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&candidate) else {
            continue;
        };
        let Some(verdict_value) = map.get("verdict") else {
            continue;
        };
        let Some(verdict) = canonicalize_verdict(verdict_value.as_str().unwrap_or_default()) else {
            // A present-but-uncanonicalizable verdict field: fall through to prose.
            continue;
        };
        let quote = ["quote", "supporting_quote", "evidence"]
            .iter()
            .find_map(|key| map.get(*key).and_then(Value::as_str))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        return Some(ParsedVerdict { verdict, quote });
    }

    let verdict = canonicalize_verdict(text)?;
    Some(ParsedVerdict {
        verdict,
        quote: first_quoted_span(text),
    })
}

/// Candidate JSON substrings, in priority order: a fenced ```` ``` ```` block, then the
/// outermost `{`…`}` brace span.
fn json_candidates(text: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    if let Some(group) = FENCE.captures(text).and_then(|caps| caps.get(1)) {
        candidates.push(group.as_str().to_string());
    }
    if let (Some(first), Some(last)) = (text.find('{'), text.rfind('}'))
        && last > first
    {
        candidates.push(text[first..=last].to_string());
    }
    candidates
}

/// The first double-quoted span (straight or curly), trimmed, if non-empty.
fn first_quoted_span(text: &str) -> Option<String> {
    QUOTED_SPAN
        .captures(text)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{canonicalize_verdict, parse_verdict_response};
    use crate::citation::verdict::Verdict;

    #[test]
    fn canonical_labels_pass_through() {
        assert_eq!(canonicalize_verdict("SUPPORTED"), Some(Verdict::Supported));
        assert_eq!(
            canonicalize_verdict("not_supported"),
            Some(Verdict::NotSupported)
        );
        assert_eq!(canonicalize_verdict("PARTIAL"), Some(Verdict::Partial));
        assert_eq!(
            canonicalize_verdict("source_unavailable"),
            Some(Verdict::SourceUnavailable)
        );
    }

    #[test]
    fn canonicalize_is_case_and_whitespace_insensitive_with_synonyms() {
        assert_eq!(
            canonicalize_verdict("fully supported"),
            Some(Verdict::Supported)
        );
        assert_eq!(
            canonicalize_verdict("partially supported"),
            Some(Verdict::Partial)
        );
        assert_eq!(
            canonicalize_verdict("partly supported"),
            Some(Verdict::Partial)
        );
        assert_eq!(
            canonicalize_verdict("unsupported"),
            Some(Verdict::NotSupported)
        );
        assert_eq!(
            canonicalize_verdict("contradicted"),
            Some(Verdict::NotSupported)
        );
        assert_eq!(
            canonicalize_verdict("could not access the source"),
            Some(Verdict::SourceUnavailable)
        );
    }

    #[test]
    fn canonicalize_rejects_garbage_and_empty() {
        assert_eq!(canonicalize_verdict("banana"), None);
        assert_eq!(canonicalize_verdict(""), None);
        assert_eq!(canonicalize_verdict("   "), None);
    }

    #[test]
    fn parses_plain_json() {
        let parsed = parse_verdict_response(r#"{"verdict": "SUPPORTED", "quote": "x"}"#)
            .expect("should parse");
        assert_eq!(parsed.verdict, Verdict::Supported);
        assert_eq!(parsed.quote.as_deref(), Some("x"));
    }

    #[test]
    fn parses_fenced_json_ignoring_surrounding_prose() {
        let text = "Here is my answer:\n```json\n{\"verdict\": \"NOT_SUPPORTED\", \"quote\": \"opened in 2002\"}\n```\nDone.";
        let parsed = parse_verdict_response(text).expect("should parse");
        assert_eq!(parsed.verdict, Verdict::NotSupported);
        assert_eq!(parsed.quote.as_deref(), Some("opened in 2002"));
    }

    #[test]
    fn parses_supporting_quote_field_name() {
        let parsed = parse_verdict_response(
            r#"{"verdict": "PARTIAL", "supporting_quote": "it is believed"}"#,
        )
        .expect("should parse");
        assert_eq!(parsed.verdict, Verdict::Partial);
        assert_eq!(parsed.quote.as_deref(), Some("it is believed"));
    }

    #[test]
    fn recovers_markdown_emphasis_and_quoted_span() {
        let parsed = parse_verdict_response(
            "Verdict: **NOT_SUPPORTED**. The source says \"opened in 2002\".",
        )
        .expect("should parse");
        assert_eq!(parsed.verdict, Verdict::NotSupported);
        assert_eq!(parsed.quote.as_deref(), Some("opened in 2002"));
    }

    #[test]
    fn loose_prose_verdict_has_no_quote() {
        let parsed = parse_verdict_response("This claim is partially supported by the text.")
            .expect("should parse");
        assert_eq!(parsed.verdict, Verdict::Partial);
        assert_eq!(parsed.quote, None);
    }

    #[test]
    fn pure_garbage_is_none() {
        assert!(parse_verdict_response("the quick brown fox").is_none());
    }

    #[test]
    fn uncanonicalizable_json_verdict_falls_back_to_prose() {
        // The JSON verdict value is junk, but the surrounding prose is decisive.
        let parsed =
            parse_verdict_response("{\"verdict\": \"banana\"} — overall the claim is supported")
                .expect("should parse");
        assert_eq!(parsed.verdict, Verdict::Supported);
    }
}
