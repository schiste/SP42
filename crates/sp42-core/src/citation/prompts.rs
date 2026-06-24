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

/// The SIDE-style co-reference context window for a claim (interpreting material only).
///
/// Rendered into the verification prompt as a **context-only** block — the model may use it
/// to interpret the claim (resolve pronouns / elliptical references) but may never quote it
/// as support. The grounding gate only ever locates quotes in the fetched source body, so
/// this can never become groundable (refines ADR-0007 Alt (e)). Carries the new contextual
/// material only; the claim itself stays single-source on `CitationVerificationRequest`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClaimContext {
    /// The article title.
    pub article_title: String,
    /// The section title, when known.
    pub section_title: Option<String>,
    /// Preceding sentences, in document order (most useful for co-reference).
    pub preceding_sentences: Vec<String>,
}

impl ClaimContext {
    /// `true` when there is no contextual material to render. An empty context renders
    /// nothing, keeping the prompt byte-identical to the no-context form.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.article_title.trim().is_empty()
            && self
                .section_title
                .as_ref()
                .is_none_or(|section| section.trim().is_empty())
            && self
                .preceding_sentences
                .iter()
                .all(|sentence| sentence.trim().is_empty())
    }
}

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

/// The repair-turn system instruction (SP42#25 layer 3): one bounded extra turn that fixes
/// *transcription*, never judgment — it asks only for a verbatim span (or `NO_SPAN`), never
/// for a verdict, so the panel's judgment cannot be re-litigated by the retry.
pub const REPAIR_SYSTEM: &str = r#"You are given a CLAIM, a SOURCE, and a previous supporting quote that did NOT match the source text verbatim.

Your only job is transcription. Find the exact SHORTEST contiguous span of the SOURCE text that backs the claim, and copy it character for character (VERBATIM). Do not paraphrase, reword, correct spelling, abbreviate, or merge separate passages. Copy from the SOURCE text only — never from the claim, and never from memory.

If no such span exists in the source, respond with NO_SPAN.

Respond with a single JSON object: {"quote": "<verbatim span copied from the source>"} or {"quote": "NO_SPAN"}."#;

/// Build the two-message verification prompt: `[system, user]`.
///
/// `context` (the co-reference window) and `metadata` (bibliographic), when present, are
/// each rendered as a context-only block before the source — never groundable bytes
/// (ADR-0007 Alt (e)). An absent or empty `context` leaves the prompt byte-identical to the
/// no-context form.
#[must_use]
pub fn build_verify_prompt(
    claim: &str,
    source_text: &str,
    source_url: &str,
    metadata: Option<&CitoidMetadata>,
    context: Option<&ClaimContext>,
) -> [ChatMessage; 2] {
    let context_block = context.map(context_section).unwrap_or_default();
    let section = metadata.map(metadata_section).unwrap_or_default();
    let user = format!(
        "CLAIM:\n{claim}\n\n{context_block}{section}SOURCE ({source_url}):\n\"\"\"\n{source_text}\n\"\"\"\n\nRespond with the JSON object described in the instructions."
    );
    [ChatMessage::system(SYSTEM), ChatMessage::user(user)]
}

