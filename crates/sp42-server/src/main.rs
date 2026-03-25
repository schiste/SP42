mod coordination;
mod local_env;
mod wikimedia_capabilities;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use axum::extract::OriginalUri;
use axum::extract::Path;
use axum::extract::Query;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, COOKIE, HOST, PRAGMA, SET_COOKIE};
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
    ActionExecutionStatusReport, BacklogRuntime, BacklogRuntimeConfig, ContextInputs,
    CoordinationRoomSummary, CoordinationSnapshot, CoordinationState, DebugSnapshotInputs,
    DevAuthBootstrapRequest, DevAuthCapabilityReport, DevAuthSessionStatus, FileStorage,
    LiftWingRequest, LiveOperatorBackendStatus, LiveOperatorPhaseTiming, LiveOperatorQuery,
    LiveOperatorTelemetry, LiveOperatorView, LocalOAuthConfigStatus, LocalOAuthSourceReport,
    OAuthCallback, OAuthClientConfig, OAuthTokenResponse, PatrolScenarioReportInputs,
    PatrolSessionDigestInputs, QueuedEdit, RecentChangesQuery, ServerDebugSummary,
    SessionActionExecutionRequest, SessionActionExecutionResponse, SessionActionKind,
    ShellStateInputs, Storage, StreamRuntimeStatus, TokenKind, UndoRequest, WikiConfig,
    WikiStorageConfig, WikiStoragePlan, WikiStoragePlanInput, build_authorization_url,
    build_debug_snapshot, build_patrol_scenario_report, build_patrol_session_digest,
    build_ranked_queue, build_review_workbench, build_scoring_context, build_shell_state_model,
    build_wiki_storage_plan, diff_lines, execute_fetch_token, execute_liftwing_score,
    execute_patrol, execute_recent_changes, execute_rollback, execute_undo, generate_oauth_state,
    generate_pkce_verifier, parse_callback_query, render_wiki_storage_document_page,
    render_wiki_storage_index_page,
};

use crate::coordination::{
    CoordinationEnvelope, CoordinationRegistry, CoordinationRoomInspection, CoordinationRoomMetrics,
};
use crate::local_env::LocalOAuthConfig;
use crate::wikimedia_capabilities::{CapabilityProbeTargets, config_for_wiki, probe_with_targets};

type SharedSessions = Arc<RwLock<HashMap<String, StoredSession>>>;
type SharedCapabilityCache = Arc<RwLock<Option<CachedCapabilityReport>>>;
type SharedPendingOAuthLogins = Arc<RwLock<HashMap<String, PendingOAuthLogin>>>;

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

