//! Reviewer-facing "see the source" links for a [`crate::CitationFinding`]
//! (dataset-validation UI, PRD-0009).
//!
//! The bounded `source_excerpt` on a finding is enough to *confirm* an obvious
//! match, but a human auditor sometimes needs to open the source itself — when
//! the excerpt window clipped the qualifying clause, or the scraper may have read
//! the wrong page. This module turns a finding's provenance into the links that
//! let them do that, as pure functions so any surface (the mobile review card
//! today, a CLI report later) builds the same links.
//!
//! Two rules shape what it produces, both carried from the citation contract:
//!
//! - **Parity (ADR-0007 §5).** The *primary* link points at the exact bytes the
//!   panel read — the rewritten archive snapshot when the panel read an archive,
//!   otherwise the fetched URL — so the human forms their judgment against the
//!   same text the model saw. The original live URL (when an archive stood in for
//!   it) is offered only as an explicitly-labelled secondary, because it may have
//!   changed since review or be dead.
//! - **Deep-link to the quote.** On a phone, dropping a reviewer at the top of a
//!   long article to hunt for one sentence is the failure mode, so when the
//!   finding has a located passage the primary link carries a `#:~:text=`
//!   fragment that scrolls to and highlights it.
//!
//! Licensing note (PRD-0007): these are *links*, not source bytes — pointing at a
//! source republishes nothing, which is why link-out is the clean evidence tier
//! for a public reviewer while the full extracted body (fair-use bounded) stays
//! gated to authenticated ones.

use crate::{CitationFinding, CitationVerdict, is_archive_url, rewrite_wayback_url};

/// Which source view a link points at, and how a reviewer surface should label it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceLinkKind {
    /// Points at the exact bytes the panel read — the reviewer should judge
    /// against this so human and model adjudicate the same text (parity).
    /// `is_archive` is `true` when those bytes are an archive snapshot.
    PanelSource {
        /// Whether the panel's source was an archive snapshot rather than a live page.
        is_archive: bool,
    },
    /// The original live URL the panel's archive snapshot stands in for. May have
    /// changed since review, or be dead (the reason the archive was used); offered
    /// only as an explicitly-labelled secondary.
    LiveOriginal,
}

/// A single reviewer-facing link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceLink {
    /// The href to open.
    pub url: String,
    /// What this link points at.
    pub kind: EvidenceLinkKind,
    /// Whether a `#:~:text=` fragment was appended so the browser scrolls to and
    /// highlights the located quote on open. `false` when there was no located
    /// passage to anchor to (e.g. a `SourceUnavailable` finding).
    pub deep_linked: bool,
}

/// The set of "see the source" links to offer for one finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceLinks {
    /// The link the reviewer should judge against (parity with the panel).
    pub primary: EvidenceLink,
    /// The original live URL, present only when it differs from `primary` (i.e.
    /// the panel read an archive). Labelled as possibly-changed / possibly-dead.
    pub live_original: Option<EvidenceLink>,
    /// `true` when the finding is a `SourceUnavailable` — the primary link is the
    /// URL SP42 could not use, so the UI should frame it as "try opening what SP42
    /// could not" rather than "confirm the quote".
    pub source_was_unavailable: bool,
}

/// Above this many whitespace-split words a quote is deep-linked by its first and
/// last few words (`textStart,textEnd`) rather than in full, because an exact
/// long match often fails when the page's whitespace or punctuation differs
/// slightly from the extracted bytes.
const WHOLE_QUOTE_WORD_LIMIT: usize = 10;
/// How many words from each end anchor a long quote's `textStart,textEnd`.
const EDGE_WORDS: usize = 4;

/// Build the reviewer-facing evidence links for a finding.
#[must_use]
pub fn build_evidence_links(finding: &CitationFinding) -> EvidenceLinks {
    let fetched = finding.provenance.url.as_str();
    // Prefer the raw archived page (Wayback `id_`) so a mobile reader sees the
    // captured source without the archive's own chrome; a live URL passes through.
    let base = rewrite_wayback_url(fetched);
    let quote = finding
        .passage
        .as_ref()
        .map(|passage| passage.quote.as_str())
        .filter(|quote| !quote.is_empty());

    let primary = EvidenceLink {
        url: match quote {
            Some(quote) => append_text_fragment(&base, quote),
            None => base,
        },
        kind: EvidenceLinkKind::PanelSource {
            is_archive: is_archive_url(fetched),
        },
        deep_linked: quote.is_some(),
    };

    let live_original = finding.archive_of.as_ref().map(|live| EvidenceLink {
        url: live.to_string(),
        kind: EvidenceLinkKind::LiveOriginal,
        deep_linked: false,
    });

    EvidenceLinks {
        primary,
        live_original,
        source_was_unavailable: matches!(finding.verdict, CitationVerdict::SourceUnavailable),
    }
}

