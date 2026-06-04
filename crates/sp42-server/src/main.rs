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
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, PRAGMA};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::middleware::Next;
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
    LiveOperatorPhaseTiming, LiveOperatorPublicDocuments, LiveOperatorTelemetry, LiveOperatorView,
    OAuthCallback, OAuthClientConfig, OAuthTokenResponse, PublicAuditLedgerEntry,
    PublicStorageDocumentData, SessionActionExecutionRequest, SessionActionExecutionResponse,
    SessionActionKind, SystemClock, TokenKind, WikiConfig, WikiStorageConfig, WikiStorageDocument,
    WikiStorageDocumentKind, WikiStorageLoadedDocument, WikiStoragePlan, WikiStoragePlanInput,
    WikiStorageWriteOutcome, WikiStorageWriteRequest, build_article_inventory,
    build_authorization_url, build_media_diff, build_wiki_storage_plan,
    default_public_storage_document, diff_lines, execute_fetch_token, generate_oauth_state,
    generate_pkce_verifier, load_wiki_storage_document, parse_callback_query,
    render_wiki_storage_document_page, render_wiki_storage_index_page,
    resolve_wiki_storage_document, save_wiki_storage_document,
};

#[cfg(test)]
use crate::coordination::CoordinationRoomInspection;
use crate::coordination::{CoordinationEnvelope, CoordinationRegistry};
use crate::deployment::DeploymentConfig;
#[cfg(test)]
use crate::endpoint_manifest::operator_endpoint_manifest;
pub(crate) use crate::live_queue::{
    IngestionSupervisorSnapshot, LiveOperatorAssembly, LiveOperatorFinalization,
    LiveOperatorProductContext, LiveOperatorPublicContextState, LivePublicDocumentLoadSpec,
    LiveViewFilterParams, build_live_operator_notes, build_live_operator_products, default_limit,
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
use crate::state::{
    AppState, CachedCapabilityReport, PendingOAuthLogin, SessionSnapshot, StoredSession,
};
use crate::wiki_registry::WikiRegistry;
use crate::wikimedia_capabilities::{CapabilityProbeTargets, probe_with_targets};

const SESSION_COOKIE_NAME: &str = "sp42_dev_session";
const CSRF_HEADER_NAME: &str = "x-sp42-csrf-token";
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
                .join("..")
                .join("target")
                .join("dist")
                .join("sp42-app")
        },
        PathBuf::from,
    )
}

fn app_static_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("sp42-app")
        .join("static")
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
      <p>Or run live development from the repository root with <code>trunk serve</code>.</p>
    </main>
  </body>
</html>"#,
    )
}

fn static_asset_path(file_name: &str) -> PathBuf {
    let dist_candidate = browser_app_dist_dir().join(file_name);
    if dist_candidate.is_file() {
        dist_candidate
    } else {
        app_static_dir().join(file_name)
    }
}

async fn get_manifest_json() -> impl IntoResponse {
    serve_static_file(
        static_asset_path("manifest.json"),
        "application/manifest+json",
    )
    .await
}

async fn get_runtime_config_js(State(state): State<AppState>) -> impl IntoResponse {
    let payload = serde_json::json!({
        "defaultWikiId": state.default_wiki_id(),
        "deploymentMode": state.deployment.mode.as_str(),
    });
    let serialized = serde_json::to_string(&payload).expect("runtime config should serialize");
    (
        [(
            CONTENT_TYPE,
            HeaderValue::from_static("application/javascript"),
        )],
        format!(
            "window.__SP42_RUNTIME_CONFIG__ = {{ ...(window.__SP42_RUNTIME_CONFIG__ || {{}}), ...{serialized} }};\n"
        ),
    )
}

async fn get_service_worker() -> impl IntoResponse {
    serve_static_file(static_asset_path("sw.js"), "application/javascript").await
}

async fn get_offline_html() -> impl IntoResponse {
    serve_static_file(
        static_asset_path("offline.html"),
        "text/html; charset=utf-8",
    )
    .await
}

