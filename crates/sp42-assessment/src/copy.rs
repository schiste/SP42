//! Every GA-facing English string in the appendix, in one place (PRD-0016:
//! reader-facing vocabulary; enables later localization / `{{GAList}}` idiom
//! swap as a copy change, not an architecture change).
//!
//! Vocabulary here renders on-wiki; keep docs/domains/assessment/what-is-this-appendix.md in sync.

use sp42_citation::{BodyUsabilityReason, GroundingStatus, SupportLevel};

/// Appendix and section headings (plain wikitext, no transclusions).
pub const APPENDIX_HEADING: &str = "== SP42 evidence appendix ==";
pub const CRITERION_2_HEADING: &str =
    "=== Criterion 2 (verifiable) — [[Wikipedia:Good article criteria|GA criteria]] ===";
pub const BUCKET_DISAGREEMENTS: &str = "==== Claim–source disagreements ====";
pub const BUCKET_RECOVERED: &str =
    "==== Supported via archive copy (citation update suggested) ====";
pub const BUCKET_DEAD_LINKS: &str = "==== Dead links (no archive copy found) ====";
pub const BUCKET_UNREADABLE: &str =
    "==== Sources the tool could not read (tool limitation — the citations may be fine) ====";
pub const BUCKET_UNCONFIRMED: &str =
    "==== Unconfirmed supports (judged supported, quote not re-located) ====";
pub const BUCKET_SUPPORTED: &str = "==== Supported spot-checks ====";
pub const BUCKET_SKIPPED: &str = "==== Not machine-verified (book and offline sources) ====";
pub const BUCKET_EXTRACTION_FAILURES: &str = "==== Refs the tool could not process ====";

/// The positive "assessed by SP42" honesty line (2b only in the MVP).
pub const ASSESSED_LINE: &str = "''This appendix carries evidence for criterion 2b \
(inline citations support the text) only; all other criteria and sub-criteria were \
not assessed by the tool.''";

/// Provenance-footer framing line.
pub const FRAMING_LINE: &str = "This is a tool-generated evidence appendix; the \
criteria judgments and the review outcome are the reviewer's.";

/// "What is this?" explainer target (repo-hosted for the MVP; Phase 3 creates it).
pub const EXPLAINER_URL: &str =
    "https://github.com/schiste/SP42/blob/main/docs/domains/assessment/what-is-this-appendix.md";

/// Reader-facing verdict for a disagreement line (PRD-0014 mismatch framing).
#[must_use]
pub fn disagreement_verdict(level: SupportLevel, panel_judged: bool) -> &'static str {
    match (level, panel_judged) {
        (SupportLevel::NotSupported, true) => {
            "the source and this claim disagree — the panel found no support for the claim in the source"
        }
        // No panel voted (a deterministic book-search miss): attribute the
        // outcome to the search, not a panel (Codex round 15, PR 154).
        (SupportLevel::NotSupported, false) => {
            "the source was searched and no supporting passage was found"
        }
        (SupportLevel::Partial, _) => "the source only partially supports this claim",
        (SupportLevel::Supported, _) => "the source supports this claim",
    }
}

/// Grounding annotation for non-exact grounding (renders wherever it lands).
#[must_use]
pub fn grounding_annotation(status: GroundingStatus) -> Option<&'static str> {
    match status {
        GroundingStatus::Unlocated | GroundingStatus::NotApplicable => {
            Some("the supporting quote could not be re-located in the source")
        }
        GroundingStatus::LocatedFuzzy => {
            Some("the supporting quote matched the source only approximately")
        }
        GroundingStatus::Located => None,
    }
}

/// No-quote verdict wording (ADR-0007: never fabricate a passage).
pub const NO_PASSAGE_LINE: &str = "no supporting passage was found in the source";

/// Label for `source_excerpt` context — never presented as evidence.
pub const EXCERPT_CONTEXT_LABEL: &str = "context the tool read (not evidence)";

/// Low-confidence annotation when the panel's winning verdict lacks a majority.
pub const PANEL_SPLIT_LINE: &str =
    "the review panel split on this reading — treat as low-confidence";

/// Repair-handle annotation for lines carrying `archive_of`.
pub const ARCHIVE_HANDLE_PREFIX: &str = "verified against an archive copy of";

/// Reader-facing reason a fetched source could not be used.
#[must_use]
pub fn unusable_reason(reason: BodyUsabilityReason) -> &'static str {
    match reason {
        BodyUsabilityReason::PdfBody => "a PDF the tool cannot read",
        BodyUsabilityReason::ViewerShell => "an interactive viewer page with no readable text",
        BodyUsabilityReason::NavChromePaywall => "a paywall or registration page",
        BodyUsabilityReason::ShortBody => "the page returned too little readable text",
        BodyUsabilityReason::AntiBotChallenge => "an anti-bot challenge page",
        BodyUsabilityReason::WaybackRedirectNotice | BodyUsabilityReason::WaybackChrome => {
            "an archive page without readable article content"
        }
        BodyUsabilityReason::JsonLdLeak | BodyUsabilityReason::CssLeak => {
            "page code instead of article text"
        }
        BodyUsabilityReason::AmazonStub => "a storefront stub page",
        BodyUsabilityReason::Ok => "a page the panel could not use",
    }
}

/// Generic unreadable wording when the report carries no specific reason.
pub const UNUSABLE_GENERIC: &str = "a page the tool fetched but could not use";

