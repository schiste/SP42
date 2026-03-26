mod auth_routes;
mod coordination;
mod local_env;
mod operator_live;
mod storage_routes;
mod wikimedia_capabilities;

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use axum::extract::OriginalUri;
use axum::extract::Path;
use axum::extract::Query;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, COOKIE, HOST, PRAGMA};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::{Json, Router};
use futures::{SinkExt, StreamExt};
use rand::Rng as _;
use sp42_core::traits::HttpClient;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use sp42_core::{
    ActionError, ActionExecutionHistoryReport, ActionExecutionLogEntry,
    ActionExecutionStatusReport, BacklogRuntime, BacklogRuntimeConfig, Clock, ContextInputs,
    CoordinationRoomSummary, CoordinationSnapshot, CoordinationState, DebugSnapshotInputs,
    DevAuthBootstrapRequest, DevAuthCapabilityReport, DevAuthSessionStatus, EditorIdentity,
    FileStorage, FlagState, LiftWingRequest, LiveOperatorBackendStatus, LiveOperatorPhaseTiming,
    LiveOperatorPublicDocuments, LiveOperatorQuery, LiveOperatorTelemetry, LiveOperatorView,
    LocalOAuthConfigStatus, LocalOAuthSourceReport, OAuthCallback, OAuthClientConfig,
    OAuthTokenResponse, PatrolScenarioReportInputs, PatrolSessionDigestInputs,
    PublicAuditLedgerEntry, PublicStorageDocumentData, QueuedEdit, RecentChangesQuery,
    ServerDebugSummary, SessionActionExecutionRequest, SessionActionExecutionResponse,
    SessionActionKind, ShellStateInputs, Storage, StreamRuntimeStatus, SystemClock, TokenKind,
    UndoRequest, WikiConfig, WikiStorageConfig, WikiStorageDocument, WikiStorageDocumentKind,
    WikiStorageLoadedDocument, WikiStoragePlan, WikiStoragePlanInput, WikiStorageWriteOutcome,
    WikiStorageWriteRequest, build_authorization_url, build_debug_snapshot,
    build_live_operator_action_preflight, build_patrol_scenario_report,
    build_patrol_session_digest, build_ranked_queue, build_review_workbench, build_scoring_context,
    build_shell_state_model, build_wiki_storage_plan, default_public_storage_document, diff_lines,
    execute_fetch_token, execute_liftwing_score, execute_patrol, execute_recent_changes,
    execute_rollback, execute_undo, generate_oauth_state, generate_pkce_verifier,
    load_wiki_storage_document, parse_callback_query,
    render_wiki_storage_document_page, render_wiki_storage_index_page,
    resolve_wiki_storage_document, save_wiki_storage_document,
};

use crate::coordination::{
    CoordinationEnvelope, CoordinationRegistry, CoordinationRoomInspection, CoordinationRoomMetrics,
};
use crate::local_env::LocalOAuthConfig;
use crate::wikimedia_capabilities::{CapabilityProbeTargets, config_for_wiki, probe_with_targets};

type SharedSessions = Arc<RwLock<HashMap<String, StoredSession>>>;
type SharedCapabilityCache = Arc<RwLock<Option<CachedCapabilityReport>>>;
type SharedPendingOAuthLogins = Arc<RwLock<HashMap<String, PendingOAuthLogin>>>;
type SharedIngestionSupervisor = Arc<RwLock<HashMap<String, IngestionSupervisorSnapshot>>>;

const SESSION_COOKIE_NAME: &str = "sp42_dev_session";
const CAPABILITY_CACHE_TTL_MS: i64 = 30_000;
const SESSION_IDLE_TIMEOUT_MS: i64 = 30 * 60 * 1000;
const SESSION_ABSOLUTE_TIMEOUT_MS: i64 = 8 * 60 * 60 * 1000;
const SESSION_COOKIE_MAX_AGE_SECONDS: i64 = SESSION_IDLE_TIMEOUT_MS / 1000;
const PENDING_OAUTH_TTL_MS: i64 = 10 * 60 * 1000;
const OPERATOR_READINESS_PATH: &str = "/operator/readiness";
const OPERATOR_REPORT_PATH: &str = "/operator/report";
const OPERATOR_STORAGE_LAYOUT_PATH: &str = "/operator/storage/layout";
const ACTION_STATUS_PATH: &str = "/dev/actions/status";
const ACTION_HISTORY_PATH: &str = "/dev/actions/history";
const ACTION_HISTORY_LIMIT: usize = 50;
const RESPONSE_BODY_PREVIEW_LIMIT: usize = 1_000;
const AUTH_LOGIN_PATH: &str = "/auth/login";
const AUTH_CALLBACK_PATH: &str = "/auth/callback";
const AUTH_SESSION_PATH: &str = "/auth/session";
const AUTH_LOGOUT_PATH: &str = "/auth/logout";

#[derive(Clone)]
struct AppState {
    capability_cache: SharedCapabilityCache,
    sessions: SharedSessions,
    pending_oauth_logins: SharedPendingOAuthLogins,
    http_client: reqwest::Client,
    local_oauth: LocalOAuthConfig,
    runtime_storage_root: PathBuf,
    ingestion_supervisor: SharedIngestionSupervisor,
    capability_targets: CapabilityProbeTargets,
    clock: Arc<dyn Clock>,
    coordination: CoordinationRegistry,
    next_client_id: Arc<AtomicU64>,
    next_session_id: Arc<AtomicU64>,
    started_at: Instant,
}

#[derive(Debug, Clone)]
struct CachedCapabilityReport {
    fetched_at_ms: i64,
    report: DevAuthCapabilityReport,
}

#[derive(Debug, Clone)]
struct StoredSession {
    username: String,
    scopes: Vec<String>,
    expires_at_ms: Option<i64>,
    access_token: String,
    refresh_token: Option<String>,
    upstream_access_expires_at_ms: Option<i64>,
    bridge_mode: String,
    created_at_ms: i64,
    last_seen_at_ms: i64,
    capability_cache: HashMap<String, CachedCapabilityReport>,
    action_history: Vec<ActionExecutionLogEntry>,
}

#[derive(Debug, Clone)]
struct PendingOAuthLogin {
    wiki_id: String,
    state: String,
    verifier: String,
    redirect_uri: String,
    redirect_after_login: String,
    expires_at_ms: i64,
}

#[derive(Debug, Clone)]
struct SessionSnapshot {
    session_id: String,
    username: String,
    scopes: Vec<String>,
    expires_at_ms: Option<i64>,
    access_token: String,
    bridge_mode: String,
}

