//! Bare-URL reference repair (PRD-0008): pure classification of bare-URL
//! references, rendering Citoid metadata into a citation template, and the
//! propose/confirm wire contracts shared by the server routes and the CLI.
//!
//! FCIS (Constitution Art. 2): everything here is pure and clock-free. The
//! server shell fetches Citoid metadata and passes the fetch date in; the
//! apply path replays a proposal verbatim through the node-anchored editor
//! (ADR-0003), inheriting its anti-drift and zero-write refusal guarantees.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::citation::citoid::CitoidMetadata;
use crate::wikitext_editor::{WikitextNodeDescriptor, WikitextNodeKind, WikitextNodeLocator};

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

/// Why a bare-URL reference did not receive a proposal.
///
/// Declines are structured outcomes, never errors: a junk URL cannot fail a
/// whole proposal response, and the reference simply stays a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BareUrlDeclineReason {
    /// Citoid metadata could not be fetched or parsed (transport failure,
    /// 520/404/non-200 status, or an unparseable/empty body).
    MetadataUnavailable,
    /// Metadata arrived but had no title, or the title merely echoed the URL
    /// (Citoid's documented scrape-failure fallback).
    NoUsableTitle,
}

impl BareUrlDeclineReason {
    /// Stable wire code for the decline reason.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::MetadataUnavailable => "metadata-unavailable",
            Self::NoUsableTitle => "no-usable-title",
        }
    }
}

/// Outcome of rendering one bare-URL reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BareUrlOutcome {
    /// A repair is proposed.
    Proposed {
        /// Replacement contents for the `<ref>` element — the bare template
        /// call, with no `<ref>` tags (the editor replaces ref *contents*,
        /// preserving the element and its attributes).
        replacement_wikitext: String,
    },
    /// The reference keeps its bare URL.
    Declined {
        /// Why no proposal was produced.
        reason: BareUrlDeclineReason,
    },
}

/// Render the wiki's citation template for one bare-URL reference, or decline.
///
/// `template_name` comes from `WikiTemplates::bare_url_citation`. `metadata`
/// is the lifted `build_citoid_header` output (`None` declines as
/// metadata-unavailable). `language` is the raw response's `language` field
/// (see [`citoid_language`]); the default map treats English as the wiki's
/// own language (testwiki MVP) and omits it. `access_date_iso` is the
/// shell-provided fetch date — the core stays clock-free.
#[must_use]
pub fn render_bare_url_citation(
    template_name: &str,
    metadata: Option<&CitoidMetadata>,
    language: Option<&str>,
    access_date_iso: &str,
) -> BareUrlOutcome {
    let Some(metadata) = metadata else {
        return BareUrlOutcome::Declined {
            reason: BareUrlDeclineReason::MetadataUnavailable,
        };
    };
    let Some(title) = metadata
        .title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
    else {
        return BareUrlOutcome::Declined {
            reason: BareUrlDeclineReason::NoUsableTitle,
        };
    };
    if title == metadata.url.trim() {
        return BareUrlOutcome::Declined {
            reason: BareUrlDeclineReason::NoUsableTitle,
        };
    }

    let mut parameters: Vec<(&str, String)> = vec![
        ("url", metadata.url.clone()),
        ("title", title.to_string()),
    ];
    if let Some(publication) = &metadata.publication {
        parameters.push(("website", publication.clone()));
    }
    if let Some(author) = &metadata.author {
        parameters.push(("author", author.clone()));
    }
    if let Some(published) = &metadata.published {
        parameters.push(("date", published.clone()));
    }
    parameters.push(("access-date", access_date_iso.to_string()));
    if let Some(language) = language
        .map(str::trim)
        .filter(|language| !language.is_empty() && !is_own_language(language))
    {
        parameters.push(("language", language.to_string()));
    }

    let body = parameters
        .iter()
        .map(|(name, value)| format!("|{name}={}", sanitize_template_value(value)))
        .collect::<Vec<_>>()
        .join(" ");
    BareUrlOutcome::Proposed {
        replacement_wikitext: format!("{{{{{template_name} {body}}}}}"),
    }
}