/// Skip reason: a ref with no URL and no usable book identifier.
pub const SKIPPED_NON_URL: &str = "cites a book or offline source the tool does not verify";

/// Skip reason: a book identifier was found but no catalog record resolved
/// (or the lookup did not complete) — distinct from "not machine-verified at all".
pub const SKIPPED_BOOK_UNRESOLVED: &str =
    "cites a book whose identifier matched no catalog record the tool could use";

/// Skip reason: the catalog lookup never completed (a tool/transport
/// problem) — the book may well have a record; nothing is implied about it.
pub const SKIPPED_BOOK_LOOKUP_FAILED: &str = "cites a book whose catalog lookup did not complete — the tool could not reach the catalog; nothing is implied about the book";

/// Books consulted section heading.
pub const BUCKET_BOOKS_CONSULTED: &str = "==== Books consulted ====";

/// Reader-facing description of scan availability states for books (exact scan available).
pub const BOOK_SCAN_EXACT: &str = "scanned (exact edition)";

/// Reader-facing description of scan availability states (similar edition scan only).
pub const BOOK_SCAN_SIMILAR_ONLY: &str = "scanned (similar edition only)";

/// Reader-facing description when no scan is available.
pub const BOOK_SCAN_NONE: &str = "not scanned in the tool's catalog";

/// An exact-edition scan is cataloged but its identity could not be
/// recovered, so the tool could not search it — a tool limitation, distinct
/// from "no scan exists".
pub const BOOK_SCAN_EXACT_UNSEARCHABLE: &str =
    "an exact-edition scan is cataloged, but the tool could not identify it to search it";

/// Reader-facing description when the tool could not verify scan availability.
pub const BOOK_SCAN_UNKNOWN: &str = "scan availability unknown (the lookup did not complete)";

/// Reader-facing description when a book catalog lookup never completed (transport problem).
pub const BOOK_LOOKUP_FAILED: &str = "catalog lookup did not complete — a tool limitation";

/// Book-scan page annotation: the scan located the passage on a different
/// page than the citation names (a disclosure, not a defect — scan
/// pagination often differs from the cited edition).
#[must_use]
pub fn book_scan_pages(scanned: u32, cited: &str) -> String {
    format!("the passage was located on scanned page {scanned}; the citation names p. {cited}")
}

/// Neutralize verdict-sounding tool wording in contract-authored strings
/// (failure reasons, scan notes) before they reach the appendix: the
/// no-pass/fail invariant binds the whole output, and these strings are
/// tool messages, not article content — the copy module is their sanctioned
/// rewrite seam (PRD-0016; Codex round 8, PR 154).
#[must_use]
pub fn neutralize_tool_wording(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (i, word) in text
        .split_inclusive(|c: char| !c.is_ascii_alphabetic())
        .enumerate()
    {
        let _ = i;
        let (core, sep) = match word.char_indices().next_back() {
            Some((idx, c)) if !c.is_ascii_alphabetic() => (&word[..idx], &word[idx..]),
            _ => (word, ""),
        };
        let replacement = match core.to_ascii_lowercase().as_str() {
            "failed" => Some("did not complete"),
            "failure" | "failures" => Some("problem"),
            "fails" | "fail" => Some("does not complete"),
            "passed" | "passes" | "pass" => Some("completed"),
            _ => None,
        };
        match replacement {
            Some(neutral) => {
                out.push_str(neutral);
                out.push_str(sep);
            }
            None => out.push_str(word),
        }
    }
    out
}

/// Reader-facing label for one validated book identifier.
#[must_use]
pub fn book_identifier(identifier: &sp42_citation::BookIdentifier) -> String {
    use sp42_citation::BookIdentifier;
    match identifier {
        BookIdentifier::Isbn(value) => format!("ISBN {value}"),
        BookIdentifier::Oclc(value) => format!("OCLC {value}"),
        BookIdentifier::Lccn(value) => format!("LCCN {value}"),
        BookIdentifier::Olid(value) => format!("Open Library id {value}"),
    }
}

#[cfg(test)]
mod tests {
    use sp42_citation::{BodyUsabilityReason, SupportLevel};

    #[test]
    fn verdict_copy_never_leaks_contract_identifiers() {
        for level in [SupportLevel::Partial, SupportLevel::NotSupported] {
            let text = super::disagreement_verdict(level, true);
            assert!(!text.is_empty());
            let text = super::disagreement_verdict(level, false);
            assert!(!text.contains("NotSupported") && !text.contains("Partial"));
            assert!(
                !text.to_lowercase().contains("fail"),
                "mismatch framing, not failure: {text}"
            );
        }
    }

    #[test]
    fn unusable_reason_copy_covers_every_variant() {
        for reason in [
            BodyUsabilityReason::JsonLdLeak,
            BodyUsabilityReason::CssLeak,
            BodyUsabilityReason::AntiBotChallenge,
            BodyUsabilityReason::WaybackRedirectNotice,
            BodyUsabilityReason::WaybackChrome,
            BodyUsabilityReason::AmazonStub,
            BodyUsabilityReason::ShortBody,
            BodyUsabilityReason::PdfBody,
            BodyUsabilityReason::ViewerShell,
            BodyUsabilityReason::NavChromePaywall,
        ] {
            let text = super::unusable_reason(reason);
            assert!(!text.is_empty());
            assert!(!text.contains('_'), "no snake_case tokens: {text}");
        }
    }
}