#[derive(Debug, Clone)]
struct AppState {
    capability_cache: SharedCapabilityCache,
    sessions: SharedSessions,
    pending_oauth_logins: SharedPendingOAuthLogins,
    http_client: reqwest::Client,
    local_oauth: LocalOAuthConfig,
    runtime_storage_root: PathBuf,
    capability_targets: CapabilityProbeTargets,
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

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
struct ActionHistoryQuery {
    limit: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
struct AuthLoginQuery {
    next: Option<String>,
    wiki_id: Option<String>,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct OAuthSessionView {
    authenticated: bool,
    username: Option<String>,
    scopes: Vec<String>,
    expires_at_ms: Option<i64>,
    upstream_access_expires_at_ms: Option<i64>,
    refresh_available: bool,
    bridge_mode: String,
    local_token_available: bool,
    oauth_client_ready: bool,
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
    let router = build_router(AppState {
        capability_cache: Arc::new(RwLock::new(None)),
        sessions: Arc::new(RwLock::new(HashMap::new())),
        pending_oauth_logins: Arc::new(RwLock::new(HashMap::new())),
        http_client: reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .user_agent(sp42_core::branding::USER_AGENT)
            .build()
            .expect("reqwest client should build"),
        local_oauth: LocalOAuthConfig::load(),
        runtime_storage_root: runtime_storage_root(),
        capability_targets: CapabilityProbeTargets::default(),
        coordination: CoordinationRegistry::default(),
        next_client_id: Arc::new(AtomicU64::new(1)),
        next_session_id: Arc::new(AtomicU64::new(1)),
        started_at: Instant::now(),
    });

    axum::serve(listener, router).await
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
    include_bots: bool,
    #[serde(default)]
    unpatrolled_only: bool,
    #[serde(default = "default_true")]
    include_minor: bool,
    #[serde(default)]
    namespaces: Option<String>,
    #[serde(default)]
    min_score: Option<i32>,
    #[serde(default)]
    rccontinue: Option<String>,
}

const fn default_limit() -> u16 {
    15
}

const fn default_true() -> bool {
    true
}

async fn get_live_operator_view(
    Path(wiki_id): Path<String>,
    axum::extract::Query(filters): axum::extract::Query<LiveViewFilterParams>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    match live_operator_view(&state, &headers, &wiki_id, &filters).await {
        Ok(view) => Ok::<_, (StatusCode, Json<serde_json::Value>)>(Json(view)),
        Err(error) => Err((
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": error })),
        )),
    }
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
            .push("Local token bootstrap is unavailable because WIKIMEDIA_ACCESS_TOKEN is not set in .env.wikimedia.local.".to_string());
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
    let session = current_session_snapshot(state, headers, false).await;
    let username = query
        .username
        .clone()
        .or_else(|| session.as_ref().map(|session| session.username.clone()))
        .ok_or_else(|| "username is required via query or authenticated session".to_string())?;
    let shared_owner_username = query
        .shared_owner_username
        .clone()
        .unwrap_or_else(|| username.clone());
    let input = WikiStoragePlanInput {
        username: username.clone(),
        home_wiki_id: query
            .home_wiki_id
            .clone()
            .unwrap_or_else(|| wiki_id.to_string()),
        target_wiki_id: wiki_id.to_string(),
        shared_owner_username,
        team_slugs: query.team_slug.clone().into_iter().collect(),
        rule_set_slugs: query.rule_set_slug.clone().into_iter().collect(),
        training_dataset_slugs: query.training_dataset_slug.clone().into_iter().collect(),
        audit_period_slugs: query.audit_period_slug.clone().into_iter().collect(),
    };
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
                    "subject": username,
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

#[allow(clippy::too_many_lines)]
async fn live_operator_view(
    state: &AppState,
    headers: &HeaderMap,
    wiki_id: &str,
    filters: &LiveViewFilterParams,
) -> Result<LiveOperatorView, String> {
    let total_started = Instant::now();
    let mut phase_timings = Vec::new();

    let phase_started = Instant::now();
    let config = resolved_wiki_config(state, wiki_id)?;
    let auth = current_status(state, headers, true).await;
    let action_status = action_status_report(state, headers).await;
    let action_history = action_history_report(state, headers, Some(10)).await;
    let capabilities = capability_report_for_request(state, headers, wiki_id, false).await;
    let stream_status = persisted_stream_status(state, wiki_id).await?;
    phase_timings.push(LiveOperatorPhaseTiming {
        phase: "bootstrap".to_string(),
        duration_ms: u64::try_from(phase_started.elapsed().as_millis()).unwrap_or(u64::MAX),
    });

    let phase_started = Instant::now();
    let access_token = access_token_for_request(state, headers)
        .await
        .ok_or_else(|| "No local Wikimedia access token is available.".to_string())?;
    let client = BearerHttpClient::new(state.http_client.clone(), access_token.clone());

    let namespace_override = filters.namespaces.as_ref().map(|ns_str| {
        ns_str
            .split(',')
            .filter_map(|s| s.trim().parse::<i32>().ok())
            .collect::<Vec<_>>()
    });

    let live_query = LiveOperatorQuery {
        limit: filters.limit.clamp(1, 500),
        include_bots: filters.include_bots,
        unpatrolled_only: filters.unpatrolled_only,
        include_minor: filters.include_minor,
        namespaces: filters.namespaces.as_ref().map_or_else(Vec::new, |ns_str| {
            ns_str
                .split(',')
                .filter_map(|s| s.trim().parse::<i32>().ok())
                .collect::<Vec<_>>()
        }),
        min_score: filters.min_score,
        rccontinue: filters.rccontinue.clone(),
    };

    let mut backlog_runtime = BacklogRuntime::new(
        config.clone(),
        runtime_storage_for(state),
        BacklogRuntimeConfig {
            limit: live_query.limit,
            include_bots: live_query.include_bots,
        },
        format!("recentchanges.rccontinue.{wiki_id}"),
    );
    backlog_runtime
        .initialize()
        .await
        .map_err(|error| format!("backlog runtime init failed: {error}"))?;
    let batch = if live_query.rccontinue.is_some()
        || live_query.unpatrolled_only
        || !live_query.include_minor
        || namespace_override.is_some()
    {
        let query = RecentChangesQuery {
            limit: live_query.limit,
            rccontinue: live_query.rccontinue.clone(),
            include_bots: live_query.include_bots,
            unpatrolled_only: live_query.unpatrolled_only,
            include_minor: live_query.include_minor,
            namespace_override,
        };
        let batch = execute_recent_changes(&client, &config, &query)
            .await
            .map_err(|error| format!("recentchanges fetch failed: {error}"))?;
        backlog_runtime
            .apply_batch(&batch)
            .await
            .map_err(|error| format!("backlog runtime apply failed: {error}"))?;
        batch
    } else {
        backlog_runtime
            .poll(&client)
            .await
            .map_err(|error| format!("recentchanges fetch failed: {error}"))?
    };
    phase_timings.push(LiveOperatorPhaseTiming {
        phase: "recentchanges".to_string(),
        duration_ms: u64::try_from(phase_started.elapsed().as_millis()).unwrap_or(u64::MAX),
    });

    let phase_started = Instant::now();
    let backlog_status = backlog_runtime.status();
    let mut queue = build_ranked_queue(batch.events, &config.scoring)
        .map_err(|error| format!("queue build failed: {error}"))?;

    // Apply min_score client-side filter (score is computed after API fetch)
    if let Some(min_score) = filters.min_score {
        queue.retain(|item| item.score.total >= min_score);
    }
    let selected_index = (!queue.is_empty()).then_some(0usize);
    let selected = selected_index.and_then(|index| queue.get(index));
    phase_timings.push(LiveOperatorPhaseTiming {
        phase: "queue".to_string(),
        duration_ms: u64::try_from(phase_started.elapsed().as_millis()).unwrap_or(u64::MAX),
    });

    let phase_started = Instant::now();
    let liftwing_risk = if let Some(item) = selected {
        execute_liftwing_score(
            &client,
            &config,
            &LiftWingRequest {
                rev_id: item.event.rev_id,
            },
        )
        .await
        .ok()
    } else {
        None
    };
    phase_timings.push(LiveOperatorPhaseTiming {
        phase: "liftwing".to_string(),
        duration_ms: u64::try_from(phase_started.elapsed().as_millis()).unwrap_or(u64::MAX),
    });

    let scoring_context = liftwing_risk.map(|probability| {
        build_scoring_context(&ContextInputs {
            talk_page_wikitext: None,
            liftwing_probability: Some(probability),
        })
    });
    let phase_started = Instant::now();
    let diff = if let Some(item) = selected {
        fetch_revision_diff(&state.http_client, &access_token, &config, item).await?
    } else {
        None
    };
    phase_timings.push(LiveOperatorPhaseTiming {
        phase: "diff".to_string(),
        duration_ms: u64::try_from(phase_started.elapsed().as_millis()).unwrap_or(u64::MAX),
    });

    let phase_started = Instant::now();
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
            &config,
            item,
            "SP42_REDACTED_TOKEN",
            auth.username.as_deref().unwrap_or("SP42"),
            Some("Generated from live operator state"),
        )
        .ok()
    });
    phase_timings.push(LiveOperatorPhaseTiming {
        phase: "reports".to_string(),
        duration_ms: u64::try_from(phase_started.elapsed().as_millis()).unwrap_or(u64::MAX),
    });
    let scenario_report = build_patrol_scenario_report(&PatrolScenarioReportInputs {
        queue: &queue,
        selected,
        scoring_context: scoring_context.as_ref(),
        diff: diff.as_ref(),
        review_workbench: review_workbench.as_ref(),
        stream_status: Some(&stream_status),
        backlog_status: Some(&backlog_status),
        coordination: coordination_state.as_ref(),
        wiki_id_hint: Some(wiki_id),
    });
    let session_digest = build_patrol_session_digest(&PatrolSessionDigestInputs {
        report: &scenario_report,
        review_workbench: review_workbench.as_ref(),
    });
    let shell_state = build_shell_state_model(&ShellStateInputs {
        report: &scenario_report,
        review_workbench: review_workbench.as_ref(),
    });
    let backend = live_operator_backend_status(&readiness, &auth);
    let debug_snapshot = build_debug_snapshot(&DebugSnapshotInputs {
        queue: &queue,
        selected,
        scoring_context: scoring_context.as_ref(),
        diff: diff.as_ref(),
        review_workbench: review_workbench.as_ref(),
        stream_status: Some(&stream_status),
        backlog_status: Some(&backlog_status),
        coordination: coordination_state.as_ref(),
    });
    let telemetry = LiveOperatorTelemetry {
        total_duration_ms: u64::try_from(total_started.elapsed().as_millis()).unwrap_or(u64::MAX),
        phase_timings,
    };

