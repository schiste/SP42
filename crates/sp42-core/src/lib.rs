#![forbid(unsafe_code)]

//! Shared SP42 domain contracts and pure platform-independent logic.
//!
//! ```
//! use sp42_core::{parse_wiki_config, branding::PROJECT_NAME};
//!
//! let config = parse_wiki_config(
//!     r#"
//! wiki_id: frwiki
//! display_name: French Wikipedia
//! api_url: https://fr.wikipedia.org/w/api.php
//! eventstreams_url: https://stream.wikimedia.org/v2/stream/recentchange
//! oauth_authorize_url: https://meta.wikimedia.org/w/rest.php/oauth2/authorize
//! oauth_token_url: https://meta.wikimedia.org/w/rest.php/oauth2/access_token
//! namespace_allowlist: [0]
//! scoring:
//!   base_score: 0
//!   max_score: 100
//!   weights:
//!     anonymous_user: 25
//!     new_page: 20
//!     reverted_before: 30
//!     large_content_removal: 15
//!     profanity: 30
//!     link_spam: 20
//!     trusted_user: -40
//!     bot_like_edit: -50
//!     liftwing_risk: 35
//!     warning_history: 25
//! "#,
//! )
//! .expect("the inline fixture is valid");
//!
//! assert_eq!(PROJECT_NAME, "SP42");
//! assert_eq!(config.wiki_id, "frwiki");
//! ```

pub mod action_executor;
pub mod backlog_runtime;
pub mod branding;
pub mod config_parser;
pub mod context_builder;
pub mod coordination_client;
pub mod coordination_codec;
pub mod coordination_runtime;
pub mod coordination_state;
pub mod debug_snapshot;
pub mod dev_auth;
pub mod diff_engine;
pub mod errors;
pub mod liftwing;
pub mod live_operator;
pub mod oauth;
pub mod operator_summary;
pub mod patrol_scenario_report;
pub mod patrol_session_digest;
pub mod priority_queue;
pub mod public_documents;
pub mod queue_builder;
pub mod recent_changes;
pub mod report_document;
pub mod review_workbench;
pub mod scoring_engine;
pub mod shell_state;
pub mod stream_ingestor;
pub mod stream_runtime;
pub mod training_data;
pub mod traits;
pub mod types;
pub mod user_analyzer;
pub mod wiki_storage;

