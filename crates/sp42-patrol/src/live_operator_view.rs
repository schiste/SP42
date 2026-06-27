//! Browser-facing live operator payload that includes reporting models.

use serde::{Deserialize, Serialize};
use sp42_coordination::{CoordinationRoomSummary, CoordinationStateSummary};
use sp42_live::{
    BacklogRuntimeStatus, LiveIngestionSupervisorStatus, LiveOperatorActionPreflight,
    LiveOperatorBackendStatus, LiveOperatorHeuristicProvenance, LiveOperatorPublicDocuments,
    LiveOperatorQuery, LiveOperatorTelemetry, StreamRuntimeStatus,
};
use sp42_platform::{
    ActionExecutionHistoryReport, ActionExecutionStatusReport, DevAuthCapabilityReport,
    DevAuthSessionStatus, MediaDiffReport, QueuedEdit, ReviewWorkbench, ScoringContext,
    StructuredDiff,
};

use crate::{DebugSnapshot, PatrolScenarioReport, PatrolSessionDigest, ShellStateModel};

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
    pub media_diff: Option<MediaDiffReport>,
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
    pub action_preflight: LiveOperatorActionPreflight,
    #[serde(default)]
    pub public_documents: LiveOperatorPublicDocuments,
    #[serde(default)]
    pub heuristic_provenance: Vec<LiveOperatorHeuristicProvenance>,
    pub selected_heuristic_provenance: Option<LiveOperatorHeuristicProvenance>,
    pub ingestion_supervisor: Option<LiveIngestionSupervisorStatus>,
    pub coordination_room: Option<CoordinationRoomSummary>,
    pub coordination_state: Option<CoordinationStateSummary>,
    #[serde(default)]
    pub debug_snapshot: DebugSnapshot,
    #[serde(default)]
    pub telemetry: LiveOperatorTelemetry,
    pub notes: Vec<String>,
    pub next_continue: Option<String>,
}
