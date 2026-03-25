//! Shared live operator payload for the browser review surface.

use serde::{Deserialize, Serialize};

use crate::{
    ActionExecutionHistoryReport, ActionExecutionStatusReport, BacklogRuntimeStatus,
    CoordinationRoomSummary, CoordinationStateSummary, DebugSnapshot, DevAuthCapabilityReport,
    DevAuthSessionStatus, LocalOAuthConfigStatus, LocalOAuthSourceReport, PatrolScenarioReport,
    PatrolSessionDigest, QueuedEdit, ReviewWorkbench, ScoringContext, ShellStateModel,
    StreamRuntimeStatus, StructuredDiff,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct LiveOperatorBackendStatus {
    pub ready_for_local_testing: bool,
    pub readiness_issues: Vec<String>,
    pub bootstrap_ready: bool,
    pub oauth: LocalOAuthConfigStatus,
    pub session: DevAuthSessionStatus,
    pub source_report: LocalOAuthSourceReport,
    pub capability_cache_present: bool,
    pub capability_cache_fresh: bool,
    pub capability_cache_age_ms: Option<u64>,
    pub capability_cache_wiki_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LiveOperatorQuery {
    pub limit: u16,
    #[serde(default)]
    pub include_bots: bool,
    #[serde(default)]
    pub unpatrolled_only: bool,
    #[serde(default)]
    pub include_minor: bool,
    #[serde(default)]
    pub namespaces: Vec<i32>,
    pub min_score: Option<i32>,
    pub rccontinue: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveOperatorPhaseTiming {
    pub phase: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LiveOperatorTelemetry {
    pub total_duration_ms: u64,
    #[serde(default)]
    pub phase_timings: Vec<LiveOperatorPhaseTiming>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveOperatorView {
    pub project: String,
    pub fetched_at_ms: i64,
    pub wiki_id: String,
    pub query: LiveOperatorQuery,
    pub queue: Vec<QueuedEdit>,
    pub selected_index: Option<usize>,
    pub scoring_context: Option<ScoringContext>,
    pub diff: Option<StructuredDiff>,
    pub review_workbench: Option<ReviewWorkbench>,
    pub stream_status: Option<StreamRuntimeStatus>,
    pub backlog_status: Option<BacklogRuntimeStatus>,
    pub scenario_report: PatrolScenarioReport,
    pub session_digest: PatrolSessionDigest,
    pub shell_state: ShellStateModel,
    pub capabilities: DevAuthCapabilityReport,
    pub auth: DevAuthSessionStatus,
    pub backend: LiveOperatorBackendStatus,
    pub action_status: ActionExecutionStatusReport,
    pub action_history: ActionExecutionHistoryReport,
    pub coordination_room: Option<CoordinationRoomSummary>,
    pub coordination_state: Option<CoordinationStateSummary>,
    #[serde(default)]
    pub debug_snapshot: DebugSnapshot,
    #[serde(default)]
    pub telemetry: LiveOperatorTelemetry,
    pub notes: Vec<String>,
    pub next_continue: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::LiveOperatorBackendStatus;
    use crate::{DevAuthSessionStatus, LocalOAuthConfigStatus, LocalOAuthSourceReport};

    #[test]
    fn live_operator_backend_status_serializes_authoritative_local_env_state() {
        let status = LiveOperatorBackendStatus {
            ready_for_local_testing: true,
            readiness_issues: vec!["capability cache cold".to_string()],
            bootstrap_ready: true,
            oauth: LocalOAuthConfigStatus {
                client_id_present: true,
                client_secret_present: true,
                access_token_present: true,
            },
            session: DevAuthSessionStatus {
                authenticated: true,
                username: Some("Tester".to_string()),
                scopes: vec!["basic".to_string()],
                expires_at_ms: Some(123),
                token_present: true,
                bridge_mode: "local-env-token".to_string(),
                local_token_available: true,
            },
            source_report: LocalOAuthSourceReport {
                file_name: ".env.wikimedia.local".to_string(),
                source_path: Some("/tmp/.env.wikimedia.local".to_string()),
                loaded_from_source: true,
            },
            capability_cache_present: true,
            capability_cache_fresh: true,
            capability_cache_age_ms: Some(5),
            capability_cache_wiki_id: Some("frwiki".to_string()),
        };
        let encoded = serde_json::to_value(&status).expect("status should serialize");
        assert_eq!(
            encoded
                .get("source_report")
                .and_then(|value| value.get("loaded_from_source"))
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
        assert_eq!(
            encoded
                .get("capability_cache_present")
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
    }
}