#[derive(Clone)]
struct AuthenticatedWikiContext {
    client: BearerHttpClient,
    config: WikiConfig,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
struct ActionHistoryQuery {
    limit: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
struct AuthLoginQuery {
    next: Option<String>,
    wiki_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct OAuthSessionView {
    authenticated: FlagState,
    username: Option<String>,
    scopes: Vec<String>,
    expires_at_ms: Option<i64>,
    upstream_access_expires_at_ms: Option<i64>,
    refresh_available: FlagState,
    bridge_mode: String,
    local_token_available: FlagState,
    oauth_client_ready: FlagState,
    login_path: String,
    logout_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
struct OAuthProfileResponse {
    username: String,
    #[serde(default)]
    grants: Vec<String>,
}

struct ServerRng;

impl sp42_core::Rng for ServerRng {
    fn next_u64(&mut self) -> u64 {
        rand::rng().random()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct DevAuthBootstrapStatus {
    bootstrap_ready: bool,
    oauth: LocalOAuthConfigStatus,
    session: DevAuthSessionStatus,
    source_path: Option<String>,
    source_report: LocalOAuthSourceReport,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct CapabilityProbeHint {
    wiki_id: String,
    endpoint: String,
    available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct CapabilityCacheStatus {
    present: bool,
    fresh: bool,
    age_ms: Option<u64>,
    wiki_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct ServerHealthStatus {
    project: String,
    ready_for_local_testing: bool,
    readiness_issues: Vec<String>,
    uptime_ms: u64,
    session_count: usize,
    coordination_room_count: usize,
    auth: DevAuthSessionStatus,
    oauth: LocalOAuthConfigStatus,
    bootstrap: DevAuthBootstrapStatus,
    capability_probe: CapabilityProbeHint,
    capability_cache: CapabilityCacheStatus,
    operator_report_path: String,
    coordination: CoordinationSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct RuntimeDebugStatus {
    project: String,
    uptime_ms: u64,
    session_count: usize,
    coordination_room_count: usize,
    auth: DevAuthSessionStatus,
    oauth: LocalOAuthConfigStatus,
    bootstrap: DevAuthBootstrapStatus,
    capabilities: DevAuthCapabilityReport,
    capability_cache: CapabilityCacheStatus,
    operator_report_path: String,
    coordination: CoordinationSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct OperatorEndpointDescriptor {
    method: String,
    path: String,
    purpose: String,
    available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct OperatorReport {
    project: String,
    readiness: ServerHealthStatus,
    runtime: RuntimeDebugStatus,
    bootstrap: DevAuthBootstrapStatus,
    debug_summary: ServerDebugSummary,
    endpoints: Vec<OperatorEndpointDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct RoomInspectionCollection {
    rooms: Vec<CoordinationRoomInspection>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct OperatorRuntimeInspection {
    wiki_id: String,
    storage_root: String,
    backlog: sp42_core::BacklogRuntimeStatus,
    stream_checkpoint_key: String,
    stream_last_event_id: Option<String>,
    notes: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
struct StorageLayoutQuery {
    username: Option<String>,
    home_wiki_id: Option<String>,
    shared_owner_username: Option<String>,
    team_slug: Option<String>,
    rule_set_slug: Option<String>,
    training_dataset_slug: Option<String>,
    audit_period_slug: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct OperatorStorageLayoutView {
    plan: WikiStoragePlan,
    personal_index_page: String,
    shared_registry_page: String,
    sample_document_pages: Vec<RenderedStorageDocumentPage>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct RenderedStorageDocumentPage {
    title: String,
    body: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
struct StorageDocumentQuery {
    title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum StorageDocumentRealmInput {
    Personal,
    Shared,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum StorageDocumentKindInput {
    Index,
    Profile,
    Preferences,
    Queue,
    Workspace,
    Labels,
    Registry,
    Team,
    RuleSet,
    TrainingDataset,
    AuditPeriod,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
struct LogicalStorageDocumentQuery {
    username: Option<String>,
    home_wiki_id: Option<String>,
    shared_owner_username: Option<String>,
    slug: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
struct StorageDocumentSavePayload {
    document: sp42_core::WikiStorageDocument,
    #[serde(default)]
    human_summary: Vec<String>,
    data: serde_json::Value,
    baserevid: Option<u64>,
    #[serde(default)]
    tags: Vec<String>,
    watchlist: Option<String>,
    create_only: FlagState,
    minor: FlagState,
    summary: Option<String>,
}

#[derive(Debug, Clone)]
struct StoragePlanRequest {
    username_override: Option<String>,
    home_wiki_id_override: Option<String>,
    shared_owner_username_override: Option<String>,
    team_slugs: Vec<String>,
    rule_set_slugs: Vec<String>,
    training_dataset_slugs: Vec<String>,
    audit_period_slugs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
struct LogicalStorageDocumentSavePayload {
    #[serde(default)]
    human_summary: Vec<String>,
    data: serde_json::Value,
    baserevid: Option<u64>,
    #[serde(default)]
    tags: Vec<String>,
    watchlist: Option<String>,
    create_only: FlagState,
    minor: FlagState,
    summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
struct LogicalStorageDocumentView {
    document: WikiStorageDocument,
    loaded: WikiStorageLoadedDocument,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
struct LogicalStorageDocumentWriteView {
    document: WikiStorageDocument,
    outcome: WikiStorageWriteOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum PublicStorageDocumentRouteKind {
    Preferences,
    Registry,
    Team,
    RuleSet,
    AuditPeriod,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
struct PublicStorageDocumentQuery {
    username: Option<String>,
    home_wiki_id: Option<String>,
    shared_owner_username: Option<String>,
    slug: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
struct PublicStorageDocumentSavePayload {
    payload: PublicStorageDocumentData,
    #[serde(default)]
    human_summary: Vec<String>,
    baserevid: Option<u64>,
    #[serde(default)]
    tags: Vec<String>,
    watchlist: Option<String>,
    create_only: FlagState,
    minor: FlagState,
    summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
struct PublicStorageDocumentView {
    document: WikiStorageDocument,
    loaded: WikiStorageLoadedDocument,
    payload: PublicStorageDocumentData,
    defaulted: FlagState,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
struct PublicStorageDocumentWriteView {
    document: WikiStorageDocument,
    payload: PublicStorageDocumentData,
    outcome: WikiStorageWriteOutcome,
}

#[derive(Debug, Clone)]
struct ResolvedPublicStorageDocument {
    document: WikiStorageDocument,
    loaded: WikiStorageLoadedDocument,
    payload: PublicStorageDocumentData,
    defaulted: FlagState,
}

#[derive(Debug, Clone, Default)]
struct LiveOperatorPublicContextState {
    preferences: Option<ResolvedPublicStorageDocument>,
    registry: Option<ResolvedPublicStorageDocument>,
    active_team: Option<ResolvedPublicStorageDocument>,
    active_rule_set: Option<ResolvedPublicStorageDocument>,
    audit_period_slug: Option<String>,
    notes: Vec<String>,
}

struct LivePublicDocumentLoadSpec {
    kind: PublicStorageDocumentRouteKind,
    query: PublicStorageDocumentQuery,
    resolved_label: &'static str,
    plan_label: &'static str,
}

async fn current_storage_document_on_conflict(
    client: &BearerHttpClient,
    config: &WikiConfig,
    title: &str,
) -> Option<WikiStorageLoadedDocument> {
    load_wiki_storage_document(client, config, title).await.ok()
}

async fn wiki_storage_save_error_response(
    client: &BearerHttpClient,
    config: &WikiConfig,
    document: &WikiStorageDocument,
    error: sp42_core::WikiStorageError,
) -> (StatusCode, Json<serde_json::Value>) {
    match error {
        sp42_core::WikiStorageError::Conflict { title, message } => {
            let current = current_storage_document_on_conflict(client, config, &title).await;
            (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": message,
                    "document": document,
                    "current": current,
                })),
            )
        }
        other => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({
                "error": other.to_string(),
                "document": document,
            })),
        ),
    }
}

#[tokio::main]
async fn main() -> Result<(), std::io::Error> {
    init_tracing();
    let bind_addr =
        std::env::var("SP42_BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:8788".to_string());
    let listener = TcpListener::bind(&bind_addr).await?;
    let local_addr = listener.local_addr()?;
    info!(
        bind = %local_addr,
        project = sp42_core::branding::PROJECT_NAME,
        "starting localhost server"
    );
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let state = AppState {
        capability_cache: Arc::new(RwLock::new(None)),
        sessions: Arc::new(RwLock::new(HashMap::new())),
        pending_oauth_logins: Arc::new(RwLock::new(HashMap::new())),
        http_client: build_http_client()?,
        local_oauth: LocalOAuthConfig::load(),
        runtime_storage_root: runtime_storage_root(),
        ingestion_supervisor: Arc::new(RwLock::new(HashMap::new())),
        capability_targets: CapabilityProbeTargets::default(),
        clock: clock.clone(),
        coordination: CoordinationRegistry::new(clock),
        next_client_id: Arc::new(AtomicU64::new(1)),
        next_session_id: Arc::new(AtomicU64::new(1)),
        started_at: Instant::now(),
    };
    spawn_ingestion_supervisors(&state);
    let router = build_router(state);

    axum::serve(listener, router).await
}

fn build_http_client() -> io::Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .user_agent(sp42_core::branding::USER_AGENT)
        .build()
        .map_err(|error| io::Error::other(format!("failed to build reqwest client: {error}")))
}

fn supervisor_poll_interval_ms() -> u64 {
    std::env::var("SP42_INGESTION_POLL_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(15_000)
}

fn supervisor_wiki_ids() -> Vec<String> {
    let configured = std::env::var("SP42_SUPERVISOR_WIKIS")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "frwiki".to_string());

    configured
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn spawn_ingestion_supervisors(state: &AppState) {
    let poll_interval_ms = supervisor_poll_interval_ms();
    for wiki_id in supervisor_wiki_ids() {
        let state_clone = state.clone();
        tokio::spawn(async move {
            run_ingestion_supervisor_for_wiki(state_clone, wiki_id, poll_interval_ms).await;
        });
    }
}

async fn run_ingestion_supervisor_for_wiki(
    state: AppState,
    wiki_id: String,
    poll_interval_ms: u64,
) {
    loop {
        let started_at_ms = state.clock.now_ms();
        let snapshot =
            supervisor_snapshot_iteration(&state, &wiki_id, poll_interval_ms, started_at_ms).await;
        let sleep_ms = poll_interval_ms.max(1_000);
        {
            let mut guard = state.ingestion_supervisor.write().await;
            guard.insert(wiki_id.clone(), snapshot);
        }
        tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
    }
}

async fn supervisor_snapshot_iteration(
    state: &AppState,
    wiki_id: &str,
    poll_interval_ms: u64,
    started_at_ms: i64,
) -> IngestionSupervisorSnapshot {
    let previous = {
        let guard = state.ingestion_supervisor.read().await;
        guard.get(wiki_id).cloned()
    };
    let previous_run_count = previous
        .as_ref()
        .map_or(0, |snapshot| snapshot.status.run_count);
    let previous_success = previous
        .as_ref()
        .and_then(|snapshot| snapshot.status.last_success_at_ms);

    let Some(access_token) = state.local_oauth.access_token().map(ToString::to_string) else {
        return supervisor_inactive_snapshot(
            wiki_id,
            poll_interval_ms,
            previous_run_count,
            started_at_ms,
            previous_success,
            "No local Wikimedia access token is available.".to_string(),
            "Supervisor is idle until a local token is configured.".to_string(),
        );
    };

    let config = match resolved_wiki_config(state, wiki_id) {
        Ok(config) => config,
        Err(error) => {
            return supervisor_inactive_snapshot(
                wiki_id,
                poll_interval_ms,
                previous_run_count,
                started_at_ms,
                previous_success,
                error,
                "Supervisor could not resolve wiki configuration.".to_string(),
            );
        }
    };

    let poll_result = perform_supervisor_poll(state, wiki_id, &config, access_token).await;

    match poll_result {
        Ok((batch, backlog_status, stream_status, queue)) => {
            let mut notes = vec![format!(
                "Supervisor polled {} edits.",
                backlog_status.last_batch_size
            )];
            if stream_status.last_event_id.is_none() && backlog_status.next_continue.is_some() {
                notes.push(
                    "Stream checkpoint is empty; backlog checkpoint is currently authoritative."
                        .to_string(),
                );
            }
            IngestionSupervisorSnapshot {
                status: sp42_core::LiveIngestionSupervisorStatus {
                    wiki_id: wiki_id.to_string(),
                    active: true,
                    poll_interval_ms,
                    run_count: previous_run_count.saturating_add(1),
                    latest_queue_depth: queue.len(),
                    last_started_at_ms: Some(started_at_ms),
                    last_success_at_ms: Some(state.clock.now_ms()),
                    last_error: None,
                    stream_status: Some(stream_status),
                    backlog_status: Some(backlog_status),
                    notes,
                },
                queue,
                next_continue: batch.next_continue,
            }
        }
        Err(error) => supervisor_inactive_snapshot(
            wiki_id,
            poll_interval_ms,
            previous_run_count,
            started_at_ms,
            previous_success,
            error.to_string(),
            "Supervisor poll failed; request path will continue to fall back.".to_string(),
        ),
    }
}

fn supervisor_inactive_snapshot(
    wiki_id: &str,
    poll_interval_ms: u64,
    previous_run_count: u64,
    started_at_ms: i64,
    previous_success: Option<i64>,
    error: String,
    note: String,
) -> IngestionSupervisorSnapshot {
    IngestionSupervisorSnapshot {
        status: sp42_core::LiveIngestionSupervisorStatus {
            wiki_id: wiki_id.to_string(),
            active: false,
            poll_interval_ms,
            run_count: previous_run_count.saturating_add(1),
            latest_queue_depth: 0,
            last_started_at_ms: Some(started_at_ms),
            last_success_at_ms: previous_success,
            last_error: Some(error),
            stream_status: None,
            backlog_status: None,
            notes: vec![note],
        },
        queue: Vec::new(),
        next_continue: None,
    }
}

async fn perform_supervisor_poll(
    state: &AppState,
    wiki_id: &str,
    config: &WikiConfig,
    access_token: String,
) -> Result<
    (
        sp42_core::RecentChangesBatch,
        sp42_core::BacklogRuntimeStatus,
        StreamRuntimeStatus,
        Vec<QueuedEdit>,
    ),
    sp42_core::BacklogRuntimeError,
> {
    let client = BearerHttpClient::new(state.http_client.clone(), access_token);
    let storage = runtime_storage_for(state);
    let mut backlog_runtime = BacklogRuntime::new(
        config.clone(),
        storage,
        BacklogRuntimeConfig {
            limit: default_limit(),
            include_bots: false,
        },
        format!("recentchanges.rccontinue.{wiki_id}"),
    );
    backlog_runtime.initialize().await?;
    let batch = backlog_runtime.poll(&client).await?;
    let backlog_status = backlog_runtime.status();
    let stream_status = persisted_stream_status(state, wiki_id)
        .await
        .map_err(|message| {
            sp42_core::BacklogRuntimeError::Storage(sp42_core::StorageError::Operation { message })
        })?;
    let queue = build_ranked_queue(batch.events.clone(), &config.scoring).map_err(|error| {
        sp42_core::BacklogRuntimeError::Storage(sp42_core::StorageError::Operation {
            message: error.to_string(),
        })
    })?;
    Ok((batch, backlog_status, stream_status, queue))
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("sp42_server=info,sp42_core=warn"));

    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_ansi(false)
        .try_init();
}

fn build_router(state: AppState) -> Router {
    let app_dist_dir = browser_app_dist_dir();
    let browser_shell = if app_dist_dir.join("index.html").is_file() {
        Some(
            ServeDir::new(&app_dist_dir)
                .not_found_service(ServeFile::new(app_dist_dir.join("index.html"))),
        )
    } else {
        None
    };

    let router = Router::new()
        .route("/coordination/rooms", get(get_coordination_snapshot))
        .route(
            "/coordination/rooms/{wiki_id}",
            get(get_coordination_room_state),
        )
        .route(
            "/coordination/rooms/{wiki_id}/inspection",
            get(get_coordination_room_inspection),
        )
        .route(
            "/coordination/inspections",
            get(get_coordination_inspections),
        )
        .route("/debug/summary", get(get_debug_summary))
        .route("/debug/runtime", get(get_runtime_debug))
        .route(AUTH_LOGIN_PATH, get(get_auth_login))
        .route(AUTH_CALLBACK_PATH, get(get_auth_callback))
        .route(AUTH_SESSION_PATH, get(get_auth_session))
        .route(AUTH_LOGOUT_PATH, axum::routing::post(post_auth_logout))
        .route(OPERATOR_READINESS_PATH, get(get_operator_readiness))
        .route(OPERATOR_REPORT_PATH, get(get_operator_report))
        .route("/operator/live/{wiki_id}", get(get_live_operator_view))
        .route("/operator/runtime/{wiki_id}", get(get_operator_runtime))
        .route(
            "/operator/storage/layout/{wiki_id}",
            get(get_operator_storage_layout),
        )
        .route(
            "/operator/storage/document/{wiki_id}",
            get(get_storage_document).put(put_storage_document),
        )
        .route(
            "/operator/storage/logical/{wiki_id}/{realm}/{kind}",
            get(get_logical_storage_document).put(put_logical_storage_document),
        )
        .route(
            "/operator/storage/public/{wiki_id}/{kind}",
            get(get_public_storage_document).put(put_public_storage_document),
        )
        .route("/ws/{wiki_id}", get(coordination_socket))
        .route("/dev/auth/session", get(get_session).delete(delete_session))
        .route("/dev/auth/capabilities/{wiki_id}", get(get_capabilities))
        .route(
            "/dev/actions/execute",
            axum::routing::post(post_execute_action),
        )
        .route(ACTION_STATUS_PATH, get(get_action_status))
        .route(ACTION_HISTORY_PATH, get(get_action_history))
        .route(
            "/dev/auth/session/bootstrap",
            axum::routing::post(post_bootstrap_session),
        )
        .route("/dev/auth/bootstrap/status", get(get_bootstrap_status))
        .route("/healthz", get(get_healthz))
        .with_state(state)
        .layer(
            CorsLayer::new()
                .allow_credentials(true)
                .allow_origin([
                    HeaderValue::from_static("http://127.0.0.1:4173"),
                    HeaderValue::from_static("http://localhost:4173"),
                    HeaderValue::from_static("http://127.0.0.1:8788"),
                    HeaderValue::from_static("http://localhost:8788"),
                ])
                .allow_methods([Method::GET, Method::POST, Method::DELETE])
                .allow_headers([CONTENT_TYPE, COOKIE]),
        )
        .layer(middleware::from_fn(disable_response_caching));

    if let Some(browser_shell) = browser_shell {
        router.fallback_service(browser_shell)
    } else {
        router.route("/", get(browser_shell_unavailable))
    }
}

async fn disable_response_caching(request: axum::extract::Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        CACHE_CONTROL,
        HeaderValue::from_static("no-store, no-cache, must-revalidate, max-age=0"),
    );
    response
        .headers_mut()
        .insert(PRAGMA, HeaderValue::from_static("no-cache"));
    response
}

fn browser_app_dist_dir() -> PathBuf {
    std::env::var_os("SP42_APP_DIST_DIR").map_or_else(
        || {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("sp42-app")
                .join("dist")
        },
        PathBuf::from,
    )
}

fn runtime_storage_root() -> PathBuf {
    std::env::var_os("SP42_RUNTIME_DIR").map_or_else(
        || {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join(".sp42-runtime")
        },
        PathBuf::from,
    )
}

fn gateway_error(message: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_GATEWAY,
        Json(serde_json::json!({ "error": message.into() })),
    )
}

async fn authenticated_wiki_context(
    state: &AppState,
    headers: &HeaderMap,
    wiki_id: &str,
) -> Result<AuthenticatedWikiContext, (StatusCode, Json<serde_json::Value>)> {
    let access_token = access_token_for_request(state, headers)
        .await
        .ok_or_else(|| unauthorized_error("No authenticated Wikimedia session is active."))?;
    let config =
        resolved_wiki_config(state, wiki_id).map_err(|message| invalid_payload(&message))?;
    Ok(AuthenticatedWikiContext {
        client: BearerHttpClient::new(state.http_client.clone(), access_token),
        config,
    })
}

async fn required_csrf_token(
    context: &AuthenticatedWikiContext,
) -> Result<String, (StatusCode, Json<serde_json::Value>)> {
    execute_fetch_token(&context.client, &context.config, TokenKind::Csrf)
        .await
        .map_err(|error| gateway_error(format!("csrf token fetch failed: {error}")))
}

async fn load_storage_document_with_context(
    context: &AuthenticatedWikiContext,
    title: &str,
) -> Result<WikiStorageLoadedDocument, (StatusCode, Json<serde_json::Value>)> {
    load_wiki_storage_document(&context.client, &context.config, title)
        .await
        .map_err(|error| gateway_error(error.to_string()))
}

async fn save_storage_document_with_context(
    context: &AuthenticatedWikiContext,
    document: WikiStorageDocument,
    request: WikiStorageWriteRequest,
) -> Result<WikiStorageWriteOutcome, (StatusCode, Json<serde_json::Value>)> {
    match save_wiki_storage_document(&context.client, &context.config, &request).await {
        Ok(outcome) => Ok(outcome),
        Err(error) => Err(wiki_storage_save_error_response(
            &context.client,
            &context.config,
            &document,
            error,
        )
        .await),
    }
}

async fn browser_shell_unavailable() -> impl IntoResponse {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        [(
            CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        )],
        r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>SP42 frontend unavailable</title>
    <style>
      body {
        margin: 0;
        min-height: 100vh;
        display: grid;
        place-items: center;
        background: #091321;
        color: #e3edf9;
        font-family: "IBM Plex Sans", "Avenir Next", sans-serif;
      }
      main {
        width: min(42rem, calc(100vw - 3rem));
        padding: 1.5rem;
        border: 1px solid rgba(227, 237, 249, 0.16);
        border-radius: 1rem;
        background: rgba(15, 28, 46, 0.92);
      }
      code {
        color: #52c7b8;
      }
    </style>
  </head>
  <body>
    <main>
      <h1>SP42 frontend build missing</h1>
      <p>Build the browser app first:</p>
      <p><code>./scripts/build-frontend.sh</code></p>
      <p>Or run live development from <code>crates/sp42-app</code> with <code>trunk serve</code>.</p>
    </main>
  </body>
</html>"#,
    )
}

async fn coordination_socket(
    ws: WebSocketUpgrade,
    Path(wiki_id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let actor = current_session_snapshot(&state, &headers, true)
        .await
        .map(|session| session.username);
    ws.on_upgrade(move |socket| handle_socket(socket, wiki_id, actor, state))
}

async fn handle_socket(socket: WebSocket, wiki_id: String, actor: Option<String>, state: AppState) {
    let client_id = state.next_client_id.fetch_add(1, Ordering::Relaxed);
    state.coordination.connect_client(&wiki_id).await;
    let mut subscriber = state.coordination.subscribe(&wiki_id).await;
    let (mut sender, mut receiver) = socket.split();

    let send_task = tokio::spawn(async move {
        while let Ok(envelope) = subscriber.recv().await {
            if envelope.sender_id == client_id {
                continue;
            }

            if sender
                .send(Message::Binary(envelope.payload.into()))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    while let Some(message_result) = receiver.next().await {
        let Ok(message) = message_result else {
            break;
        };

        match message {
            Message::Binary(bytes) => {
                let payload = sanitize_coordination_payload(bytes.to_vec(), actor.as_deref());
                state
                    .coordination
                    .publish(
                        &wiki_id,
                        CoordinationEnvelope {
                            sender_id: client_id,
                            payload,
                        },
                    )
                    .await;
            }
            Message::Text(text) => {
                let payload = sanitize_coordination_payload(
                    text.as_str().as_bytes().to_vec(),
                    actor.as_deref(),
                );
                state
                    .coordination
                    .publish(
                        &wiki_id,
                        CoordinationEnvelope {
                            sender_id: client_id,
                            payload,
                        },
                    )
                    .await;
            }
            Message::Close(_) => break,
            Message::Ping(_) | Message::Pong(_) => {}
        }
    }

    send_task.abort();
    state.coordination.disconnect_client(&wiki_id).await;
}

async fn get_coordination_snapshot(State(state): State<AppState>) -> Json<CoordinationSnapshot> {
    Json(state.coordination.snapshot().await)
}

async fn get_coordination_room_state(
    Path(wiki_id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if let Some(summary) = state.coordination.room_state_summary(&wiki_id).await {
        Ok::<_, StatusCode>(Json(summary))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

async fn get_coordination_room_inspection(
    Path(wiki_id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    Ok::<_, StatusCode>(Json(
        room_inspection(&state.coordination, &wiki_id)
            .await
            .unwrap_or_else(|| empty_room_inspection(&wiki_id)),
    ))
}

async fn get_coordination_inspections(
    State(state): State<AppState>,
) -> Json<RoomInspectionCollection> {
    Json(RoomInspectionCollection {
        rooms: state.coordination.room_inspections().await,
    })
}

async fn get_debug_summary(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Json<ServerDebugSummary> {
    Json(server_debug_summary(&state, &headers).await)
}

async fn get_operator_readiness(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Json<ServerHealthStatus> {
    Json(server_readiness(&state, &headers).await)
}

async fn get_operator_report(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Json<OperatorReport> {
    Json(operator_report(&state, &headers).await)
}

#[derive(Debug, serde::Deserialize)]
struct LiveViewFilterParams {
    #[serde(default = "default_limit")]
    limit: u16,
    #[serde(default)]
    include_bots: FlagState,
    #[serde(default)]
    unpatrolled_only: FlagState,
    #[serde(default = "default_true")]
    include_minor: FlagState,
    #[serde(default = "default_true")]
    include_registered: FlagState,
    #[serde(default = "default_true")]
    include_anonymous: FlagState,
    #[serde(default = "default_true")]
    include_temporary: FlagState,
    #[serde(default = "default_true")]
    include_new_pages: FlagState,
    #[serde(default)]
    namespaces: Option<String>,
    #[serde(default)]
    min_score: Option<i32>,
    #[serde(default)]
    tag_filter: Option<String>,
    #[serde(default)]
    rccontinue: Option<String>,
}

struct LiveOperatorBootstrap {
    config: WikiConfig,
    auth: DevAuthSessionStatus,
    action_status: ActionExecutionStatusReport,
    action_history: ActionExecutionHistoryReport,
    capabilities: DevAuthCapabilityReport,
    stream_status: StreamRuntimeStatus,
}

struct LiveQueueState {
    query: LiveOperatorQuery,
    batch: sp42_core::RecentChangesBatch,
    backlog_status: sp42_core::BacklogRuntimeStatus,
    queue: Vec<QueuedEdit>,
    selected_index: Option<usize>,
}

#[derive(Debug, Clone)]
struct IngestionSupervisorSnapshot {
    status: sp42_core::LiveIngestionSupervisorStatus,
    queue: Vec<QueuedEdit>,
    next_continue: Option<String>,
}

struct SelectedReviewState {
    scoring_context: Option<sp42_core::ScoringContext>,
    diff: Option<sp42_core::StructuredDiff>,
    review_workbench: Option<sp42_core::ReviewWorkbench>,
    readiness: ServerHealthStatus,
    coordination_state: Option<sp42_core::CoordinationStateSummary>,
    coordination_room: Option<CoordinationRoomSummary>,
}

struct LiveOperatorProducts {
    scenario_report: sp42_core::PatrolScenarioReport,
    session_digest: sp42_core::PatrolSessionDigest,
    shell_state: sp42_core::ShellStateModel,
    backend: LiveOperatorBackendStatus,
    debug_snapshot: sp42_core::DebugSnapshot,
    action_preflight: sp42_core::LiveOperatorActionPreflight,
}

struct LiveOperatorProductContext<'a> {
    stream_status: &'a StreamRuntimeStatus,
    backlog_status: &'a sp42_core::BacklogRuntimeStatus,
    auth: &'a DevAuthSessionStatus,
    action_status: &'a ActionExecutionStatusReport,
    capabilities: &'a DevAuthCapabilityReport,
}

struct LiveOperatorFinalization {
    queue_state: LiveQueueState,
    bootstrap: LiveOperatorBootstrap,
    selected_review: SelectedReviewState,
    products: LiveOperatorProducts,
    public_documents: LiveOperatorPublicDocuments,
    telemetry: LiveOperatorTelemetry,
    notes: Vec<String>,
    ingestion_supervisor: Option<sp42_core::LiveIngestionSupervisorStatus>,
}

struct LiveOperatorAssembly {
    bootstrap: LiveOperatorBootstrap,
    public_context: LiveOperatorPublicContextState,
    queue_state: LiveQueueState,
    selected_review: SelectedReviewState,
    telemetry_phase_timings: Vec<LiveOperatorPhaseTiming>,
}

enum CapabilityProbeSubject<'a> {
    LocalToken,
    Session(&'a SessionSnapshot),
}

const fn default_limit() -> u16 {
    15
}

const fn default_true() -> FlagState {
    FlagState::Enabled
}

async fn get_operator_runtime(
    Path(wiki_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<OperatorRuntimeInspection>, (StatusCode, Json<serde_json::Value>)> {
    operator_runtime_inspection(&state, &wiki_id)
        .await
        .map(Json)
        .map_err(|error| {
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": error })),
            )
        })
}

async fn get_operator_storage_layout(
    Path(wiki_id): Path<String>,
    Query(query): Query<StorageLayoutQuery>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<OperatorStorageLayoutView>, (StatusCode, Json<serde_json::Value>)> {
    operator_storage_layout_view(&state, &headers, &wiki_id, &query)
        .await
        .map(Json)
        .map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": error })),
            )
        })
}

async fn get_live_operator_view(
    Path(wiki_id): Path<String>,
    axum::extract::Query(filters): axum::extract::Query<LiveViewFilterParams>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    operator_live::get_live_operator_view(
        Path(wiki_id),
        axum::extract::Query(filters),
        State(state),
        headers,
    )
    .await
}

async fn get_storage_document(
    Path(wiki_id): Path<String>,
    Query(query): Query<StorageDocumentQuery>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<WikiStorageLoadedDocument>, (StatusCode, Json<serde_json::Value>)> {
    storage_routes::get_storage_document(Path(wiki_id), Query(query), State(state), headers).await
}

async fn put_storage_document(
    Path(wiki_id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<StorageDocumentSavePayload>,
) -> Result<Json<WikiStorageWriteOutcome>, (StatusCode, Json<serde_json::Value>)> {
    storage_routes::put_storage_document(Path(wiki_id), State(state), headers, Json(payload)).await
}

async fn get_logical_storage_document(
    Path((wiki_id, realm, kind)): Path<(
        String,
        StorageDocumentRealmInput,
        StorageDocumentKindInput,
    )>,
    Query(query): Query<LogicalStorageDocumentQuery>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<LogicalStorageDocumentView>, (StatusCode, Json<serde_json::Value>)> {
    storage_routes::get_logical_storage_document(
        Path((wiki_id, realm, kind)),
        Query(query),
        State(state),
        headers,
    )
    .await
}

async fn put_logical_storage_document(
    Path((wiki_id, realm, kind)): Path<(
        String,
        StorageDocumentRealmInput,
        StorageDocumentKindInput,
    )>,
    Query(query): Query<LogicalStorageDocumentQuery>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<LogicalStorageDocumentSavePayload>,
) -> Result<Json<LogicalStorageDocumentWriteView>, (StatusCode, Json<serde_json::Value>)> {
    storage_routes::put_logical_storage_document(
        Path((wiki_id, realm, kind)),
        Query(query),
        State(state),
        headers,
        Json(payload),
    )
    .await
}

async fn get_public_storage_document(
    Path((wiki_id, kind)): Path<(String, PublicStorageDocumentRouteKind)>,
    Query(query): Query<PublicStorageDocumentQuery>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<PublicStorageDocumentView>, (StatusCode, Json<serde_json::Value>)> {
    storage_routes::get_public_storage_document(
        Path((wiki_id, kind)),
        Query(query),
        State(state),
        headers,
    )
    .await
}

async fn put_public_storage_document(
    Path((wiki_id, kind)): Path<(String, PublicStorageDocumentRouteKind)>,
    Query(query): Query<PublicStorageDocumentQuery>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<PublicStorageDocumentSavePayload>,
) -> Result<Json<PublicStorageDocumentWriteView>, (StatusCode, Json<serde_json::Value>)> {
    storage_routes::put_public_storage_document(
        Path((wiki_id, kind)),
        Query(query),
        State(state),
        headers,
        Json(payload),
    )
    .await
}

async fn server_debug_summary(state: &AppState, headers: &HeaderMap) -> ServerDebugSummary {
    let auth = current_status(state, headers, true).await;
    let oauth = state.local_oauth.status();
    let capabilities = capability_report_for_request(state, headers, "frwiki", false).await;
    ServerDebugSummary {
        project: sp42_core::branding::PROJECT_NAME.to_string(),
        auth,
        oauth,
        capabilities,
        coordination: state.coordination.snapshot().await,
    }
}

async fn server_readiness(state: &AppState, headers: &HeaderMap) -> ServerHealthStatus {
    let auth = current_status(state, headers, false).await;
    let bootstrap = bootstrap_status(state, &auth);
    let capability_probe = CapabilityProbeHint {
        wiki_id: "frwiki".to_string(),
        endpoint: "/dev/auth/capabilities/frwiki".to_string(),
        available: state.local_oauth.access_token().is_some(),
    };
    let capability_cache = capability_cache_status(state, "frwiki").await;
    let session_count = session_count(state).await;
    let coordination_snapshot = state.coordination.snapshot().await;
    let coordination_room_count = coordination_snapshot.rooms.len();
    let mut readiness_issues = Vec::new();
    let session_ready = auth.authenticated;
    let bootstrap_ready = bootstrap.bootstrap_ready;

    if !bootstrap_ready {
        readiness_issues
            .push("Local token bootstrap is unavailable because WIKIMEDIA_ACCESS_TOKEN is not set in process environment, .env.wikimedia.local, or .env.".to_string());
    }
    if !session_ready && !bootstrap_ready {
        readiness_issues.push("No dev session is bootstrapped yet.".to_string());
    }
    if !capability_cache.present {
        readiness_issues.push("No capability cache has been populated yet.".to_string());
    }

    ServerHealthStatus {
        project: sp42_core::branding::PROJECT_NAME.to_string(),
        ready_for_local_testing: session_ready || bootstrap_ready,
        readiness_issues,
        uptime_ms: uptime_ms(&state.started_at),
        session_count,
        coordination_room_count,
        auth,
        oauth: state.local_oauth.status(),
        bootstrap,
        capability_probe,
        capability_cache,
        operator_report_path: OPERATOR_REPORT_PATH.to_string(),
        coordination: coordination_snapshot,
    }
}

async fn runtime_debug(state: &AppState, headers: &HeaderMap) -> RuntimeDebugStatus {
    let auth = current_status(state, headers, true).await;
    let bootstrap = bootstrap_status(state, &auth);
    let capabilities = capability_report_for_request(state, headers, "frwiki", false).await;
    let capability_cache = capability_cache_status(state, "frwiki").await;
    let coordination = state.coordination.snapshot().await;

    RuntimeDebugStatus {
        project: sp42_core::branding::PROJECT_NAME.to_string(),
        uptime_ms: uptime_ms(&state.started_at),
        session_count: session_count(state).await,
        coordination_room_count: coordination.rooms.len(),
        bootstrap,
        auth,
        oauth: state.local_oauth.status(),
        capabilities,
        capability_cache,
        operator_report_path: OPERATOR_REPORT_PATH.to_string(),
        coordination,
    }
}

async fn operator_storage_layout_view(
    state: &AppState,
    headers: &HeaderMap,
    wiki_id: &str,
    query: &StorageLayoutQuery,
) -> Result<OperatorStorageLayoutView, String> {
    let input = storage_plan_input(
        state,
        headers,
        wiki_id,
        StoragePlanRequest {
            username_override: query.username.clone(),
            home_wiki_id_override: query.home_wiki_id.clone(),
            shared_owner_username_override: query.shared_owner_username.clone(),
            team_slugs: query.team_slug.clone().into_iter().collect(),
            rule_set_slugs: query.rule_set_slug.clone().into_iter().collect(),
            training_dataset_slugs: query.training_dataset_slug.clone().into_iter().collect(),
            audit_period_slugs: query.audit_period_slug.clone().into_iter().collect(),
        },
    )
    .await?;
    let plan = build_wiki_storage_plan(&WikiStorageConfig::default(), &input);
    let personal_index_page =
        render_wiki_storage_index_page(&plan.personal_root, &plan.personal_documents, &plan.notes);
    let shared_registry_page =
        render_wiki_storage_index_page(&plan.shared_root, &plan.shared_documents, &plan.notes);
    let sample_document_pages = plan
        .personal_documents
        .iter()
        .chain(plan.shared_documents.iter())
        .take(3)
        .map(|document| {
            let body = render_wiki_storage_document_page(
                document,
                &[format!(
                    "Canonical public SP42 document for `{}`.",
                    document.title
                )],
                &serde_json::json!({
                    "wiki_id": wiki_id,
                    "owner": input.shared_owner_username,
                    "subject": input.username,
                    "document": document.title,
                }),
            )
            .map_err(|error| error.to_string())?;
            Ok(RenderedStorageDocumentPage {
                title: document.title.clone(),
                body,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    Ok(OperatorStorageLayoutView {
        plan,
        personal_index_page,
        shared_registry_page,
        sample_document_pages,
    })
}

async fn storage_plan_input(
    state: &AppState,
    headers: &HeaderMap,
    wiki_id: &str,
    request: StoragePlanRequest,
) -> Result<WikiStoragePlanInput, String> {
    let session = current_session_snapshot(state, headers, false).await;
    let username = request
        .username_override
        .or_else(|| session.as_ref().map(|session| session.username.clone()))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "username is required via query or authenticated session".to_string())?;
    let shared_owner_username = request
        .shared_owner_username_override
        .unwrap_or_else(|| username.clone());

    Ok(WikiStoragePlanInput {
        username,
        home_wiki_id: request
            .home_wiki_id_override
            .unwrap_or_else(|| wiki_id.to_string()),
        target_wiki_id: wiki_id.to_string(),
        shared_owner_username,
        team_slugs: request.team_slugs,
        rule_set_slugs: request.rule_set_slugs,
        training_dataset_slugs: request.training_dataset_slugs,
        audit_period_slugs: request.audit_period_slugs,
    })
}

async fn resolve_logical_storage_document(
    state: &AppState,
    headers: &HeaderMap,
    wiki_id: &str,
    realm: &StorageDocumentRealmInput,
    kind: &StorageDocumentKindInput,
    query: &LogicalStorageDocumentQuery,
) -> Result<WikiStorageDocument, String> {
    let slug = query
        .slug
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let input = storage_plan_input(
        state,
        headers,
        wiki_id,
        StoragePlanRequest {
            username_override: query.username.clone(),
            home_wiki_id_override: query.home_wiki_id.clone(),
            shared_owner_username_override: query.shared_owner_username.clone(),
            team_slugs: slug.clone().into_iter().collect(),
            rule_set_slugs: slug.clone().into_iter().collect(),
            training_dataset_slugs: slug.clone().into_iter().collect(),
            audit_period_slugs: slug.clone().into_iter().collect(),
        },
    )
    .await?;
    let plan = build_wiki_storage_plan(&WikiStorageConfig::default(), &input);
    let document_kind = logical_storage_document_kind(wiki_id, realm, kind, slug)?;

    resolve_wiki_storage_document(&plan, &document_kind)
        .ok_or_else(|| format!("no canonical storage document matched `{document_kind:?}`"))
}

fn logical_storage_document_kind(
    wiki_id: &str,
    realm: &StorageDocumentRealmInput,
    kind: &StorageDocumentKindInput,
    slug: Option<String>,
) -> Result<WikiStorageDocumentKind, String> {
    match (realm, kind) {
        (StorageDocumentRealmInput::Personal, StorageDocumentKindInput::Index) => {
            Ok(WikiStorageDocumentKind::PersonalIndex)
        }
        (StorageDocumentRealmInput::Personal, StorageDocumentKindInput::Profile) => {
            Ok(WikiStorageDocumentKind::PersonalProfile)
        }
        (StorageDocumentRealmInput::Personal, StorageDocumentKindInput::Preferences) => {
            Ok(WikiStorageDocumentKind::PersonalPreferences)
        }
        (StorageDocumentRealmInput::Personal, StorageDocumentKindInput::Queue) => {
            Ok(WikiStorageDocumentKind::PersonalQueue {
                wiki_id: wiki_id.to_string(),
            })
        }
        (StorageDocumentRealmInput::Personal, StorageDocumentKindInput::Workspace) => {
            Ok(WikiStorageDocumentKind::PersonalWorkspace {
                wiki_id: wiki_id.to_string(),
            })
        }
        (StorageDocumentRealmInput::Personal, StorageDocumentKindInput::Labels) => {
            Ok(WikiStorageDocumentKind::PersonalLabels {
                wiki_id: wiki_id.to_string(),
            })
        }
        (StorageDocumentRealmInput::Shared, StorageDocumentKindInput::Registry) => {
            Ok(WikiStorageDocumentKind::SharedRegistry {
                wiki_id: wiki_id.to_string(),
            })
        }
        (StorageDocumentRealmInput::Shared, StorageDocumentKindInput::Team) => {
            Ok(WikiStorageDocumentKind::SharedTeam {
                wiki_id: wiki_id.to_string(),
                team_slug: require_logical_storage_slug(kind, slug)?,
            })
        }
        (StorageDocumentRealmInput::Shared, StorageDocumentKindInput::RuleSet) => {
            Ok(WikiStorageDocumentKind::SharedRuleSet {
                wiki_id: wiki_id.to_string(),
                rule_set_slug: require_logical_storage_slug(kind, slug)?,
            })
        }
        (StorageDocumentRealmInput::Shared, StorageDocumentKindInput::TrainingDataset) => {
            Ok(WikiStorageDocumentKind::SharedTrainingDataset {
                wiki_id: wiki_id.to_string(),
                dataset_slug: require_logical_storage_slug(kind, slug)?,
            })
        }
        (StorageDocumentRealmInput::Shared, StorageDocumentKindInput::AuditPeriod) => {
            Ok(WikiStorageDocumentKind::SharedAuditPeriod {
                wiki_id: wiki_id.to_string(),
                period_slug: require_logical_storage_slug(kind, slug)?,
            })
        }
        _ => Err(format!(
            "logical storage document `{realm:?}/{kind:?}` is not supported"
        )),
    }
}

fn require_logical_storage_slug(
    kind: &StorageDocumentKindInput,
    slug: Option<String>,
) -> Result<String, String> {
    slug.ok_or_else(|| format!("slug is required for shared `{kind:?}` documents"))
}

async fn operator_report(state: &AppState, headers: &HeaderMap) -> OperatorReport {
    let readiness = server_readiness(state, headers).await;
    let runtime = runtime_debug(state, headers).await;
    let bootstrap = readiness.bootstrap.clone();
    let debug_summary = server_debug_summary(state, headers).await;

    OperatorReport {
        project: sp42_core::branding::PROJECT_NAME.to_string(),
        readiness,
        runtime,
        bootstrap,
        debug_summary,
        endpoints: operator_endpoint_manifest(),
    }
}

fn runtime_storage_for(state: &AppState) -> FileStorage {
    FileStorage::new(state.runtime_storage_root.clone())
}

async fn operator_runtime_inspection(
    state: &AppState,
    wiki_id: &str,
) -> Result<OperatorRuntimeInspection, String> {
    if let Some(snapshot) = supervisor_snapshot_for_wiki(state, wiki_id).await {
        let mut notes = snapshot.status.notes.clone();
        notes.push(
            "Runtime inspection is backed by the continuous ingestion supervisor.".to_string(),
        );
        return Ok(OperatorRuntimeInspection {
            wiki_id: wiki_id.to_string(),
            storage_root: state.runtime_storage_root.display().to_string(),
            backlog: snapshot
                .status
                .backlog_status
                .unwrap_or(sp42_core::BacklogRuntimeStatus {
                    checkpoint_key: format!("recentchanges.rccontinue.{wiki_id}"),
                    next_continue: snapshot.next_continue,
                    last_batch_size: 0,
                    total_events: 0,
                    poll_count: 0,
                }),
            stream_checkpoint_key: format!("stream.last_event_id.{wiki_id}"),
            stream_last_event_id: snapshot
                .status
                .stream_status
                .and_then(|status| status.last_event_id),
            notes,
        });
    }

    let config = resolved_wiki_config(state, wiki_id)?;
    let storage = runtime_storage_for(state);
    let mut backlog = BacklogRuntime::new(
        config,
        storage.clone(),
        BacklogRuntimeConfig {
            limit: 15,
            include_bots: false,
        },
        format!("recentchanges.rccontinue.{wiki_id}"),
    );
    backlog
        .initialize()
        .await
        .map_err(|error| format!("backlog runtime init failed: {error}"))?;
    let backlog_status = backlog.status();
    let stream_checkpoint_key = format!("stream.last_event_id.{wiki_id}");
    let stream_last_event_id = storage
        .get(&stream_checkpoint_key)
        .await
        .map_err(|error| format!("stream checkpoint read failed: {error}"))?
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .filter(|value| !value.trim().is_empty());

    let mut notes = vec![format!("runtime_storage_root={}", storage.root().display())];
    notes.push(format!(
        "backlog_next_continue={}",
        backlog_status.next_continue.as_deref().unwrap_or("none")
    ));
    notes.push(format!(
        "stream_last_event_id={}",
        stream_last_event_id.as_deref().unwrap_or("none")
    ));

    Ok(OperatorRuntimeInspection {
        wiki_id: wiki_id.to_string(),
        storage_root: storage.root().display().to_string(),
        backlog: backlog_status,
        stream_checkpoint_key,
        stream_last_event_id,
        notes,
    })
}

async fn persisted_stream_status(
    state: &AppState,
    wiki_id: &str,
) -> Result<StreamRuntimeStatus, String> {
    let checkpoint_key = format!("stream.last_event_id.{wiki_id}");
    let storage = runtime_storage_for(state);
    let last_event_id = storage
        .get(&checkpoint_key)
        .await
        .map_err(|error| format!("stream checkpoint read failed: {error}"))?
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .filter(|value| !value.trim().is_empty());

    Ok(StreamRuntimeStatus {
        checkpoint_key,
        last_event_id,
        delivered_events: 0,
        filtered_events: 0,
        reconnect_attempts: 0,
    })
}

async fn load_live_operator_bootstrap(
    state: &AppState,
    headers: &HeaderMap,
    wiki_id: &str,
) -> Result<LiveOperatorBootstrap, String> {
    Ok(LiveOperatorBootstrap {
        config: resolved_wiki_config(state, wiki_id)?,
        auth: current_status(state, headers, true).await,
        action_status: action_status_report(state, headers).await,
        action_history: action_history_report(state, headers, Some(10)).await,
        capabilities: capability_report_for_request(state, headers, wiki_id, false).await,
        stream_status: persisted_stream_status(state, wiki_id).await?,
    })
}

fn parsed_namespaces(namespaces: Option<&String>) -> Vec<i32> {
    namespaces.map_or_else(Vec::new, |value| {
        value
            .split(',')
            .filter_map(|entry| entry.trim().parse::<i32>().ok())
            .collect()
    })
}

fn uses_default_live_filters(query: &LiveOperatorQuery) -> bool {
    query.rccontinue.is_none()
        && !query.include_bots.is_enabled()
        && !query.unpatrolled_only.is_enabled()
        && query.include_minor.is_enabled()
        && query.include_anonymous.is_enabled()
        && query.include_registered.is_enabled()
        && query.include_temporary.is_enabled()
        && query.include_new_pages.is_enabled()
        && query.tag_filter.is_none()
        && query.namespaces.is_empty()
        && query.min_score.is_none()
}

async fn supervisor_snapshot_for_wiki(
    state: &AppState,
    wiki_id: &str,
) -> Option<IngestionSupervisorSnapshot> {
    let guard = state.ingestion_supervisor.read().await;
    guard.get(wiki_id).cloned()
}

fn live_operator_query_from_filters(filters: &LiveViewFilterParams) -> LiveOperatorQuery {
    LiveOperatorQuery {
        limit: filters.limit.clamp(1, 500),
        include_bots: filters.include_bots,
        unpatrolled_only: filters.unpatrolled_only,
        include_minor: filters.include_minor,
        include_registered: filters.include_registered,
        include_anonymous: filters.include_anonymous,
        include_temporary: filters.include_temporary,
        include_new_pages: filters.include_new_pages,
        namespaces: parsed_namespaces(filters.namespaces.as_ref()),
        min_score: filters.min_score,
        tag_filter: filters.tag_filter.clone(),
        rccontinue: filters.rccontinue.clone(),
    }
}

fn apply_public_defaults_to_live_query(
    mut query: LiveOperatorQuery,
    context: &LiveOperatorPublicContextState,
) -> LiveOperatorQuery {
    if let Some(preferences) =
        context
            .preferences
            .as_ref()
            .and_then(|resolved| match &resolved.payload {
                PublicStorageDocumentData::Preferences(value) => Some(value),
                _ => None,
            })
    {
        if query.limit == default_limit() {
            query.limit = preferences.queue_limit.clamp(1, 500);
        }
        if !preferences.hide_bots {
            query.include_bots = FlagState::Enabled;
        }
        if preferences.hide_minor {
            query.include_minor = FlagState::Disabled;
        }
        if !preferences.editor_types.is_empty() {
            query.include_registered = FlagState::from(
                preferences
                    .editor_types
                    .iter()
                    .any(|value| value == "registered"),
            );
            query.include_anonymous = FlagState::from(
                preferences
                    .editor_types
                    .iter()
                    .any(|value| value == "anonymous"),
            );
            query.include_temporary = FlagState::from(
                preferences
                    .editor_types
                    .iter()
                    .any(|value| value == "temporary"),
            );
        }
        if query.tag_filter.is_none() {
            query.tag_filter = preferences.tag_filters.first().cloned();
        }
    }

    if let Some(rule_set) =
        context
            .active_rule_set
            .as_ref()
            .and_then(|resolved| match &resolved.payload {
                PublicStorageDocumentData::RuleSet(value) => Some(value),
                _ => None,
            })
    {
        if query.namespaces.is_empty() && !rule_set.namespace_allowlist.is_empty() {
            query.namespaces.clone_from(&rule_set.namespace_allowlist);
        }
        if rule_set.hide_minor {
            query.include_minor = FlagState::Disabled;
        }
        if rule_set.hide_bots {
            query.include_bots = FlagState::Disabled;
        }
        if query.tag_filter.is_none() {
            query.tag_filter = rule_set.tag_filters.first().cloned();
        }
    }

    query
}

fn filter_ranked_queue(mut queue: Vec<QueuedEdit>, query: &LiveOperatorQuery) -> Vec<QueuedEdit> {
    if let Some(min_score) = query.min_score {
        queue.retain(|item| item.score.total >= min_score);
    }
    if !query.include_registered.is_enabled() {
        queue.retain(|item| !matches!(item.event.performer, EditorIdentity::Registered { .. }));
    }
    if !query.include_temporary.is_enabled() {
        queue.retain(|item| !matches!(item.event.performer, EditorIdentity::Temporary { .. }));
    }
    if !query.include_anonymous.is_enabled() {
        queue.retain(|item| !matches!(item.event.performer, EditorIdentity::Anonymous { .. }));
    }
    if !query.include_bots.is_enabled() {
        queue.retain(|item| !item.event.is_bot.is_enabled());
    }
    if !query.include_minor.is_enabled() {
        queue.retain(|item| !item.event.is_minor.is_enabled());
    }
    if !query.include_new_pages.is_enabled() {
        queue.retain(|item| !item.event.is_new_page.is_enabled());
    }
    if query.unpatrolled_only.is_enabled() {
        queue.retain(|item| !item.event.is_patrolled.is_enabled());
    }
    if !query.namespaces.is_empty() {
        queue.retain(|item| query.namespaces.contains(&item.event.namespace));
    }
    if let Some(tag_filter) = query.tag_filter.as_ref() {
        queue.retain(|item| item.event.tags.iter().any(|tag| tag == tag_filter));
    }
    queue.truncate(usize::from(query.limit));
    queue
}

fn queue_state_from_supervisor(
    wiki_id: &str,
    query: LiveOperatorQuery,
    snapshot: IngestionSupervisorSnapshot,
) -> LiveQueueState {
    let queue = filter_ranked_queue(snapshot.queue, &query);
    LiveQueueState {
        query,
        batch: sp42_core::RecentChangesBatch {
            events: queue.iter().map(|item| item.event.clone()).collect(),
            next_continue: snapshot.next_continue,
        },
        backlog_status: snapshot
            .status
            .backlog_status
            .unwrap_or(sp42_core::BacklogRuntimeStatus {
                checkpoint_key: format!("recentchanges.rccontinue.{wiki_id}"),
                next_continue: None,
                last_batch_size: 0,
                total_events: 0,
                poll_count: 0,
            }),
        selected_index: (!queue.is_empty()).then_some(0),
        queue,
    }
}

async fn queue_state_from_recentchanges(
    state: &AppState,
    wiki_id: &str,
    query: LiveOperatorQuery,
    config: &WikiConfig,
    client: &BearerHttpClient,
) -> Result<LiveQueueState, String> {
    let namespace_override = (!query.namespaces.is_empty()).then(|| query.namespaces.clone());
    let mut backlog_runtime = BacklogRuntime::new(
        config.clone(),
        runtime_storage_for(state),
        BacklogRuntimeConfig {
            limit: query.limit,
            include_bots: query.include_bots.is_enabled(),
        },
        format!("recentchanges.rccontinue.{wiki_id}"),
    );
    backlog_runtime
        .initialize()
        .await
        .map_err(|error| format!("backlog runtime init failed: {error}"))?;
    let uses_custom_filters = query.rccontinue.is_some()
        || query.unpatrolled_only.is_enabled()
        || !query.include_minor.is_enabled()
        || !query.include_anonymous.is_enabled()
        || !query.include_registered.is_enabled()
        || !query.include_temporary.is_enabled()
        || !query.include_new_pages.is_enabled()
        || query.tag_filter.is_some()
        || namespace_override.is_some();
    let batch = if uses_custom_filters {
        let batch = execute_recent_changes(
            client,
            config,
            &RecentChangesQuery {
                limit: query.limit,
                rccontinue: query.rccontinue.clone(),
                include_bots: query.include_bots,
                unpatrolled_only: query.unpatrolled_only,
                include_minor: query.include_minor,
                include_anonymous: query.include_anonymous,
                include_new_pages: query.include_new_pages,
                tag_filter: query.tag_filter.clone(),
                namespace_override,
            },
        )
        .await
        .map_err(|error| format!("recentchanges fetch failed: {error}"))?;
        backlog_runtime
            .apply_batch(&batch)
            .await
            .map_err(|error| format!("backlog runtime apply failed: {error}"))?;
        batch
    } else {
        backlog_runtime
            .poll(client)
            .await
            .map_err(|error| format!("recentchanges fetch failed: {error}"))?
    };
    let backlog_status = backlog_runtime.status();
    let queue = filter_ranked_queue(
        build_ranked_queue(batch.events.clone(), &config.scoring)
            .map_err(|error| format!("queue build failed: {error}"))?,
        &query,
    );

    Ok(LiveQueueState {
        query,
        batch,
        backlog_status,
        selected_index: (!queue.is_empty()).then_some(0),
        queue,
    })
}

async fn load_live_queue_state(
    state: &AppState,
    wiki_id: &str,
    filters: &LiveViewFilterParams,
    config: &WikiConfig,
    client: &BearerHttpClient,
    public_context: &LiveOperatorPublicContextState,
) -> Result<LiveQueueState, String> {
    let query = apply_public_defaults_to_live_query(
        live_operator_query_from_filters(filters),
        public_context,
    );
    if uses_default_live_filters(&query)
        && let Some(snapshot) = supervisor_snapshot_for_wiki(state, wiki_id).await
        && snapshot.status.active
    {
        return Ok(queue_state_from_supervisor(wiki_id, query, snapshot));
    }
    queue_state_from_recentchanges(state, wiki_id, query, config, client).await
}

async fn load_selected_review_state(
    state: &AppState,
    headers: &HeaderMap,
    wiki_id: &str,
    config: &WikiConfig,
    access_token: &str,
    auth: &DevAuthSessionStatus,
    selected: Option<&QueuedEdit>,
) -> Result<SelectedReviewState, String> {
    let liftwing_risk = if let Some(item) = selected {
        execute_liftwing_score(
            &BearerHttpClient::new(state.http_client.clone(), access_token.to_string()),
            config,
            &LiftWingRequest {
                rev_id: item.event.rev_id,
            },
        )
        .await
        .ok()
    } else {
        None
    };
    let scoring_context = liftwing_risk.map(|probability| {
        build_scoring_context(&ContextInputs {
            talk_page_wikitext: None,
            liftwing_probability: Some(probability),
        })
    });
    let diff = if let Some(item) = selected {
        fetch_revision_diff(&state.http_client, access_token, config, item).await?
    } else {
        None
    };
    let readiness = server_readiness(state, headers).await;
    let coordination_state = state.coordination.room_state_summary(wiki_id).await;
    let coordination_room = state
        .coordination
        .snapshot()
        .await
        .rooms
        .into_iter()
        .find(|room| room.wiki_id == wiki_id);
    let review_workbench = selected.and_then(|item| {
        build_review_workbench(
            config,
            item,
            "SP42_REDACTED_TOKEN",
            auth.username.as_deref().unwrap_or("SP42"),
            Some("Generated from live operator state"),
        )
        .ok()
    });

    Ok(SelectedReviewState {
        scoring_context,
        diff,
        review_workbench,
        readiness,
        coordination_state,
        coordination_room,
    })
}

fn build_live_operator_notes(
    query: &LiveOperatorQuery,
    backlog_status: &sp42_core::BacklogRuntimeStatus,
    queue: &[QueuedEdit],
    scoring_context: Option<&sp42_core::ScoringContext>,
    diff: Option<&sp42_core::StructuredDiff>,
    review_workbench: Option<&sp42_core::ReviewWorkbench>,
) -> Vec<String> {
    let mut notes =
        vec!["Queue and selected review are built from live recent changes.".to_string()];
    notes.push(format!(
        "Applied filters: limit={} include_bots={} unpatrolled_only={} include_minor={} namespaces={} min_score={} rccontinue={}",
        query.limit,
        query.include_bots,
        query.unpatrolled_only,
        query.include_minor,
        if query.namespaces.is_empty() {
            "default".to_string()
        } else {
            query
                .namespaces
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        },
        query
            .min_score
            .map_or_else(|| "none".to_string(), |value| value.to_string()),
        query.rccontinue.as_deref().unwrap_or("none"),
    ));
    if review_workbench.is_some() {
        notes.push(
            "Safe review workbench previews are attached with a redacted token placeholder."
                .to_string(),
        );
    }
    notes.push(format!(
        "Persistent backlog checkpoint={} next_continue={} total_events={} poll_count={}",
        backlog_status.checkpoint_key,
        backlog_status.next_continue.as_deref().unwrap_or("none"),
        backlog_status.total_events,
        backlog_status.poll_count
    ));
    if scoring_context.is_none() {
        notes.push("LiftWing score is unavailable for the selected edit right now.".to_string());
    }
    if diff.is_none() {
        notes.push("Live diff could not be loaded for the selected edit.".to_string());
    }
    if queue.is_empty() {
        notes.push("No edits matched the current live filter set.".to_string());
    }
    notes
}

fn build_live_operator_products(
    wiki_id: &str,
    queue: &[QueuedEdit],
    selected: Option<&QueuedEdit>,
    selected_review: &SelectedReviewState,
    context: &LiveOperatorProductContext<'_>,
) -> LiveOperatorProducts {
    let scenario_report = build_patrol_scenario_report(&PatrolScenarioReportInputs {
        queue,
        selected,
        scoring_context: selected_review.scoring_context.as_ref(),
        diff: selected_review.diff.as_ref(),
        review_workbench: selected_review.review_workbench.as_ref(),
        stream_status: Some(context.stream_status),
        backlog_status: Some(context.backlog_status),
        coordination: selected_review.coordination_state.as_ref(),
        wiki_id_hint: Some(wiki_id),
    });
    let session_digest = build_patrol_session_digest(&PatrolSessionDigestInputs {
        report: &scenario_report,
        review_workbench: selected_review.review_workbench.as_ref(),
    });
    let shell_state = build_shell_state_model(&ShellStateInputs {
        report: &scenario_report,
        review_workbench: selected_review.review_workbench.as_ref(),
    });
    let backend = live_operator_backend_status(&selected_review.readiness, context.auth);
    let debug_snapshot = build_debug_snapshot(&DebugSnapshotInputs {
        queue,
        selected,
        scoring_context: selected_review.scoring_context.as_ref(),
        diff: selected_review.diff.as_ref(),
        review_workbench: selected_review.review_workbench.as_ref(),
        stream_status: Some(context.stream_status),
        backlog_status: Some(context.backlog_status),
        coordination: selected_review.coordination_state.as_ref(),
    });
    let action_preflight =
        build_live_operator_action_preflight(selected, context.capabilities, context.action_status);

    LiveOperatorProducts {
        scenario_report,
        session_digest,
        shell_state,
        backend,
        debug_snapshot,
        action_preflight,
    }
}

fn finalize_live_operator_view(
    state: &AppState,
    wiki_id: &str,
    finalized: LiveOperatorFinalization,
) -> LiveOperatorView {
    let LiveOperatorFinalization {
        queue_state,
        bootstrap,
        selected_review,
        products,
        public_documents,
        telemetry,
        notes,
        ingestion_supervisor,
    } = finalized;

    LiveOperatorView {
        project: sp42_core::branding::PROJECT_NAME.to_string(),
        fetched_at_ms: state.clock.now_ms(),
        wiki_id: wiki_id.to_string(),
        query: queue_state.query,
        queue: queue_state.queue,
        selected_index: queue_state.selected_index,
        scoring_context: selected_review.scoring_context,
        diff: selected_review.diff,
        review_workbench: selected_review.review_workbench,
        stream_status: Some(bootstrap.stream_status),
        backlog_status: Some(queue_state.backlog_status),
        scenario_report: products.scenario_report,
        session_digest: products.session_digest,
        shell_state: products.shell_state,
        capabilities: bootstrap.capabilities,
        auth: bootstrap.auth,
        backend: products.backend,
        action_status: bootstrap.action_status,
        action_history: bootstrap.action_history,
        action_preflight: products.action_preflight,
        public_documents,
        ingestion_supervisor,
        coordination_room: selected_review.coordination_room,
        coordination_state: selected_review.coordination_state,
        debug_snapshot: products.debug_snapshot,
        telemetry,
        notes,
        next_continue: queue_state.batch.next_continue,
    }
}

async fn access_token_for_request(state: &AppState, headers: &HeaderMap) -> Option<String> {
    current_session_snapshot(state, headers, true)
        .await
        .map(|session| session.access_token)
        .or_else(|| state.local_oauth.access_token().map(ToString::to_string))
}

async fn fetch_revision_diff(
    client: &reqwest::Client,
    access_token: &str,
    config: &WikiConfig,
    item: &QueuedEdit,
) -> Result<Option<sp42_core::StructuredDiff>, String> {
    let Some(old_rev_id) = item.event.old_rev_id else {
        return Ok(None);
    };

    let revisions = fetch_revision_texts(
        client,
        access_token,
        config,
        &[old_rev_id, item.event.rev_id],
    )
    .await?;
    let Some(before) = revisions.get(&old_rev_id) else {
        return Ok(None);
    };
    let Some(after) = revisions.get(&item.event.rev_id) else {
        return Ok(None);
    };

    Ok(Some(diff_lines(before, after)))
}

async fn fetch_revision_texts(
    client: &reqwest::Client,
    access_token: &str,
    config: &WikiConfig,
    revision_ids: &[u64],
) -> Result<HashMap<u64, String>, String> {
    if revision_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let revids = revision_ids
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("|");
    let response = client
        .get(config.api_url.clone())
        .bearer_auth(access_token)
        .query(&[
            ("action", "query"),
            ("prop", "revisions"),
            ("revids", revids.as_str()),
            ("rvprop", "ids|content"),
            ("rvslots", "main"),
            ("format", "json"),
            ("formatversion", "2"),
        ])
        .send()
        .await
        .map_err(|error| format!("revision lookup transport failed: {error}"))?;
    let status = response.status();
    let body = response
        .bytes()
        .await
        .map_err(|error| format!("revision lookup body failed: {error}"))?;
    if !status.is_success() {
        return Err(format!(
            "revision lookup failed with HTTP {}: {}",
            status.as_u16(),
            truncate_response_body(&body)
        ));
    }

    let parsed: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|error| format!("revision lookup JSON failed: {error}"))?;
    let mut map = HashMap::new();
    let pages = parsed
        .get("query")
        .and_then(|value| value.get("pages"))
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "revision lookup payload does not contain query.pages".to_string())?;

    for page in pages {
        let Some(revisions) = page.get("revisions").and_then(serde_json::Value::as_array) else {
            continue;
        };
        for revision in revisions {
            let Some(rev_id) = revision.get("revid").and_then(serde_json::Value::as_u64) else {
                continue;
            };
            let content = revision
                .get("slots")
                .and_then(|value| value.get("main"))
                .and_then(|value| value.get("content"))
                .and_then(serde_json::Value::as_str)
                .map(ToString::to_string);
            if let Some(content) = content {
                map.insert(rev_id, content);
            }
        }
    }

    Ok(map)
}

fn operator_endpoint_manifest() -> Vec<OperatorEndpointDescriptor> {
    let mut endpoints = operator_core_endpoints();
    endpoints.extend(operator_storage_endpoints());
    endpoints.extend(operator_dev_endpoints());
    endpoints
}

fn operator_core_endpoints() -> Vec<OperatorEndpointDescriptor> {
    vec![
        OperatorEndpointDescriptor {
            method: "GET".to_string(),
            path: "/healthz".to_string(),
            purpose: "Minimal health indicator for probes and process supervisors.".to_string(),
            available: true,
        },
        OperatorEndpointDescriptor {
            method: "GET".to_string(),
            path: "/debug/summary".to_string(),
            purpose: "Shared auth, capability, and coordination summary.".to_string(),
            available: true,
        },
        OperatorEndpointDescriptor {
            method: "GET".to_string(),
            path: "/debug/runtime".to_string(),
            purpose: "Runtime-oriented operator state with cache and room counts.".to_string(),
            available: true,
        },
        OperatorEndpointDescriptor {
            method: "GET".to_string(),
            path: OPERATOR_READINESS_PATH.to_string(),
            purpose: "Consolidated operator readiness report.".to_string(),
            available: true,
        },
        OperatorEndpointDescriptor {
            method: "GET".to_string(),
            path: OPERATOR_REPORT_PATH.to_string(),
            purpose: "Full operator report with debug summary and endpoint manifest.".to_string(),
            available: true,
        },
        OperatorEndpointDescriptor {
            method: "GET".to_string(),
            path: "/operator/live/{wiki_id}".to_string(),
            purpose: "Authoritative live patrol queue, selected review details, backend auth status, and shell state.".to_string(),
            available: true,
        },
        OperatorEndpointDescriptor {
            method: "GET".to_string(),
            path: "/operator/runtime/{wiki_id}".to_string(),
            purpose: "Persistent backlog and stream checkpoint inspection for the selected wiki.".to_string(),
            available: true,
        },
        OperatorEndpointDescriptor {
            method: "GET".to_string(),
            path: format!("{OPERATOR_STORAGE_LAYOUT_PATH}/{{wiki_id}}"),
            purpose: "Canonical personal/shared on-wiki storage layout and sample page renderings.".to_string(),
            available: true,
        },
    ]
}

fn operator_storage_endpoints() -> Vec<OperatorEndpointDescriptor> {
    vec![
        OperatorEndpointDescriptor {
            method: "GET".to_string(),
            path: "/operator/storage/document/{wiki_id}?title=...".to_string(),
            purpose: "Load a canonical public SP42 on-wiki document and parse its machine payload."
                .to_string(),
            available: true,
        },
        OperatorEndpointDescriptor {
            method: "PUT".to_string(),
            path: "/operator/storage/document/{wiki_id}".to_string(),
            purpose: "Save a canonical public SP42 on-wiki document with conflict-aware writes."
                .to_string(),
            available: true,
        },
        OperatorEndpointDescriptor {
            method: "GET".to_string(),
            path: "/operator/storage/logical/{wiki_id}/{realm}/{kind}".to_string(),
            purpose: "Resolve a canonical SP42 public document by logical kind and load its current on-wiki content.".to_string(),
            available: true,
        },
        OperatorEndpointDescriptor {
            method: "PUT".to_string(),
            path: "/operator/storage/logical/{wiki_id}/{realm}/{kind}".to_string(),
            purpose: "Save a canonical SP42 public document by logical kind without exposing raw wiki titles to clients.".to_string(),
            available: true,
        },
        OperatorEndpointDescriptor {
            method: "GET".to_string(),
            path: "/operator/storage/public/{wiki_id}/{kind}".to_string(),
            purpose: "Load a typed public SP42 document like preferences, registry, team, rules, or audit ledger.".to_string(),
            available: true,
        },
        OperatorEndpointDescriptor {
            method: "PUT".to_string(),
            path: "/operator/storage/public/{wiki_id}/{kind}".to_string(),
            purpose: "Save a typed public SP42 document while keeping durable state on canonical wiki pages.".to_string(),
            available: true,
        },
    ]
}

fn operator_dev_endpoints() -> Vec<OperatorEndpointDescriptor> {
    vec![
        OperatorEndpointDescriptor {
            method: "GET".to_string(),
            path: "/dev/auth/bootstrap/status".to_string(),
            purpose: "Authoritative local token bootstrap and source-report status.".to_string(),
            available: true,
        },
        OperatorEndpointDescriptor {
            method: "GET".to_string(),
            path: "/dev/auth/capabilities/frwiki".to_string(),
            purpose: "Capability probe for the default wiki.".to_string(),
            available: true,
        },
        OperatorEndpointDescriptor {
            method: "GET".to_string(),
            path: ACTION_STATUS_PATH.to_string(),
            purpose: "Current shell feedback and latest action result.".to_string(),
            available: true,
        },
        OperatorEndpointDescriptor {
            method: "GET".to_string(),
            path: ACTION_HISTORY_PATH.to_string(),
            purpose: "Recent local action execution history.".to_string(),
            available: true,
        },
        OperatorEndpointDescriptor {
            method: "GET".to_string(),
            path: "/coordination/rooms".to_string(),
            purpose: "Coordination room inventory and summaries.".to_string(),
            available: true,
        },
        OperatorEndpointDescriptor {
            method: "GET".to_string(),
            path: "/coordination/inspections".to_string(),
            purpose: "Room-by-room coordination inspection collection.".to_string(),
            available: true,
        },
    ]
}

async fn session_count(state: &AppState) -> usize {
    let mut sessions = state.sessions.write().await;
    prune_expired_sessions(&mut sessions, state.clock.now_ms());
    sessions.len()
}

async fn capability_cache_status(state: &AppState, wiki_id: &str) -> CapabilityCacheStatus {
    let guard = state.capability_cache.read().await;
    let Some(cache) = guard.as_ref() else {
        return CapabilityCacheStatus {
            present: false,
            fresh: false,
            age_ms: None,
            wiki_id: None,
        };
    };

    let age_ms = state.clock.now_ms().saturating_sub(cache.fetched_at_ms);
    let valid =
        cache.report.wiki_id == wiki_id && cache.report.checked && cache.report.error.is_none();
    CapabilityCacheStatus {
        present: valid,
        fresh: valid && cache_is_fresh(cache, state.clock.now_ms()),
        age_ms: Some(u64::try_from(age_ms).unwrap_or(u64::MAX)),
        wiki_id: Some(cache.report.wiki_id.clone()),
    }
}

async fn capability_report_for_request(
    state: &AppState,
    headers: &HeaderMap,
    wiki_id: &str,
    force_refresh: bool,
) -> DevAuthCapabilityReport {
    if let Some(session) = current_session_snapshot(state, headers, true).await {
        return capability_report_for_subject(
            state,
            CapabilityProbeSubject::Session(&session),
            wiki_id,
            force_refresh,
        )
        .await;
    }

    capability_report_for_subject(
        state,
        CapabilityProbeSubject::LocalToken,
        wiki_id,
        force_refresh,
    )
    .await
}

async fn cached_capability_report_for_subject(
    state: &AppState,
    subject: &CapabilityProbeSubject<'_>,
    wiki_id: &str,
    force_refresh: bool,
) -> Option<DevAuthCapabilityReport> {
    if force_refresh {
        return None;
    }

    match subject {
        CapabilityProbeSubject::LocalToken => {
            let guard = state.capability_cache.read().await;
            if let Some(cache) = guard.as_ref()
                && cache.report.wiki_id == wiki_id
                && cache_is_fresh(cache, state.clock.now_ms())
            {
                return Some(cache.report.clone());
            }
        }
        CapabilityProbeSubject::Session(session) => {
            if let Some(report) =
                cached_capabilities_for_session(state, &session.session_id, wiki_id).await
            {
                return Some(report);
            }
        }
    }

    None
}

fn capability_probe_token<'a>(
    state: &'a AppState,
    subject: &'a CapabilityProbeSubject<'a>,
) -> Option<&'a str> {
    match subject {
        CapabilityProbeSubject::LocalToken => state.local_oauth.access_token(),
        CapabilityProbeSubject::Session(session) => Some(session.access_token.as_str()),
    }
}

fn log_capability_probe_result(
    subject: &CapabilityProbeSubject<'_>,
    wiki_id: &str,
    report: &DevAuthCapabilityReport,
) {
    if let Some(error) = &report.error {
        match subject {
            CapabilityProbeSubject::LocalToken => {
                warn!(wiki_id, error, "local capability probe failed");
            }
            CapabilityProbeSubject::Session(session) => {
                warn!(
                    session_id = session.session_id.as_str(),
                    wiki_id, error, "session capability probe failed"
                );
            }
        }
    } else {
        match subject {
            CapabilityProbeSubject::LocalToken => {
                info!(wiki_id, username = ?report.username, "local capability probe succeeded");
            }
            CapabilityProbeSubject::Session(session) => {
                info!(
                    session_id = session.session_id.as_str(),
                    wiki_id,
                    username = ?report.username,
                    "session capability probe succeeded"
                );
            }
        }
    }
}

async fn store_capability_report_for_subject(
    state: &AppState,
    subject: &CapabilityProbeSubject<'_>,
    report: &DevAuthCapabilityReport,
) {
    match subject {
        CapabilityProbeSubject::LocalToken => {
            let mut guard = state.capability_cache.write().await;
            *guard = Some(CachedCapabilityReport {
                fetched_at_ms: state.clock.now_ms(),
                report: report.clone(),
            });
        }
        CapabilityProbeSubject::Session(session) => {
            store_capabilities_for_session(state, &session.session_id, report).await;
        }
    }
}

async fn capability_report_for_subject(
    state: &AppState,
    subject: CapabilityProbeSubject<'_>,
    wiki_id: &str,
    force_refresh: bool,
) -> DevAuthCapabilityReport {
    if let Some(report) =
        cached_capability_report_for_subject(state, &subject, wiki_id, force_refresh).await
    {
        return report;
    }

    debug_assert!(!wiki_id.is_empty());
    let oauth = state.local_oauth.status();
    let report = probe_with_targets(
        &state.http_client,
        capability_probe_token(state, &subject),
        &oauth,
        wiki_id,
        &state.capability_targets,
    )
    .await;
    log_capability_probe_result(&subject, wiki_id, &report);
    store_capability_report_for_subject(state, &subject, &report).await;
    report
}

async fn capability_report_for_local_token(
    state: &AppState,
    wiki_id: &str,
    force_refresh: bool,
) -> DevAuthCapabilityReport {
    capability_report_for_subject(
        state,
        CapabilityProbeSubject::LocalToken,
        wiki_id,
        force_refresh,
    )
    .await
}

async fn capability_report_for_session(
    state: &AppState,
    session: &SessionSnapshot,
    wiki_id: &str,
    force_refresh: bool,
) -> DevAuthCapabilityReport {
    capability_report_for_subject(
        state,
        CapabilityProbeSubject::Session(session),
        wiki_id,
        force_refresh,
    )
    .await
}

async fn get_runtime_debug(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Json<RuntimeDebugStatus> {
    auth_routes::get_runtime_debug(State(state), headers).await
}

async fn get_auth_login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AuthLoginQuery>,
) -> Result<Redirect, (StatusCode, Json<serde_json::Value>)> {
    auth_routes::get_auth_login(State(state), headers, Query(query)).await
}

async fn get_auth_callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    auth_routes::get_auth_callback(State(state), headers, OriginalUri(uri)).await
}

async fn get_auth_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Json<OAuthSessionView> {
    auth_routes::get_auth_session(State(state), headers).await
}

async fn post_auth_logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    auth_routes::post_auth_logout(State(state), headers).await
}

async fn get_session(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    auth_routes::get_session(State(state), headers).await
}

async fn get_capabilities(
    Path(wiki_id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Json<DevAuthCapabilityReport> {
    auth_routes::get_capabilities(Path(wiki_id), State(state), headers).await
}

async fn get_action_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Json<ActionExecutionStatusReport> {
    Json(action_status_report(&state, &headers).await)
}

async fn get_action_history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ActionHistoryQuery>,
) -> Json<ActionExecutionHistoryReport> {
    Json(action_history_report(&state, &headers, query.limit).await)
}

async fn post_bootstrap_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<DevAuthBootstrapRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    auth_routes::post_bootstrap_session(State(state), headers, Json(payload)).await
}

async fn delete_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    auth_routes::delete_session(State(state), headers).await
}

async fn get_bootstrap_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Json<DevAuthBootstrapStatus> {
    auth_routes::get_bootstrap_status(State(state), headers).await
}

fn action_response_payload(
    payload: &SessionActionExecutionRequest,
    actor: String,
    response: &sp42_core::HttpResponse,
    summary: &sp42_core::ActionResponseSummary,
    response_preview: &str,
) -> SessionActionExecutionResponse {
    SessionActionExecutionResponse {
        wiki_id: payload.wiki_id.clone(),
        kind: payload.kind,
        rev_id: payload.rev_id,
        accepted: true,
        actor: Some(actor),
        http_status: Some(response.status),
        api_code: summary.api_code.clone(),
        retryable: summary.retryable,
        warnings: summary.warnings.clone(),
        result: summary.result.clone(),
        message: Some(format!(
            "MediaWiki HTTP {} {}",
            response.status, response_preview
        )),
    }
}

async fn record_action_side_effects(
    state: &AppState,
    session: &SessionSnapshot,
    headers: &HeaderMap,
    payload: &SessionActionExecutionRequest,
    log_entry: &ActionExecutionLogEntry,
) -> Option<String> {
    record_action_execution(state, &session.session_id, log_entry.clone()).await;
    storage_routes::append_public_audit_entry(state, headers, session, payload, log_entry)
        .await
        .err()
}

async fn handle_action_success(
    state: &AppState,
    session: &SessionSnapshot,
    headers: &HeaderMap,
    payload: &SessionActionExecutionRequest,
    executed_at_ms: i64,
    response: sp42_core::HttpResponse,
) -> Result<(StatusCode, Json<SessionActionExecutionResponse>), (StatusCode, Json<serde_json::Value>)>
{
    let response_preview = truncate_response_body(&response.body);
    let response_summary =
        sp42_core::parse_action_response_summary(&response, payload.kind.label())
            .map_err(|error| action_error_response(&error))?;
    let log_entry = build_action_log_entry(
        executed_at_ms,
        payload,
        ActionLogOutcome {
            accepted: true,
            http_status: Some(response.status),
            api_code: response_summary.api_code.clone(),
            retryable: response_summary.retryable,
            warnings: response_summary.warnings.clone(),
            result: response_summary.result.clone(),
            response_preview: Some(response_preview.clone()),
            error: None,
        },
    );
    let audit_warning =
        record_action_side_effects(state, session, headers, payload, &log_entry).await;
    let mut response_payload = action_response_payload(
        payload,
        session.username.clone(),
        &response,
        &response_summary,
        &response_preview,
    );
    if let Some(warning) = audit_warning {
        response_payload
            .warnings
            .push(format!("public audit write failed: {warning}"));
    }

    Ok((StatusCode::OK, Json(response_payload)))
}

async fn handle_action_failure(
    state: &AppState,
    session: &SessionSnapshot,
    headers: &HeaderMap,
    payload: &SessionActionExecutionRequest,
    executed_at_ms: i64,
    error: ActionError,
) -> (StatusCode, Json<serde_json::Value>) {
    let (api_code, retryable, logged_http_status) = match &error {
        ActionError::Execution {
            code,
            http_status,
            retryable,
            ..
        } => (code.clone(), *retryable, *http_status),
    };
    let api_error = action_error_response(&error);
    let status = api_error.0.as_u16();
    let error_message = action_error_message(&api_error.1);
    let log_entry = build_action_log_entry(
        executed_at_ms,
        payload,
        ActionLogOutcome {
            accepted: false,
            http_status: logged_http_status.or(Some(status)),
            api_code,
            retryable,
            warnings: Vec::new(),
            result: None,
            response_preview: None,
            error: Some(error_message),
        },
    );
    let audit_warning =
        record_action_side_effects(state, session, headers, payload, &log_entry).await;
    if let Some(warning) = audit_warning {
        let mut body = api_error.1.0;
        body["audit_warning"] = serde_json::Value::String(warning);
        (api_error.0, Json(body))
    } else {
        api_error
    }
}

async fn post_execute_action(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<SessionActionExecutionRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let Some(session) = current_session_snapshot(&state, &headers, true).await else {
        return Err(unauthorized_error(
            "No authenticated bridge session is active.",
        ));
    };

    let capabilities =
        capability_report_for_session(&state, &session, &payload.wiki_id, false).await;
    validate_action_request(&payload, &capabilities)?;
    if matches!(payload.kind, SessionActionKind::Undo) && payload.undo_after_rev_id.is_none() {
        return Err(invalid_payload("undo_after_rev_id is required for undo"));
    }
    let config = config_for_state_wiki(&state, &payload.wiki_id)?;
    let client = BearerHttpClient::new(state.http_client.clone(), session.access_token.clone());
    let executed_at_ms = state.clock.now_ms();
    let outcome = execute_session_action(&client, &config, &payload).await;
    info!(
        session_id = session.session_id.as_str(),
        wiki_id = payload.wiki_id.as_str(),
        rev_id = payload.rev_id,
        kind = ?payload.kind,
        "executing session action"
    );

    match outcome {
        Ok(response) => {
            let result = handle_action_success(
                &state,
                &session,
                &headers,
                &payload,
                executed_at_ms,
                response,
            )
            .await?;
            Ok(result)
        }
        Err(error) => {
            Err(
                handle_action_failure(&state, &session, &headers, &payload, executed_at_ms, error)
                    .await,
            )
        }
    }
}

async fn execute_session_action(
    client: &BearerHttpClient,
    config: &sp42_core::WikiConfig,
    payload: &SessionActionExecutionRequest,
) -> Result<sp42_core::HttpResponse, ActionError> {
    match payload.kind {
        SessionActionKind::Rollback => {
            let token = execute_fetch_token(client, config, TokenKind::Rollback).await?;
            execute_rollback(
                client,
                config,
                &sp42_core::RollbackRequest {
                    title: payload.title.clone().unwrap_or_default(),
                    user: payload.target_user.clone().unwrap_or_default(),
                    token,
                    summary: payload.summary.clone(),
                },
            )
            .await
        }
        SessionActionKind::Patrol => {
            let token = execute_fetch_token(client, config, TokenKind::Patrol).await?;
            execute_patrol(
                client,
                config,
                &sp42_core::PatrolRequest {
                    rev_id: payload.rev_id,
                    token,
                },
            )
            .await
        }
        SessionActionKind::Undo => {
            let token = execute_fetch_token(client, config, TokenKind::Csrf).await?;
            let Some(undo_after_rev_id) = payload.undo_after_rev_id else {
                return Err(ActionError::Execution {
                    message: "undo actions require undo_after_rev_id to be present".to_string(),
                    code: Some("invalid-input".to_string()),
                    http_status: None,
                    retryable: false,
                });
            };
            execute_undo(
                client,
                config,
                &UndoRequest {
                    title: payload.title.clone().unwrap_or_default(),
                    undo_rev_id: payload.rev_id,
                    undo_after_rev_id,
                    token,
                    summary: payload.summary.clone(),
                },
            )
            .await
        }
    }
}

fn build_action_log_entry(
    executed_at_ms: i64,
    payload: &SessionActionExecutionRequest,
    outcome: ActionLogOutcome,
) -> ActionExecutionLogEntry {
    ActionExecutionLogEntry {
        executed_at_ms,
        wiki_id: payload.wiki_id.clone(),
        kind: payload.kind,
        rev_id: payload.rev_id,
        title: payload.title.clone(),
        target_user: payload.target_user.clone(),
        summary: payload.summary.clone(),
        accepted: outcome.accepted,
        http_status: outcome.http_status,
        api_code: outcome.api_code,
        retryable: outcome.retryable,
        warnings: outcome.warnings,
        result: outcome.result,
        response_preview: outcome.response_preview,
        error: outcome.error,
    }
}

struct ActionLogOutcome {
    accepted: bool,
    http_status: Option<u16>,
    api_code: Option<String>,
    retryable: bool,
    warnings: Vec<String>,
    result: Option<String>,
    response_preview: Option<String>,
    error: Option<String>,
}

struct ActionHistoryStats {
    total_actions: usize,
    successful_actions: usize,
    retryable_failures: usize,
    last_execution: Option<ActionExecutionLogEntry>,
}

async fn record_action_execution(
    state: &AppState,
    session_id: &str,
    entry: ActionExecutionLogEntry,
) {
    let mut sessions = state.sessions.write().await;
    prune_expired_sessions(&mut sessions, state.clock.now_ms());
    if let Some(session) = sessions.get_mut(session_id) {
        session.action_history.push(entry);
        if session.action_history.len() > ACTION_HISTORY_LIMIT {
            let overflow = session.action_history.len() - ACTION_HISTORY_LIMIT;
            session.action_history.drain(0..overflow);
        }
    }
}

fn action_feedback_for_entry(entry: &ActionExecutionLogEntry) -> String {
    let verb = match entry.kind {
        SessionActionKind::Rollback => "Rollback",
        SessionActionKind::Patrol => "Patrol",
        SessionActionKind::Undo => "Undo",
    };

    if entry.accepted {
        format!(
            "{verb} on {} rev {} accepted{}{}{}.",
            entry.wiki_id,
            entry.rev_id,
            entry
                .http_status
                .map(|status| format!(" with HTTP {status}"))
                .unwrap_or_default(),
            entry
                .result
                .as_ref()
                .map(|result| format!(" ({result})"))
                .unwrap_or_default(),
            if entry.warnings.is_empty() {
                String::new()
            } else {
                format!(" warnings={}", entry.warnings.join(" | "))
            }
        )
    } else {
        format!(
            "{verb} on {} rev {} failed{}{}{}.",
            entry.wiki_id,
            entry.rev_id,
            entry
                .error
                .as_ref()
                .map(|error| format!(": {error}"))
                .unwrap_or_default(),
            entry
                .api_code
                .as_ref()
                .map(|code| format!(" code={code}"))
                .unwrap_or_default(),
            if entry.retryable {
                " retryable=true".to_string()
            } else {
                String::new()
            }
        )
    }
}

fn action_error_message(error: &Json<serde_json::Value>) -> String {
    error
        .0
        .get("error")
        .and_then(|value| value.as_str())
        .map_or_else(|| error.0.to_string(), ToString::to_string)
}

fn internal_error(message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "error": message })),
    )
}

async fn action_status_report(
    state: &AppState,
    headers: &HeaderMap,
) -> ActionExecutionStatusReport {
    let current = current_session_snapshot(state, headers, false).await;
    let Some(session) = current else {
        return ActionExecutionStatusReport {
            authenticated: false,
            session_id: None,
            username: None,
            total_actions: 0,
            successful_actions: 0,
            failed_actions: 0,
            retryable_failures: 0,
            last_execution: None,
            shell_feedback: vec!["No authenticated shell session is active.".to_string()],
        };
    };

    let history_summary = action_history_stats_for_session(state, &session.session_id).await;
    let last_execution = history_summary.last_execution.clone();
    let total_actions = history_summary.total_actions;
    let successful_actions = history_summary.successful_actions;
    let failed_actions = total_actions.saturating_sub(successful_actions);
    ActionExecutionStatusReport {
        authenticated: true,
        session_id: Some(session.session_id),
        username: Some(session.username),
        total_actions,
        successful_actions,
        failed_actions,
        retryable_failures: history_summary.retryable_failures,
        last_execution: last_execution.clone(),
        shell_feedback: action_shell_feedback(total_actions, last_execution.as_ref()),
    }
}

async fn action_history_report(
    state: &AppState,
    headers: &HeaderMap,
    limit: Option<usize>,
) -> ActionExecutionHistoryReport {
    let current = current_session_snapshot(state, headers, false).await;
    let Some(session) = current else {
        return ActionExecutionHistoryReport {
            authenticated: false,
            session_id: None,
            username: None,
            entries: Vec::new(),
        };
    };

    let entries = action_history_for_session(
        state,
        &session.session_id,
        limit.unwrap_or(10).min(ACTION_HISTORY_LIMIT),
    )
    .await;
    ActionExecutionHistoryReport {
        authenticated: true,
        session_id: Some(session.session_id),
        username: Some(session.username),
        entries,
    }
}

async fn action_history_for_session(
    state: &AppState,
    session_id: &str,
    limit: usize,
) -> Vec<ActionExecutionLogEntry> {
    let sessions = state.sessions.read().await;
    if let Some(session) = sessions.get(session_id) {
        session
            .action_history
            .iter()
            .rev()
            .take(limit)
            .cloned()
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    }
}

async fn action_history_stats_for_session(
    state: &AppState,
    session_id: &str,
) -> ActionHistoryStats {
    let sessions = state.sessions.read().await;
    sessions.get(session_id).map_or(
        ActionHistoryStats {
            total_actions: 0,
            successful_actions: 0,
            retryable_failures: 0,
            last_execution: None,
        },
        |session| {
            let mut successful_actions = 0usize;
            let mut retryable_failures = 0usize;
            for entry in &session.action_history {
                if entry.accepted {
                    successful_actions = successful_actions.saturating_add(1);
                } else if entry.retryable {
                    retryable_failures = retryable_failures.saturating_add(1);
                }
            }

            ActionHistoryStats {
                total_actions: session.action_history.len(),
                successful_actions,
                retryable_failures,
                last_execution: session.action_history.last().cloned(),
            }
        },
    )
}

fn action_shell_feedback(
    total_actions: usize,
    last_execution: Option<&ActionExecutionLogEntry>,
) -> Vec<String> {
    let mut feedback = Vec::new();
    feedback.push(format!(
        "{total_actions} action(s) recorded in this shell session."
    ));

    if let Some(last) = last_execution {
        feedback.push(action_feedback_for_entry(last));
        if let Some(preview) = &last.response_preview {
            feedback.push(format!("Latest response excerpt: {preview}"));
        }
        if let Some(code) = &last.api_code {
            feedback.push(format!("Latest API code: {code}"));
        }
    } else {
        feedback.push("No actions have been executed yet.".to_string());
    }

    feedback
}

fn invalid_payload(message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "error": message })),
    )
}

fn effective_session_scopes(report: &DevAuthCapabilityReport) -> Vec<String> {
    let mut scopes = Vec::new();

    if report.capabilities.read.can_authenticate {
        scopes.push("basic".to_string());
    }
    if report.capabilities.editing.can_edit {
        scopes.push("editpage".to_string());
    }
    if report.capabilities.moderation.can_patrol {
        scopes.push("patrol".to_string());
    }
    if report.capabilities.moderation.can_rollback {
        scopes.push("rollback".to_string());
    }

    scopes
}

fn split_scope_string(value: &str) -> Vec<String> {
    value
        .split_whitespace()
        .filter(|scope| !scope.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn auth_session_view_without_session(state: &AppState) -> OAuthSessionView {
    OAuthSessionView {
        authenticated: FlagState::Disabled,
        username: None,
        scopes: Vec::new(),
        expires_at_ms: None,
        upstream_access_expires_at_ms: None,
        refresh_available: FlagState::Disabled,
        bridge_mode: "inactive".to_string(),
        local_token_available: FlagState::from(state.local_oauth.access_token().is_some()),
        oauth_client_ready: FlagState::from(state.local_oauth.has_confidential_oauth_client()),
        login_path: AUTH_LOGIN_PATH.to_string(),
        logout_path: AUTH_LOGOUT_PATH.to_string(),
    }
}

async fn auth_session_view(state: &AppState, headers: &HeaderMap, touch: bool) -> OAuthSessionView {
    match current_session_snapshot(state, headers, touch).await {
        Some(session) => OAuthSessionView {
            authenticated: FlagState::Enabled,
            username: Some(session.username),
            scopes: session.scopes,
            expires_at_ms: session.expires_at_ms,
            upstream_access_expires_at_ms: sessions_upstream_access_expiry(state, headers).await,
            refresh_available: FlagState::from(sessions_refresh_available(state, headers).await),
            bridge_mode: session.bridge_mode,
            local_token_available: FlagState::from(state.local_oauth.access_token().is_some()),
            oauth_client_ready: FlagState::from(state.local_oauth.has_confidential_oauth_client()),
            login_path: AUTH_LOGIN_PATH.to_string(),
            logout_path: AUTH_LOGOUT_PATH.to_string(),
        },
        None => auth_session_view_without_session(state),
    }
}

fn sanitize_redirect_target(next: Option<&str>) -> String {
    let Some(target) = next.map(str::trim).filter(|value| !value.is_empty()) else {
        return "/".to_string();
    };
    if target.starts_with('/') && !target.starts_with("//") {
        target.to_string()
    } else {
        "/".to_string()
    }
}

fn redirect_with_status(target: &str, key: &str, value: &str) -> String {
    let separator = if target.contains('?') { '&' } else { '?' };
    format!(
        "{target}{separator}{key}={}",
        url::form_urlencoded::byte_serialize(value.as_bytes()).collect::<String>()
    )
}

fn public_base_url(headers: &HeaderMap) -> Result<String, String> {
    if let Ok(base_url) = std::env::var("SP42_PUBLIC_BASE_URL")
        && !base_url.trim().is_empty()
    {
        return Ok(base_url.trim_end_matches('/').to_string());
    }

    let host = headers
        .get(HOST)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "host header is required to build oauth redirect URI".to_string())?;
    if !is_local_host(host) {
        return Err(
            "non-local oauth redirects require SP42_PUBLIC_BASE_URL instead of trusting Host"
                .to_string(),
        );
    }
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("http");
    Ok(format!("{scheme}://{host}"))
}

fn is_local_host(host: &str) -> bool {
    let authority = host.trim();
    let host_without_port = if authority.starts_with('[') {
        authority
            .split_once(']')
            .map_or(authority, |(head, _)| &head[1..])
    } else {
        authority.split(':').next().unwrap_or(authority)
    };

    matches!(host_without_port, "localhost" | "127.0.0.1" | "::1")
}

fn oauth_client_config_for_request(
    state: &AppState,
    headers: &HeaderMap,
    wiki_id: &str,
) -> Result<OAuthClientConfig, (StatusCode, Json<serde_json::Value>)> {
    let config =
        resolved_wiki_config(state, wiki_id).map_err(|message| invalid_payload(&message))?;
    let client_id = state
        .local_oauth
        .client_id()
        .ok_or_else(|| invalid_payload("oauth client id is missing"))?
        .to_string();
    let redirect_uri = reqwest::Url::parse(&format!(
        "{}{}",
        public_base_url(headers).map_err(|message| invalid_payload(&message))?,
        AUTH_CALLBACK_PATH
    ))
    .map_err(|error| invalid_payload(&format!("oauth redirect URI was invalid: {error}")))?;

    Ok(OAuthClientConfig {
        client_id,
        authorize_url: config.oauth_authorize_url,
        token_url: config.oauth_token_url,
        redirect_uri,
        scopes: vec!["basic".to_string(), "patrol".to_string()],
    })
}

fn oauth_client_config_from_pending(
    state: &AppState,
    pending: &PendingOAuthLogin,
) -> Result<OAuthClientConfig, (StatusCode, Json<serde_json::Value>)> {
    let config = resolved_wiki_config(state, &pending.wiki_id)
        .map_err(|message| invalid_payload(&message))?;
    let client_id = state
        .local_oauth
        .client_id()
        .ok_or_else(|| invalid_payload("oauth client id is missing"))?
        .to_string();
    let redirect_uri = reqwest::Url::parse(&pending.redirect_uri)
        .map_err(|error| invalid_payload(&format!("pending redirect URI was invalid: {error}")))?;

    Ok(OAuthClientConfig {
        client_id,
        authorize_url: config.oauth_authorize_url,
        token_url: config.oauth_token_url,
        redirect_uri,
        scopes: vec!["basic".to_string(), "patrol".to_string()],
    })
}

async fn store_pending_oauth_login(state: &AppState, pending: PendingOAuthLogin) {
    let mut pending_logins = state.pending_oauth_logins.write().await;
    prune_expired_pending_oauth_logins(&mut pending_logins, state.clock.now_ms());
    pending_logins.insert(pending.state.clone(), pending);
}

async fn take_pending_oauth_login(
    state: &AppState,
    state_token: &str,
) -> Option<PendingOAuthLogin> {
    let mut pending_logins = state.pending_oauth_logins.write().await;
    prune_expired_pending_oauth_logins(&mut pending_logins, state.clock.now_ms());
    pending_logins.remove(state_token)
}

fn prune_expired_pending_oauth_logins(
    pending_logins: &mut HashMap<String, PendingOAuthLogin>,
    current_time_ms: i64,
) {
    pending_logins.retain(|_, pending| pending.expires_at_ms > current_time_ms);
}

async fn install_session(
    state: &AppState,
    prior_session_id: Option<String>,
    stored: StoredSession,
    current_ms: i64,
) -> String {
    let session_id = next_session_id(state, current_ms);
    let mut sessions = state.sessions.write().await;
    prune_expired_sessions(&mut sessions, current_ms);
    if let Some(prior_session_id) = prior_session_id {
        sessions.remove(&prior_session_id);
    }
    sessions.insert(session_id.clone(), stored);
    session_id
}

async fn exchange_authorization_code(
    client: &reqwest::Client,
    local_oauth: &LocalOAuthConfig,
    oauth_config: &OAuthClientConfig,
    code: &str,
    verifier: &str,
) -> Result<OAuthTokenResponse, String> {
    let client_secret = local_oauth
        .client_secret()
        .ok_or_else(|| "oauth client secret is missing".to_string())?;
    let response = client
        .post(oauth_config.token_url.clone())
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", oauth_config.client_id.as_str()),
            ("client_secret", client_secret),
            ("redirect_uri", oauth_config.redirect_uri.as_ref()),
            ("code", code),
            ("code_verifier", verifier),
        ])
        .send()
        .await
        .map_err(|error| format!("oauth token exchange failed: {error}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("oauth token response body could not be read: {error}"))?;
    if !status.is_success() {
        return Err(format!(
            "oauth token exchange returned HTTP {status}: {body}"
        ));
    }
    serde_json::from_str::<OAuthTokenResponse>(&body)
        .map_err(|error| format!("oauth token response was invalid: {error}"))
}

async fn fetch_oauth_profile(
    client: &reqwest::Client,
    access_token: &str,
    profile_url: &str,
) -> Result<OAuthProfileResponse, String> {
    let response = client
        .get(profile_url)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|error| format!("oauth profile fetch failed: {error}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("oauth profile body could not be read: {error}"))?;
    if !status.is_success() {
        return Err(format!(
            "oauth profile fetch returned HTTP {status}: {body}"
        ));
    }
    serde_json::from_str::<OAuthProfileResponse>(&body)
        .map_err(|error| format!("oauth profile response was invalid: {error}"))
}

fn to_status(
    session: Option<&StoredSession>,
    local_oauth: &LocalOAuthConfig,
    now_ms: i64,
) -> DevAuthSessionStatus {
    DevAuthSessionStatus {
        authenticated: session.is_some(),
        username: session.map(|entry| entry.username.clone()),
        scopes: session.map_or_else(Vec::new, |entry| entry.scopes.clone()),
        expires_at_ms: session.map(|entry| session_expires_at_ms(entry, now_ms)),
        token_present: session.is_some_and(|entry| !entry.access_token.is_empty()),
        bridge_mode: session
            .map_or_else(|| "inactive".to_string(), |entry| entry.bridge_mode.clone()),
        local_token_available: local_oauth.access_token().is_some(),
    }
}

fn bootstrap_status(state: &AppState, auth: &DevAuthSessionStatus) -> DevAuthBootstrapStatus {
    let source_report = state.local_oauth.source_report();

    DevAuthBootstrapStatus {
        bootstrap_ready: state.local_oauth.access_token().is_some(),
        oauth: state.local_oauth.status(),
        session: auth.clone(),
        source_path: source_report
            .loaded_from_source
            .then_some(source_report.file_name.clone()),
        source_report,
    }
}

fn live_operator_backend_status(
    readiness: &ServerHealthStatus,
    auth: &DevAuthSessionStatus,
) -> LiveOperatorBackendStatus {
    LiveOperatorBackendStatus {
        ready_for_local_testing: FlagState::from(readiness.ready_for_local_testing),
        readiness_issues: readiness.readiness_issues.clone(),
        bootstrap_ready: FlagState::from(readiness.bootstrap.bootstrap_ready),
        oauth: readiness.oauth.clone(),
        session: auth.clone(),
        source_report: readiness.bootstrap.source_report.clone(),
        capability_cache_present: FlagState::from(readiness.capability_cache.present),
        capability_cache_fresh: FlagState::from(readiness.capability_cache.fresh),
        capability_cache_age_ms: readiness.capability_cache.age_ms,
        capability_cache_wiki_id: readiness.capability_cache.wiki_id.clone(),
    }
}

fn validate_bootstrap_payload(
    payload: &DevAuthBootstrapRequest,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if !payload.username.trim().is_empty() {
        return Err(invalid_payload(
            "username is derived from the local Wikimedia token; leave it blank",
        ));
    }
    if !payload.scopes.is_empty() {
        return Err(invalid_payload(
            "scopes are derived from the local Wikimedia token capabilities; leave them empty",
        ));
    }
    if payload.expires_at_ms.is_some() {
        return Err(invalid_payload(
            "expires_at_ms is derived server-side for the local token path; omit it",
        ));
    }

    Ok(())
}

#[cfg(test)]
fn now_ms() -> i64 {
    SystemClock.now_ms()
}

fn session_cookie_value(headers: &HeaderMap) -> Option<String> {
    headers
        .get(COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            value.split(';').find_map(|entry| {
                let mut parts = entry.trim().splitn(2, '=');
                let key = parts.next()?.trim();
                let value = parts.next()?.trim();
                (key == SESSION_COOKIE_NAME && !value.is_empty()).then(|| value.to_string())
            })
        })
}

fn next_session_id(state: &AppState, current_ms: i64) -> String {
    let counter = state.next_session_id.fetch_add(1, Ordering::Relaxed);
    format!(
        "{:016x}{:016x}{:08x}",
        u64::try_from(current_ms).unwrap_or(u64::MAX),
        counter,
        std::process::id()
    )
}

fn session_cookie_header(session_id: &str) -> Option<HeaderValue> {
    HeaderValue::from_str(&format!(
        "{SESSION_COOKIE_NAME}={session_id}; HttpOnly; SameSite=Lax; Path=/; Max-Age={SESSION_COOKIE_MAX_AGE_SECONDS}"
    ))
    .ok()
}

fn expired_session_cookie_header() -> HeaderValue {
    HeaderValue::from_static("sp42_dev_session=deleted; HttpOnly; SameSite=Lax; Path=/; Max-Age=0")
}

fn session_expires_at_ms(session: &StoredSession, current_time_ms: i64) -> i64 {
    let idle_deadline = session
        .last_seen_at_ms
        .saturating_add(SESSION_IDLE_TIMEOUT_MS);
    let absolute_deadline = session
        .created_at_ms
        .saturating_add(SESSION_ABSOLUTE_TIMEOUT_MS);
    let deadline = idle_deadline.min(absolute_deadline);
    deadline.max(current_time_ms)
}

fn session_is_expired(session: &StoredSession, current_time_ms: i64) -> bool {
    current_time_ms >= session_expires_at_ms(session, current_time_ms)
}

fn prune_expired_sessions(sessions: &mut HashMap<String, StoredSession>, current_time_ms: i64) {
    sessions.retain(|_, session| !session_is_expired(session, current_time_ms));
}

async fn current_session_snapshot(
    state: &AppState,
    headers: &HeaderMap,
    touch: bool,
) -> Option<SessionSnapshot> {
    let session_id = session_cookie_value(headers)?;
    let mut sessions = state.sessions.write().await;
    let current_time_ms = state.clock.now_ms();
    prune_expired_sessions(&mut sessions, current_time_ms);
    let session = sessions.get_mut(&session_id)?;
    if touch {
        session.last_seen_at_ms = current_time_ms;
        session.expires_at_ms = Some(session_expires_at_ms(session, current_time_ms));
    }

    Some(SessionSnapshot {
        session_id,
        username: session.username.clone(),
        scopes: session.scopes.clone(),
        expires_at_ms: session.expires_at_ms,
        access_token: session.access_token.clone(),
        bridge_mode: session.bridge_mode.clone(),
    })
}

async fn sessions_upstream_access_expiry(state: &AppState, headers: &HeaderMap) -> Option<i64> {
    let session_id = session_cookie_value(headers)?;
    let sessions = state.sessions.read().await;
    sessions
        .get(&session_id)
        .and_then(|session| session.upstream_access_expires_at_ms)
}

async fn sessions_refresh_available(state: &AppState, headers: &HeaderMap) -> bool {
    let Some(session_id) = session_cookie_value(headers) else {
        return false;
    };
    let sessions = state.sessions.read().await;
    sessions
        .get(&session_id)
        .and_then(|session| session.refresh_token.as_ref())
        .is_some_and(|token| !token.is_empty())
}

async fn current_status(
    state: &AppState,
    headers: &HeaderMap,
    touch: bool,
) -> DevAuthSessionStatus {
    match current_session_snapshot(state, headers, touch).await {
        Some(session) => DevAuthSessionStatus {
            authenticated: true,
            username: Some(session.username),
            scopes: session.scopes,
            expires_at_ms: session.expires_at_ms,
            token_present: true,
            bridge_mode: session.bridge_mode,
            local_token_available: state.local_oauth.access_token().is_some(),
        },
        None => to_status(None, &state.local_oauth, state.clock.now_ms()),
    }
}

async fn cached_capabilities_for_session(
    state: &AppState,
    session_id: &str,
    wiki_id: &str,
) -> Option<DevAuthCapabilityReport> {
    let mut sessions = state.sessions.write().await;
    let current_time_ms = state.clock.now_ms();
    prune_expired_sessions(&mut sessions, current_time_ms);
    let session = sessions.get_mut(session_id)?;
    let cache = session.capability_cache.get(wiki_id)?;
    if cache_is_fresh(cache, current_time_ms) {
        session.last_seen_at_ms = current_time_ms;
        session.expires_at_ms = Some(session_expires_at_ms(session, current_time_ms));
        Some(cache.report.clone())
    } else {
        session.capability_cache.remove(wiki_id);
        None
    }
}

async fn store_capabilities_for_session(
    state: &AppState,
    session_id: &str,
    report: &DevAuthCapabilityReport,
) {
    let mut sessions = state.sessions.write().await;
    let current_time_ms = state.clock.now_ms();
    prune_expired_sessions(&mut sessions, current_time_ms);
    if let Some(session) = sessions.get_mut(session_id) {
        session.capability_cache.insert(
            report.wiki_id.clone(),
            CachedCapabilityReport {
                fetched_at_ms: current_time_ms,
                report: report.clone(),
            },
        );
    }
}

fn cache_is_fresh(cache: &CachedCapabilityReport, current_time_ms: i64) -> bool {
    current_time_ms.saturating_sub(cache.fetched_at_ms) < CAPABILITY_CACHE_TTL_MS
}

fn sanitize_coordination_payload(payload: Vec<u8>, actor: Option<&str>) -> Vec<u8> {
    let Some(actor) = actor else {
        return payload;
    };
    let Ok(message) = sp42_core::decode_message(&payload) else {
        warn!("received undecodable coordination payload while rewriting actor");
        return payload;
    };
    let rewritten = match message {
        sp42_core::CoordinationMessage::ActionBroadcast(mut action) => {
            action.actor = actor.to_string();
            sp42_core::CoordinationMessage::ActionBroadcast(action)
        }
        sp42_core::CoordinationMessage::EditClaim(mut claim) => {
            claim.actor = actor.to_string();
            sp42_core::CoordinationMessage::EditClaim(claim)
        }
        sp42_core::CoordinationMessage::PresenceHeartbeat(mut presence) => {
            presence.actor = actor.to_string();
            sp42_core::CoordinationMessage::PresenceHeartbeat(presence)
        }
        sp42_core::CoordinationMessage::RaceResolution(mut resolution) => {
            resolution.winning_actor = actor.to_string();
            sp42_core::CoordinationMessage::RaceResolution(resolution)
        }
        other => other,
    };

    sp42_core::encode_message(&rewritten).unwrap_or(payload)
}

fn config_for_state_wiki(
    state: &AppState,
    wiki_id: &str,
) -> Result<sp42_core::WikiConfig, (StatusCode, Json<serde_json::Value>)> {
    resolved_wiki_config(state, wiki_id).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": error })),
        )
    })
}

fn resolved_wiki_config(state: &AppState, wiki_id: &str) -> Result<sp42_core::WikiConfig, String> {
    let mut config = config_for_wiki(wiki_id)?;
    if let Some(api_url) = &state.capability_targets.api_url {
        config.api_url = reqwest::Url::parse(api_url)
            .map_err(|error| format!("api_url override was invalid: {error}"))?;
    }
    if let Some(liftwing_url) = std::env::var("SP42_TEST_LIFTWING_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        config.liftwing_url = Some(
            reqwest::Url::parse(&liftwing_url)
                .map_err(|error| format!("liftwing_url override was invalid: {error}"))?,
        );
    }
    Ok(config)
}

fn validate_action_request(
    payload: &SessionActionExecutionRequest,
    capabilities: &DevAuthCapabilityReport,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if payload.wiki_id.trim().is_empty() {
        return Err(invalid_payload("wiki_id is required"));
    }
    if payload.rev_id == 0 {
        return Err(invalid_payload("rev_id must be non-zero"));
    }

    match payload.kind {
        SessionActionKind::Rollback => {
            if payload.title.as_deref().is_none_or(str::is_empty) {
                return Err(invalid_payload("title is required for rollback"));
            }
            if payload.target_user.as_deref().is_none_or(str::is_empty) {
                return Err(invalid_payload("target_user is required for rollback"));
            }
            if !capabilities.capabilities.moderation.can_rollback {
                return Err(forbidden_error(
                    "The authenticated session does not currently have rollback capability on this wiki.",
                ));
            }
        }
        SessionActionKind::Patrol => {
            if !capabilities.capabilities.moderation.can_patrol {
                return Err(forbidden_error(
                    "The authenticated session does not currently have patrol capability on this wiki.",
                ));
            }
        }
        SessionActionKind::Undo => {
            if payload.title.as_deref().is_none_or(str::is_empty) {
                return Err(invalid_payload("title is required for undo"));
            }
            if payload.undo_after_rev_id.is_none() {
                return Err(invalid_payload("undo_after_rev_id is required for undo"));
            }
            if !capabilities.capabilities.editing.can_undo {
                return Err(forbidden_error(
                    "The authenticated session does not currently have undo capability on this wiki.",
                ));
            }
        }
    }

    Ok(())
}

fn forbidden_error(message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({ "error": message })),
    )
}

fn unauthorized_error(message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": message })),
    )
}

fn action_error_response(error: &ActionError) -> (StatusCode, Json<serde_json::Value>) {
    let (message, code, http_status, retryable) = match error {
        ActionError::Execution {
            message,
            code,
            http_status,
            retryable,
        } => (message.clone(), code.clone(), *http_status, *retryable),
    };
    (
        match http_status {
            Some(400..=499) => StatusCode::BAD_REQUEST,
            _ => StatusCode::BAD_GATEWAY,
        },
        Json(serde_json::json!({
            "error": format!("wiki action failed: {message}"),
            "code": code,
            "http_status": http_status,
            "retryable": retryable,
        })),
    )
}

fn truncate_response_body(body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body);
    if text.chars().count() > RESPONSE_BODY_PREVIEW_LIMIT {
        let truncated = text
            .chars()
            .take(RESPONSE_BODY_PREVIEW_LIMIT)
            .collect::<String>();
        format!("{truncated}...")
    } else {
        text.into_owned()
    }
}

#[derive(Debug, Clone)]
struct BearerHttpClient {
    client: reqwest::Client,
    access_token: String,
}

impl BearerHttpClient {
    fn new(client: reqwest::Client, access_token: String) -> Self {
        Self {
            client,
            access_token,
        }
    }
}

#[async_trait]
impl HttpClient for BearerHttpClient {
    async fn execute(
        &self,
        request: sp42_core::HttpRequest,
    ) -> Result<sp42_core::HttpResponse, sp42_core::HttpClientError> {
        let mut builder = match request.method {
            sp42_core::HttpMethod::Get => self.client.get(request.url),
            sp42_core::HttpMethod::Post => self.client.post(request.url),
            sp42_core::HttpMethod::Put => self.client.put(request.url),
            sp42_core::HttpMethod::Patch => self.client.patch(request.url),
            sp42_core::HttpMethod::Delete => self.client.delete(request.url),
        }
        .bearer_auth(&self.access_token);

        for (key, value) in request.headers {
            builder = builder.header(&key, &value);
        }

        let response = if request.body.is_empty() {
            builder.send().await
        } else {
            builder.body(request.body).send().await
        }
        .map_err(|error| sp42_core::HttpClientError::Transport {
            message: error.to_string(),
        })?;

        let status = response.status().as_u16();
        let mut headers = HashMap::new();
        for (key, value) in response.headers() {
            if let Ok(value) = value.to_str() {
                headers.insert(key.to_string(), value.to_string());
            }
        }
        let body = response.bytes().await.map_err(|error| {
            sp42_core::HttpClientError::InvalidResponse {
                message: error.to_string(),
            }
        })?;

        Ok(sp42_core::HttpResponse {
            status,
            headers: headers.into_iter().collect(),
            body: body.to_vec(),
        })
    }
}

async fn get_healthz(State(state): State<AppState>) -> Json<ServerHealthStatus> {
    Json(server_readiness(&state, &HeaderMap::new()).await)
}

async fn room_inspection(
    coordination: &CoordinationRegistry,
    wiki_id: &str,
) -> Option<CoordinationRoomInspection> {
    coordination.room_inspection(wiki_id).await
}

fn empty_room_inspection(wiki_id: &str) -> CoordinationRoomInspection {
    CoordinationRoomInspection {
        room: CoordinationRoomSummary {
            wiki_id: wiki_id.to_string(),
            connected_clients: 0,
            published_messages: 0,
            claim_count: 0,
            presence_count: 0,
            flagged_edit_count: 0,
            score_delta_count: 0,
            race_resolution_count: 0,
            recent_action_count: 0,
        },
        state: Some(CoordinationState::new(wiki_id).summary()),
        metrics: CoordinationRoomMetrics {
            last_activity_ms: None,
            published_messages: 0,
            accepted_messages: 0,
            invalid_messages: 0,
        },
    }
}

fn uptime_ms(started_at: &Instant) -> u64 {
    u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::atomic::AtomicU64;
    use std::time::Duration;
    use std::time::Instant;

    use axum::body::{Body, to_bytes};
    use axum::http::header::HOST;
    use axum::http::{HeaderMap, Method, Request, StatusCode};
    use axum::routing::get;
    use axum::{Json, Router};
    use sp42_core::{Clock, FileStorage, LocalOAuthSourceReport, Storage, SystemClock};
    use tower::util::ServiceExt;

    use super::{
        ACTION_HISTORY_PATH, ACTION_STATUS_PATH, ActionExecutionHistoryReport,
        ActionExecutionLogEntry, ActionExecutionStatusReport, AppState, CoordinationRoomInspection,
        DevAuthBootstrapStatus, OPERATOR_READINESS_PATH, OPERATOR_REPORT_PATH,
        OPERATOR_STORAGE_LAYOUT_PATH, OperatorReport, OperatorRuntimeInspection,
        OperatorStorageLayoutView, RoomInspectionCollection, RuntimeDebugStatus,
        ServerHealthStatus, SessionActionKind, StoredSession, build_router, now_ms,
        operator_endpoint_manifest, public_base_url, to_status,
    };
    use crate::coordination::CoordinationRegistry;
    use crate::local_env::LocalOAuthConfig;
    use crate::wikimedia_capabilities::CapabilityProbeTargets;
    use futures::{SinkExt, StreamExt};
    use tokio::net::TcpListener;
    use tokio_tungstenite::{
        connect_async,
        tungstenite::{Message as WebSocketMessage, client::IntoClientRequest, http::HeaderValue},
    };

    type TestWebSocket = tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >;

    fn test_state() -> AppState {
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        AppState {
            capability_cache: Arc::new(tokio::sync::RwLock::new(None)),
            sessions: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            pending_oauth_logins: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            http_client: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .user_agent(sp42_core::branding::USER_AGENT)
                .build()
                .expect("reqwest client should build"),
            local_oauth: LocalOAuthConfig::default(),
            runtime_storage_root: std::env::temp_dir()
                .join(format!("sp42-server-runtime-{}", std::process::id())),
            ingestion_supervisor: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            capability_targets: CapabilityProbeTargets::default(),
            clock: clock.clone(),
            coordination: CoordinationRegistry::new(clock),
            next_client_id: Arc::new(AtomicU64::new(1)),
            next_session_id: Arc::new(AtomicU64::new(1)),
            started_at: Instant::now(),
        }
    }

    fn temp_local_env_file(contents: &str) -> std::path::PathBuf {
        let temp_dir = std::env::temp_dir().join(format!(
            "sp42-server-test-{}-{}",
            std::process::id(),
            SystemClock.now_ms()
        ));
        std::fs::create_dir_all(&temp_dir).expect("temp dir should create");
        let path = temp_dir.join(".env.wikimedia.local");
        std::fs::write(&path, contents).expect("temp env file should write");
        path
    }

    fn mock_recentchanges_response(continued: bool) -> serde_json::Value {
        serde_json::json!({
            "continue": { "rccontinue": if continued { "20260324010203|789" } else { "20260324010202|456" } },
            "query": {
                "recentchanges": [
                    {
                        "type": "edit",
                        "title": if continued { "Live route sample page 2" } else { "Live route sample" },
                        "ns": 0,
                        "revid": if continued { 123_457 } else { 123_456 },
                        "old_revid": if continued { 123_456 } else { 123_455 },
                        "user": "192.0.2.44",
                        "timestamp": "2026-03-24T01:02:03Z",
                        "bot": false,
                        "minor": false,
                        "new": false,
                        "oldlen": 120,
                        "newlen": 80,
                        "comment": "sample edit",
                        "tags": ["mw-reverted"]
                    }
                ]
            }
        })
    }

    fn mock_storage_page(title: &str) -> String {
        let (kind, document) = if title.ends_with("/Preferences") {
            (
                "preferences",
                serde_json::json!({
                    "type": "preferences",
                    "document": {
                        "preferred_wiki_id": "frwiki",
                        "queue_limit": 25,
                        "hide_minor": false,
                        "hide_bots": true,
                        "editor_types": ["anonymous", "temporary"],
                        "tag_filters": [],
                    }
                }),
            )
        } else {
            (
                "personal-profile",
                serde_json::json!({
                    "owner": "Schiste",
                    "document": title
                }),
            )
        };
        format!(
            "== SP42 Document ==\nLoaded by the logical storage route.\n<!-- SP42:BEGIN -->\n<syntaxhighlight lang=\"json\">\n{{\n  \"project\": \"SP42\",\n  \"version\": 1,\n  \"title\": \"{title}\",\n  \"kind\": \"{kind}\",\n  \"site_wiki_id\": \"frwiki\",\n  \"realm\": \"PersonalUserSpace\",\n  \"data\": {document}\n}}\n</syntaxhighlight>\n<!-- SP42:END -->"
        )
    }

    fn mock_revisions_response(
        title: &str,
        include_second: bool,
        title_query: bool,
    ) -> serde_json::Value {
        serde_json::json!({
            "query": {
                "pages": [
                    {
                        "pageid": 1,
                        "title": title,
                        "revisions": if title_query {
                            serde_json::json!([
                                {
                                    "revid": 123_456,
                                    "slots": { "main": { "content": mock_storage_page(title) } }
                                }
                            ])
                        } else if include_second {
                            serde_json::json!([
                                {
                                    "revid": 123_456,
                                    "slots": { "main": { "content": "After text with removal" } }
                                },
                                {
                                    "revid": 123_457,
                                    "slots": { "main": { "content": "Page 2 after text" } }
                                }
                            ])
                        } else {
                            serde_json::json!([
                                {
                                    "revid": 123_455,
                                    "slots": { "main": { "content": "Before text" } }
                                },
                                {
                                    "revid": 123_456,
                                    "slots": { "main": { "content": "After text with removal" } }
                                }
                            ])
                        }
                    }
                ]
            }
        })
    }

    fn mock_api_response(params: &std::collections::HashMap<String, String>) -> serde_json::Value {
        match (
            params.get("meta"),
            params.get("type"),
            params.get("list"),
            params.get("prop"),
        ) {
            (Some(meta), None, _, _) if meta == "userinfo" => serde_json::json!({
                "query": {
                    "userinfo": {
                        "name": "Schiste",
                        "groups": ["*", "user", "autoconfirmed", "autopatrolled"],
                        "rights": ["edit", "patrol", "rollback"]
                    }
                }
            }),
            (Some(meta), Some(kind), _, _)
                if meta == "tokens" && kind == "patrol|rollback|csrf" =>
            {
                serde_json::json!({
                    "query": {
                        "tokens": {
                            "csrftoken": "csrf",
                            "patroltoken": "patrol",
                            "rollbacktoken": "rollback"
                        }
                    }
                })
            }
            (_, _, Some(list), _) if list == "recentchanges" => {
                mock_recentchanges_response(params.contains_key("rccontinue"))
            }
            (_, _, _, Some(prop)) if prop == "revisions" => {
                let revids = params.get("revids").cloned().unwrap_or_default();
                let include_second = revids.contains("123457");
                let title = params.get("titles").cloned().unwrap_or_else(|| {
                    if include_second {
                        "Live route sample page 2".to_string()
                    } else {
                        "Live route sample".to_string()
                    }
                });
                mock_revisions_response(&title, include_second, params.contains_key("titles"))
            }
            _ => serde_json::json!({ "error": "unexpected request" }),
        }
    }

    async fn mock_capability_server() -> (String, tokio::task::JoinHandle<()>) {
        async fn profile() -> Json<serde_json::Value> {
            Json(serde_json::json!({
                "username": "Schiste",
                "grants": ["basic", "editpage", "patrol", "rollback"]
            }))
        }

        async fn api(
            axum::extract::Query(params): axum::extract::Query<
                std::collections::HashMap<String, String>,
            >,
        ) -> Json<serde_json::Value> {
            Json(mock_api_response(&params))
        }

        let router = Router::new()
            .route("/oauth2/resource/profile", get(profile))
            .route("/w/api.php", get(api));

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener should bind");
        let addr = listener.local_addr().expect("listener address");
        let handle = tokio::spawn(async move {
            axum::serve(listener, router)
                .await
                .expect("mock capability server should run");
        });

        (format!("http://{addr}"), handle)
    }

    async fn spawn_test_server(router: Router) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener should bind");
        let addr = listener.local_addr().expect("listener address");
        let handle = tokio::spawn(async move {
            axum::serve(listener, router)
                .await
                .expect("test server should run");
        });

        (format!("http://{addr}"), handle)
    }

    fn session_cookie_header(session_id: &str) -> String {
        format!("sp42_dev_session={session_id}")
    }

    async fn connect_socket(
        base_url: &str,
        wiki_id: &str,
        session_id: Option<&str>,
    ) -> TestWebSocket {
        let ws_url = format!("{}/ws/{wiki_id}", base_url.replacen("http", "ws", 1));
        let mut request = ws_url
            .into_client_request()
            .expect("websocket request should build");
        if let Some(session_id) = session_id {
            request.headers_mut().insert(
                "Cookie",
                HeaderValue::from_str(&session_cookie_header(session_id))
                    .expect("cookie header should be valid"),
            );
        }

        let (socket, _) = connect_async(request)
            .await
            .expect("websocket should connect");
        socket
    }

    async fn connect_session_socket(
        base_url: &str,
        wiki_id: &str,
        session_id: &str,
    ) -> TestWebSocket {
        connect_socket(base_url, wiki_id, Some(session_id)).await
    }

    async fn connect_anonymous_socket(base_url: &str, wiki_id: &str) -> TestWebSocket {
        connect_socket(base_url, wiki_id, None).await
    }

    async fn send_coordination_message(
        socket: &mut TestWebSocket,
        message: sp42_core::CoordinationMessage,
    ) {
        let payload = sp42_core::encode_message(&message).expect("message should encode");
        socket
            .send(WebSocketMessage::Binary(payload.into()))
            .await
            .expect("websocket send should succeed");
    }

    async fn recv_coordination_message(
        socket: &mut TestWebSocket,
    ) -> sp42_core::CoordinationMessage {
        loop {
            let frame = socket
                .next()
                .await
                .expect("websocket stream should stay open")
                .expect("websocket frame should be readable");

            match frame {
                WebSocketMessage::Binary(bytes) => {
                    return sp42_core::decode_message(&bytes)
                        .expect("binary payload should decode");
                }
                WebSocketMessage::Text(text) => {
                    return sp42_core::decode_message(text.as_str().as_bytes())
                        .expect("text payload should decode");
                }
                WebSocketMessage::Ping(_)
                | WebSocketMessage::Pong(_)
                | WebSocketMessage::Frame(_) => {}
                WebSocketMessage::Close(frame) => {
                    panic!("websocket closed unexpectedly: {frame:?}");
                }
            }
        }
    }

    async fn recv_binary_frame(socket: &mut TestWebSocket) -> Vec<u8> {
        loop {
            let frame = socket
                .next()
                .await
                .expect("websocket stream should stay open")
                .expect("websocket frame should be readable");

            match frame {
                WebSocketMessage::Binary(bytes) => return bytes.to_vec(),
                WebSocketMessage::Text(text) => return text.as_str().as_bytes().to_vec(),
                WebSocketMessage::Ping(_)
                | WebSocketMessage::Pong(_)
                | WebSocketMessage::Frame(_) => {}
                WebSocketMessage::Close(frame) => {
                    panic!("websocket closed unexpectedly: {frame:?}");
                }
            }
        }
    }

    async fn expect_no_coordination_message(socket: &mut TestWebSocket) {
        let no_message =
            tokio::time::timeout(std::time::Duration::from_millis(75), socket.next()).await;
        assert!(
            no_message.is_err(),
            "expected no websocket replay for a fresh subscriber"
        );
    }

    fn test_session(username: &str, access_token: &str, created_at_ms: i64) -> StoredSession {
        StoredSession {
            username: username.to_string(),
            scopes: vec!["patrol".to_string()],
            expires_at_ms: None,
            access_token: access_token.to_string(),
            refresh_token: None,
            upstream_access_expires_at_ms: None,
            bridge_mode: "local-env-token".to_string(),
            created_at_ms,
            last_seen_at_ms: created_at_ms,
            capability_cache: HashMap::new(),
            action_history: Vec::new(),
        }
    }

    fn assert_claim_actor(
        message: &sp42_core::CoordinationMessage,
        expected_actor: &str,
        expected_rev_id: u64,
    ) {
        let sp42_core::CoordinationMessage::EditClaim(claim) = message else {
            panic!("expected edit claim message, got {message:?}");
        };
        assert_eq!(claim.actor, expected_actor);
        assert_eq!(claim.rev_id, expected_rev_id);
    }

    fn assert_presence_actor(
        message: &sp42_core::CoordinationMessage,
        expected_actor: &str,
        expected_edit_count: u32,
    ) {
        let sp42_core::CoordinationMessage::PresenceHeartbeat(heartbeat) = message else {
            panic!("expected presence heartbeat message, got {message:?}");
        };
        assert_eq!(heartbeat.actor, expected_actor);
        assert_eq!(heartbeat.active_edit_count, expected_edit_count);
    }

    fn assert_action_actor(
        message: &sp42_core::CoordinationMessage,
        expected_actor: &str,
        expected_action: &sp42_core::Action,
    ) {
        let sp42_core::CoordinationMessage::ActionBroadcast(action) = message else {
            panic!("expected action broadcast message, got {message:?}");
        };
        assert_eq!(action.actor, expected_actor);
        assert_eq!(&action.action, expected_action);
    }

    fn assert_race_resolution_actor(
        message: &sp42_core::CoordinationMessage,
        expected_actor: &str,
        expected_rev_id: u64,
    ) {
        let sp42_core::CoordinationMessage::RaceResolution(resolution) = message else {
            panic!("expected race resolution message, got {message:?}");
        };
        assert_eq!(resolution.winning_actor, expected_actor);
        assert_eq!(resolution.rev_id, expected_rev_id);
    }

    async fn fetch_room_inspection(base_url: &str, wiki_id: &str) -> CoordinationRoomInspection {
        reqwest::Client::builder()
            .user_agent(sp42_core::branding::USER_AGENT)
            .build()
            .expect("reqwest client should build")
            .get(format!(
                "{base_url}/coordination/rooms/{wiki_id}/inspection"
            ))
            .send()
            .await
            .expect("inspection request should succeed")
            .error_for_status()
            .expect("inspection response should succeed")
            .json::<CoordinationRoomInspection>()
            .await
            .expect("inspection should parse")
    }

    #[test]
    fn to_status_hides_token_value() {
        let status = to_status(
            Some(&StoredSession {
                username: "Example".to_string(),
                scopes: vec!["rollback".to_string()],
                expires_at_ms: Some(42),
                access_token: "secret".to_string(),
                refresh_token: None,
                upstream_access_expires_at_ms: None,
                bridge_mode: "manual-dev-token".to_string(),
                created_at_ms: 0,
                last_seen_at_ms: 0,
                capability_cache: HashMap::new(),
                action_history: Vec::new(),
            }),
            &LocalOAuthConfig::default(),
            now_ms(),
        );

        assert!(status.authenticated);
        assert!(status.token_present);
        assert_eq!(status.username.as_deref(), Some("Example"));
    }

    #[tokio::test]
    async fn put_session_is_disabled_for_single_user_local_token_path() {
        let router = build_router(test_state());
        let put_request = Request::builder()
            .method(Method::PUT)
            .uri("/dev/auth/session")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "username": "Example",
                    "access_token": "secret-token",
                    "scopes": ["rollback"],
                    "expires_at_ms": 123
                })
                .to_string(),
            ))
            .expect("request should build");

        let put_response = router
            .clone()
            .oneshot(put_request)
            .await
            .expect("put request should succeed");
        assert_eq!(put_response.status(), StatusCode::METHOD_NOT_ALLOWED);

        let get_response = router
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/dev/auth/session")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("get request should succeed");
        assert_eq!(get_response.status(), StatusCode::OK);

        let body = to_bytes(get_response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        let status: sp42_core::DevAuthSessionStatus =
            serde_json::from_slice(&body).expect("status should parse");
        assert!(!status.authenticated);
        assert_eq!(status.bridge_mode, "inactive");
    }

    #[tokio::test]
    async fn healthz_route_is_available() {
        let router = build_router(test_state());
        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/healthz")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("health request should succeed");

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        let status: ServerHealthStatus =
            serde_json::from_slice(&body).expect("health status should parse");

        assert_eq!(status.project, sp42_core::branding::PROJECT_NAME);
        assert!(!status.ready_for_local_testing);
        assert!(
            status
                .readiness_issues
                .iter()
                .any(|issue| issue.contains("WIKIMEDIA_ACCESS_TOKEN"))
        );
        assert_eq!(
            status.capability_probe.endpoint,
            "/dev/auth/capabilities/frwiki"
        );
    }

    #[tokio::test]
    async fn healthz_reports_ready_when_local_token_is_loaded() {
        let local_env_path = temp_local_env_file(
            "WIKIMEDIA_CLIENT_APPLICATION_KEY=client-key\nWIKIMEDIA_CLIENT_APPLICATION_SECRET=client-secret\nWIKIMEDIA_ACCESS_TOKEN=token-value\n",
        );
        let (profile_base, server) = mock_capability_server().await;
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);

        let state = AppState {
            capability_cache: Arc::new(tokio::sync::RwLock::new(None)),
            sessions: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            pending_oauth_logins: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            http_client: reqwest::Client::builder()
                .user_agent(sp42_core::branding::USER_AGENT)
                .build()
                .expect("reqwest client should build"),
            local_oauth: LocalOAuthConfig::load_from_candidates([local_env_path.clone()]),
            runtime_storage_root: std::env::temp_dir().join("sp42-server-runtime-healthz"),
            ingestion_supervisor: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            capability_targets: CapabilityProbeTargets {
                profile_url: format!("{profile_base}/oauth2/resource/profile"),
                api_url: Some(format!("{profile_base}/w/api.php")),
            },
            clock: clock.clone(),
            coordination: CoordinationRegistry::new(clock),
            next_client_id: Arc::new(AtomicU64::new(1)),
            next_session_id: Arc::new(AtomicU64::new(1)),
            started_at: Instant::now(),
        };

        let router = build_router(state);
        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/healthz")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("health request should succeed");

        server.abort();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        let status: ServerHealthStatus =
            serde_json::from_slice(&body).expect("health status should parse");

        assert!(status.ready_for_local_testing);
        assert!(status.bootstrap.bootstrap_ready);
        assert!(status.bootstrap.source_report.loaded_from_source);
        assert!(status.bootstrap.source_report.source_path.is_none());
        assert_eq!(
            status.bootstrap.source_report.file_name,
            ".env.wikimedia.local"
        );
        assert!(status.capability_probe.available);
    }

    #[tokio::test]
    async fn bootstrap_status_route_is_available() {
        let router = build_router(test_state());
        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/dev/auth/bootstrap/status")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("bootstrap status request should succeed");

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        let status: DevAuthBootstrapStatus =
            serde_json::from_slice(&body).expect("bootstrap status should parse");

        assert!(!status.bootstrap_ready);
        assert!(!status.oauth.access_token_present);
        assert!(status.source_path.is_none());
        assert!(!status.source_report.loaded_from_source);
        assert_eq!(status.source_report.file_name, ".env.wikimedia.local");
    }

    #[test]
    fn live_operator_backend_status_reflects_readiness() {
        let readiness = ServerHealthStatus {
            project: sp42_core::branding::PROJECT_NAME.to_string(),
            ready_for_local_testing: true,
            readiness_issues: vec!["capability cache cold".to_string()],
            uptime_ms: 42,
            session_count: 1,
            coordination_room_count: 2,
            auth: sp42_core::DevAuthSessionStatus {
                authenticated: true,
                username: Some("Tester".to_string()),
                scopes: vec!["basic".to_string()],
                expires_at_ms: Some(123),
                token_present: true,
                bridge_mode: "local-env-token".to_string(),
                local_token_available: true,
            },
            oauth: sp42_core::LocalOAuthConfigStatus {
                client_id_present: true,
                client_secret_present: true,
                access_token_present: true,
            },
            bootstrap: DevAuthBootstrapStatus {
                bootstrap_ready: true,
                oauth: sp42_core::LocalOAuthConfigStatus {
                    client_id_present: true,
                    client_secret_present: true,
                    access_token_present: true,
                },
                session: sp42_core::DevAuthSessionStatus {
                    authenticated: true,
                    username: Some("Tester".to_string()),
                    scopes: vec!["basic".to_string()],
                    expires_at_ms: Some(123),
                    token_present: true,
                    bridge_mode: "local-env-token".to_string(),
                    local_token_available: true,
                },
                source_path: Some(".env.wikimedia.local".to_string()),
                source_report: LocalOAuthSourceReport {
                    file_name: ".env.wikimedia.local".to_string(),
                    source_path: None,
                    loaded_from_source: true,
                },
            },
            capability_probe: super::CapabilityProbeHint {
                wiki_id: "frwiki".to_string(),
                endpoint: "/dev/auth/capabilities/frwiki".to_string(),
                available: true,
            },
            capability_cache: super::CapabilityCacheStatus {
                present: true,
                fresh: true,
                age_ms: Some(7),
                wiki_id: Some("frwiki".to_string()),
            },
            operator_report_path: OPERATOR_REPORT_PATH.to_string(),
            coordination: sp42_core::CoordinationSnapshot::default(),
        };

        let backend = super::live_operator_backend_status(&readiness, &readiness.auth);

        assert!(backend.ready_for_local_testing.is_enabled());
        assert!(backend.bootstrap_ready.is_enabled());
        assert!(backend.source_report.loaded_from_source);
        assert!(backend.capability_cache_present.is_enabled());
        assert_eq!(backend.capability_cache_wiki_id.as_deref(), Some("frwiki"));
    }

    #[tokio::test]
    async fn runtime_debug_route_is_available() {
        let router = build_router(test_state());
        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/debug/runtime")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("runtime debug request should succeed");

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        let runtime: RuntimeDebugStatus =
            serde_json::from_slice(&body).expect("runtime debug should parse");

        assert_eq!(runtime.project, sp42_core::branding::PROJECT_NAME);
        assert!(runtime.uptime_ms < 10_000);
        assert_eq!(runtime.session_count, 0);
        assert_eq!(runtime.coordination_room_count, 0);
        assert_eq!(runtime.coordination.rooms.len(), 0);
        assert!(!runtime.capabilities.checked);
        assert!(!runtime.capability_cache.present);
        assert_eq!(runtime.operator_report_path, OPERATOR_REPORT_PATH);
    }

    #[tokio::test]
    async fn operator_readiness_route_is_available() {
        let router = build_router(test_state());
        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(OPERATOR_READINESS_PATH)
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("operator readiness request should succeed");

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        let status: ServerHealthStatus =
            serde_json::from_slice(&body).expect("readiness should parse");

        assert_eq!(status.project, sp42_core::branding::PROJECT_NAME);
        assert_eq!(status.session_count, 0);
        assert_eq!(status.coordination_room_count, 0);
        assert_eq!(status.operator_report_path, OPERATOR_REPORT_PATH);
        assert!(!status.capability_cache.present);
    }

    #[tokio::test]
    async fn operator_report_route_is_available() {
        let router = build_router(test_state());
        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(OPERATOR_REPORT_PATH)
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("operator report request should succeed");

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        let report: OperatorReport =
            serde_json::from_slice(&body).expect("operator report should parse");

        assert_eq!(report.project, sp42_core::branding::PROJECT_NAME);
        assert_eq!(report.endpoints.len(), operator_endpoint_manifest().len());
        assert_eq!(report.readiness.operator_report_path, OPERATOR_REPORT_PATH);
        assert_eq!(report.runtime.operator_report_path, OPERATOR_REPORT_PATH);
        assert_eq!(
            report.bootstrap.source_report.file_name,
            ".env.wikimedia.local"
        );
    }

    #[test]
    fn operator_endpoint_manifest_contains_core_endpoints() {
        let endpoints = operator_endpoint_manifest();
        assert!(
            endpoints
                .iter()
                .any(|entry| entry.path == OPERATOR_READINESS_PATH)
        );
        assert!(
            endpoints
                .iter()
                .any(|entry| entry.path == OPERATOR_REPORT_PATH)
        );
        assert!(endpoints.iter().any(|entry| entry.path == "/healthz"));
        assert!(
            endpoints
                .iter()
                .any(|entry| entry.path == "/operator/storage/layout/{wiki_id}")
        );
        assert!(endpoints.iter().all(|entry| entry.available));
    }

    #[test]
    fn public_base_url_accepts_loopback_host() {
        let mut headers = HeaderMap::new();
        headers.insert(HOST, axum::http::HeaderValue::from_static("127.0.0.1:8788"));

        let base = public_base_url(&headers).expect("loopback host should be accepted");

        assert_eq!(base, "http://127.0.0.1:8788");
    }

    #[test]
    fn public_base_url_rejects_non_local_host_without_override() {
        let mut headers = HeaderMap::new();
        headers.insert(HOST, axum::http::HeaderValue::from_static("example.org"));

        let error = public_base_url(&headers).expect_err("non-local host should be rejected");

        assert!(error.contains("SP42_PUBLIC_BASE_URL"));
    }

    #[tokio::test]
    async fn action_history_route_returns_recorded_entries() {
        let state = test_state();
        let session_id = "session-history".to_string();
        let created_at_ms = now_ms();
        state.sessions.write().await.insert(
            session_id.clone(),
            StoredSession {
                username: "Example".to_string(),
                scopes: vec!["patrol".to_string()],
                expires_at_ms: None,
                access_token: "secret".to_string(),
                refresh_token: None,
                upstream_access_expires_at_ms: None,
                bridge_mode: "manual-dev-token".to_string(),
                created_at_ms,
                last_seen_at_ms: created_at_ms,
                capability_cache: HashMap::new(),
                action_history: vec![
                    ActionExecutionLogEntry {
                        executed_at_ms: 10,
                        wiki_id: "frwiki".to_string(),
                        kind: sp42_core::SessionActionKind::Rollback,
                        rev_id: 123_456,
                        title: Some("Example".to_string()),
                        target_user: Some("Bob".to_string()),
                        summary: Some("undo".to_string()),
                        accepted: true,
                        http_status: Some(200),
                        api_code: None,
                        retryable: false,
                        warnings: vec!["rollback warning".to_string()],
                        result: Some("rollback=true".to_string()),
                        response_preview: Some("{\"ok\":true}".to_string()),
                        error: None,
                    },
                    ActionExecutionLogEntry {
                        executed_at_ms: 11,
                        wiki_id: "frwiki".to_string(),
                        kind: sp42_core::SessionActionKind::Patrol,
                        rev_id: 123_457,
                        title: None,
                        target_user: None,
                        summary: None,
                        accepted: false,
                        http_status: Some(502),
                        api_code: Some("maxlag".to_string()),
                        retryable: true,
                        warnings: Vec::new(),
                        result: None,
                        response_preview: None,
                        error: Some("wiki action failed".to_string()),
                    },
                ],
            },
        );

        let router = build_router(state);
        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!("{ACTION_HISTORY_PATH}?limit=1"))
                    .header("cookie", format!("sp42_dev_session={session_id}"))
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("history request should succeed");

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        let history: ActionExecutionHistoryReport =
            serde_json::from_slice(&body).expect("history should parse");

        assert!(history.authenticated);
        assert_eq!(history.session_id.as_deref(), Some(session_id.as_str()));
        assert_eq!(history.entries.len(), 1);
        assert_eq!(history.entries[0].rev_id, 123_457);
        assert!(!history.entries[0].accepted);
    }

    #[tokio::test]
    async fn action_status_route_returns_shell_feedback() {
        let state = test_state();
        let session_id = "session-status".to_string();
        let created_at_ms = now_ms();
        state.sessions.write().await.insert(
            session_id.clone(),
            StoredSession {
                username: "Example".to_string(),
                scopes: vec!["rollback".to_string()],
                expires_at_ms: None,
                access_token: "secret".to_string(),
                refresh_token: None,
                upstream_access_expires_at_ms: None,
                bridge_mode: "manual-dev-token".to_string(),
                created_at_ms,
                last_seen_at_ms: created_at_ms,
                capability_cache: HashMap::new(),
                action_history: vec![ActionExecutionLogEntry {
                    executed_at_ms: 10,
                    wiki_id: "frwiki".to_string(),
                    kind: sp42_core::SessionActionKind::Patrol,
                    rev_id: 444,
                    title: None,
                    target_user: None,
                    summary: Some("patched".to_string()),
                    accepted: true,
                    http_status: Some(200),
                    api_code: None,
                    retryable: false,
                    warnings: vec!["already patrolled".to_string()],
                    result: Some("patrol=true".to_string()),
                    response_preview: Some("{\"status\":\"ok\"}".to_string()),
                    error: None,
                }],
            },
        );

        let router = build_router(state);
        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(ACTION_STATUS_PATH)
                    .header("cookie", format!("sp42_dev_session={session_id}"))
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("status request should succeed");

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        let status: ActionExecutionStatusReport =
            serde_json::from_slice(&body).expect("status should parse");

        assert!(status.authenticated);
        assert_eq!(status.total_actions, 1);
        assert_eq!(status.successful_actions, 1);
        assert_eq!(status.failed_actions, 0);
        assert_eq!(status.retryable_failures, 0);
        assert!(status.last_execution.is_some());
        assert!(
            status
                .shell_feedback
                .iter()
                .any(|line| line.contains("action(s) recorded"))
        );
        assert!(
            status
                .shell_feedback
                .iter()
                .any(|line| line.contains("Latest response excerpt"))
        );
        assert!(
            status
                .shell_feedback
                .iter()
                .any(|line| line.contains("patrol=true"))
        );
    }

    #[tokio::test]
    async fn capability_route_uses_injected_targets() {
        let local_env_path = temp_local_env_file(
            "WIKIMEDIA_CLIENT_APPLICATION_KEY=client-key\nWIKIMEDIA_CLIENT_APPLICATION_SECRET=client-secret\nWIKIMEDIA_ACCESS_TOKEN=token-value\n",
        );
        let (profile_base, server) = mock_capability_server().await;
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        let state = AppState {
            capability_cache: Arc::new(tokio::sync::RwLock::new(None)),
            sessions: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            pending_oauth_logins: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            http_client: reqwest::Client::builder()
                .user_agent(sp42_core::branding::USER_AGENT)
                .build()
                .expect("reqwest client should build"),
            local_oauth: LocalOAuthConfig::load_from_candidates([local_env_path.clone()]),
            runtime_storage_root: std::env::temp_dir().join("sp42-server-runtime-capability"),
            ingestion_supervisor: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            capability_targets: CapabilityProbeTargets {
                profile_url: format!("{profile_base}/oauth2/resource/profile"),
                api_url: Some(format!("{profile_base}/w/api.php")),
            },
            clock: clock.clone(),
            coordination: CoordinationRegistry::new(clock),
            next_client_id: Arc::new(AtomicU64::new(1)),
            next_session_id: Arc::new(AtomicU64::new(1)),
            started_at: Instant::now(),
        };
        let router = build_router(state);
        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/dev/auth/capabilities/frwiki")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("capabilities request should succeed");

        server.abort();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        let report: sp42_core::DevAuthCapabilityReport =
            serde_json::from_slice(&body).expect("capability report should parse");

        assert!(report.checked);
        assert_eq!(report.wiki_id, "frwiki");
        assert!(report.capabilities.read.can_authenticate);
        assert!(report.capabilities.moderation.can_patrol);
        assert!(report.capabilities.moderation.can_rollback);
        assert!(
            report
                .notes
                .iter()
                .any(|note| note.contains("Capability probe verified"))
        );
    }

    #[tokio::test]
    async fn live_operator_route_returns_canonical_operator_contract() {
        let local_env_path = temp_local_env_file(
            "WIKIMEDIA_CLIENT_APPLICATION_KEY=client-key\nWIKIMEDIA_CLIENT_APPLICATION_SECRET=client-secret\nWIKIMEDIA_ACCESS_TOKEN=token-value\n",
        );
        let (profile_base, server) = mock_capability_server().await;
        let runtime_root = std::env::temp_dir().join(format!(
            "sp42-live-operator-runtime-{}",
            SystemClock.now_ms()
        ));
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        let state = AppState {
            capability_cache: Arc::new(tokio::sync::RwLock::new(None)),
            sessions: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            pending_oauth_logins: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            http_client: reqwest::Client::builder()
                .user_agent(sp42_core::branding::USER_AGENT)
                .build()
                .expect("reqwest client should build"),
            local_oauth: LocalOAuthConfig::load_from_candidates([local_env_path]),
            runtime_storage_root: runtime_root.clone(),
            ingestion_supervisor: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            capability_targets: CapabilityProbeTargets {
                profile_url: format!("{profile_base}/oauth2/resource/profile"),
                api_url: Some(format!("{profile_base}/w/api.php")),
            },
            clock: clock.clone(),
            coordination: CoordinationRegistry::new(clock),
            next_client_id: Arc::new(AtomicU64::new(1)),
            next_session_id: Arc::new(AtomicU64::new(1)),
            started_at: Instant::now(),
        };

        let router = build_router(state);
        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/operator/live/frwiki?limit=1")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("live operator request should succeed");

        server.abort();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        let view: sp42_core::LiveOperatorView =
            serde_json::from_slice(&body).expect("live operator view should parse");

        assert_eq!(view.wiki_id, "frwiki");
        assert_eq!(view.query.limit, 1);
        assert_eq!(view.queue.len(), 1);
        assert!(view.review_workbench.is_some());
        assert!(view.backlog_status.is_some());
        assert!(view.stream_status.is_some());
        assert!(view.diff.is_some());
        assert!(view.capabilities.checked);
        assert!(view.backend.bootstrap_ready.is_enabled());
        assert!(!view.telemetry.phase_timings.is_empty());
        assert_eq!(
            view.action_preflight.recommended_kind,
            Some(SessionActionKind::Patrol)
        );
        assert!(
            view.action_preflight
                .recommendations
                .iter()
                .any(|entry| entry.available && entry.recommended)
        );
        assert!(
            view.debug_snapshot
                .summary_lines
                .iter()
                .any(|line| line.contains("queue_depth"))
        );
        assert!(
            view.notes
                .iter()
                .any(|line| line.contains("Persistent backlog checkpoint"))
        );

        let _ = std::fs::remove_dir_all(runtime_root);
    }

    #[tokio::test]
    async fn live_operator_route_surfaces_cached_backlog_state() {
        let local_env_path = temp_local_env_file(
            "WIKIMEDIA_CLIENT_APPLICATION_KEY=client-key\nWIKIMEDIA_CLIENT_APPLICATION_SECRET=client-secret\nWIKIMEDIA_ACCESS_TOKEN=token-value\n",
        );
        let (profile_base, server) = mock_capability_server().await;
        let runtime_root = std::env::temp_dir().join(format!(
            "sp42-live-operator-runtime-persist-{}",
            SystemClock.now_ms()
        ));
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        let state = AppState {
            capability_cache: Arc::new(tokio::sync::RwLock::new(None)),
            sessions: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            pending_oauth_logins: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            http_client: reqwest::Client::builder()
                .user_agent(sp42_core::branding::USER_AGENT)
                .build()
                .expect("reqwest client should build"),
            local_oauth: LocalOAuthConfig::load_from_candidates([local_env_path]),
            runtime_storage_root: runtime_root.clone(),
            ingestion_supervisor: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            capability_targets: CapabilityProbeTargets {
                profile_url: format!("{profile_base}/oauth2/resource/profile"),
                api_url: Some(format!("{profile_base}/w/api.php")),
            },
            clock: clock.clone(),
            coordination: CoordinationRegistry::new(clock),
            next_client_id: Arc::new(AtomicU64::new(1)),
            next_session_id: Arc::new(AtomicU64::new(1)),
            started_at: Instant::now(),
        };

        let router = build_router(state);
        let first = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/operator/live/frwiki?limit=1")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("first live operator request should succeed");
        assert_eq!(first.status(), StatusCode::OK);
        let first_body = to_bytes(first.into_body(), usize::MAX)
            .await
            .expect("first response body should read");
        let first_view: sp42_core::LiveOperatorView =
            serde_json::from_slice(&first_body).expect("first live operator view should parse");

        let second = router
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/operator/live/frwiki?limit=1&min_score=0")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("second live operator request should succeed");
        assert_eq!(second.status(), StatusCode::OK);

        server.abort();

        let second_body = to_bytes(second.into_body(), usize::MAX)
            .await
            .expect("second response body should read");
        let second_view: sp42_core::LiveOperatorView =
            serde_json::from_slice(&second_body).expect("second live operator view should parse");

        assert_eq!(first_view.queue[0].event.title, "Live route sample");
        assert_eq!(
            first_view
                .backlog_status
                .as_ref()
                .and_then(|status| status.next_continue.as_deref()),
            Some("20260324010202|456")
        );
        assert_eq!(second_view.queue[0].event.title, "Live route sample");
        assert_eq!(
            second_view
                .backlog_status
                .as_ref()
                .and_then(|status| status.next_continue.as_deref()),
            Some("20260324010202|456")
        );

        let _ = std::fs::remove_dir_all(runtime_root);
    }

    #[tokio::test]
    async fn operator_runtime_route_reports_persisted_checkpoints() {
        let state = test_state();
        let runtime_root = state.runtime_storage_root.clone();
        let storage = FileStorage::new(runtime_root.clone());
        storage
            .set(
                "recentchanges.rccontinue.frwiki".to_string(),
                b"20260324010202|456".to_vec(),
            )
            .await
            .expect("backlog checkpoint should persist");
        storage
            .set(
                "stream.last_event_id.frwiki".to_string(),
                b"event-99".to_vec(),
            )
            .await
            .expect("stream checkpoint should persist");

        let router = build_router(state);
        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/operator/runtime/frwiki")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("runtime inspection request should succeed");

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        let inspection: OperatorRuntimeInspection =
            serde_json::from_slice(&body).expect("runtime inspection should parse");

        assert_eq!(inspection.wiki_id, "frwiki");
        assert_eq!(
            inspection.backlog.next_continue.as_deref(),
            Some("20260324010202|456")
        );
        assert_eq!(inspection.stream_last_event_id.as_deref(), Some("event-99"));

        let _ = std::fs::remove_dir_all(runtime_root);
    }

    #[tokio::test]
    async fn operator_storage_layout_route_returns_canonical_plan() {
        let router = build_router(test_state());
        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!(
                        "{OPERATOR_STORAGE_LAYOUT_PATH}/frwiki?username=Schiste&shared_owner_username=Schiste"
                    ))
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("storage layout request should succeed");

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        let view: OperatorStorageLayoutView =
            serde_json::from_slice(&body).expect("storage layout should parse");

        assert_eq!(view.plan.personal_root.title, "User:Schiste/SP42");
        assert_eq!(
            view.plan.shared_root.title,
            "User:Schiste/SP42/frwiki/Registry"
        );
        assert!(
            view.personal_index_page
                .contains("[[User:Schiste/SP42/Profile]]")
        );
        assert_eq!(view.sample_document_pages.len(), 3);
    }

    #[tokio::test]
    async fn logical_storage_document_route_resolves_profile_page() {
        let (profile_base, server) = mock_capability_server().await;
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        let state = AppState {
            capability_cache: Arc::new(tokio::sync::RwLock::new(None)),
            sessions: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            pending_oauth_logins: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            http_client: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .user_agent(sp42_core::branding::USER_AGENT)
                .build()
                .expect("reqwest client should build"),
            local_oauth: LocalOAuthConfig::default(),
            runtime_storage_root: std::env::temp_dir()
                .join("sp42-server-runtime-logical-storage-route"),
            ingestion_supervisor: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            capability_targets: CapabilityProbeTargets {
                api_url: Some(format!("{profile_base}/w/api.php")),
                profile_url: format!("{profile_base}/oauth2/resource/profile"),
            },
            clock: clock.clone(),
            coordination: CoordinationRegistry::new(clock),
            next_client_id: Arc::new(AtomicU64::new(1)),
            next_session_id: Arc::new(AtomicU64::new(1)),
            started_at: Instant::now(),
        };
        let current_ms = state.clock.now_ms();
        let session_id = crate::install_session(
            &state,
            None,
            StoredSession {
                username: "Schiste".to_string(),
                scopes: vec!["basic".to_string(), "patrol".to_string()],
                expires_at_ms: Some(current_ms + 60_000),
                access_token: "token-value".to_string(),
                refresh_token: None,
                upstream_access_expires_at_ms: Some(current_ms + 60_000),
                bridge_mode: "oauth".to_string(),
                created_at_ms: current_ms,
                last_seen_at_ms: current_ms,
                capability_cache: HashMap::new(),
                action_history: Vec::new(),
            },
            current_ms,
        )
        .await;

        let router = build_router(state);
        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/operator/storage/logical/frwiki/personal/profile?username=Schiste")
                    .header(
                        axum::http::header::COOKIE,
                        format!("{}={session_id}", crate::SESSION_COOKIE_NAME),
                    )
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("logical storage request should succeed");

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        let view: crate::LogicalStorageDocumentView =
            serde_json::from_slice(&body).expect("logical storage view should parse");

        assert_eq!(view.document.title, "User:Schiste/SP42/Profile");
        assert_eq!(view.loaded.title, "User:Schiste/SP42/Profile");
        assert!(view.loaded.exists);

        server.abort();
    }

    #[tokio::test]
    async fn public_storage_document_route_returns_typed_preferences() {
        let (profile_base, server) = mock_capability_server().await;
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        let state = AppState {
            capability_cache: Arc::new(tokio::sync::RwLock::new(None)),
            sessions: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            pending_oauth_logins: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            http_client: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .user_agent(sp42_core::branding::USER_AGENT)
                .build()
                .expect("reqwest client should build"),
            local_oauth: LocalOAuthConfig::default(),
            runtime_storage_root: std::env::temp_dir()
                .join("sp42-server-runtime-public-storage-route"),
            ingestion_supervisor: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            capability_targets: CapabilityProbeTargets {
                api_url: Some(format!("{profile_base}/w/api.php")),
                profile_url: format!("{profile_base}/oauth2/resource/profile"),
            },
            clock: clock.clone(),
            coordination: CoordinationRegistry::new(clock),
            next_client_id: Arc::new(AtomicU64::new(1)),
            next_session_id: Arc::new(AtomicU64::new(1)),
            started_at: Instant::now(),
        };
        let current_ms = state.clock.now_ms();
        let session_id = crate::install_session(
            &state,
            None,
            StoredSession {
                username: "Schiste".to_string(),
                scopes: vec!["basic".to_string(), "patrol".to_string()],
                expires_at_ms: Some(current_ms + 60_000),
                access_token: "token-value".to_string(),
                refresh_token: None,
                upstream_access_expires_at_ms: Some(current_ms + 60_000),
                bridge_mode: "oauth".to_string(),
                created_at_ms: current_ms,
                last_seen_at_ms: current_ms,
                capability_cache: HashMap::new(),
                action_history: Vec::new(),
            },
            current_ms,
        )
        .await;

        let router = build_router(state);
        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/operator/storage/public/frwiki/preferences?username=Schiste")
                    .header(
                        axum::http::header::COOKIE,
                        format!("{}={session_id}", crate::SESSION_COOKIE_NAME),
                    )
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("public storage request should succeed");

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        let view: crate::PublicStorageDocumentView =
            serde_json::from_slice(&body).expect("public storage view should parse");

        assert_eq!(view.document.title, "User:Schiste/SP42/Preferences");
        assert_eq!(view.loaded.title, "User:Schiste/SP42/Preferences");
        assert!(matches!(
            view.payload,
            crate::PublicStorageDocumentData::Preferences(_)
        ));

        server.abort();
    }

    #[tokio::test]
    async fn bootstrap_derives_username_and_scopes_from_validated_token() {
        let local_env_path = temp_local_env_file(
            "WIKIMEDIA_CLIENT_APPLICATION_KEY=client-key\nWIKIMEDIA_CLIENT_APPLICATION_SECRET=client-secret\nWIKIMEDIA_ACCESS_TOKEN=token-value\n",
        );
        let (profile_base, server) = mock_capability_server().await;
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        let state = AppState {
            capability_cache: Arc::new(tokio::sync::RwLock::new(None)),
            sessions: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            pending_oauth_logins: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            http_client: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .user_agent(sp42_core::branding::USER_AGENT)
                .build()
                .expect("reqwest client should build"),
            local_oauth: LocalOAuthConfig::load_from_candidates([local_env_path]),
            runtime_storage_root: std::env::temp_dir().join("sp42-server-runtime-bootstrap"),
            ingestion_supervisor: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            capability_targets: CapabilityProbeTargets {
                profile_url: format!("{profile_base}/oauth2/resource/profile"),
                api_url: Some(format!("{profile_base}/w/api.php")),
            },
            clock: clock.clone(),
            coordination: CoordinationRegistry::new(clock),
            next_client_id: Arc::new(AtomicU64::new(1)),
            next_session_id: Arc::new(AtomicU64::new(1)),
            started_at: Instant::now(),
        };

        let router = build_router(state);
        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/dev/auth/session/bootstrap")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "username": "",
                            "scopes": [],
                            "expires_at_ms": null
                        })
                        .to_string(),
                    ))
                    .expect("request should build"),
            )
            .await
            .expect("bootstrap request should succeed");

        server.abort();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        let status: sp42_core::DevAuthSessionStatus =
            serde_json::from_slice(&body).expect("status should parse");

        assert!(status.authenticated);
        assert_eq!(status.username.as_deref(), Some("Schiste"));
        assert_eq!(
            status.scopes,
            vec![
                "basic".to_string(),
                "editpage".to_string(),
                "patrol".to_string(),
                "rollback".to_string()
            ]
        );
        assert_eq!(status.bridge_mode, "local-env-token");
        assert!(status.expires_at_ms.is_some());
    }

    #[tokio::test]
    async fn bootstrap_rejects_caller_supplied_identity_scope_and_expiry() {
        let router = build_router(test_state());
        for payload in [
            serde_json::json!({
                "username": "Alice",
                "scopes": [],
                "expires_at_ms": null
            }),
            serde_json::json!({
                "username": "",
                "scopes": ["rollback"],
                "expires_at_ms": null
            }),
            serde_json::json!({
                "username": "",
                "scopes": [],
                "expires_at_ms": 42
            }),
        ] {
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .method(Method::POST)
                        .uri("/dev/auth/session/bootstrap")
                        .header("content-type", "application/json")
                        .body(Body::from(payload.to_string()))
                        .expect("request should build"),
                )
                .await
                .expect("bootstrap request should succeed");

            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        }
    }

