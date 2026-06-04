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
    ACTION_HISTORY_PATH, ACTION_STATUS_PATH, ActionExecutionHistoryReport, ActionExecutionLogEntry,
    ActionExecutionStatusReport, AppState, CoordinationRoomInspection, DevAuthBootstrapStatus,
    OPERATOR_READINESS_PATH, OPERATOR_REPORT_PATH, OPERATOR_STORAGE_LAYOUT_PATH, OperatorReport,
    OperatorRuntimeInspection, OperatorStorageLayoutView, RoomInspectionCollection,
    RuntimeDebugStatus, ServerHealthStatus, SessionActionKind, StoredSession, build_router, now_ms,
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

type TestWebSocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

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
        (Some(meta), Some(kind), _, _) if meta == "tokens" && kind == "patrol|rollback|csrf" => {
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

async fn connect_socket(base_url: &str, wiki_id: &str, session_id: Option<&str>) -> TestWebSocket {
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

async fn connect_session_socket(base_url: &str, wiki_id: &str, session_id: &str) -> TestWebSocket {
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

async fn recv_coordination_message(socket: &mut TestWebSocket) -> sp42_core::CoordinationMessage {
    loop {
        let frame = socket
            .next()
            .await
            .expect("websocket stream should stay open")
            .expect("websocket frame should be readable");

        match frame {
            WebSocketMessage::Binary(bytes) => {
                return sp42_core::decode_message(&bytes).expect("binary payload should decode");
            }
            WebSocketMessage::Text(text) => {
                return sp42_core::decode_message(text.as_str().as_bytes())
                    .expect("text payload should decode");
            }
            WebSocketMessage::Ping(_) | WebSocketMessage::Pong(_) | WebSocketMessage::Frame(_) => {}
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
            WebSocketMessage::Ping(_) | WebSocketMessage::Pong(_) | WebSocketMessage::Frame(_) => {}
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
    let payload: serde_json::Value = serde_json::from_slice(&body).expect("response should parse");
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
    let status: ServerHealthStatus = serde_json::from_slice(&body).expect("readiness should parse");

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

    let base =
        crate::oauth_runtime::public_base_url(&headers).expect("loopback host should be accepted");

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
        runtime_storage_root: unique_test_temp_path("sp42-server-runtime-logical-storage-route"),
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
