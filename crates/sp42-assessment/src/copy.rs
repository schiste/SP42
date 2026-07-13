//! Every GA-facing English string in the appendix, in one place (PRD-0016:
//! reader-facing vocabulary; enables later localization / `{{GAList}}` idiom
//! swap as a copy change, not an architecture change).

use sp42_citation::{BodyUsabilityReason, GroundingStatus, SupportLevel};

/// Appendix and section headings (plain wikitext, no transclusions).
pub const APPENDIX_HEADING: &str = "== SP42 evidence appendix ==";
pub const CRITERION_2_HEADING: &str =
    "=== Criterion 2 (verifiable) — [[Wikipedia:Good article criteria|GA criteria]] ===";
pub const BUCKET_DISAGREEMENTS: &str = "==== Claim–source disagreements ====";
pub const BUCKET_RECOVERED: &str = "==== Supported via archive copy (citation update suggested) ====";
pub const BUCKET_DEAD_LINKS: &str = "==== Dead links (no archive copy found) ====";
pub const BUCKET_UNREADABLE: &str = "==== Sources the tool could not read (tool limitation — the citations may be fine) ====";
pub const BUCKET_UNCONFIRMED: &str = "==== Unconfirmed supports (judged supported, quote not re-located) ====";
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
pub fn disagreement_verdict(level: SupportLevel) -> &'static str {
    match level {
        SupportLevel::NotSupported => "the source and this claim disagree — the panel found no support for the claim in the source",
        SupportLevel::Partial => "the source only partially supports this claim",
        SupportLevel::Supported => "the source supports this claim",
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

/// Skip reason (single contract variant today: non-URL source).
pub const SKIPPED_NON_URL: &str =
    "cites a book or offline source the tool does not verify";

#[cfg(test)]
mod tests {
    use sp42_citation::{BodyUsabilityReason, SupportLevel};

    #[test]
    fn verdict_copy_never_leaks_contract_identifiers() {
        for level in [SupportLevel::Partial, SupportLevel::NotSupported] {
            let text = super::disagreement_verdict(level);
            assert!(!text.contains("NotSupported") && !text.contains("Partial"));
            assert!(!text.to_lowercase().contains("fail"), "mismatch framing, not failure: {text}");
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