    #[tokio::test]
    async fn coordination_snapshot_route_is_available() {
        let state = test_state();
        state.coordination.connect_client("frwiki").await;
        let router = build_router(state);
        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/coordination/rooms")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("coordination request should succeed");

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        let snapshot: sp42_core::CoordinationSnapshot =
            serde_json::from_slice(&body).expect("snapshot should parse");

        assert_eq!(snapshot.rooms.len(), 1);
        assert_eq!(snapshot.rooms[0].wiki_id, "frwiki");
        assert_eq!(snapshot.rooms[0].connected_clients, 1);
        assert_eq!(snapshot.rooms[0].published_messages, 0);
    }

    #[tokio::test]
    async fn coordination_inspections_route_is_available() {
        let state = test_state();
        let payload = sp42_core::encode_message(&sp42_core::CoordinationMessage::EditClaim(
            sp42_core::EditClaim {
                wiki_id: "frwiki".to_string(),
                rev_id: 123_456,
                actor: "Alice".to_string(),
            },
        ))
        .expect("message should encode");
        state
            .coordination
            .publish(
                "frwiki",
                crate::coordination::CoordinationEnvelope {
                    sender_id: 1,
                    payload,
                },
            )
            .await;
        let router = build_router(state);
        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/coordination/inspections")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("inspection request should succeed");

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        let inspections: RoomInspectionCollection =
            serde_json::from_slice(&body).expect("inspection collection should parse");

        assert_eq!(inspections.rooms.len(), 1);
        assert_eq!(inspections.rooms[0].room.wiki_id, "frwiki");
        assert_eq!(
            inspections.rooms[0]
                .state
                .as_ref()
                .map(|state| state.claims.len()),
            Some(1)
        );
    }

