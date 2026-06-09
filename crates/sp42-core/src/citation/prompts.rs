//! The gold two-step verification prompt (ADR-0007; ported verbatim from wikiharness).
//!
//! STEP 1 is a source-usability check that short-circuits to `SOURCE_UNAVAILABLE`;
//! STEP 2 verifies the claim against the source body only, requiring a verbatim quote
//! for any `SUPPORTED`/`PARTIAL` and forbidding any numeric confidence. Bibliographic
//! metadata, when present, is rendered as a clearly-labeled **context-only** block whose
//! contents can never pass the grounding gate (the gate hashes/locates only the source
//! body, never this section — ADR-0007 Alt (e)).

use sp42_types::ChatMessage;

use super::citoid::CitoidMetadata;

/// The verbatim two-step verifier system instruction.
pub const SYSTEM: &str = r#"You verify whether a cited SOURCE supports a CLAIM from a Wikipedia article.

Judge using ONLY the text of the provided source. Do NOT use outside knowledge, and do NOT assume facts that are not present in the source.

Use this two-step process for every claim.

STEP 1 — Source check:
Determine whether the source text contains usable article body content: real paragraphs, quotes, narrative passages, or factual statements. This holds true even when that content is surrounded by navigation, headers, footers, web.archive.org captures, or other page chrome.

The source is NOT usable if it contains only: a library/database catalog page (Google Books, WorldCat, a JSTOR preview), a paywall, a login wall, a 404, a cookie/consent notice, an anti-bot challenge, or bibliographic metadata with no article body.

Long sources may arrive as an excerpt — gaps between paragraphs, blank lines, text ending mid-sentence, or passages separated by "..." are NORMAL and mean "not shown here", not "failed to load". Brevity alone is not a SOURCE_UNAVAILABLE signal: if any article prose is present, evaluate it. If STEP 1 fails, return SOURCE_UNAVAILABLE and do NOT attempt STEP 2.

STEP 2 — Claim verification:
Identify what the claim asserts (specific dates, numbers, names, events, attributions), then look in the source for support, contradiction, or partial coverage.
- DATES: the source must contain the date in some form. Equivalent expressions count — "Wednesday" supports "January 7, 2026" if the article is dated that day; "7 Jan 2026" counts for "7 January 2026".
- NUMBERS, NAMES, QUOTED statements: the source must contain that specific number/name/quote, or a directly equivalent paraphrase.
- Accept paraphrasing and direct implications, but NOT speculative inferences or logical leaps.
- Distinguish definitive statements from hedged language ("it is believed", "some sources suggest"). A claim stated as fact requires source text that is also definitive.
- Names from non-Latin scripts have multiple valid romanizations; treat transliteration variant spellings of the same name ("Chekhov"/"Tchekhov") as equal, not as factual errors.

Return exactly one verdict from this graded scale:
- SUPPORTED — the source contains all of the claim's specific assertions (paraphrase OK if substance matches).
- PARTIAL — the source addresses the claim but contains only some of its assertions, OR asserts it only with hedged/uncertain language.
- NOT_SUPPORTED — the source addresses the topic but contradicts the claim, or has no evidence for its specific assertions.
- SOURCE_UNAVAILABLE — STEP 1 failed: no usable article body.

For SUPPORTED or PARTIAL you MUST quote a short, VERBATIM span copied exactly (character for character) from the source that backs the claim. Never paraphrase, reword, or invent the quote. If you cannot find such a verbatim span, the verdict is NOT_SUPPORTED.

Do NOT output any confidence score, probability, or percentage — only the categorical verdict and the verbatim quote.

Respond with a single JSON object: {"verdict": "<one of the four>", "quote": "<verbatim span or empty>"}.

Examples:

Claim: "The company was founded in 1985 by John Smith."
Source: "Acme Corp was established in 1985. Its founder, John Smith, served as CEO until 2001."
{"verdict": "SUPPORTED", "quote": "Acme Corp was established in 1985. Its founder, John Smith"}

Claim: "The committee published its findings in 1932."
Source: "History of Modern Economics - Google Books Sign in ... My library Help Advanced Book Search"
{"verdict": "SOURCE_UNAVAILABLE", "quote": ""}

Claim: "The bridge was completed in 1998."
Source: "The Morrison Bridge broke ground in 1994. The bridge was finally opened to traffic in August 2002, four years behind schedule."
{"verdict": "NOT_SUPPORTED", "quote": "finally opened to traffic in August 2002"}

Claim: "The treaty was signed in Paris."
Source: "It is believed the treaty was signed in Paris, though some historians dispute this."
{"verdict": "PARTIAL", "quote": "It is believed the treaty was signed in Paris"}"#;

