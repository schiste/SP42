#![forbid(unsafe_code)]

//! Shared SP42 domain contracts and pure platform-independent logic.
//!
//! ```
//! use sp42_core::{WikiConfig, branding::PROJECT_NAME};
//!
//! let config = WikiConfig {
//!     wiki_id: "frwiki".to_string(),
//!     display_name: "French Wikipedia".to_string(),
//!     api_url: "https://fr.wikipedia.org/w/api.php".parse().unwrap(),
//!     eventstreams_url: "https://stream.wikimedia.org/v2/stream/recentchange".parse().unwrap(),
//!     oauth_authorize_url: "https://meta.wikimedia.org/w/rest.php/oauth2/authorize".parse().unwrap(),
//!     oauth_token_url: "https://meta.wikimedia.org/w/rest.php/oauth2/access_token".parse().unwrap(),
//!     liftwing_url: None,
//!     coordination_url: None,
//!     inference_url: None,
//!     namespace_allowlist: vec![0],
//!     scoring_policy_ref: "active/frwiki-vandalism".to_string(),
//!     scoring: Default::default(),
//!     templates: Default::default(),
//! };
//!
//! assert_eq!(PROJECT_NAME, "SP42");
//! assert_eq!(config.wiki_id, "frwiki");
//! ```

pub mod action_contracts;
pub mod action_executor;
pub mod article_inventory;
pub mod branding;
pub mod citation;
pub mod context_builder;
pub mod dev_auth;
pub mod diff_engine;
pub mod errors;
pub mod liftwing;
pub mod media_diff;
pub mod oauth;
pub mod priority_queue;
pub mod public_documents;
pub mod queue_builder;
pub mod review_workbench;
pub mod routes;
pub mod scoring_engine;
pub mod scoring_evaluation;
pub mod scoring_policy;
#[cfg(test)]
pub(crate) mod test_fixtures;
pub mod training_data;
pub mod traits;
pub mod types;
pub mod user_analyzer;
pub mod wiki_storage;

