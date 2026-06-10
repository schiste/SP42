//! Bare-URL reference repair (PRD-0008): pure classification of bare-URL
//! references, rendering Citoid metadata into a citation template, and the
//! propose/confirm wire contracts shared by the server routes and the CLI.
//!
//! FCIS (Constitution Art. 2): everything here is pure and clock-free. The
//! server shell fetches Citoid metadata and passes the fetch date in; the
//! apply path replays a proposal verbatim through the node-anchored editor
//! (ADR-0003), inheriting its anti-drift and zero-write refusal guarantees.

use crate::wikitext_editor::{WikitextNodeDescriptor, WikitextNodeKind};

/// One bare-URL reference found among a revision's `Reference` nodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BareUrlReference {
    /// Zero-based document-order position among `Reference` nodes.
    pub ordinal: usize,
    /// The reference's single plain URL (the trimmed anchor text).
    pub url: String,
    /// The anchor text exactly as enumerated — echoed back as the locator's
    /// `expected_text` so the anti-drift re-check can hold.
    pub anchor_text: String,
}

/// The trimmed anchor text iff it is exactly one plain `http(s)` URL
/// (PRD-0008 Resolved question 1).
///
/// Reference anchors are the node's rendered text content, so a bare-URL
/// reference's anchor is the URL itself. Bracket-wrapped refs render as a
/// numbered/labelled link (anchor is not the URL) and are excluded; any
/// other prose is operator-authored content this feature must not discard.
#[must_use]
pub fn classify_bare_url(anchor_text: &str) -> Option<&str> {
    let trimmed = anchor_text.trim();
    if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
        return None;
    }
    if trimmed.chars().any(char::is_whitespace) {
        return None;
    }
    if url::Url::parse(trimmed).is_err() {
        return None;
    }
    Some(trimmed)
}

/// The bare-URL references among `descriptors`, in document order.
///
/// Non-`Reference` descriptors are ignored; ordinals are preserved from the
/// enumeration (they index into the `Reference` node family).
#[must_use]
pub fn bare_url_references(descriptors: &[WikitextNodeDescriptor]) -> Vec<BareUrlReference> {
    descriptors
        .iter()
        .filter(|descriptor| descriptor.kind == WikitextNodeKind::Reference)
        .filter_map(|descriptor| {
            classify_bare_url(&descriptor.anchor_text).map(|url| BareUrlReference {
                ordinal: descriptor.ordinal,
                url: url.to_string(),
                anchor_text: descriptor.anchor_text.clone(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_accepts_exactly_one_plain_http_url() {
        assert_eq!(
            classify_bare_url("https://example.org/article"),
            Some("https://example.org/article")
        );
        assert_eq!(
            classify_bare_url("  http://example.org/a?b=c  "),
            Some("http://example.org/a?b=c")
        );
    }

    #[test]
    fn classify_rejects_brackets_prose_and_non_http() {
        assert_eq!(classify_bare_url("[https://example.org/a]"), None);
        assert_eq!(classify_bare_url("Example A citation"), None);
        assert_eq!(classify_bare_url("https://example.org/a extra words"), None);
        assert_eq!(classify_bare_url("see https://example.org/a"), None);
        assert_eq!(classify_bare_url("ftp://example.org/a"), None);
        assert_eq!(classify_bare_url("https://"), None);
        assert_eq!(classify_bare_url(""), None);
    }

    #[test]
    fn bare_url_references_filters_by_kind_and_keeps_ordinals() {
        let descriptors = vec![
            WikitextNodeDescriptor {
                kind: WikitextNodeKind::Reference,
                ordinal: 0,
                anchor_text: "https://example.org/a".to_string(),
            },
            WikitextNodeDescriptor {
                kind: WikitextNodeKind::Reference,
                ordinal: 1,
                anchor_text: "Prose citation".to_string(),
            },
            WikitextNodeDescriptor {
                kind: WikitextNodeKind::Template,
                ordinal: 0,
                anchor_text: "https://example.org/a".to_string(),
            },
            WikitextNodeDescriptor {
                kind: WikitextNodeKind::Reference,
                ordinal: 2,
                anchor_text: "https://example.org/a".to_string(),
            },
        ];

        let bare = bare_url_references(&descriptors);

        assert_eq!(bare.len(), 2);
        assert_eq!(bare[0].ordinal, 0);
        assert_eq!(bare[1].ordinal, 2);
        assert_eq!(bare[0].url, "https://example.org/a");
        assert_eq!(bare[1].anchor_text, "https://example.org/a");
    }
}