/// Pure helper for the shell: the `language` field of a parsed Citoid object.
#[must_use]
pub fn citoid_language(raw: &Map<String, Value>) -> Option<String> {
    raw.get("language")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

/// The default map hardcodes English as the wiki's own language (testwiki
/// MVP); per-wiki own-language arrives with the frwiki-enablement follow-on.
fn is_own_language(language: &str) -> bool {
    let lowered = language.to_ascii_lowercase();
    lowered == "en" || lowered.starts_with("en-")
}

/// Keep a metadata value from breaking out of the template: pipes become
/// `{{!}}` and newlines collapse to spaces.
fn sanitize_template_value(value: &str) -> String {
    value.replace('|', "{{!}}").replace(['\n', '\r'], " ")
}

/// One replayable bare-URL repair proposal (wire type, PRD-0008).
///
/// The locator (`kind`/`ordinal`/`expected_text`) plus the replacement wikitext is
/// a complete, drift-guarded edit payload: apply replays it verbatim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BareUrlProposal {
    /// Drift-guarded address of the reference (`kind` is always `reference`).
    pub locator: WikitextNodeLocator,
    /// The bare URL being repaired.
    pub url: String,
    /// The reference's current anchor text — the operator's "before" view.
    pub current_anchor: String,
    /// Proposed replacement contents for the `<ref>` element.
    pub replacement_wikitext: String,
}

/// Request body for `POST /dev/citation/bare-url-proposals`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BareUrlProposalsRequest {
    /// Wiki the revision belongs to (must be enabled via config).
    pub wiki_id: String,
    /// Page title.
    pub title: String,
    /// Revision the proposals are generated against (`baserevid` at apply).
    pub rev_id: u64,
}

/// One declined reference in the proposals response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BareUrlDeclined {
    /// Zero-based document-order position among `Reference` nodes.
    pub ordinal: usize,
    /// The reference's bare URL.
    pub url: String,
    /// Why no proposal was produced.
    pub reason: BareUrlDeclineReason,
}

/// Response body for `POST /dev/citation/bare-url-proposals`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BareUrlProposalsResponse {
    /// Replayable proposals, in document order.
    pub proposals: Vec<BareUrlProposal>,
    /// Bare references that declined, with reasons.
    #[serde(default)]
    pub declined: Vec<BareUrlDeclined>,
}

/// Request body for `POST /dev/citation/bare-url-apply`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BareUrlApplyRequest {
    /// Wiki the revision belongs to (must be enabled via config).
    pub wiki_id: String,
    /// Page title.
    pub title: String,
    /// Revision the proposal was generated against (sent as `baserevid`).
    pub rev_id: u64,
    /// The proposal's locator, replayed verbatim.
    pub locator: WikitextNodeLocator,
    /// The proposal's replacement wikitext, replayed verbatim.
    pub replacement_wikitext: String,
    /// Optional operator note; wins over the default edit summary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

/// Response body for `POST /dev/citation/bare-url-apply` — the
/// execute-action outcome shape, minus the session-action `kind`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BareUrlApplyResponse {
    /// Wiki the edit was applied to.
    pub wiki_id: String,
    /// Revision the apply was guarded against.
    pub rev_id: u64,
    /// Whether the wiki accepted the edit (false on no-change).
    pub accepted: bool,
    /// Acting operator username.
    pub actor: Option<String>,
    /// Upstream `MediaWiki` HTTP status.
    pub http_status: Option<u16>,
    /// Upstream `MediaWiki` API error code, when present.
    pub api_code: Option<String>,
    /// Whether a failed apply is retryable.
    #[serde(default)]
    pub retryable: bool,
    /// Upstream warnings (including the no-change advisory).
    #[serde(default)]
    pub warnings: Vec<String>,
    /// Upstream API result token, when present.
    pub result: Option<String>,
    /// Human-readable outcome summary.
    pub message: Option<String>,
}

