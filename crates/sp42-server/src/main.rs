mod action_routes;
mod auth_routes;
mod coordination;
mod deployment;
mod endpoint_manifest;
mod ingestion_supervisor;
mod live_queue;
mod local_env;
mod oauth_runtime;
mod operator_live;
mod revision_artifacts;
mod routes;
mod runtime_status;
mod session_runtime;
mod state;
mod static_assets;
mod storage_routes;
mod wiki_registry;
mod wikimedia_capabilities;

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use axum::Json;
use axum::extract::OriginalUri;
use axum::extract::Path;
use axum::extract::Query;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use futures::{SinkExt, StreamExt};
use rand::Rng as _;
use sp42_core::traits::HttpClient;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use sp42_core::{
    ActionExecutionHistoryReport, ActionExecutionLogEntry, ActionExecutionStatusReport,
    ArticleInventory, Clock, CoordinationSnapshot, DevAuthBootstrapRequest,
    DevAuthCapabilityReport, DevAuthSessionStatus, FlagState, LiveOperatorBackendStatus,
    LiveOperatorPhaseTiming, LiveOperatorPublicDocuments, LiveOperatorTelemetry, OAuthCallback,
    OAuthClientConfig, OAuthTokenResponse, PublicAuditLedgerEntry, PublicStorageDocumentData,
    SessionActionExecutionRequest, SessionActionExecutionResponse, SessionActionKind, SystemClock,
    TokenKind, WikiConfig, WikiStorageConfig, WikiStorageDocument, WikiStorageDocumentKind,
    WikiStorageLoadedDocument, WikiStoragePlan, WikiStoragePlanInput, WikiStorageWriteOutcome,
    WikiStorageWriteRequest, build_article_inventory, build_authorization_url, build_media_diff,
    build_wiki_storage_plan, default_public_storage_document, diff_lines, execute_fetch_token,
    generate_oauth_state, generate_pkce_verifier, load_wiki_storage_document, parse_callback_query,
    render_wiki_storage_document_page, render_wiki_storage_index_page,
    resolve_wiki_storage_document, save_wiki_storage_document,
};
use sp42_reporting::LiveOperatorView;

#[cfg(test)]
use crate::coordination::CoordinationRoomInspection;
use crate::coordination::{CoordinationEnvelope, CoordinationRegistry};
use crate::deployment::DeploymentConfig;
#[cfg(test)]
use crate::endpoint_manifest::operator_endpoint_manifest;
pub(crate) use crate::live_queue::{
    IngestionSupervisorSnapshot, LiveOperatorAssembly, LiveOperatorFinalization,
    LiveOperatorProductContext, LiveOperatorPublicContextState, LivePublicDocumentLoadSpec,
    LiveViewFilterParams, build_live_operator_notes, build_live_operator_products,
    finalize_live_operator_view, load_live_operator_bootstrap, load_live_queue_state,
    load_selected_review_state, supervisor_snapshot_for_wiki,
};
use crate::local_env::LocalOAuthConfig;
pub(crate) use crate::revision_artifacts::{
    fetch_revision_diff, fetch_revision_media_diff, get_rendered_hunk_preview, get_revision_diff,
    get_revision_media_diff,
};
use crate::routes::build_router;
#[cfg(test)]
pub(crate) use crate::runtime_status::{
    CapabilityCacheStatus, CapabilityProbeHint, OperatorReport, OperatorRuntimeInspection,
    RuntimeDebugStatus,
};
pub(crate) use crate::runtime_status::{
    DevAuthBootstrapStatus, RoomInspectionCollection, ServerHealthStatus, empty_room_inspection,
    get_debug_summary, get_healthz, get_operator_readiness, get_operator_report,
    get_operator_runtime, get_runtime_debug, persisted_stream_status, room_inspection,
    runtime_storage_for, server_readiness,
};
use crate::session_runtime::{
    current_session_snapshot, prune_expired_sessions, session_expires_at_ms,
};
#[cfg(test)]
pub(crate) use crate::session_runtime::{install_session, session_cookie_header, to_status};
use crate::state::{
    AppState, CachedCapabilityReport, PendingOAuthLogin, SessionSnapshot, StoredSession,
};
use crate::wiki_registry::WikiRegistry;
use crate::wikimedia_capabilities::{CapabilityProbeTargets, probe_with_targets};
#[cfg(test)]
pub(crate) use sp42_core::routes::{
    ACTION_HISTORY_PATH, ACTION_STATUS_PATH, OPERATOR_READINESS_PATH, OPERATOR_STORAGE_LAYOUT_PATH,
};
pub(crate) use sp42_core::routes::{
    AUTH_CALLBACK_PATH, AUTH_LOGIN_PATH, AUTH_LOGOUT_PATH, OPERATOR_REPORT_PATH,
};