pub use action_executor::{
    ActionResponseSummary, PatrolRequest, RollbackRequest, SessionActionExecutionRequest,
    SessionActionExecutionResponse, SessionActionKind, TokenKind, UndoRequest, WikiPageSaveRequest,
    build_patrol_request, build_rollback_request, build_token_request, build_undo_request,
    build_wiki_page_save_request, execute_fetch_token, execute_patrol, execute_rollback,
    execute_undo, execute_wiki_page_save, parse_action_response_summary, parse_token_response,
};
pub use backlog_runtime::{BacklogRuntime, BacklogRuntimeConfig, BacklogRuntimeStatus};
pub use config_parser::parse_wiki_config;
pub use context_builder::{ContextInputs, build_scoring_context};
pub use coordination_client::CoordinationClient;
pub use coordination_codec::{decode_message, encode_message};
pub use coordination_runtime::{CoordinationRuntime, CoordinationRuntimeStatus};
pub use coordination_state::{CoordinationState, CoordinationStateSummary};
pub use debug_snapshot::{
    DebugSnapshot, DebugSnapshotInputs, DecisionTrace, PerformanceMarker, TraceLevel,
    build_debug_snapshot,
};
pub use dev_auth::{
    ActionExecutionHistoryReport, ActionExecutionLogEntry, ActionExecutionStatusReport,
    DEV_AUTH_ACTION_HISTORY_PATH, DEV_AUTH_ACTION_STATUS_PATH, DEV_AUTH_BOOTSTRAP_SESSION_PATH,
    DEV_AUTH_DEFAULT_BASE_URL, DEV_AUTH_SESSION_PATH, DevAuthActionTokenAvailability,
    DevAuthBootstrapRequest, DevAuthCapabilityReadiness, DevAuthCapabilityReport,
    DevAuthDerivedCapabilities, DevAuthEditCapabilities, DevAuthModerationCapabilities,
    DevAuthProbeAcceptance, DevAuthSessionStatus, LocalOAuthConfigStatus,
    build_dev_auth_bootstrap_request, build_dev_auth_clear_request, parse_action_execution_history,
    parse_action_execution_status, parse_dev_auth_status,
};
pub use diff_engine::{
    DiffSegment, DiffSegmentKind, DiffStats, StructuredDiff, diff_chars, diff_lines,
};
pub use errors::{
    ActionError, BacklogRuntimeError, CodecError, ConfigError, CoordinationError, DevAuthError,
    DiffError, EventSourceError, HttpClientError, LiftWingError, OAuthError, PublicDocumentError,
    RecentChangesError, ReviewWorkbenchError, ScoringError, StorageError, StreamIngestorError,
    StreamRuntimeError, TrainingDataError, UserAnalysisError, WebSocketError, WikiStorageError,
};
pub use liftwing::{
    LiftWingRequest, build_liftwing_score_request, execute_liftwing_score,
    parse_liftwing_score_response,
};
pub use live_operator::{
    LiveIngestionSupervisorStatus, LiveOperatorActionPreflight, LiveOperatorActionRecommendation,
    LiveOperatorBackendStatus, LiveOperatorHeuristicProvenance, LiveOperatorPhaseTiming,
    LiveOperatorPublicDocuments, LiveOperatorQuery, LiveOperatorRetryClass, LiveOperatorTelemetry,
    LiveOperatorView, build_live_operator_action_preflight, classify_retry,
};
pub use oauth::{
    OAuthCallback, OAuthClientConfig, OAuthLaunchContext, OAuthTokenResponse,
    build_access_token_request, build_authorization_url, code_challenge_from_verifier,
    generate_oauth_state, generate_pkce_verifier, parse_callback_query, prepare_oauth_launch,
    prepare_token_exchange_from_callback, validate_code_verifier,
};
pub use operator_summary::{
    PatrolOperatorSectionSummary, PatrolOperatorSummary, PatrolOperatorSummaryInputs,
    build_patrol_operator_summary, render_patrol_operator_summary_markdown,
    render_patrol_operator_summary_text,
};
pub use patrol_scenario_report::{
    PatrolScenarioFinding, PatrolScenarioReadiness, PatrolScenarioReport,
    PatrolScenarioReportInputs, PatrolScenarioSection, PatrolScenarioSelectedEdit, ReportSeverity,
    build_patrol_scenario_report, render_patrol_scenario_markdown, render_patrol_scenario_text,
};
pub use patrol_session_digest::{
    PatrolSessionDigest, PatrolSessionDigestInputs, PatrolSessionSectionSummary,
    PatrolSessionSelectedSummary, PatrolSessionSeverityCount, PatrolSessionWorkbenchSummary,
    build_patrol_session_digest, render_patrol_session_digest_markdown,
    render_patrol_session_digest_text,
};
pub use priority_queue::{PriorityQueue, QueueEntry};
pub use public_documents::{
    PublicAuditLedgerDocument, PublicAuditLedgerEntry, PublicRuleSetDocument,
    PublicStorageDocumentData, PublicTeamDefinitionDocument, PublicTeamRegistryDocument,
    PublicTeamRegistryEntry, PublicUserPreferencesDocument, default_public_storage_document,
    parse_public_storage_document, validate_public_storage_document,
};
pub use queue_builder::{
    build_ranked_queue, build_ranked_queue_with_contexts, build_ranked_queue_with_policy,
};
pub use recent_changes::{
    RecentChangesBatch, RecentChangesQuery, build_recent_changes_request, execute_recent_changes,
    parse_recent_changes_response,
};
pub use report_document::{
    ReportDocument, ReportSection, render_report_document_markdown, render_report_document_text,
};
pub use review_workbench::{
    PreparedRequestPreview, ReviewWorkbench, build_review_workbench,
    build_session_action_execution_requests,
};
pub use scoring_engine::{score_edit, score_edit_with_context};
pub use shell_state::{
    ShellPanelSummary, ShellStateInputs, ShellStateModel, ShellTimelineEntry, ShellTimelineStage,
    build_shell_state_model, render_shell_state_markdown, render_shell_state_text,
};
pub use stream_ingestor::StreamIngestor;
pub use stream_runtime::{StreamRuntime, StreamRuntimeStatus};
pub use training_data::{
    TrainingLabel, encode_csv, encode_json, encode_json_line, encode_json_lines,
};
pub use traits::{
    Clock, EventSource, FileStorage, FixedClock, HttpClient, LoopbackWebSocket, MemoryStorage,
    ReplayEventSource, Rng, SequenceRng, Storage, StubHttpClient, SystemClock, WebSocket,
};
pub use types::{
    Action, ActionBroadcast, CompositeScore, CoordinationMessage, CoordinationRoomSummary,
    CoordinationSnapshot, EditClaim, EditEvent, EditorIdentity, FlagState, FlaggedEdit, HttpMethod,
    HttpRequest, HttpResponse, LocalOAuthSourceReport, PresenceHeartbeat, QueueHeuristicPolicy,
    QueuedEdit, RaceResolution, ScoreDelta, ScoreWeights, ScoringConfig, ScoringContext,
    ScoringSignal, ServerDebugSummary, ServerSentEvent, SignalContribution, UserRiskProfile,
    WarningLevel, WebSocketFrame, WikiConfig,
};
pub use user_analyzer::{
    UserRiskCache, build_user_risk_profile, count_warning_templates, parse_warning_level,
};
pub use wiki_storage::{
    WikiStorageConfig, WikiStorageDocument, WikiStorageDocumentKind, WikiStorageLoadedDocument,
    WikiStoragePayloadEnvelope, WikiStoragePlan, WikiStoragePlanInput, WikiStorageRealm,
    WikiStorageWriteOutcome, WikiStorageWriteRequest, build_wiki_storage_document_load_request,
    build_wiki_storage_plan, load_wiki_storage_document, parse_wiki_storage_document_response,
    parse_wiki_storage_payload_envelope, render_wiki_storage_document_page,
    render_wiki_storage_index_page, resolve_wiki_storage_document, save_wiki_storage_document,
};
