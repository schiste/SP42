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
    use sp42_platform::WikiConfig;

    pub(crate) fn fixture_wiki_config() -> WikiConfig {
        sp42_wiki::test_fixtures::frwiki_config()
    }
}