    #[tokio::test]
    async fn coordination_room_state_route_is_available() {
        let state = test_state();
        let payload = sp42_core::encode_message(&sp42_core::CoordinationMessage::EditClaim(
            sp42_core::EditClaim {
                wiki_id: "frwiki".to_string(),
                rev_id: 123_456,
                actor: "Alice".to_string(),
            },
        ))
        .expect("message should encode");
        state
            .coordination
            .publish(
                "frwiki",
                crate::coordination::CoordinationEnvelope {
                    sender_id: 1,
                    payload,
                },
            )
            .await;
        let router = build_router(state);
        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/coordination/rooms/frwiki")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("coordination room request should succeed");

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        let summary: sp42_core::CoordinationStateSummary =
            serde_json::from_slice(&body).expect("room summary should parse");

        assert_eq!(summary.wiki_id, "frwiki");
        assert_eq!(summary.claims.len(), 1);
    }

    #[tokio::test]
    async fn coordination_room_inspection_route_is_available() {
        let state = test_state();
        let payload = sp42_core::encode_message(&sp42_core::CoordinationMessage::EditClaim(
            sp42_core::EditClaim {
                wiki_id: "frwiki".to_string(),
                rev_id: 123_456,
                actor: "Alice".to_string(),
            },
        ))
        .expect("message should encode");
        state
            .coordination
            .publish(
                "frwiki",
                crate::coordination::CoordinationEnvelope {
                    sender_id: 1,
                    payload,
                },
            )
            .await;
        let router = build_router(state);
        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/coordination/rooms/frwiki/inspection")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("room inspection request should succeed");

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        let report: CoordinationRoomInspection =
            serde_json::from_slice(&body).expect("room inspection should parse");

        assert_eq!(report.room.wiki_id, "frwiki");
        assert_eq!(
            report.state.as_ref().map(|state| state.claims.len()),
            Some(1)
        );
        assert_eq!(report.metrics.accepted_messages, 1);
    }