/// Civil ISO `YYYY-MM-DD` (UTC) for an epoch timestamp in milliseconds.
///
/// Days-from-epoch → civil conversion (Howard Hinnant's algorithm), pure so
/// the shell can pass `clock.now_ms()` and the core stays clock-free
/// (Constitution Art. 1.4).
#[must_use]
pub fn iso_date_from_epoch_ms(epoch_ms: i64) -> String {
    let days = epoch_ms.div_euclid(86_400_000);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
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

    fn rendered(fixture: &str, source_url: &str) -> BareUrlOutcome {
        let raw = crate::citation::citoid::parse_citoid_response(fixture.as_bytes())
            .expect("fixture should parse as a citoid array");
        let metadata = crate::citation::citoid::build_citoid_header(&raw, source_url);
        let language = citoid_language(&raw);
        render_bare_url_citation("cite web", metadata.as_ref(), language.as_deref(), "2026-06-09")
    }

    #[test]
    fn renders_every_field_from_the_basic_fixture() {
        let outcome = rendered(
            include_str!("../../../fixtures/citoid/basic.json"),
            "https://example.org/article",
        );
        let expected = "{{cite web |url=https://example.org/article |title=Headline |website=The Guardian |author=Jane Doe, John Smith |date=2020-01-01 |access-date=2026-06-09 |language=fr}}";
        assert_eq!(
            outcome,
            BareUrlOutcome::Proposed { replacement_wikitext: expected.to_string() }
        );
    }

    #[test]
    fn website_falls_back_and_partial_dates_pass_through() {
        let BareUrlOutcome::Proposed { replacement_wikitext } = rendered(
            include_str!("../../../fixtures/citoid/partial_date.json"),
            "https://example.org/report",
        ) else {
            panic!("partial-date fixture should produce a proposal");
        };
        assert!(replacement_wikitext.contains("|website=Example Site"));
        assert!(replacement_wikitext.contains("|date=2024-05"));
        assert!(!replacement_wikitext.contains("|language="));
    }

    #[test]
    fn creators_fallback_formats_authors() {
        let BareUrlOutcome::Proposed { replacement_wikitext } = rendered(
            include_str!("../../../fixtures/citoid/creators_fallback.json"),
            "https://example.org/mixed",
        ) else {
            panic!("creators fixture should produce a proposal");
        };
        assert!(replacement_wikitext.contains("|author=Ada Lovelace"));
    }

    #[test]
    fn declines_when_metadata_is_unavailable() {
        let outcome = render_bare_url_citation("cite web", None, None, "2026-06-09");
        assert_eq!(
            outcome,
            BareUrlOutcome::Declined { reason: BareUrlDeclineReason::MetadataUnavailable }
        );
    }

    #[test]
    fn declines_without_a_usable_title() {
        let outcome = rendered(
            include_str!("../../../fixtures/citoid/no_title.json"),
            "https://example.org/untitled",
        );
        assert_eq!(
            outcome,
            BareUrlOutcome::Declined { reason: BareUrlDeclineReason::NoUsableTitle }
        );
    }

    #[test]
    fn declines_when_the_title_echoes_the_url() {
        let outcome = rendered(
            include_str!("../../../fixtures/citoid/degenerate_title_url.json"),
            "https://degenerate.example/x",
        );
        assert_eq!(
            outcome,
            BareUrlOutcome::Declined { reason: BareUrlDeclineReason::NoUsableTitle }
        );
    }

    #[test]
    fn own_language_is_omitted_and_foreign_language_kept() {
        let metadata = CitoidMetadata {
            publication: None,
            published: None,
            author: None,
            title: Some("Headline".to_string()),
            url: "https://example.org/a".to_string(),
        };
        let en = render_bare_url_citation("cite web", Some(&metadata), Some("en"), "2026-06-09");
        let en_us =
            render_bare_url_citation("cite web", Some(&metadata), Some("en-US"), "2026-06-09");
        let fr = render_bare_url_citation("cite web", Some(&metadata), Some("fr"), "2026-06-09");
        let BareUrlOutcome::Proposed { replacement_wikitext: en } = en else { panic!("en") };
        let BareUrlOutcome::Proposed { replacement_wikitext: en_us } = en_us else {
            panic!("en-US")
        };
        let BareUrlOutcome::Proposed { replacement_wikitext: fr } = fr else { panic!("fr") };
        assert!(!en.contains("|language="));
        assert!(!en_us.contains("|language="));
        assert!(fr.contains("|language=fr"));
    }

    #[test]
    fn renders_the_configured_template_name() {
        let metadata = CitoidMetadata {
            publication: None,
            published: None,
            author: None,
            title: Some("Titre".to_string()),
            url: "https://example.org/fr".to_string(),
        };
        let BareUrlOutcome::Proposed { replacement_wikitext } =
            render_bare_url_citation("Lien web", Some(&metadata), None, "2026-06-09")
        else {
            panic!("should propose");
        };
        assert!(replacement_wikitext.starts_with("{{Lien web |url="));
    }

    #[test]
    fn sanitizes_pipes_and_newlines_in_values() {
        let metadata = CitoidMetadata {
            publication: None,
            published: None,
            author: None,
            title: Some("A|B\nC".to_string()),
            url: "https://example.org/p".to_string(),
        };
        let BareUrlOutcome::Proposed { replacement_wikitext } =
            render_bare_url_citation("cite web", Some(&metadata), None, "2026-06-09")
        else {
            panic!("should propose");
        };
        assert!(replacement_wikitext.contains("|title=A{{!}}B C"));
    }

    #[test]
    fn citoid_language_reads_the_language_field() {
        let with = serde_json::json!({ "language": "fr" });
        let without = serde_json::json!({ "title": "x" });
        let empty = serde_json::json!({ "language": "  " });
        assert_eq!(
            citoid_language(with.as_object().expect("object")).as_deref(),
            Some("fr")
        );
        assert_eq!(citoid_language(without.as_object().expect("object")), None);
        assert_eq!(citoid_language(empty.as_object().expect("object")), None);
    }

    #[test]
    fn iso_date_from_epoch_ms_converts_civil_dates() {
        assert_eq!(iso_date_from_epoch_ms(0), "1970-01-01");
        assert_eq!(iso_date_from_epoch_ms(951_782_400_000), "2000-02-29");
        assert_eq!(iso_date_from_epoch_ms(1_780_963_200_000), "2026-06-09");
        // Mid-day timestamps land on the same UTC date.
        assert_eq!(iso_date_from_epoch_ms(1_780_963_200_000 + 43_200_000), "2026-06-09");
    }

    #[test]
    fn wire_types_round_trip_and_use_kebab_reason_codes() {
        use crate::wikitext_editor::WikitextNodeLocator;

        let proposal = BareUrlProposal {
            locator: WikitextNodeLocator {
                kind: WikitextNodeKind::Reference,
                ordinal: 2,
                expected_text: "https://example.org/a".to_string(),
            },
            url: "https://example.org/a".to_string(),
            current_anchor: "https://example.org/a".to_string(),
            replacement_wikitext: "{{cite web |url=https://example.org/a |title=T |access-date=2026-06-09}}".to_string(),
        };
        let response = BareUrlProposalsResponse {
            proposals: vec![proposal],
            declined: vec![BareUrlDeclined {
                ordinal: 4,
                url: "https://degenerate.example/x".to_string(),
                reason: BareUrlDeclineReason::NoUsableTitle,
            }],
        };

        let json = serde_json::to_value(&response).expect("response should serialize");
        assert_eq!(json["declined"][0]["reason"], "no-usable-title");
        assert_eq!(json["proposals"][0]["locator"]["kind"], "reference");

        let back: BareUrlProposalsResponse =
            serde_json::from_value(json).expect("response should deserialize");
        assert_eq!(back, response);
    }
}