pub use action_contracts::{
    ActionResponseSummary, PatrolRequest, RollbackRequest, SessionActionExecutionRequest,
    SessionActionExecutionResponse, SessionActionKind, TokenKind, UndoRequest, WikiPageSaveRequest,
    is_retryable_action_api_error,
};
pub use action_executor::{
    build_patrol_request, build_rollback_request, build_token_request, build_undo_request,
    build_wiki_page_save_request, execute_fetch_token, execute_patrol, execute_rollback,
    execute_undo, execute_wiki_page_save, parse_action_response_summary, parse_token_response,
};
pub use article_inventory::{
    ArticleInventory, ArticleReference, article_inventory_notes, build_article_inventory,
};
pub use citation::body_classifier::{BodyUsability, BodyUsabilityReason, classify_body_usability};
pub use citation::citoid::{
    CitoidMetadata, build_citoid_header, build_citoid_request, parse_citoid_response,
};
pub use citation::concurrency::map_with_concurrency;
pub use citation::locate_quote::locate_quote;
pub use citation::parsing::{ParsedVerdict, canonicalize_verdict, parse_verdict_response};
pub use citation::prompts::build_verify_prompt;
pub use citation::source_fetch::{html_to_text, looks_like_html, recover_wayback_body};
pub use citation::storage::{
    SnapshotEnvelope, VerdictEnvelope, build_snapshot, build_verdict_envelope, load_snapshot,
    load_verdict, store_snapshot, store_verdict,
};
pub use citation::urls::{
    ResolvedUrl, build_article_html_url, is_archive_url, is_valid_wiki_code,
    parse_revision_from_etag, resolve_citation_url, rewrite_wayback_url,
};
pub use citation::verdict::{CitationFindingKind, CitationVerdict, SupportLevel, Verdict};
pub use citation::verify::{
    CitationFinding, CitationVerificationRequest, GroundingAssertion, LocatedPassage, ModelVerdict,
    ModelVote, SourceProvenance, VerificationOutcome, VerifyModelInputs, VerifyOptions,
    assemble_citation_finding, build_model_votes, execute_citation_verify, sha256_hex,
    verify_citation_use_site,
};
pub use citation::voting::{BinaryVote, NClassVote, PanelAgreement, binary_vote, n_class_vote};
pub use context_builder::{ContextInputs, build_scoring_context};
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
    DiffHunk, DiffHunkKind, DiffLineSpan, DiffMarker, DiffMode, DiffMoveRole, DiffScoringHints,
    DiffSectionContext, DiffSegment, DiffSegmentKind, DiffStats, InlineSpan, RenderedHunkPreview,
    RenderedHunkSide, StructuredDiff, analyze_diff_for_scoring, detect_link_addition_only,
    diff_chars, diff_lines,
};
pub use errors::{
    ActionError, BacklogRuntimeError, CitationStorageError, CitationVerificationError,
    DevAuthError, DiffError, EventSourceError, HttpClientError, LiftWingError, OAuthError,
    PublicDocumentError, RecentChangesError, ReviewWorkbenchError, ScoringError,
    ScoringEvaluationError, ScoringPolicyError, StorageError, StreamIngestorError,
    StreamRuntimeError, TrainingDataError, UserAnalysisError, WebSocketError, WikiStorageError,
};
pub use liftwing::{
    LiftWingRequest, build_liftwing_score_request, execute_liftwing_score,
    parse_liftwing_score_response,
};
pub use media_diff::{
    MediaDiffEntry, MediaDiffKind, MediaDiffReport, MediaReference, build_media_diff,
    extract_media_references,
};
pub use oauth::{
    OAuthCallback, OAuthClientConfig, OAuthLaunchContext, OAuthTokenResponse,
    build_access_token_request, build_authorization_url, code_challenge_from_verifier,
    generate_oauth_state, generate_pkce_verifier, parse_callback_query, prepare_oauth_launch,
    prepare_token_exchange_from_callback, validate_code_verifier,
};
pub use priority_queue::{PriorityQueue, QueueEntry};
pub use public_documents::{
    PublicAuditLedgerDocument, PublicAuditLedgerEntry, PublicAuditLedgerReasoning,
    PublicRuleSetDocument, PublicStorageDocumentData, PublicTeamDefinitionDocument,
    PublicTeamRegistryDocument, PublicTeamRegistryEntry, PublicUserPreferencesDocument,
    default_public_storage_document, parse_public_storage_document,
    validate_public_storage_document,
};
pub use queue_builder::{
    build_ranked_queue, build_ranked_queue_with_contexts, build_ranked_queue_with_policy,
};
pub use review_workbench::{
    PreparedRequestPreview, ReviewWorkbench, build_review_workbench,
    build_session_action_execution_requests,
};
pub use scoring_engine::{score_edit, score_edit_with_context};
pub use scoring_evaluation::{
    FairnessFixtureCheck, FairnessFixtureSet, InvariantFixtureRule, InvariantFixtureSet,
    RankingFixtureComparison, RankingFixtureSet, RegressionFixtureCase, RegressionFixtureSet,
    parse_fairness_fixture_set, parse_invariant_fixture_set, parse_ranking_fixture_set,
    parse_regression_fixture_set,
};
pub use scoring_policy::{
    CombinationRulePolicy, CompiledScoringPolicy, EvaluationFairnessProfile,
    EvaluationFixtureSetPaths, ExternalEvaluationPolicyConfig, ExternalEvaluatorRole,
    FairnessPolicyConfig, IdentityPolicyConfig, LiftWingPolicyConfig, PolicyLifecycle,
    QueuePolicyConfig, RulePolicyConfig, ScoringDimensionWeights, ScoringDomain,
    ScoringEvaluationProfile, ScoringPolicyDocument, SignalParametersPolicyConfig,
    compile_scoring_policy, default_active_compiled_scoring_policy,
    load_embedded_compiled_scoring_policy, parse_scoring_evaluation_profile, parse_scoring_policy,
    validate_scoring_evaluation_profile, validate_scoring_policy,
};
pub use sp42_types::{
    ChatMessage, ChatRole, EndpointMode, ModelClient, ModelClientError, ModelCompletion,
    ModelCompletionRequest, ModelEndpointConfig, ModelInvocation, ModelRef, SamplingParams,
    StubModelClient,
};
pub use training_data::{
    TrainingLabel, encode_csv, encode_json, encode_json_line, encode_json_lines,
};
pub use traits::{
    Clock, EventSource, FileStorage, FixedClock, HttpClient, LoopbackWebSocket, MemoryStorage,
    ReplayEventSource, Rng, SequenceRng, Storage, StubHttpClient, SystemClock, WebSocket,
};
pub use types::{
    Action, CompositeScore, EditEvent, EditorIdentity, FlagState, HttpMethod, HttpRequest,
    HttpResponse, LocalOAuthSourceReport, QueueHeuristicPolicy, QueuedEdit, ScoreWeights,
    ScoringCombinationRule, ScoringConfig, ScoringContext, ScoringExternalEvaluationConfig,
    ScoringIdentityConfig, ScoringSignal, ScoringSignalParameters, ServerSentEvent,
    SignalContribution, UserRiskProfile, WarningLevel, WebSocketFrame, WikiConfig, WikiTemplates,
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