    #[tokio::test]
    async fn missing_coordination_room_inspection_returns_empty_bootstrap_model() {
        let router = build_router(test_state());
        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/coordination/rooms/frwiki/inspection")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("room inspection request should succeed");

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        let report: CoordinationRoomInspection =
            serde_json::from_slice(&body).expect("room inspection should parse");

        assert_eq!(report.room.wiki_id, "frwiki");
        assert_eq!(report.room.connected_clients, 0);
        assert_eq!(report.room.published_messages, 0);
        assert_eq!(
            report.state.as_ref().map(|state| state.wiki_id.as_str()),
            Some("frwiki")
        );
        assert_eq!(
            report.state.as_ref().map(|state| state.claims.len()),
            Some(0)
        );
        assert_eq!(report.metrics.accepted_messages, 0);
    }

    #[tokio::test]
    async fn debug_summary_route_is_available() {
        let state = test_state();
        state.coordination.connect_client("frwiki").await;
        let router = build_router(state);
        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/debug/summary")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("debug summary request should succeed");

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        let summary: sp42_core::ServerDebugSummary =
            serde_json::from_slice(&body).expect("summary should parse");

        assert_eq!(summary.project, sp42_core::branding::PROJECT_NAME);
        assert!(!summary.auth.authenticated);
        assert!(!summary.oauth.access_token_present);
        assert!(!summary.capabilities.checked);
        assert_eq!(summary.coordination.rooms.len(), 1);
    }