/// Build the two-message repair-turn prompt (SP42#25 layer 3): the claim, the quote that
/// failed to locate, and the source again. The response is re-located deterministically by
/// the caller; an unrepairable quote stays unlocated.
#[must_use]
pub fn build_repair_prompt(
    claim: &str,
    source_text: &str,
    source_url: &str,
    failed_quote: &str,
) -> [ChatMessage; 2] {
    let user = format!(
        "CLAIM:\n{claim}\n\nPREVIOUS QUOTE (did not match the source verbatim):\n\"\"\"\n{failed_quote}\n\"\"\"\n\nSOURCE ({source_url}):\n\"\"\"\n{source_text}\n\"\"\"\n\nRespond with the JSON object described in the instructions."
    );
    [ChatMessage::system(REPAIR_SYSTEM), ChatMessage::user(user)]
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

/// Render the co-reference context window as a context-only block (empty string when the
/// context has nothing to show, so the prompt is byte-identical to the no-context form).
fn context_section(context: &ClaimContext) -> String {
    if context.is_empty() {
        return String::new();
    }
    let mut lines = Vec::new();
    if !context.article_title.trim().is_empty() {
        lines.push(format!("- article: {}", context.article_title));
    }
    if let Some(section) = &context.section_title
        && !section.trim().is_empty()
    {
        lines.push(format!("- section: {section}"));
    }
    let preceding: Vec<&String> = context
        .preceding_sentences
        .iter()
        .filter(|sentence| !sentence.trim().is_empty())
        .collect();
    if !preceding.is_empty() {
        lines.push("- preceding text:".to_string());
        for sentence in preceding {
            lines.push(format!("    {sentence}"));
        }
    }
    format!(
        "CLAIM CONTEXT — BACKGROUND ONLY. Use this solely to resolve references and pronouns in the CLAIM (what its names, dates, and \"it\"/\"he\"/\"there\" refer to). It is NOT part of the claim and NOT a source. Do NOT verify it, DO NOT quote from it (your supporting quote MUST come verbatim from the SOURCE text below), and do NOT let it widen the claim or make you hedge. Judge the CLAIM against the SOURCE exactly as if the claim were already self-contained:\n{}\n\n",
        lines.join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::{
        ClaimContext, REPAIR_SYSTEM, SYSTEM, build_repair_prompt, build_verify_prompt,
        context_section,
    };
    use crate::citation::citoid::CitoidMetadata;
    use sp42_types::ChatRole;

    #[test]
    fn context_section_is_empty_for_empty_context() {
        assert_eq!(context_section(&ClaimContext::default()), String::new());
        let blank = ClaimContext {
            article_title: "   ".to_string(),
            section_title: Some(String::new()),
            preceding_sentences: vec!["  ".to_string()],
        };
        assert_eq!(context_section(&blank), String::new());
    }

    #[test]
    fn context_section_renders_labeled_context_only_block() {
        let ctx = ClaimContext {
            article_title: "Ann Jansson".to_string(),
            section_title: Some("Career".to_string()),
            preceding_sentences: vec!["She joined the club in 1985.".to_string()],
        };
        let rendered = context_section(&ctx);
        assert!(rendered.contains("Ann Jansson"));
        assert!(rendered.contains("Career"));
        assert!(rendered.contains("She joined the club in 1985."));
        // Context-only discipline: the supporting quote must still come from the SOURCE.
        assert!(rendered.contains("DO NOT quote"));
        assert!(rendered.contains("SOURCE"));
    }

    #[test]
    fn returns_system_then_user() {
        let prompt = build_verify_prompt(
            "a claim",
            "some source body",
            "https://example.com",
            None,
            None,
        );
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
        let prompt = build_verify_prompt("c", "body", "https://example.com", Some(&metadata), None);
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
        let prompt = build_verify_prompt("c", "body", "https://example.com", None, None);
        assert!(!prompt[1].content.contains("METADATA"));
    }

    #[test]
    fn empty_context_is_byte_identical_to_no_context() {
        let with_none = build_verify_prompt("c", "body", "https://example.com", None, None);
        let with_empty = build_verify_prompt(
            "c",
            "body",
            "https://example.com",
            None,
            Some(&ClaimContext::default()),
        );
        assert_eq!(with_none[1].content, with_empty[1].content);
    }

    #[test]
    fn context_block_precedes_the_source_block() {
        let ctx = ClaimContext {
            article_title: "Ann Jansson".to_string(),
            ..Default::default()
        };
        let prompt = build_verify_prompt("c", "body", "https://example.com", None, Some(&ctx));
        let user = &prompt[1].content;
        let ctx_at = user.find("CLAIM CONTEXT").expect("context block present");
        let source_at = user.find("SOURCE (").expect("source block present");
        assert!(ctx_at < source_at);
    }

    // --- repair turn (SP42#25 layer 3) ---

    #[test]
    fn repair_prompt_returns_system_then_user() {
        let prompt = build_repair_prompt(
            "the claim",
            "the source body",
            "https://example.com",
            "the failed quote",
        );
        assert_eq!(prompt[0].role, ChatRole::System);
        assert_eq!(prompt[1].role, ChatRole::User);
        assert_eq!(prompt[0].content, REPAIR_SYSTEM);
    }

    #[test]
    fn repair_user_message_carries_claim_source_url_and_failed_quote() {
        let prompt = build_repair_prompt(
            "The bridge opened in 1998",
            "The bridge opened to traffic in 1998.",
            "https://example.com/bridge",
            "bridge was opened in 1998",
        );
        let user = &prompt[1].content;
        assert!(user.contains("The bridge opened in 1998"));
        assert!(user.contains("The bridge opened to traffic in 1998."));
        assert!(user.contains("https://example.com/bridge"));
        assert!(user.contains("bridge was opened in 1998"));
    }

    #[test]
    fn repair_system_states_the_transcription_only_discipline() {
        // The repair turn fixes TRANSCRIPTION, never judgment: it must demand a verbatim
        // shortest span, offer NO_SPAN as the out, and never ask for a verdict.
        assert!(REPAIR_SYSTEM.contains("NO_SPAN"));
        assert!(
            REPAIR_SYSTEM.contains("VERBATIM") || REPAIR_SYSTEM.contains("character for character")
        );
        assert!(REPAIR_SYSTEM.to_lowercase().contains("shortest"));
        assert!(!REPAIR_SYSTEM.to_lowercase().contains("verdict"));
        assert!(REPAIR_SYSTEM.contains("\"quote\":"));
    }
}