    let mut notes =
        vec!["Queue and selected review are built from live recent changes.".to_string()];
    notes.push(format!(
        "Applied filters: limit={} include_bots={} unpatrolled_only={} include_minor={} namespaces={} min_score={} rccontinue={}",
        live_query.limit,
        live_query.include_bots,
        live_query.unpatrolled_only,
        live_query.include_minor,
        if live_query.namespaces.is_empty() {
            "default".to_string()
        } else {
            live_query
                .namespaces
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        },
        live_query
            .min_score
            .map_or_else(|| "none".to_string(), |value| value.to_string()),
        live_query.rccontinue.as_deref().unwrap_or("none"),
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

    Ok(LiveOperatorView {
        project: sp42_core::branding::PROJECT_NAME.to_string(),
        fetched_at_ms: now_ms(),
        wiki_id: wiki_id.to_string(),
        query: live_query,
        queue,
        selected_index,
        scoring_context,
        diff,
        review_workbench,
        stream_status: Some(stream_status),
        backlog_status: Some(backlog_status),
        scenario_report,
        session_digest,
        shell_state,
        capabilities,
        auth,
        backend,
        action_status,
        action_history,
        coordination_room,
        coordination_state,
        debug_snapshot,
        telemetry,
        notes,
        next_continue: batch.next_continue.clone(),
    })
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
    prune_expired_sessions(&mut sessions);
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

    let age_ms = now_ms().saturating_sub(cache.fetched_at_ms);
    let valid =
        cache.report.wiki_id == wiki_id && cache.report.checked && cache.report.error.is_none();
    CapabilityCacheStatus {
        present: valid,
        fresh: valid && cache_is_fresh(cache),
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
        return capability_report_for_session(state, &session, wiki_id, force_refresh).await;
    }

    capability_report_for_local_token(state, wiki_id, force_refresh).await
}

async fn capability_report_for_local_token(
    state: &AppState,
    wiki_id: &str,
    force_refresh: bool,
) -> DevAuthCapabilityReport {
    if !force_refresh {
        let guard = state.capability_cache.read().await;
        if let Some(cache) = guard.as_ref()
            && cache.report.wiki_id == wiki_id
            && cache_is_fresh(cache)
        {
            return cache.report.clone();
        }
    }

    let oauth = state.local_oauth.status();
    debug_assert!(!wiki_id.is_empty());
    let report = probe_with_targets(
        &state.http_client,
        state.local_oauth.access_token(),
        &oauth,
        wiki_id,
        &state.capability_targets,
    )
    .await;
    if let Some(error) = &report.error {
        warn!(wiki_id, error, "local capability probe failed");
    } else {
        info!(wiki_id, username = ?report.username, "local capability probe succeeded");
    }

    let mut guard = state.capability_cache.write().await;
    *guard = Some(CachedCapabilityReport {
        fetched_at_ms: now_ms(),
        report: report.clone(),
    });

    report
}

async fn capability_report_for_session(
    state: &AppState,
    session: &SessionSnapshot,
    wiki_id: &str,
    force_refresh: bool,
) -> DevAuthCapabilityReport {
    if !force_refresh
        && let Some(report) =
            cached_capabilities_for_session(state, &session.session_id, wiki_id).await
    {
        return report;
    }

    let oauth = state.local_oauth.status();
    let report = probe_with_targets(
        &state.http_client,
        Some(session.access_token.as_str()),
        &oauth,
        wiki_id,
        &state.capability_targets,
    )
    .await;
    if let Some(error) = &report.error {
        warn!(
            session_id = session.session_id.as_str(),
            wiki_id, error, "session capability probe failed"
        );
    } else {
        info!(
            session_id = session.session_id.as_str(),
            wiki_id,
            username = ?report.username,
            "session capability probe succeeded"
        );
    }

    store_capabilities_for_session(state, &session.session_id, &report).await;
    report
}

async fn get_runtime_debug(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Json<RuntimeDebugStatus> {
    Json(runtime_debug(&state, &headers).await)
}

async fn get_auth_login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AuthLoginQuery>,
) -> Result<Redirect, (StatusCode, Json<serde_json::Value>)> {
    if !state.local_oauth.has_confidential_oauth_client() {
        return Err((
            StatusCode::PRECONDITION_FAILED,
            Json(serde_json::json!({
                "error": "WIKIMEDIA_CLIENT_APPLICATION_KEY and WIKIMEDIA_CLIENT_APPLICATION_SECRET are required for OAuth login"
            })),
        ));
    }

    let wiki_id = query.wiki_id.as_deref().unwrap_or("frwiki");
    let oauth_config = oauth_client_config_for_request(&state, &headers, wiki_id)?;
    let redirect_after_login = sanitize_redirect_target(query.next.as_deref());
    let mut rng = ServerRng;
    let state_token = generate_oauth_state(&mut rng);
    let verifier = generate_pkce_verifier(&mut rng);
    let challenge = sp42_core::code_challenge_from_verifier(&verifier)
        .map_err(|error| invalid_payload(&error.to_string()))?;
    let authorization_url = build_authorization_url(&oauth_config, &state_token, &challenge)
        .map_err(|error| invalid_payload(&error.to_string()))?;
    let now = now_ms();
    let pending = PendingOAuthLogin {
        wiki_id: wiki_id.to_string(),
        state: state_token.clone(),
        verifier,
        redirect_uri: oauth_config.redirect_uri.to_string(),
        redirect_after_login,
        expires_at_ms: now.saturating_add(PENDING_OAUTH_TTL_MS),
    };
    store_pending_oauth_login(&state, pending).await;

    Ok(Redirect::temporary(authorization_url.as_ref()))
}

#[allow(clippy::too_many_lines)]
async fn get_auth_callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    let query = uri.query().unwrap_or_default();
    let callback =
        parse_callback_query(query).map_err(|error| invalid_payload(&error.to_string()))?;

    let callback_state = match &callback {
        OAuthCallback::AuthorizationCode { state, .. } => Some(state.as_str()),
        OAuthCallback::AuthorizationError { state, .. } => state.as_deref(),
    };
    let pending = match callback_state {
        Some(state_token) => take_pending_oauth_login(&state, state_token).await,
        None => None,
    };

    match callback {
        OAuthCallback::AuthorizationError {
            error,
            error_description,
            ..
        } => {
            let target = pending.as_ref().map_or_else(
                || "/".to_string(),
                |entry| entry.redirect_after_login.clone(),
            );
            let message = error_description.unwrap_or(error);
            Ok(
                Redirect::temporary(&redirect_with_status(&target, "auth_error", &message))
                    .into_response(),
            )
        }
        OAuthCallback::AuthorizationCode {
            code,
            state: callback_state,
        } => {
            let Some(pending) = pending else {
                return Err(invalid_payload("oauth callback state was not recognized"));
            };
            if pending.state != callback_state {
                return Err(invalid_payload("oauth callback state mismatch"));
            }

            let oauth_config = oauth_client_config_from_pending(&state, &pending)?;
            let token_response = exchange_authorization_code(
                &state.http_client,
                &state.local_oauth,
                &oauth_config,
                &code,
                &pending.verifier,
            )
            .await
            .map_err(|message| invalid_payload(&message))?;
            let profile = fetch_oauth_profile(
                &state.http_client,
                &token_response.access_token,
                &state.capability_targets.profile_url,
            )
            .await
            .map_err(|message| invalid_payload(&message))?;
            let capability_report = probe_with_targets(
                &state.http_client,
                Some(&token_response.access_token),
                &state.local_oauth.status(),
                &pending.wiki_id,
                &state.capability_targets,
            )
            .await;
            let current_ms = now_ms();
            let stored = StoredSession {
                username: profile.username,
                scopes: if capability_report.checked && capability_report.error.is_none() {
                    effective_session_scopes(&capability_report)
                } else if !profile.grants.is_empty() {
                    profile.grants
                } else {
                    token_response
                        .scope
                        .as_deref()
                        .map_or_else(Vec::new, split_scope_string)
                },
                expires_at_ms: Some(current_ms + SESSION_IDLE_TIMEOUT_MS),
                access_token: token_response.access_token,
                refresh_token: token_response.refresh_token,
                upstream_access_expires_at_ms: token_response
                    .expires_in
                    .and_then(|seconds| i64::try_from(seconds).ok())
                    .map(|seconds| current_ms.saturating_add(seconds.saturating_mul(1000))),
                bridge_mode: "wikimedia-oauth".to_string(),
                created_at_ms: current_ms,
                last_seen_at_ms: current_ms,
                capability_cache: HashMap::from([(
                    pending.wiki_id.clone(),
                    CachedCapabilityReport {
                        fetched_at_ms: current_ms,
                        report: capability_report,
                    },
                )]),
                action_history: Vec::new(),
            };
            let session_id =
                install_session(&state, session_cookie_value(&headers), stored, current_ms).await;

            Ok((
                [(SET_COOKIE, session_cookie_header(&session_id))],
                Redirect::temporary(&redirect_with_status(
                    &pending.redirect_after_login,
                    "auth",
                    "oauth-ok",
                )),
            )
                .into_response())
        }
    }
}

async fn get_auth_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Json<OAuthSessionView> {
    Json(auth_session_view(&state, &headers, true).await)
}

async fn post_auth_logout(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if let Some(session_id) = session_cookie_value(&headers) {
        let mut sessions = state.sessions.write().await;
        sessions.remove(&session_id);
    }

    (
        StatusCode::OK,
        [(SET_COOKIE, expired_session_cookie_header())],
        Json(auth_session_view_without_session(&state)),
    )
}

async fn get_session(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    Json(current_status(&state, &headers, true).await)
}

async fn get_capabilities(
    Path(wiki_id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Json<DevAuthCapabilityReport> {
    Json(capability_report_for_request(&state, &headers, &wiki_id, true).await)
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
    validate_bootstrap_payload(&payload)?;

    let Some(access_token) = state.local_oauth.access_token() else {
        return Err((
            StatusCode::PRECONDITION_FAILED,
            Json(serde_json::json!({
                "error": "WIKIMEDIA_ACCESS_TOKEN is not available in .env.wikimedia.local"
            })),
        ));
    };

    let capabilities = capability_report_for_local_token(&state, "frwiki", true).await;
    let Some(username) = capabilities.username.clone() else {
        let message = capabilities
            .error
            .unwrap_or_else(|| "token validation did not return a Wikimedia username".to_string());
        return Err((
            StatusCode::PRECONDITION_FAILED,
            Json(serde_json::json!({ "error": message })),
        ));
    };

    let current_ms = now_ms();
    let session_id = next_session_id(&state, current_ms);
    let stored = StoredSession {
        username,
        scopes: effective_session_scopes(&capabilities),
        expires_at_ms: Some(current_ms + SESSION_IDLE_TIMEOUT_MS),
        access_token: access_token.to_string(),
        refresh_token: None,
        upstream_access_expires_at_ms: None,
        bridge_mode: "local-env-token".to_string(),
        created_at_ms: current_ms,
        last_seen_at_ms: current_ms,
        capability_cache: HashMap::from([(
            "frwiki".to_string(),
            CachedCapabilityReport {
                fetched_at_ms: current_ms,
                report: capabilities,
            },
        )]),
        action_history: Vec::new(),
    };

    let prior_session_id = session_cookie_value(&headers);
    let mut sessions = state.sessions.write().await;
    prune_expired_sessions(&mut sessions);
    if let Some(prior_session_id) = prior_session_id {
        sessions.remove(&prior_session_id);
    }
    sessions.insert(session_id.clone(), stored);
    let status = to_status(sessions.get(&session_id), &state.local_oauth, current_ms);
    drop(sessions);
    info!(
        session_id = session_id.as_str(),
        bridge_mode = "local-env-token",
        "bootstrapped local dev-auth session"
    );

    Ok((
        StatusCode::OK,
        [(SET_COOKIE, session_cookie_header(&session_id))],
        Json(status),
    ))
}

async fn delete_session(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if let Some(session_id) = session_cookie_value(&headers) {
        let mut sessions = state.sessions.write().await;
        sessions.remove(&session_id);
        info!(
            session_id = session_id.as_str(),
            "cleared local dev-auth session"
        );
    }

    (
        StatusCode::OK,
        [(SET_COOKIE, expired_session_cookie_header())],
        Json(to_status(None, &state.local_oauth, now_ms())),
    )
}

async fn get_bootstrap_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Json<DevAuthBootstrapStatus> {
    let auth = current_status(&state, &headers, true).await;

    Json(bootstrap_status(&state, &auth))
}

#[allow(clippy::too_many_lines)]
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
    let executed_at_ms = now_ms();
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
            let response_preview = truncate_response_body(&response.body);
            let response_summary =
                sp42_core::parse_action_response_summary(&response, payload.kind.label())
                    .map_err(|error| action_error_response(&error))?;
            record_action_execution(
                &state,
                &session.session_id,
                build_action_log_entry(
                    executed_at_ms,
                    &payload,
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
                ),
            )
            .await;

            Ok((
                StatusCode::OK,
                Json(SessionActionExecutionResponse {
                    wiki_id: payload.wiki_id,
                    kind: payload.kind,
                    rev_id: payload.rev_id,
                    accepted: true,
                    actor: Some(session.username),
                    http_status: Some(response.status),
                    api_code: response_summary.api_code.clone(),
                    retryable: response_summary.retryable,
                    warnings: response_summary.warnings.clone(),
                    result: response_summary.result.clone(),
                    message: Some(format!(
                        "MediaWiki HTTP {} {}",
                        response.status, response_preview
                    )),
                }),
            ))
        }
        Err(error) => {
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
            record_action_execution(
                &state,
                &session.session_id,
                build_action_log_entry(
                    executed_at_ms,
                    &payload,
                    ActionLogOutcome {
                        accepted: false,
                        http_status: logged_http_status.or(Some(status)),
                        api_code,
                        retryable,
                        warnings: Vec::new(),
                        result: None,
                        response_preview: None,
                        error: Some(error_message.clone()),
                    },
                ),
            )
            .await;
            Err(api_error)
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
            let undo_after_rev_id = payload
                .undo_after_rev_id
                .expect("undo_after_rev_id is validated above");
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

#[allow(clippy::too_many_arguments)]
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

async fn record_action_execution(
    state: &AppState,
    session_id: &str,
    entry: ActionExecutionLogEntry,
) {
    let mut sessions = state.sessions.write().await;
    prune_expired_sessions(&mut sessions);
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

    let history = action_history_for_session(state, &session.session_id, 1).await;
    let last_execution = history.first().cloned();
    let total_actions = action_history_len_for_session(state, &session.session_id).await;
    let successful_actions = sessions_action_count(state, &session.session_id, true).await;
    let failed_actions = total_actions.saturating_sub(successful_actions);
    let retryable_failures = retryable_failure_count(state, &session.session_id).await;
    ActionExecutionStatusReport {
        authenticated: true,
        session_id: Some(session.session_id),
        username: Some(session.username),
        total_actions,
        successful_actions,
        failed_actions,
        retryable_failures,
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

async fn action_history_len_for_session(state: &AppState, session_id: &str) -> usize {
    let sessions = state.sessions.read().await;
    if let Some(session) = sessions.get(session_id) {
        session.action_history.len()
    } else {
        0
    }
}

async fn sessions_action_count(state: &AppState, session_id: &str, accepted: bool) -> usize {
    let sessions = state.sessions.read().await;
    sessions.get(session_id).map_or(0, |session| {
        session
            .action_history
            .iter()
            .filter(|entry| entry.accepted == accepted)
            .count()
    })
}

async fn retryable_failure_count(state: &AppState, session_id: &str) -> usize {
    let sessions = state.sessions.read().await;
    sessions.get(session_id).map_or(0, |session| {
        session
            .action_history
            .iter()
            .filter(|entry| !entry.accepted && entry.retryable)
            .count()
    })
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
        authenticated: false,
        username: None,
        scopes: Vec::new(),
        expires_at_ms: None,
        upstream_access_expires_at_ms: None,
        refresh_available: false,
        bridge_mode: "inactive".to_string(),
        local_token_available: state.local_oauth.access_token().is_some(),
        oauth_client_ready: state.local_oauth.has_confidential_oauth_client(),
        login_path: AUTH_LOGIN_PATH.to_string(),
        logout_path: AUTH_LOGOUT_PATH.to_string(),
    }
}

async fn auth_session_view(state: &AppState, headers: &HeaderMap, touch: bool) -> OAuthSessionView {
    match current_session_snapshot(state, headers, touch).await {
        Some(session) => OAuthSessionView {
            authenticated: true,
            username: Some(session.username),
            scopes: session.scopes,
            expires_at_ms: session.expires_at_ms,
            upstream_access_expires_at_ms: sessions_upstream_access_expiry(state, headers).await,
            refresh_available: sessions_refresh_available(state, headers).await,
            bridge_mode: session.bridge_mode,
            local_token_available: state.local_oauth.access_token().is_some(),
            oauth_client_ready: state.local_oauth.has_confidential_oauth_client(),
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
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("http");
    Ok(format!("{scheme}://{host}"))
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
    prune_expired_pending_oauth_logins(&mut pending_logins);
    pending_logins.insert(pending.state.clone(), pending);
}

async fn take_pending_oauth_login(
    state: &AppState,
    state_token: &str,
) -> Option<PendingOAuthLogin> {
    let mut pending_logins = state.pending_oauth_logins.write().await;
    prune_expired_pending_oauth_logins(&mut pending_logins);
    pending_logins.remove(state_token)
}

fn prune_expired_pending_oauth_logins(pending_logins: &mut HashMap<String, PendingOAuthLogin>) {
    let current_time_ms = now_ms();
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
    prune_expired_sessions(&mut sessions);
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
    DevAuthBootstrapStatus {
        bootstrap_ready: state.local_oauth.access_token().is_some(),
        oauth: state.local_oauth.status(),
        session: auth.clone(),
        source_path: state
            .local_oauth
            .source_path()
            .map(|path| path.display().to_string()),
        source_report: state.local_oauth.source_report(),
    }
}

fn live_operator_backend_status(
    readiness: &ServerHealthStatus,
    auth: &DevAuthSessionStatus,
) -> LiveOperatorBackendStatus {
    LiveOperatorBackendStatus {
        ready_for_local_testing: readiness.ready_for_local_testing,
        readiness_issues: readiness.readiness_issues.clone(),
        bootstrap_ready: readiness.bootstrap.bootstrap_ready,
        oauth: readiness.oauth.clone(),
        session: auth.clone(),
        source_report: readiness.bootstrap.source_report.clone(),
        capability_cache_present: readiness.capability_cache.present,
        capability_cache_fresh: readiness.capability_cache.fresh,
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

fn now_ms() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => i64::try_from(duration.as_millis()).unwrap_or(i64::MAX),
        Err(_) => 0,
    }
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

fn session_cookie_header(session_id: &str) -> HeaderValue {
    HeaderValue::from_str(&format!(
        "{SESSION_COOKIE_NAME}={session_id}; HttpOnly; SameSite=Lax; Path=/; Max-Age={SESSION_COOKIE_MAX_AGE_SECONDS}"
    ))
    .expect("session cookie header should be valid")
}

fn expired_session_cookie_header() -> HeaderValue {
    HeaderValue::from_str(&format!(
        "{SESSION_COOKIE_NAME}=deleted; HttpOnly; SameSite=Lax; Path=/; Max-Age=0"
    ))
    .expect("expired session cookie header should be valid")
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

fn prune_expired_sessions(sessions: &mut HashMap<String, StoredSession>) {
    let current_time_ms = now_ms();
    sessions.retain(|_, session| !session_is_expired(session, current_time_ms));
}

async fn current_session_snapshot(
    state: &AppState,
    headers: &HeaderMap,
    touch: bool,
) -> Option<SessionSnapshot> {
    let session_id = session_cookie_value(headers)?;
    let mut sessions = state.sessions.write().await;
    prune_expired_sessions(&mut sessions);
    let session = sessions.get_mut(&session_id)?;
    if touch {
        session.last_seen_at_ms = now_ms();
        session.expires_at_ms = Some(session_expires_at_ms(session, now_ms()));
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
        None => to_status(None, &state.local_oauth, now_ms()),
    }
}

async fn cached_capabilities_for_session(
    state: &AppState,
    session_id: &str,
    wiki_id: &str,
) -> Option<DevAuthCapabilityReport> {
    let mut sessions = state.sessions.write().await;
    prune_expired_sessions(&mut sessions);
    let session = sessions.get_mut(session_id)?;
    let cache = session.capability_cache.get(wiki_id)?;
    if cache_is_fresh(cache) {
        session.last_seen_at_ms = now_ms();
        session.expires_at_ms = Some(session_expires_at_ms(session, now_ms()));
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
    prune_expired_sessions(&mut sessions);
    if let Some(session) = sessions.get_mut(session_id) {
        session.capability_cache.insert(
            report.wiki_id.clone(),
            CachedCapabilityReport {
                fetched_at_ms: now_ms(),
                report: report.clone(),
            },
        );
    }
}

fn cache_is_fresh(cache: &CachedCapabilityReport) -> bool {
    now_ms().saturating_sub(cache.fetched_at_ms) < CAPABILITY_CACHE_TTL_MS
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
    use std::time::Instant;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use axum::body::{Body, to_bytes};
    use axum::http::{Method, Request, StatusCode};
    use axum::routing::get;
    use axum::{Json, Router};
    use sp42_core::{FileStorage, LocalOAuthSourceReport, Storage};
    use tower::util::ServiceExt;

    use super::{
        ACTION_HISTORY_PATH, ACTION_STATUS_PATH, ActionExecutionHistoryReport,
        ActionExecutionLogEntry, ActionExecutionStatusReport, AppState, CoordinationRoomInspection,
        DevAuthBootstrapStatus, OPERATOR_READINESS_PATH, OPERATOR_REPORT_PATH,
        OPERATOR_STORAGE_LAYOUT_PATH, OperatorReport, OperatorRuntimeInspection,
        OperatorStorageLayoutView, RoomInspectionCollection, RuntimeDebugStatus,
        ServerHealthStatus, StoredSession, build_router, now_ms, operator_endpoint_manifest,
        to_status,
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
            capability_targets: CapabilityProbeTargets::default(),
            coordination: CoordinationRegistry::default(),
            next_client_id: Arc::new(AtomicU64::new(1)),
            next_session_id: Arc::new(AtomicU64::new(1)),
            started_at: Instant::now(),
        }
    }

    fn temp_local_env_file(contents: &str) -> std::path::PathBuf {
        let temp_dir = std::env::temp_dir().join(format!(
            "sp42-server-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be valid")
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_dir).expect("temp dir should create");
        let path = temp_dir.join(".env.wikimedia.local");
        std::fs::write(&path, contents).expect("temp env file should write");
        path
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
                let continued = params.contains_key("rccontinue");
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
            (_, _, _, Some(prop)) if prop == "revisions" => {
                let revids = params.get("revids").cloned().unwrap_or_default();
                let include_second = revids.contains("123457");
                serde_json::json!({
                    "query": {
                        "pages": [
                            {
                                "pageid": 1,
                                "title": if include_second { "Live route sample page 2" } else { "Live route sample" },
                                "revisions": if include_second {
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
            capability_targets: CapabilityProbeTargets {
                profile_url: format!("{profile_base}/oauth2/resource/profile"),
                api_url: Some(format!("{profile_base}/w/api.php")),
            },
            coordination: CoordinationRegistry::default(),
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
        assert_eq!(
            status.bootstrap.source_report.source_path.as_deref(),
            local_env_path.to_str()
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
                    source_path: Some("/tmp/.env.wikimedia.local".to_string()),
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

        assert!(backend.ready_for_local_testing);
        assert!(backend.bootstrap_ready);
        assert!(backend.source_report.loaded_from_source);
        assert!(backend.capability_cache_present);
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
            capability_targets: CapabilityProbeTargets {
                profile_url: format!("{profile_base}/oauth2/resource/profile"),
                api_url: Some(format!("{profile_base}/w/api.php")),
            },
            coordination: CoordinationRegistry::default(),
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
        let runtime_root =
            std::env::temp_dir().join(format!("sp42-live-operator-runtime-{}", now_ms()));
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
            capability_targets: CapabilityProbeTargets {
                profile_url: format!("{profile_base}/oauth2/resource/profile"),
                api_url: Some(format!("{profile_base}/w/api.php")),
            },
            coordination: CoordinationRegistry::default(),
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
        assert!(view.backend.bootstrap_ready);
        assert!(!view.telemetry.phase_timings.is_empty());
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
    async fn live_operator_route_reuses_persisted_backlog_checkpoint() {
        let local_env_path = temp_local_env_file(
            "WIKIMEDIA_CLIENT_APPLICATION_KEY=client-key\nWIKIMEDIA_CLIENT_APPLICATION_SECRET=client-secret\nWIKIMEDIA_ACCESS_TOKEN=token-value\n",
        );
        let (profile_base, server) = mock_capability_server().await;
        let runtime_root =
            std::env::temp_dir().join(format!("sp42-live-operator-runtime-persist-{}", now_ms()));
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
            capability_targets: CapabilityProbeTargets {
                profile_url: format!("{profile_base}/oauth2/resource/profile"),
                api_url: Some(format!("{profile_base}/w/api.php")),
            },
            coordination: CoordinationRegistry::default(),
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
        let first_body = to_bytes(first.into_body(), usize::MAX)
            .await
            .expect("first response body should read");
        let first_view: sp42_core::LiveOperatorView =
            serde_json::from_slice(&first_body).expect("first live operator view should parse");

        let second = router
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/operator/live/frwiki?limit=1")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("second live operator request should succeed");

        server.abort();

        let second_body = to_bytes(second.into_body(), usize::MAX)
            .await
            .expect("second response body should read");
        let second_view: sp42_core::LiveOperatorView =
            serde_json::from_slice(&second_body).expect("second live operator view should parse");

        assert_eq!(first_view.queue[0].event.title, "Live route sample");
        assert_eq!(second_view.queue[0].event.title, "Live route sample page 2");
        assert_eq!(
            second_view
                .backlog_status
                .as_ref()
                .and_then(|status| status.next_continue.as_deref()),
            Some("20260324010203|789")
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
    async fn bootstrap_derives_username_and_scopes_from_validated_token() {
        let local_env_path = temp_local_env_file(
            "WIKIMEDIA_CLIENT_APPLICATION_KEY=client-key\nWIKIMEDIA_CLIENT_APPLICATION_SECRET=client-secret\nWIKIMEDIA_ACCESS_TOKEN=token-value\n",
        );
        let (profile_base, server) = mock_capability_server().await;
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
            capability_targets: CapabilityProbeTargets {
                profile_url: format!("{profile_base}/oauth2/resource/profile"),
                api_url: Some(format!("{profile_base}/w/api.php")),
            },
            coordination: CoordinationRegistry::default(),
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