    #[tokio::test]
    async fn multi_user_coordination_flow_round_trips_across_authenticated_clients() {
        let state = test_state();
        let created_at_ms = now_ms();
        state.sessions.write().await.extend([
            (
                "session-a".to_string(),
                test_session("Alice", "token-a", created_at_ms),
            ),
            (
                "session-b".to_string(),
                test_session("Bob", "token-b", created_at_ms),
            ),
            (
                "session-c".to_string(),
                test_session("Carol", "token-c", created_at_ms),
            ),
        ]);

        let (base_url, server) = spawn_test_server(build_router(state)).await;
        let mut alice = connect_session_socket(&base_url, "frwiki", "session-a").await;
        let mut bob = connect_session_socket(&base_url, "frwiki", "session-b").await;
        let mut carol = connect_session_socket(&base_url, "frwiki", "session-c").await;

        send_coordination_message(
            &mut alice,
            sp42_core::CoordinationMessage::EditClaim(sp42_core::EditClaim {
                wiki_id: "frwiki".to_string(),
                rev_id: 123_456,
                actor: "Mallory".to_string(),
            }),
        )
        .await;

        let bob_claim = recv_coordination_message(&mut bob).await;
        let carol_claim = recv_coordination_message(&mut carol).await;
        assert_eq!(bob_claim, carol_claim);
        assert_claim_actor(&bob_claim, "Alice", 123_456);

        send_coordination_message(
            &mut bob,
            sp42_core::CoordinationMessage::PresenceHeartbeat(sp42_core::PresenceHeartbeat {
                wiki_id: "frwiki".to_string(),
                actor: "Mallory".to_string(),
                active_edit_count: 2,
            }),
        )
        .await;

        let alice_presence = recv_coordination_message(&mut alice).await;
        let carol_presence = recv_coordination_message(&mut carol).await;
        assert_eq!(alice_presence, carol_presence);
        assert_presence_actor(&alice_presence, "Bob", 2);

        send_coordination_message(
            &mut carol,
            sp42_core::CoordinationMessage::ActionBroadcast(sp42_core::ActionBroadcast {
                wiki_id: "frwiki".to_string(),
                rev_id: 123_456,
                action: sp42_core::Action::Warn,
                actor: "Mallory".to_string(),
            }),
        )
        .await;

        let alice_action = recv_coordination_message(&mut alice).await;
        let bob_action = recv_coordination_message(&mut bob).await;
        assert_eq!(alice_action, bob_action);
        assert_action_actor(&alice_action, "Carol", &sp42_core::Action::Warn);

        send_coordination_message(
            &mut bob,
            sp42_core::CoordinationMessage::RaceResolution(sp42_core::RaceResolution {
                wiki_id: "frwiki".to_string(),
                rev_id: 123_456,
                winning_actor: "Mallory".to_string(),
            }),
        )
        .await;

        let alice_resolution = recv_coordination_message(&mut alice).await;
        let carol_resolution = recv_coordination_message(&mut carol).await;
        assert_eq!(alice_resolution, carol_resolution);
        assert_race_resolution_actor(&alice_resolution, "Bob", 123_456);

        let inspection = fetch_room_inspection(&base_url, "frwiki").await;

        assert_eq!(inspection.room.connected_clients, 3);
        assert_eq!(inspection.room.claim_count, 1);
        assert_eq!(inspection.room.presence_count, 1);
        assert_eq!(inspection.room.recent_action_count, 1);
        assert_eq!(inspection.room.race_resolution_count, 1);
        assert_eq!(inspection.metrics.accepted_messages, 4);
        assert_eq!(inspection.metrics.invalid_messages, 0);
        let state = inspection.state.expect("room state should exist");
        assert_eq!(state.claims.len(), 1);
        // Final room state should reflect the winner after race resolution, not the initial claimer.
        assert_eq!(state.claims[0].actor, "Bob");
        assert_eq!(state.presence.len(), 1);
        assert_eq!(state.presence[0].actor, "Bob");
        assert_eq!(state.recent_actions.len(), 1);
        assert_eq!(state.recent_actions[0].actor, "Carol");
        assert_eq!(state.race_resolutions.len(), 1);
        assert_eq!(state.race_resolutions[0].winning_actor, "Bob");

        let _ = alice.close(None).await;
        let _ = bob.close(None).await;
        let _ = carol.close(None).await;
        server.abort();
    }