const SESSION_COOKIE_NAME: &str = "sp42_dev_session";
const CSRF_HEADER_NAME: &str = "x-sp42-csrf-token";
const CAPABILITY_CACHE_TTL_MS: i64 = 30_000;
const SESSION_IDLE_TIMEOUT_MS: i64 = 30 * 60 * 1000;
const SESSION_ABSOLUTE_TIMEOUT_MS: i64 = 8 * 60 * 60 * 1000;
const SESSION_COOKIE_MAX_AGE_SECONDS: i64 = SESSION_IDLE_TIMEOUT_MS / 1000;
const PENDING_OAUTH_TTL_MS: i64 = 10 * 60 * 1000;
const ACTION_HISTORY_LIMIT: usize = 50;
const RESPONSE_BODY_PREVIEW_LIMIT: usize = 1_000;
const REVISION_ARTIFACT_CACHE_TTL_MS: i64 = 5 * 60 * 1000;
const RENDERED_HUNK_CACHE_TTL_MS: i64 = 5 * 60 * 1000;
const WIKIMEDIA_API_RETRY_ATTEMPTS: usize = 3;
const WIKIMEDIA_API_RETRY_DELAY_MS: u64 = 150;

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
    csrf_token: Option<String>,
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
    let deployment = DeploymentConfig::load().map_err(io::Error::other)?;
    let wiki_registry = WikiRegistry::load().map_err(io::Error::other)?;
    info!(
        deployment_mode = deployment.mode.as_str(),
        public_base_url = deployment.public_base_url.as_deref().unwrap_or(""),
        allowed_origin_count = deployment.allowed_origins.len(),
        "loaded runtime deployment configuration"
    );
    info!(
        default_wiki_id = wiki_registry.default_wiki_id(),
        wiki_count = wiki_registry.wiki_count(),
        source = wiki_registry.source(),
        "loaded wiki registry"
    );
    let state = AppState {
        capability_cache: Arc::new(RwLock::new(None)),
        sessions: Arc::new(RwLock::new(HashMap::new())),
        pending_oauth_logins: Arc::new(RwLock::new(HashMap::new())),
        revision_artifacts: Arc::new(RwLock::new(HashMap::new())),
        rendered_hunks: Arc::new(RwLock::new(HashMap::new())),
        http_client: build_http_client()?,
        local_oauth: LocalOAuthConfig::load(),
        runtime_storage_root: runtime_storage_root(),
        ingestion_supervisor: Arc::new(RwLock::new(HashMap::new())),
        capability_targets: CapabilityProbeTargets::default(),
        clock: clock.clone(),
        coordination: CoordinationRegistry::new(clock),
        deployment,
        wiki_registry,
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

fn spawn_ingestion_supervisors(state: &AppState) {
    ingestion_supervisor::spawn_ingestion_supervisors(state);
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

pub(crate) fn validate_csrf_header(
    headers: &HeaderMap,
    session: &SessionSnapshot,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let Some(header_value) = headers
        .get(CSRF_HEADER_NAME)
        .and_then(|value| value.to_str().ok())
    else {
        return Err(forbidden_error("Missing CSRF token header."));
    };

    if header_value == session.csrf_token {
        Ok(())
    } else {
        Err(forbidden_error("Invalid CSRF token header."))
    }
}

pub(crate) async fn require_session_csrf(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let Some(session) = current_session_snapshot(state, headers, false).await else {
        return Err(unauthorized_error(
            "No authenticated bridge session is active.",
        ));
    };
    validate_csrf_header(headers, &session)
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

#[derive(Debug, serde::Deserialize)]
struct ArticleInventoryQuery {
    title: String,
}

enum CapabilityProbeSubject<'a> {
    LocalToken,
    Session(&'a SessionSnapshot),
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
    require_session_csrf(&state, &headers).await?;
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
    require_session_csrf(&state, &headers).await?;
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
    require_session_csrf(&state, &headers).await?;
    storage_routes::put_public_storage_document(
        Path((wiki_id, kind)),
        Query(query),
        State(state),
        headers,
        Json(payload),
    )
    .await
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

async fn access_token_for_request(state: &AppState, headers: &HeaderMap) -> Option<String> {
    current_session_snapshot(state, headers, true)
        .await
        .map(|session| session.access_token)
        .or_else(|| state.local_oauth.access_token().map(ToString::to_string))
}

async fn get_article_inventory(
    Path(wiki_id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ArticleInventoryQuery>,
) -> Result<Json<ArticleInventory>, (StatusCode, Json<serde_json::Value>)> {
    let title = query.title.trim();
    if title.is_empty() {
        return Err(invalid_payload("title is required"));
    }

    let context = authenticated_wiki_context(&state, &headers, &wiki_id).await?;
    let wikitext = fetch_page_wikitext(&context.client, &context.config, title)
        .await
        .map_err(|error| gateway_error(format!("article fetch failed: {error}")))?;

    Ok(Json(build_article_inventory(&wiki_id, title, &wikitext)))
}

pub(crate) async fn fetch_page_wikitext(
    client: &BearerHttpClient,
    config: &WikiConfig,
    title: &str,
) -> Result<String, sp42_core::ActionError> {
    use sp42_core::{ActionError, HttpMethod, HttpRequest};

    let mut url = config.api_url.clone();
    url.query_pairs_mut()
        .append_pair("action", "query")
        .append_pair("prop", "revisions")
        .append_pair("titles", title)
        .append_pair("rvprop", "content")
        .append_pair("rvslots", "main")
        .append_pair("rvlimit", "1")
        .append_pair("format", "json")
        .append_pair("formatversion", "2");

    let response = client
        .execute(HttpRequest {
            method: HttpMethod::Get,
            url,
            headers: std::collections::BTreeMap::new(),
            body: Vec::new(),
        })
        .await
        .map_err(|e| ActionError::Execution {
            message: format!("page fetch failed: {e}"),
            code: None,
            http_status: None,
            retryable: true,
        })?;

    if !(200..300).contains(&response.status) {
        return Err(ActionError::Execution {
            message: format!("page fetch HTTP {}", response.status),
            code: None,
            http_status: Some(response.status),
            retryable: response.status >= 500,
        });
    }

    let v: serde_json::Value =
        serde_json::from_slice(&response.body).map_err(|e| ActionError::Execution {
            message: format!("page JSON failed: {e}"),
            code: None,
            http_status: None,
            retryable: false,
        })?;

    // Navigate: query.pages[0].revisions[0].slots.main.content
    let content = v
        .pointer("/query/pages/0/revisions/0/slots/main/content")
        .and_then(serde_json::Value::as_str);

    content
        .map(ToString::to_string)
        .ok_or_else(|| ActionError::Execution {
            message: format!("page content not found for: {title}"),
            code: Some("missing-content".to_string()),
            http_status: None,
            retryable: false,
        })
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
    let config = match resolved_wiki_config(state, wiki_id) {
        Ok(config) => config,
        Err(error) => {
            return DevAuthCapabilityReport {
                checked: true,
                wiki_id: wiki_id.to_string(),
                error: Some(error),
                ..DevAuthCapabilityReport::default()
            };
        }
    };
    let oauth = state.local_oauth.status();
    let report = probe_with_targets(
        &state.http_client,
        capability_probe_token(state, &subject),
        &oauth,
        &config,
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
    action_routes::get_action_status(State(state), headers).await
}

async fn get_action_history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ActionHistoryQuery>,
) -> Json<ActionExecutionHistoryReport> {
    action_routes::get_action_history(State(state), headers, Query(query)).await
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

async fn post_execute_action(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<SessionActionExecutionRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    action_routes::post_execute_action(State(state), headers, Json(payload)).await
}

fn action_feedback_for_entry(entry: &ActionExecutionLogEntry) -> String {
    action_routes::action_feedback_for_entry(entry)
}

async fn action_status_report(
    state: &AppState,
    headers: &HeaderMap,
) -> ActionExecutionStatusReport {
    action_routes::action_status_report(state, headers).await
}

async fn action_history_report(
    state: &AppState,
    headers: &HeaderMap,
    limit: Option<usize>,
) -> ActionExecutionHistoryReport {
    action_routes::action_history_report(state, headers, limit).await
}

fn invalid_payload(message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "error": message })),
    )
}

fn split_scope_string(value: &str) -> Vec<String> {
    value
        .split_whitespace()
        .filter(|scope| !scope.is_empty())
        .map(ToString::to_string)
        .collect()
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

#[cfg(test)]
fn now_ms() -> i64 {
    SystemClock.now_ms()
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
    let mut config = state.wiki_registry.config(wiki_id)?;
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

fn truncate_response_body(body: &[u8]) -> String {
    action_routes::truncate_response_body(body)
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

#[cfg(test)]
mod tests;