async fn get_static_icon(Path(icon_name): Path<String>) -> impl IntoResponse {
    let candidate = app_static_dir().join("icons").join(&icon_name);
    if candidate.is_file() {
        serve_static_file(candidate, "image/svg+xml").await
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

async fn get_favicon() -> impl IntoResponse {
    Redirect::temporary("/icons/sp42-icon-192.svg")
}

async fn serve_static_file(path: PathBuf, content_type: &'static str) -> Response {
    match tokio::fs::read(path).await {
        Ok(bytes) => (
            StatusCode::OK,
            [(CONTENT_TYPE, HeaderValue::from_static(content_type))],
            bytes,
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
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

fn effective_session_scopes(report: &DevAuthCapabilityReport) -> Vec<String> {
    session_runtime::effective_session_scopes(report)
}

fn split_scope_string(value: &str) -> Vec<String> {
    value
        .split_whitespace()
        .filter(|scope| !scope.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn auth_session_view_without_session(state: &AppState) -> OAuthSessionView {
    session_runtime::auth_session_view_without_session(state)
}

async fn auth_session_view(state: &AppState, headers: &HeaderMap, touch: bool) -> OAuthSessionView {
    session_runtime::auth_session_view(state, headers, touch).await
}

async fn store_pending_oauth_login(state: &AppState, pending: PendingOAuthLogin) {
    session_runtime::store_pending_oauth_login(state, pending).await;
}

async fn take_pending_oauth_login(
    state: &AppState,
    state_token: &str,
) -> Option<PendingOAuthLogin> {
    session_runtime::take_pending_oauth_login(state, state_token).await
}

async fn install_session(
    state: &AppState,
    prior_session_id: Option<String>,
    stored: StoredSession,
    current_ms: i64,
) -> String {
    session_runtime::install_session(state, prior_session_id, stored, current_ms).await
}

fn to_status(
    session: Option<&StoredSession>,
    local_oauth: &LocalOAuthConfig,
    now_ms: i64,
) -> DevAuthSessionStatus {
    session_runtime::to_status(session, local_oauth, now_ms)
}

fn bootstrap_status(state: &AppState, auth: &DevAuthSessionStatus) -> DevAuthBootstrapStatus {
    session_runtime::bootstrap_status(state, auth)
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

fn session_cookie_value(headers: &HeaderMap) -> Option<String> {
    session_runtime::session_cookie_value(headers)
}

fn next_session_id(state: &AppState, current_ms: i64) -> String {
    session_runtime::next_session_id(state, current_ms)
}

fn session_cookie_header(state: &AppState, session_id: &str) -> Option<HeaderValue> {
    session_runtime::session_cookie_header(state, session_id)
}

fn expired_session_cookie_header(state: &AppState) -> HeaderValue {
    session_runtime::expired_session_cookie_header(state)
}

fn session_expires_at_ms(session: &StoredSession, current_time_ms: i64) -> i64 {
    session_runtime::session_expires_at_ms(session, current_time_ms)
}

fn prune_expired_sessions(sessions: &mut HashMap<String, StoredSession>, current_time_ms: i64) {
    session_runtime::prune_expired_sessions(sessions, current_time_ms);
}

async fn current_session_snapshot(
    state: &AppState,
    headers: &HeaderMap,
    touch: bool,
) -> Option<SessionSnapshot> {
    session_runtime::current_session_snapshot(state, headers, touch).await
}

async fn current_status(
    state: &AppState,
    headers: &HeaderMap,
    touch: bool,
) -> DevAuthSessionStatus {
    session_runtime::current_status(state, headers, touch).await
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
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;
    use std::time::Instant;

    use axum::body::{Body, to_bytes};
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
        operator_endpoint_manifest, to_status,
    };
    use crate::coordination::CoordinationRegistry;
    use crate::deployment::{DeploymentConfig, DeploymentMode};
    use crate::local_env::LocalOAuthConfig;
    use crate::wiki_registry::WikiRegistry;
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

    static TEST_TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);

    fn unique_test_temp_path(prefix: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "{prefix}-{}-{}",
            std::process::id(),
            TEST_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn test_deployment_for_mode(mode: DeploymentMode) -> DeploymentConfig {
        DeploymentConfig {
            mode,
            public_base_url: None,
            allowed_origins: Vec::new(),
        }
    }

    fn test_deployment() -> DeploymentConfig {
        test_deployment_for_mode(DeploymentMode::Local)
    }

    fn test_wiki_registry() -> WikiRegistry {
        WikiRegistry::embedded_default().expect("embedded wiki registry should load")
    }

    fn test_state() -> AppState {
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        AppState {
            capability_cache: Arc::new(tokio::sync::RwLock::new(None)),
            sessions: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            pending_oauth_logins: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            revision_artifacts: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            rendered_hunks: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            http_client: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .user_agent(sp42_core::branding::USER_AGENT)
                .build()
                .expect("reqwest client should build"),
            local_oauth: LocalOAuthConfig::default(),
            runtime_storage_root: unique_test_temp_path("sp42-server-runtime"),
            ingestion_supervisor: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            capability_targets: CapabilityProbeTargets::default(),
            clock: clock.clone(),
            coordination: CoordinationRegistry::new(clock),
            deployment: test_deployment(),
            wiki_registry: test_wiki_registry(),
            next_client_id: Arc::new(AtomicU64::new(1)),
            next_session_id: Arc::new(AtomicU64::new(1)),
            started_at: Instant::now(),
        }
    }

    fn temp_local_env_file(contents: &str) -> std::path::PathBuf {
        let temp_dir = unique_test_temp_path("sp42-server-test");
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
            csrf_token: "csrf-token".to_string(),
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
                csrf_token: "csrf-token".to_string(),
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
    async fn dev_session_delete_requires_csrf_for_cookie_session() {
        let state = test_state();
        let session_id = "session-delete";
        let created_at_ms = now_ms();
        state.sessions.write().await.insert(
            session_id.to_string(),
            test_session("Example", "secret-token", created_at_ms),
        );
        let router = build_router(state.clone());
        let cookie = format!("{}={session_id}", crate::SESSION_COOKIE_NAME);

        let missing_csrf = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri("/dev/auth/session")
                    .header(axum::http::header::COOKIE, &cookie)
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("delete request should succeed");

        assert_eq!(missing_csrf.status(), StatusCode::FORBIDDEN);
        assert!(state.sessions.read().await.contains_key(session_id));

        let valid_csrf = router
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri("/dev/auth/session")
                    .header(axum::http::header::COOKIE, cookie)
                    .header(crate::CSRF_HEADER_NAME, "csrf-token")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("delete request should succeed");

        assert_eq!(valid_csrf.status(), StatusCode::OK);
        assert!(!state.sessions.read().await.contains_key(session_id));
    }

    #[tokio::test]
    async fn bootstrap_session_is_disabled_outside_local_mode() {
        let mut state = test_state();
        state.deployment = test_deployment_for_mode(DeploymentMode::Vps);
        let router = build_router(state);
        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/dev/auth/session/bootstrap")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .expect("request should build"),
            )
            .await
            .expect("bootstrap request should succeed");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        let payload: serde_json::Value =
            serde_json::from_slice(&body).expect("response should parse");
        assert!(
            payload
                .get("error")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|message| message.contains("SP42_DEPLOYMENT_MODE=local"))
        );
    }

    #[test]
    fn vps_session_cookie_is_secure() {
        let mut state = test_state();
        state.deployment = test_deployment_for_mode(DeploymentMode::Vps);
        let cookie = super::session_cookie_header(&state, "session-cookie")
            .expect("session cookie header should build")
            .to_str()
            .expect("session cookie header should be text")
            .to_string();

        assert!(cookie.contains("; Secure"));
        assert!(cookie.contains("SameSite=Lax"));
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
            revision_artifacts: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            rendered_hunks: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            http_client: reqwest::Client::builder()
                .user_agent(sp42_core::branding::USER_AGENT)
                .build()
                .expect("reqwest client should build"),
            local_oauth: LocalOAuthConfig::load_from_candidates([local_env_path.clone()]),
            runtime_storage_root: unique_test_temp_path("sp42-server-runtime-healthz"),
            ingestion_supervisor: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            capability_targets: CapabilityProbeTargets {
                profile_url: format!("{profile_base}/oauth2/resource/profile"),
                api_url: Some(format!("{profile_base}/w/api.php")),
            },
            clock: clock.clone(),
            coordination: CoordinationRegistry::new(clock),
            deployment: test_deployment(),
            wiki_registry: test_wiki_registry(),
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

    #[tokio::test]
    async fn runtime_config_js_exposes_default_wiki() {
        let router = build_router(test_state());
        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/runtime-config.js")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("runtime config request should succeed");

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        let script = String::from_utf8(body.to_vec()).expect("runtime config should be utf8");

        assert!(script.contains("window.__SP42_RUNTIME_CONFIG__"));
        assert!(script.contains("\"defaultWikiId\":\"frwiki\""));
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
                csrf_token: None,
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
                    csrf_token: None,
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
        assert_eq!(
            report.endpoints.len(),
            operator_endpoint_manifest(test_state().default_wiki_id()).len()
        );
        assert_eq!(report.readiness.operator_report_path, OPERATOR_REPORT_PATH);
        assert_eq!(report.runtime.operator_report_path, OPERATOR_REPORT_PATH);
        assert_eq!(
            report.bootstrap.source_report.file_name,
            ".env.wikimedia.local"
        );
    }

    #[test]
    fn operator_endpoint_manifest_contains_core_endpoints() {
        let endpoints = operator_endpoint_manifest(test_state().default_wiki_id());
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
        headers.insert(
            axum::http::header::HOST,
            axum::http::HeaderValue::from_static("127.0.0.1:8788"),
        );

        let base = crate::oauth_runtime::public_base_url(&headers)
            .expect("loopback host should be accepted");

        assert_eq!(base, "http://127.0.0.1:8788");
    }

    #[test]
    fn public_base_url_rejects_non_local_host_without_override() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::HOST,
            axum::http::HeaderValue::from_static("example.org"),
        );

        let error = crate::oauth_runtime::public_base_url(&headers)
            .expect_err("non-local host should be rejected");

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
                csrf_token: "csrf-token".to_string(),
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
                csrf_token: "csrf-token".to_string(),
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
            revision_artifacts: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            rendered_hunks: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            http_client: reqwest::Client::builder()
                .user_agent(sp42_core::branding::USER_AGENT)
                .build()
                .expect("reqwest client should build"),
            local_oauth: LocalOAuthConfig::load_from_candidates([local_env_path.clone()]),
            runtime_storage_root: unique_test_temp_path("sp42-server-runtime-capability"),
            ingestion_supervisor: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            capability_targets: CapabilityProbeTargets {
                profile_url: format!("{profile_base}/oauth2/resource/profile"),
                api_url: Some(format!("{profile_base}/w/api.php")),
            },
            clock: clock.clone(),
            coordination: CoordinationRegistry::new(clock),
            deployment: test_deployment(),
            wiki_registry: test_wiki_registry(),
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
        let runtime_root = unique_test_temp_path("sp42-live-operator-runtime");
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        let state = AppState {
            capability_cache: Arc::new(tokio::sync::RwLock::new(None)),
            sessions: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            pending_oauth_logins: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            revision_artifacts: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            rendered_hunks: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
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
            deployment: test_deployment(),
            wiki_registry: test_wiki_registry(),
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
        let runtime_root = unique_test_temp_path("sp42-live-operator-runtime-persist");
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        let state = AppState {
            capability_cache: Arc::new(tokio::sync::RwLock::new(None)),
            sessions: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            pending_oauth_logins: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            revision_artifacts: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            rendered_hunks: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
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
            deployment: test_deployment(),
            wiki_registry: test_wiki_registry(),
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
            revision_artifacts: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            rendered_hunks: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            http_client: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .user_agent(sp42_core::branding::USER_AGENT)
                .build()
                .expect("reqwest client should build"),
            local_oauth: LocalOAuthConfig::default(),
            runtime_storage_root: unique_test_temp_path(
                "sp42-server-runtime-logical-storage-route",
            ),
            ingestion_supervisor: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            capability_targets: CapabilityProbeTargets {
                api_url: Some(format!("{profile_base}/w/api.php")),
                profile_url: format!("{profile_base}/oauth2/resource/profile"),
            },
            clock: clock.clone(),
            coordination: CoordinationRegistry::new(clock),
            deployment: test_deployment(),
            wiki_registry: test_wiki_registry(),
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
                csrf_token: "csrf-token".to_string(),
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
            revision_artifacts: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            rendered_hunks: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            http_client: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .user_agent(sp42_core::branding::USER_AGENT)
                .build()
                .expect("reqwest client should build"),
            local_oauth: LocalOAuthConfig::default(),
            runtime_storage_root: unique_test_temp_path("sp42-server-runtime-public-storage-route"),
            ingestion_supervisor: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            capability_targets: CapabilityProbeTargets {
                api_url: Some(format!("{profile_base}/w/api.php")),
                profile_url: format!("{profile_base}/oauth2/resource/profile"),
            },
            clock: clock.clone(),
            coordination: CoordinationRegistry::new(clock),
            deployment: test_deployment(),
            wiki_registry: test_wiki_registry(),
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
                csrf_token: "csrf-token".to_string(),
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
            revision_artifacts: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            rendered_hunks: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            http_client: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .user_agent(sp42_core::branding::USER_AGENT)
                .build()
                .expect("reqwest client should build"),
            local_oauth: LocalOAuthConfig::load_from_candidates([local_env_path]),
            runtime_storage_root: unique_test_temp_path("sp42-server-runtime-bootstrap"),
            ingestion_supervisor: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            capability_targets: CapabilityProbeTargets {
                profile_url: format!("{profile_base}/oauth2/resource/profile"),
                api_url: Some(format!("{profile_base}/w/api.php")),
            },
            clock: clock.clone(),
            coordination: CoordinationRegistry::new(clock),
            deployment: test_deployment(),
            wiki_registry: test_wiki_registry(),
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