    #[tokio::test]
    async fn anonymous_multi_user_flow_preserves_actor_and_clears_presence() {
        let state = test_state();
        let (base_url, server) = spawn_test_server(build_router(state)).await;
        let mut alpha = connect_anonymous_socket(&base_url, "frwiki").await;
        let mut beta = connect_anonymous_socket(&base_url, "frwiki").await;

        send_coordination_message(
            &mut alpha,
            sp42_core::CoordinationMessage::EditClaim(sp42_core::EditClaim {
                wiki_id: "frwiki".to_string(),
                rev_id: 900_001,
                actor: "AnonymousUser".to_string(),
            }),
        )
        .await;
        let beta_claim = recv_coordination_message(&mut beta).await;
        assert_claim_actor(&beta_claim, "AnonymousUser", 900_001);

        send_coordination_message(
            &mut alpha,
            sp42_core::CoordinationMessage::PresenceHeartbeat(sp42_core::PresenceHeartbeat {
                wiki_id: "frwiki".to_string(),
                actor: "AnonymousUser".to_string(),
                active_edit_count: 1,
            }),
        )
        .await;
        let beta_presence = recv_coordination_message(&mut beta).await;
        assert_presence_actor(&beta_presence, "AnonymousUser", 1);

        send_coordination_message(
            &mut alpha,
            sp42_core::CoordinationMessage::PresenceHeartbeat(sp42_core::PresenceHeartbeat {
                wiki_id: "frwiki".to_string(),
                actor: "AnonymousUser".to_string(),
                active_edit_count: 0,
            }),
        )
        .await;
        let beta_presence_clear = recv_coordination_message(&mut beta).await;
        assert_presence_actor(&beta_presence_clear, "AnonymousUser", 0);

        let inspection = fetch_room_inspection(&base_url, "frwiki").await;
        assert_eq!(inspection.room.connected_clients, 2);
        assert_eq!(inspection.room.claim_count, 1);
        assert_eq!(inspection.room.presence_count, 0);
        assert_eq!(inspection.metrics.accepted_messages, 3);
        assert_eq!(inspection.metrics.invalid_messages, 0);
        let state = inspection.state.expect("room state should exist");
        assert_eq!(state.claims.len(), 1);
        assert_eq!(state.claims[0].actor, "AnonymousUser");
        assert!(state.presence.is_empty());

        let _ = alpha.close(None).await;
        let _ = beta.close(None).await;
        server.abort();
    }

