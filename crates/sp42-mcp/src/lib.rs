//! Agent-facing contract types for the SP42 citation-verification MCP surface (PRD-0010).
//!
//! This crate is **host-only** — it is not part of the `wasm32` `sp42-app` build. Phase 1
//! (this module) defines the typed request/response shapes the MCP verbs exchange, reusing the
//! governed verdict types from `sp42-core` rather than redefining them. Transport (`rmcp`) and
//! the verb handlers themselves arrive in later phases.
//!
//! Signatures here are **proposed** (PRD-0010 §Proposal) and may be revised as the verbs are
//! implemented; the one fixed contract is that verdicts ride the unchanged ADR-0007/0008
//! taxonomy.

use serde::{Deserialize, Serialize};

mod http;
mod page;
mod probe;
mod server;
mod verify;
mod wikidata;

pub use http::GuardedHttpClient;
pub use page::{PageFinding, PageInput, PageVerifyResult, verify_wikipedia_page};
pub use probe::probe_source;
pub use server::Sp42McpServer;
pub use verify::verify_claim;
pub use wikidata::verify_wikidata_statement;

// Re-export the governed core types the contract embeds, so consumers get one import surface
// and cannot drift from the ADR-0007/0008 verdict taxonomy.
pub use sp42_citation::{BodyUsabilityReason, PanelAgreement, Verdict};

/// The source a claim is verified against.
///
/// Either a URL for SP42 to fetch through its hardened pipeline, or content the caller already
/// fetched — so a caller that just expanded a bare URL (via Citoid) or pulled an archive
/// snapshot is not forced into a re-fetch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Source {
    /// A URL for SP42 to fetch and extract.
    Url {
        /// The source URL.
        url: String,
    },
    /// Caller-supplied source content, bypassing the fetch.
    Text {
        /// The already-fetched source text.
        text: String,
        /// Optional provenance: where the caller obtained the text.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        retrieved_from: Option<String>,
    },
}

/// Result of `probe_source`: deterministic, model-free source accessibility.
///
/// Distinguishes *unreachable* from *reachable-but-unextractable*, so a consumer learns that a
/// human could still read a source the automated pipeline cannot use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeResult {
    /// `true` iff the URL was fetched with a 2xx status.
    pub reachable: bool,
    /// The observed HTTP status, when a response was received.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    /// `true` iff SP42's pipeline could extract usable article text.
    pub extractable: bool,
    /// Why the body was unusable, when reachable but not extractable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unusable_reason: Option<BodyUsabilityReason>,
    /// `true` when the source is reachable but not pipeline-extractable — i.e. a human could
    /// still read it even though the model cannot.
    pub human_readable_hint: bool,
}

/// Optional overrides for the model panel.
///
/// Empty fields fall back to the server's configured panel (the `SP42_INFERENCE_*` seam).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PanelConfig {
    /// Explicit model ids to vote; empty = the server's configured default panel.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<String>,
}

/// A reference to the Wikidata statement to verify.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct StatementRef {
    /// The Wikidata entity id, e.g. `Q42`.
    pub entity: String,
    /// The property id, e.g. `P569`.
    pub property: String,
    /// An optional specific statement GUID, when the entity has several statements for the
    /// property.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statement_id: Option<String>,
}

/// Result of `verify_claim`: the governed verdict plus its evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifyResult {
    /// The four-value support verdict (ADR-0007/0008), unchanged from the core taxonomy.
    pub verdict: Verdict,
    /// The supporting quote, re-located verbatim in the source; `None` when none was located.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote: Option<String>,
    /// Measured panel agreement, when a multi-model panel voted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agreement: Option<PanelAgreement>,
}

/// Result of `verify_wikidata_statement`: the rendered claim, its reference URL, and the verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatementVerifyResult {
    /// The statement rendered as a natural-language claim — returned for inspection, since
    /// rendering is best-effort (PRD-0010 open question #1).
    pub claim_rendered: String,
    /// The P854 reference URL the statement was verified against, when the statement carries one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ref_url: Option<String>,
    /// The verification result. `SourceUnavailable` when the statement has no reference URL.
    pub result: VerifyResult,
}