/// Build the two-message verification prompt: `[system, user]`.
///
/// `metadata`, when present, is rendered as a context-only block before the source —
/// never groundable bytes (ADR-0007 Alt (e)).
#[must_use]
pub fn build_verify_prompt(
    claim: &str,
    source_text: &str,
    source_url: &str,
    metadata: Option<&CitoidMetadata>,
) -> [ChatMessage; 2] {
    let section = metadata.map(metadata_section).unwrap_or_default();
    let user = format!(
        "CLAIM:\n{claim}\n\n{section}SOURCE ({source_url}):\n\"\"\"\n{source_text}\n\"\"\"\n\nRespond with the JSON object described in the instructions."
    );
    [ChatMessage::system(SYSTEM), ChatMessage::user(user)]
}

/// Render the bibliographic metadata as a context-only section (empty when no field is
/// present, so the prompt is byte-identical to the no-metadata form).
fn metadata_section(meta: &CitoidMetadata) -> String {
    let mut lines = Vec::new();
    if let Some(value) = &meta.publication {
        lines.push(format!("- publication: {value}"));
    }
    if let Some(value) = &meta.published {
        lines.push(format!("- published: {value}"));
    }
    if let Some(value) = &meta.author {
        lines.push(format!("- author: {value}"));
    }
    if let Some(value) = &meta.title {
        lines.push(format!("- title: {value}"));
    }
    if lines.is_empty() {
        return String::new();
    }
    format!(
        "SOURCE METADATA (bibliographic context only — DO NOT quote from here; your supporting quote MUST come verbatim from the SOURCE text below):\n{}\n\n",
        lines.join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::{SYSTEM, build_verify_prompt};
    use crate::citation::citoid::CitoidMetadata;
    use sp42_types::ChatRole;

    #[test]
    fn returns_system_then_user() {
        let prompt =
            build_verify_prompt("a claim", "some source body", "https://example.com", None);
        assert_eq!(prompt[0].role, ChatRole::System);
        assert_eq!(prompt[1].role, ChatRole::User);
        assert_eq!(prompt[0].content, SYSTEM);
    }

    #[test]
    fn user_message_carries_claim_source_and_url() {
        let prompt = build_verify_prompt(
            "The bridge opened in 1998",
            "The bridge opened to traffic in 1998.",
            "https://example.com/bridge",
            None,
        );
        let user = &prompt[1].content;
        assert!(user.contains("The bridge opened in 1998"));
        assert!(user.contains("The bridge opened to traffic in 1998."));
        assert!(user.contains("https://example.com/bridge"));
    }

    #[test]
    fn system_states_the_discipline() {
        assert!(SYSTEM.contains("ONLY"));
        assert!(SYSTEM.contains("Do NOT") || SYSTEM.contains("do NOT") || SYSTEM.contains("Never"));
        assert!(SYSTEM.contains("VERBATIM"));
        assert!(SYSTEM.contains("STEP 1"));
        assert!(SYSTEM.contains("STEP 2"));
    }

    #[test]
    fn system_names_all_four_verdicts() {
        for verdict in [
            "SUPPORTED",
            "PARTIAL",
            "NOT_SUPPORTED",
            "SOURCE_UNAVAILABLE",
        ] {
            assert!(SYSTEM.contains(verdict), "missing {verdict}");
        }
    }

    #[test]
    fn system_forbids_confidence_numbers_and_has_an_example() {
        assert!(SYSTEM.contains("confidence"));
        assert!(SYSTEM.contains("percentage"));
        assert!(SYSTEM.contains("\"verdict\":"));
    }

    #[test]
    fn metadata_renders_as_context_only_when_present() {
        let metadata = CitoidMetadata {
            publication: Some("The Guardian".to_string()),
            published: Some("2020-01-01".to_string()),
            author: Some("Jane Doe".to_string()),
            title: Some("Headline".to_string()),
            url: "https://example.com".to_string(),
        };
        let prompt = build_verify_prompt("c", "body", "https://example.com", Some(&metadata));
        let user = &prompt[1].content;
        assert!(user.contains("METADATA"));
        assert!(user.contains("DO NOT quote"));
        assert!(user.contains("The Guardian"));
        assert!(user.contains("Jane Doe"));
        assert!(user.contains("Headline"));
        // Order: the metadata block precedes the SOURCE block.
        let meta_at = user.find("METADATA").expect("metadata present");
        let source_at = user.find("SOURCE (").expect("source present");
        assert!(meta_at < source_at);
    }

    #[test]
    fn no_metadata_means_no_metadata_section() {
        let prompt = build_verify_prompt("c", "body", "https://example.com", None);
        assert!(!prompt[1].content.contains("METADATA"));
    }
}