    #[tokio::test]
    async fn invalid_coordination_payload_is_counted_without_mutating_state() {
        let state = test_state();
        let created_at_ms = now_ms();
        state.sessions.write().await.extend([
            (
                "session-a".to_string(),
                test_session("Alice", "token-a", created_at_ms),
            ),
            (
                "session-b".to_string(),
                test_session("Bob", "token-b", created_at_ms),
            ),
        ]);

        let (base_url, server) = spawn_test_server(build_router(state)).await;
        let mut alice = connect_session_socket(&base_url, "frwiki", "session-a").await;
        let mut bob = connect_session_socket(&base_url, "frwiki", "session-b").await;

        alice
            .send(WebSocketMessage::Binary(b"not-msgpack".to_vec().into()))
            .await
            .expect("invalid binary payload should send");
        let echoed = recv_binary_frame(&mut bob).await;
        assert_eq!(echoed, b"not-msgpack".to_vec());

        let inspection = fetch_room_inspection(&base_url, "frwiki").await;
        assert_eq!(inspection.room.connected_clients, 2);
        assert_eq!(inspection.metrics.published_messages, 1);
        assert_eq!(inspection.metrics.accepted_messages, 0);
        assert_eq!(inspection.metrics.invalid_messages, 1);
        let state = inspection.state.expect("room state should exist");
        assert!(state.claims.is_empty());
        assert!(state.presence.is_empty());
        assert!(state.recent_actions.is_empty());

        let _ = alice.close(None).await;
        let _ = bob.close(None).await;
        server.abort();
    }

    #[tokio::test]
    async fn coordination_room_persists_after_disconnect_and_reports_zero_clients() {
        let state = test_state();
        let created_at_ms = now_ms();
        state.sessions.write().await.insert(
            "session-a".to_string(),
            test_session("Alice", "token-a", created_at_ms),
        );

        let (base_url, server) = spawn_test_server(build_router(state)).await;
        let mut alice = connect_session_socket(&base_url, "frwiki", "session-a").await;

        send_coordination_message(
            &mut alice,
            sp42_core::CoordinationMessage::EditClaim(sp42_core::EditClaim {
                wiki_id: "frwiki".to_string(),
                rev_id: 777_001,
                actor: "Mallory".to_string(),
            }),
        )
        .await;
        let _ = alice.close(None).await;

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let inspection = fetch_room_inspection(&base_url, "frwiki").await;
        assert_eq!(inspection.room.connected_clients, 0);
        assert_eq!(inspection.room.claim_count, 1);
        assert_eq!(inspection.metrics.published_messages, 1);
        assert_eq!(inspection.metrics.accepted_messages, 1);
        assert_eq!(inspection.metrics.invalid_messages, 0);
        let state = inspection.state.expect("room state should exist");
        assert_eq!(state.claims.len(), 1);
        assert_eq!(state.claims[0].actor, "Alice");

        server.abort();
    }

    #[tokio::test]
    async fn reconnecting_client_resubscribes_and_room_state_persists() {
        let state = test_state();
        let created_at_ms = now_ms();
        state.sessions.write().await.extend([
            (
                "session-a".to_string(),
                test_session("Alice", "token-a", created_at_ms),
            ),
            (
                "session-b".to_string(),
                test_session("Bob", "token-b", created_at_ms),
            ),
        ]);

        let (base_url, server) = spawn_test_server(build_router(state)).await;
        let mut alice = connect_session_socket(&base_url, "frwiki", "session-a").await;
        let mut bob = connect_session_socket(&base_url, "frwiki", "session-b").await;

        send_coordination_message(
            &mut alice,
            sp42_core::CoordinationMessage::EditClaim(sp42_core::EditClaim {
                wiki_id: "frwiki".to_string(),
                rev_id: 880_001,
                actor: "Mallory".to_string(),
            }),
        )
        .await;
        let bob_claim = recv_coordination_message(&mut bob).await;
        assert_claim_actor(&bob_claim, "Alice", 880_001);

        let _ = bob.close(None).await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let inspection_after_disconnect = fetch_room_inspection(&base_url, "frwiki").await;
        assert_eq!(inspection_after_disconnect.room.connected_clients, 1);
        let disconnected_state = inspection_after_disconnect
            .state
            .expect("room state should persist after disconnect");
        assert_eq!(disconnected_state.claims.len(), 1);
        assert_eq!(disconnected_state.claims[0].actor, "Alice");

        let mut bob_reconnected = connect_session_socket(&base_url, "frwiki", "session-b").await;
        send_coordination_message(
            &mut alice,
            sp42_core::CoordinationMessage::PresenceHeartbeat(sp42_core::PresenceHeartbeat {
                wiki_id: "frwiki".to_string(),
                actor: "Mallory".to_string(),
                active_edit_count: 3,
            }),
        )
        .await;
        let bob_presence = recv_coordination_message(&mut bob_reconnected).await;
        assert_presence_actor(&bob_presence, "Alice", 3);

        let inspection_after_reconnect = fetch_room_inspection(&base_url, "frwiki").await;
        assert_eq!(inspection_after_reconnect.room.connected_clients, 2);
        assert_eq!(inspection_after_reconnect.room.claim_count, 1);
        assert_eq!(inspection_after_reconnect.room.presence_count, 1);
        assert_eq!(inspection_after_reconnect.metrics.accepted_messages, 2);
        let reconnected_state = inspection_after_reconnect
            .state
            .expect("room state should exist after reconnect");
        assert_eq!(reconnected_state.claims[0].actor, "Alice");
        assert_eq!(reconnected_state.presence[0].actor, "Alice");

        let _ = alice.close(None).await;
        let _ = bob_reconnected.close(None).await;
        server.abort();
    }

    #[tokio::test]
    async fn competing_claims_follow_last_writer_until_race_resolution() {
        let state = test_state();
        let created_at_ms = now_ms();
        state.sessions.write().await.extend([
            (
                "session-a".to_string(),
                test_session("Alice", "token-a", created_at_ms),
            ),
            (
                "session-b".to_string(),
                test_session("Bob", "token-b", created_at_ms),
            ),
            (
                "session-c".to_string(),
                test_session("Carol", "token-c", created_at_ms),
            ),
        ]);

        let (base_url, server) = spawn_test_server(build_router(state)).await;
        let mut alice = connect_session_socket(&base_url, "frwiki", "session-a").await;
        let mut bob = connect_session_socket(&base_url, "frwiki", "session-b").await;
        let mut carol = connect_session_socket(&base_url, "frwiki", "session-c").await;

        send_coordination_message(
            &mut alice,
            sp42_core::CoordinationMessage::EditClaim(sp42_core::EditClaim {
                wiki_id: "frwiki".to_string(),
                rev_id: 990_001,
                actor: "Mallory".to_string(),
            }),
        )
        .await;
        let bob_claim = recv_coordination_message(&mut bob).await;
        let carol_claim = recv_coordination_message(&mut carol).await;
        assert_eq!(bob_claim, carol_claim);
        assert_claim_actor(&bob_claim, "Alice", 990_001);

        send_coordination_message(
            &mut bob,
            sp42_core::CoordinationMessage::EditClaim(sp42_core::EditClaim {
                wiki_id: "frwiki".to_string(),
                rev_id: 990_001,
                actor: "Mallory".to_string(),
            }),
        )
        .await;
        let alice_claim = recv_coordination_message(&mut alice).await;
        let carol_conflict_claim = recv_coordination_message(&mut carol).await;
        assert_eq!(alice_claim, carol_conflict_claim);
        assert_claim_actor(&alice_claim, "Bob", 990_001);

        let inspection_before_resolution = fetch_room_inspection(&base_url, "frwiki").await;
        assert_eq!(inspection_before_resolution.room.claim_count, 1);
        let state_before_resolution = inspection_before_resolution
            .state
            .expect("state should exist before race resolution");
        assert_eq!(state_before_resolution.claims.len(), 1);
        assert_eq!(state_before_resolution.claims[0].actor, "Bob");
        assert_eq!(inspection_before_resolution.metrics.accepted_messages, 2);

        send_coordination_message(
            &mut alice,
            sp42_core::CoordinationMessage::RaceResolution(sp42_core::RaceResolution {
                wiki_id: "frwiki".to_string(),
                rev_id: 990_001,
                winning_actor: "Mallory".to_string(),
            }),
        )
        .await;
        let bob_resolution = recv_coordination_message(&mut bob).await;
        let carol_resolution = recv_coordination_message(&mut carol).await;
        assert_eq!(bob_resolution, carol_resolution);
        assert_race_resolution_actor(&bob_resolution, "Alice", 990_001);

        send_coordination_message(
            &mut bob,
            sp42_core::CoordinationMessage::EditClaim(sp42_core::EditClaim {
                wiki_id: "frwiki".to_string(),
                rev_id: 990_001,
                actor: "Mallory".to_string(),
            }),
        )
        .await;
        let alice_post_resolution_claim = recv_coordination_message(&mut alice).await;
        let carol_post_resolution_claim = recv_coordination_message(&mut carol).await;
        assert_eq!(alice_post_resolution_claim, carol_post_resolution_claim);
        assert_claim_actor(&alice_post_resolution_claim, "Bob", 990_001);

        let inspection_after_resolution = fetch_room_inspection(&base_url, "frwiki").await;
        assert_eq!(inspection_after_resolution.room.claim_count, 1);
        assert_eq!(inspection_after_resolution.room.race_resolution_count, 1);
        assert_eq!(inspection_after_resolution.metrics.accepted_messages, 4);
        let state_after_resolution = inspection_after_resolution
            .state
            .expect("state should exist after race resolution");
        assert_eq!(state_after_resolution.claims.len(), 1);
        assert_eq!(state_after_resolution.claims[0].actor, "Alice");
        assert_eq!(state_after_resolution.race_resolutions.len(), 1);
        assert_eq!(
            state_after_resolution.race_resolutions[0].winning_actor,
            "Alice"
        );

        let _ = alice.close(None).await;
        let _ = bob.close(None).await;
        let _ = carol.close(None).await;
        server.abort();
    }

    #[tokio::test]
    async fn stale_presence_is_pruned_from_room_state_reports() {
        let state = test_state();
        let created_at_ms = now_ms();
        state.sessions.write().await.extend([
            (
                "session-a".to_string(),
                test_session("Alice", "token-a", created_at_ms),
            ),
            (
                "session-b".to_string(),
                test_session("Bob", "token-b", created_at_ms),
            ),
        ]);

        let (base_url, server) = spawn_test_server(build_router(state.clone())).await;
        let mut alice = connect_session_socket(&base_url, "frwiki", "session-a").await;
        let mut bob = connect_session_socket(&base_url, "frwiki", "session-b").await;

        send_coordination_message(
            &mut alice,
            sp42_core::CoordinationMessage::PresenceHeartbeat(sp42_core::PresenceHeartbeat {
                wiki_id: "frwiki".to_string(),
                actor: "Mallory".to_string(),
                active_edit_count: 2,
            }),
        )
        .await;
        let bob_presence = recv_coordination_message(&mut bob).await;
        assert_presence_actor(&bob_presence, "Alice", 2);

        state
            .coordination
            .set_presence_last_seen_for_test("frwiki", "Alice", now_ms() - 60_001)
            .await;

        let inspection = fetch_room_inspection(&base_url, "frwiki").await;
        assert_eq!(inspection.room.connected_clients, 2);
        assert_eq!(inspection.room.presence_count, 0);
        let state = inspection.state.expect("room state should exist");
        assert!(state.presence.is_empty());

        let _ = alice.close(None).await;
        let _ = bob.close(None).await;
        server.abort();
    }

    #[tokio::test]
    async fn fresh_client_recovers_race_resolved_state_via_room_inspection() {
        let state = test_state();
        let created_at_ms = now_ms();
        state.sessions.write().await.extend([
            (
                "session-a".to_string(),
                test_session("Alice", "token-a", created_at_ms),
            ),
            (
                "session-b".to_string(),
                test_session("Bob", "token-b", created_at_ms),
            ),
            (
                "session-c".to_string(),
                test_session("Carol", "token-c", created_at_ms),
            ),
        ]);

        let (base_url, server) = spawn_test_server(build_router(state)).await;
        let mut alice = connect_session_socket(&base_url, "frwiki", "session-a").await;
        let mut bob = connect_session_socket(&base_url, "frwiki", "session-b").await;

        send_coordination_message(
            &mut bob,
            sp42_core::CoordinationMessage::EditClaim(sp42_core::EditClaim {
                wiki_id: "frwiki".to_string(),
                rev_id: 991_001,
                actor: "Mallory".to_string(),
            }),
        )
        .await;
        let alice_claim = recv_coordination_message(&mut alice).await;
        assert_claim_actor(&alice_claim, "Bob", 991_001);

        send_coordination_message(
            &mut alice,
            sp42_core::CoordinationMessage::RaceResolution(sp42_core::RaceResolution {
                wiki_id: "frwiki".to_string(),
                rev_id: 991_001,
                winning_actor: "Mallory".to_string(),
            }),
        )
        .await;
        let bob_resolution = recv_coordination_message(&mut bob).await;
        assert_race_resolution_actor(&bob_resolution, "Alice", 991_001);

        let mut carol = connect_session_socket(&base_url, "frwiki", "session-c").await;
        expect_no_coordination_message(&mut carol).await;

        let recovered = fetch_room_inspection(&base_url, "frwiki").await;
        assert_eq!(recovered.room.connected_clients, 3);
        assert_eq!(recovered.room.claim_count, 1);
        assert_eq!(recovered.room.race_resolution_count, 1);
        let recovered_state = recovered.state.expect("room state should exist");
        assert_eq!(recovered_state.claims.len(), 1);
        assert_eq!(recovered_state.claims[0].actor, "Alice");
        assert_eq!(recovered_state.race_resolutions.len(), 1);
        assert_eq!(recovered_state.race_resolutions[0].winning_actor, "Alice");

        send_coordination_message(
            &mut bob,
            sp42_core::CoordinationMessage::ActionBroadcast(sp42_core::ActionBroadcast {
                wiki_id: "frwiki".to_string(),
                rev_id: 991_001,
                action: sp42_core::Action::MarkPatrolled,
                actor: "Mallory".to_string(),
            }),
        )
        .await;
        let alice_action = recv_coordination_message(&mut alice).await;
        let carol_action = recv_coordination_message(&mut carol).await;
        assert_eq!(alice_action, carol_action);
        assert_action_actor(&carol_action, "Bob", &sp42_core::Action::MarkPatrolled);

        let _ = alice.close(None).await;
        let _ = bob.close(None).await;
        let _ = carol.close(None).await;
        server.abort();
    }

    #[tokio::test]
    async fn reconnect_storm_keeps_room_counts_and_live_delivery_consistent() {
        let state = test_state();
        let created_at_ms = now_ms();
        state.sessions.write().await.extend([
            (
                "session-a".to_string(),
                test_session("Alice", "token-a", created_at_ms),
            ),
            (
                "session-b".to_string(),
                test_session("Bob", "token-b", created_at_ms),
            ),
            (
                "session-c".to_string(),
                test_session("Carol", "token-c", created_at_ms),
            ),
        ]);

        let (base_url, server) = spawn_test_server(build_router(state)).await;
        let mut alice = connect_session_socket(&base_url, "frwiki", "session-a").await;

        for cycle in 0..3u64 {
            let mut bob = connect_session_socket(&base_url, "frwiki", "session-b").await;
            let mut carol = connect_session_socket(&base_url, "frwiki", "session-c").await;

            send_coordination_message(
                &mut alice,
                sp42_core::CoordinationMessage::PresenceHeartbeat(sp42_core::PresenceHeartbeat {
                    wiki_id: "frwiki".to_string(),
                    actor: "Mallory".to_string(),
                    active_edit_count: u32::try_from(cycle + 1).expect("cycle fits in u32"),
                }),
            )
            .await;

            let bob_presence = recv_coordination_message(&mut bob).await;
            let carol_presence = recv_coordination_message(&mut carol).await;
            assert_eq!(bob_presence, carol_presence);
            assert_presence_actor(
                &bob_presence,
                "Alice",
                u32::try_from(cycle + 1).expect("cycle fits in u32"),
            );

            let inspection = fetch_room_inspection(&base_url, "frwiki").await;
            assert_eq!(inspection.room.connected_clients, 3);
            assert_eq!(inspection.room.presence_count, 1);

            let _ = bob.close(None).await;
            let _ = carol.close(None).await;
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;

            let inspection_after_disconnect = fetch_room_inspection(&base_url, "frwiki").await;
            assert_eq!(inspection_after_disconnect.room.connected_clients, 1);
            assert_eq!(inspection_after_disconnect.room.presence_count, 1);
        }

        let final_inspection = fetch_room_inspection(&base_url, "frwiki").await;
        assert_eq!(final_inspection.room.connected_clients, 1);
        assert_eq!(final_inspection.room.presence_count, 1);
        assert_eq!(final_inspection.metrics.accepted_messages, 3);
        let final_state = final_inspection.state.expect("room state should exist");
        assert_eq!(final_state.presence.len(), 1);
        assert_eq!(final_state.presence[0].actor, "Alice");
        assert_eq!(final_state.presence[0].active_edit_count, 3);

        let _ = alice.close(None).await;
        server.abort();
    }
}
