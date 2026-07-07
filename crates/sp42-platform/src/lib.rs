#![forbid(unsafe_code)]

//! SP42 **platform** layer: reusable mechanisms, primitives, frameworks, and the
//! contracts (traits + shared DTOs) that domains build on (ADR-0013).
//!
//! This crate owns the platform-independent machinery extracted from the former
//! `sp42-core`: the scoring engine and policy-compilation framework, the action
//! contracts/executor, the wikitext editor and wiki storage, OAuth/dev-auth, the
//! diff engine, the priority queue, and the shared types/traits/errors. It never
//! depends on a domain or shell crate.
//!
//! ```
//! use sp42_platform::{WikiConfig, branding::PROJECT_NAME};
//!
//! let config = WikiConfig {
//!     wiki_id: "frwiki".to_string(),
//!     display_name: "French Wikipedia".to_string(),
//!     api_url: "https://fr.wikipedia.org/w/api.php".parse().expect("valid url"),
//!     eventstreams_url: "https://stream.wikimedia.org/v2/stream/recentchange".parse().expect("valid url"),
//!     oauth_authorize_url: "https://meta.wikimedia.org/w/rest.php/oauth2/authorize".parse().expect("valid url"),
//!     oauth_token_url: "https://meta.wikimedia.org/w/rest.php/oauth2/access_token".parse().expect("valid url"),
//!     liftwing_url: None,
//!     coordination_url: None,
//!     parsoid_url: None,
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
pub mod context_builder;
pub mod dev_auth;
pub mod diff_engine;
pub mod errors;
pub mod liftwing;
pub mod media_diff;
pub mod oauth;
pub mod origin;
pub mod priority_queue;
pub mod public_documents;
pub mod queue_builder;
pub mod review_workbench;
pub mod routes;
pub mod score_tier;
pub mod scoring_engine;
pub mod scoring_policy;
#[cfg(any(test, feature = "test-support"))]
pub mod test_fixtures;
pub mod training_data;
pub mod traits;
pub mod types;
pub mod user_analyzer;
pub mod wiki_storage;
pub mod wikitext_editor;

pub use action_contracts::{
    ActionResponseSummary, CitationConcernKind, PatrolRequest, RollbackRequest,
    SessionActionExecutionRequest, SessionActionExecutionResponse, SessionActionKind, TokenKind,
    UndoRequest, WikiPageSaveRequest, is_retryable_action_api_error,
};
pub use action_executor::{
    build_patrol_request, build_rollback_request, build_token_request, build_undo_request,
    build_wiki_page_save_request, execute_fetch_token, execute_patrol, execute_rollback,
    execute_undo, execute_wiki_page_save, parse_action_response_summary, parse_token_response,
    replace_exactly_once,
};
pub use article_inventory::{
    ArticleInventory, ArticleReference, article_inventory_notes, build_article_inventory,
};
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
pub use origin::{origin_of, origin_of_url, origins_match};
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
pub use score_tier::{HIGH_SCORE_THRESHOLD, MEDIUM_SCORE_THRESHOLD, ScoreTier, score_tier};
pub use scoring_engine::{score_edit, score_edit_with_context};
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
    Action, CompositeScore, DEFAULT_PATROL_NAMESPACES, DEFAULT_SCORING_POLICY_REF,
    DEFAULT_SCORING_POLICY_WIKI_ID, EditEvent, EditorIdentity, FlagState, HttpMethod, HttpRequest,
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
pub use wikitext_editor::{
    BlockKind, BlockRef, BookIdentifier, BookSource, CitedSource, ParsoidBlock,
    ScriptedEditorInvocation, ScriptedWikitextEditor, ScriptedWikitextNode, WikitextEditOutcome,
    WikitextEditRefusal, WikitextEditor, WikitextEditorError, WikitextNodeDescriptor,
    WikitextNodeKind, WikitextNodeLocator, WikitextPageRef, normalize_anchor_text,
};
