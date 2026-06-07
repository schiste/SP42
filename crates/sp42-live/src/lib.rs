#![forbid(unsafe_code)]

//! Live patrol ingestion, backlog polling, queue filtering, and operator contracts.
//!
//! Shell crates own HTTP routing, browser rendering, and runtime orchestration.
//! This crate owns deterministic live-domain behavior shared across those shells.

pub mod backlog_runtime;
pub mod live_operator;
pub mod recent_changes;
pub mod stream_ingestor;
pub mod stream_runtime;

pub use backlog_runtime::{BacklogRuntime, BacklogRuntimeConfig, BacklogRuntimeStatus};
pub use live_operator::{
    DEFAULT_LIVE_OPERATOR_LIMIT, LiveIngestionSupervisorStatus, LiveOperatorActionPreflight,
    LiveOperatorActionRecommendation, LiveOperatorBackendStatus, LiveOperatorHeuristicProvenance,
    LiveOperatorPhaseTiming, LiveOperatorPublicDocuments, LiveOperatorQuery,
    LiveOperatorRetryClass, LiveOperatorTelemetry, MAX_LIVE_OPERATOR_LIMIT,
    build_live_operator_action_preflight, classify_retry, filter_live_operator_queue,
    live_operator_query_matches,
};
pub use recent_changes::{
    RecentChangesBatch, RecentChangesQuery, build_recent_changes_request, execute_recent_changes,
    parse_recent_changes_response,
};
pub use stream_ingestor::StreamIngestor;
pub use stream_runtime::{StreamRuntime, StreamRuntimeStatus};

#[cfg(test)]
pub(crate) mod test_fixtures {
    use sp42_core::{WikiConfig, WikiTemplates, scoring_policy};

    pub(crate) fn fixture_wiki_config() -> WikiConfig {
        let compiled =
            scoring_policy::load_embedded_compiled_scoring_policy("active/frwiki-vandalism")
                .expect("embedded frwiki scoring policy should compile");

        WikiConfig {
            wiki_id: "frwiki".to_string(),
            display_name: "French Wikipedia".to_string(),
            api_url: "https://fr.wikipedia.org/w/api.php"
                .parse()
                .expect("fixture api_url should parse"),
            eventstreams_url: "https://stream.wikimedia.org/v2/stream/recentchange"
                .parse()
                .expect("fixture eventstreams_url should parse"),
            oauth_authorize_url: "https://meta.wikimedia.org/w/rest.php/oauth2/authorize"
                .parse()
                .expect("fixture oauth_authorize_url should parse"),
            oauth_token_url: "https://meta.wikimedia.org/w/rest.php/oauth2/access_token"
                .parse()
                .expect("fixture oauth_token_url should parse"),
            liftwing_url: Some(
                "https://api.wikimedia.org/service/lw/inference/v1/models/revertrisk-language-agnostic:predict"
                    .parse()
                    .expect("fixture liftwing_url should parse"),
            ),
            coordination_url: None,
            namespace_allowlist: vec![0, 2, 4, 6, 10, 14],
            scoring_policy_ref: "active/frwiki-vandalism".to_string(),
            scoring: compiled.scoring_config.clone(),
            templates: WikiTemplates::default(),
        }
    }
}