/// Append a URL Text Fragment (`#:~:text=`) that a modern browser uses to scroll
/// to and highlight `quote` on open. `base` is assumed to carry no existing
/// fragment (article-HTML and archive-snapshot URLs here do not); a pre-existing
/// one would collide with the text directive.
fn append_text_fragment(base: &str, quote: &str) -> String {
    let words: Vec<&str> = quote.split_whitespace().collect();
    let directive = if words.len() <= WHOLE_QUOTE_WORD_LIMIT {
        encode_fragment_part(&words.join(" "))
    } else {
        let start = words[..EDGE_WORDS].join(" ");
        let end = words[words.len() - EDGE_WORDS..].join(" ");
        format!(
            "{},{}",
            encode_fragment_part(&start),
            encode_fragment_part(&end)
        )
    };
    format!("{base}#:~:text={directive}")
}

/// Percent-encode one Text Fragment text component. Only the unreserved set
/// `A-Za-z0-9 . _ ~` passes through; `-`, `,`, `&` and whitespace are encoded
/// because they are significant in the fragment grammar (`,` and the boundary
/// `-` delimit `prefix-,textStart,textEnd,-suffix`, `&` separates directives).
fn encode_fragment_part(input: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(input.len());
    for &byte in input.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{EvidenceLinkKind, build_evidence_links};
    use crate::{
        CitationFinding, CitationFindingKind, CitationVerdict, GroundingAssertion, GroundingStatus,
        LocatedPassage, PanelAgreement, SourceProvenance, SupportLevel,
    };

    fn finding(url: &str, verdict: CitationVerdict, quote: Option<&str>) -> CitationFinding {
        CitationFinding {
            kind: CitationFindingKind::CitationVerdict,
            verdict,
            grounding_status: GroundingStatus::NotApplicable,
            source_unavailable_reason: None,
            unusable_reason: None,
            agreement: PanelAgreement::new(3, 3),
            passage: quote.map(|quote| LocatedPassage {
                quote: quote.to_string(),
                offset: 0,
            }),
            provenance: SourceProvenance {
                url: url::Url::parse(url).expect("test url"),
                content_hash: "hash".to_string(),
                fetched_at: 0,
                http_status: Some(200),
            },
            source_excerpt: None,
            metadata: None,
            grounding: GroundingAssertion::SourceFetched {
                source_hash: "hash".to_string(),
            },
            use_site_ordinal: 0,
            ref_id: "ref".to_string(),
            claim: "claim".to_string(),
            preceding_context: Vec::new(),
            archive_of: None,
            schema_version: 1,
        }
    }

    #[test]
    fn live_source_with_quote_is_deep_linked_and_has_no_secondary() {
        let links = build_evidence_links(&finding(
            "https://example.org/a",
            CitationVerdict::Judged(SupportLevel::Supported),
            Some("the sky is blue"),
        ));
        assert_eq!(
            links.primary.url,
            "https://example.org/a#:~:text=the%20sky%20is%20blue"
        );
        assert_eq!(
            links.primary.kind,
            EvidenceLinkKind::PanelSource { is_archive: false }
        );
        assert!(links.primary.deep_linked);
        assert!(links.live_original.is_none());
        assert!(!links.source_was_unavailable);
    }

    #[test]
    fn archive_source_rewrites_to_raw_snapshot_and_offers_the_live_original() {
        let mut f = finding(
            "https://web.archive.org/web/20200101120000/http://example.com/a",
            CitationVerdict::Judged(SupportLevel::Supported),
            Some("water is wet"),
        );
        f.archive_of = Some(url::Url::parse("http://example.com/a").expect("live url"));

        let links = build_evidence_links(&f);
        assert_eq!(
            links.primary.url,
            "https://web.archive.org/web/20200101120000id_/http://example.com/a#:~:text=water%20is%20wet"
        );
        assert_eq!(
            links.primary.kind,
            EvidenceLinkKind::PanelSource { is_archive: true }
        );
        let live = links.live_original.expect("live original offered");
        assert_eq!(live.url, "http://example.com/a");
        assert_eq!(live.kind, EvidenceLinkKind::LiveOriginal);
        assert!(!live.deep_linked);
    }

    #[test]
    fn long_quote_uses_start_and_end_anchors() {
        let long = "one two three four five six seven eight nine ten eleven twelve";
        let links = build_evidence_links(&finding(
            "https://example.org/a",
            CitationVerdict::Judged(SupportLevel::Supported),
            Some(long),
        ));
        assert_eq!(
            links.primary.url,
            "https://example.org/a#:~:text=one%20two%20three%20four,nine%20ten%20eleven%20twelve"
        );
    }

    #[test]
    fn source_unavailable_has_no_fragment_and_is_flagged() {
        let links = build_evidence_links(&finding(
            "https://paywalled.example/pdf",
            CitationVerdict::SourceUnavailable,
            None,
        ));
        assert_eq!(links.primary.url, "https://paywalled.example/pdf");
        assert!(!links.primary.deep_linked);
        assert!(links.source_was_unavailable);
    }

    #[test]
    fn fragment_encodes_delimiter_characters() {
        let links = build_evidence_links(&finding(
            "https://example.org/a",
            CitationVerdict::Judged(SupportLevel::Supported),
            Some("a, well-known fact"),
        ));
        // comma -> %2C, hyphen -> %2D, space -> %20: none leak as fragment delimiters.
        assert_eq!(
            links.primary.url,
            "https://example.org/a#:~:text=a%2C%20well%2Dknown%20fact"
        );
    }
}