#[cfg(test)]
mod tests {
    use super::{
        BodyUsabilityReason, PanelAgreement, PanelConfig, ProbeResult, Source, StatementRef,
        Verdict, VerifyResult,
    };

    #[test]
    fn source_url_round_trips_to_tagged_form() {
        let s = Source::Url {
            url: "https://example.org/a".to_owned(),
        };
        let json = serde_json::to_string(&s).expect("serialize");
        assert_eq!(json, r#"{"type":"url","url":"https://example.org/a"}"#);
        assert_eq!(
            serde_json::from_str::<Source>(&json).expect("deserialize"),
            s
        );
    }

    #[test]
    fn source_text_round_trips_with_and_without_provenance() {
        let with = Source::Text {
            text: "body".to_owned(),
            retrieved_from: Some("https://archive.example/snap".to_owned()),
        };
        let j = serde_json::to_string(&with).expect("serialize");
        assert_eq!(
            serde_json::from_str::<Source>(&j).expect("deserialize"),
            with
        );

        let without = Source::Text {
            text: "body".to_owned(),
            retrieved_from: None,
        };
        let j2 = serde_json::to_string(&without).expect("serialize");
        assert!(!j2.contains("retrieved_from"));
        assert_eq!(
            serde_json::from_str::<Source>(&j2).expect("deserialize"),
            without
        );
    }

    #[test]
    fn probe_result_round_trips_and_omits_none_reason() {
        let unusable = ProbeResult {
            reachable: true,
            http_status: Some(200),
            extractable: false,
            unusable_reason: Some(BodyUsabilityReason::AntiBotChallenge),
            human_readable_hint: true,
        };
        let j = serde_json::to_string(&unusable).expect("serialize");
        assert!(j.contains(r#""unusable_reason":"anti_bot_challenge""#));
        assert_eq!(
            serde_json::from_str::<ProbeResult>(&j).expect("deserialize"),
            unusable
        );

        let clean = ProbeResult {
            reachable: true,
            http_status: Some(200),
            extractable: true,
            unusable_reason: None,
            human_readable_hint: false,
        };
        let jc = serde_json::to_string(&clean).expect("serialize");
        assert!(!jc.contains("unusable_reason"));
        assert_eq!(
            serde_json::from_str::<ProbeResult>(&jc).expect("deserialize"),
            clean
        );
    }

    #[test]
    fn verify_result_embeds_wire_verdict_and_agreement() {
        let v = VerifyResult {
            verdict: Verdict::Supported,
            quote: Some("the located quote".to_owned()),
            agreement: Some(PanelAgreement::new(3, 2)),
        };
        let j = serde_json::to_string(&v).expect("serialize");
        assert!(j.contains(r#""verdict":"supported""#));
        assert!(j.contains(r#""agreement":{"panel_size":3,"winner_votes":2}"#));
        assert_eq!(
            serde_json::from_str::<VerifyResult>(&j).expect("deserialize"),
            v
        );
    }

    #[test]
    fn verify_result_omits_absent_quote_and_agreement() {
        let v = VerifyResult {
            verdict: Verdict::SourceUnavailable,
            quote: None,
            agreement: None,
        };
        let j = serde_json::to_string(&v).expect("serialize");
        assert_eq!(j, r#"{"verdict":"source_unavailable"}"#);
        assert_eq!(
            serde_json::from_str::<VerifyResult>(&j).expect("deserialize"),
            v
        );
    }

    #[test]
    fn statement_ref_round_trips() {
        let s = StatementRef {
            entity: "Q42".to_owned(),
            property: "P569".to_owned(),
            statement_id: None,
        };
        let j = serde_json::to_string(&s).expect("serialize");
        assert_eq!(
            serde_json::from_str::<StatementRef>(&j).expect("deserialize"),
            s
        );
    }

    #[test]
    fn panel_config_default_serializes_empty() {
        let c = PanelConfig::default();
        let j = serde_json::to_string(&c).expect("serialize");
        assert_eq!(j, "{}");
        assert_eq!(
            serde_json::from_str::<PanelConfig>("{}").expect("deserialize"),
            c
        );
    }
}
