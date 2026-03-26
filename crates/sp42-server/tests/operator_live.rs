use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::extract::Query;
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::future::join_all;
use reqwest::Client;
use serde::Deserialize;
use sp42_core::{FileStorage, LiveOperatorView, Storage, branding};
use tokio::net::TcpListener as TokioTcpListener;

#[derive(Clone)]
struct MockState {
    recentchanges_calls: Arc<AtomicUsize>,
}

#[derive(Debug, Deserialize, serde::Serialize)]
struct ApiQuery {
    action: Option<String>,
    meta: Option<String>,
    list: Option<String>,
    prop: Option<String>,
    revids: Option<String>,
    rccontinue: Option<String>,
}

#[derive(Debug, Deserialize)]
struct HealthzResponse {
    ready_for_local_testing: bool,
    bootstrap: HealthzBootstrap,
    capability_probe: HealthzCapabilityProbe,
}

#[derive(Debug, Deserialize)]
struct HealthzBootstrap {
    bootstrap_ready: bool,
}

#[derive(Debug, Deserialize)]
struct HealthzCapabilityProbe {
    available: bool,
}

fn unique_temp_dir(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "sp42-{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be valid")
            .as_nanos()
    ))
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .expect("free port should be available")
        .port()
}

#[allow(clippy::too_many_lines)]
async fn start_mock_backend() -> (String, tokio::task::JoinHandle<()>) {
    async fn profile() -> Json<serde_json::Value> {
        Json(serde_json::json!({
            "username": "Schiste",
            "grants": ["basic", "editpage", "patrol", "rollback"]
        }))
    }

    async fn liftwing() -> Json<serde_json::Value> {
        Json(serde_json::json!({ "probability": 0.73 }))
    }

    #[allow(clippy::too_many_lines)]
    async fn api(
        axum::extract::State(state): axum::extract::State<MockState>,
        Query(params): Query<ApiQuery>,
    ) -> Json<serde_json::Value> {
        match (
            params.action.as_deref(),
            params.meta.as_deref(),
            params.list.as_deref(),
            params.prop.as_deref(),
        ) {
            (Some("query"), Some("userinfo"), _, _) => Json(serde_json::json!({
                "query": {
                    "userinfo": {
                        "name": "Schiste",
                        "groups": ["*", "user", "autoconfirmed", "autopatrolled"],
                        "rights": ["edit", "patrol", "rollback"]
                    }
                }
            })),
            (Some("query"), Some("tokens"), _, _) => Json(serde_json::json!({
                "query": {
                    "tokens": {
                        "csrftoken": "csrf",
                        "patroltoken": "patrol",
                        "rollbacktoken": "rollback"
                    }
                }
            })),
            (Some("query"), None, Some("recentchanges"), _) => {
                let call_index = state.recentchanges_calls.fetch_add(1, Ordering::SeqCst);
                if call_index == 0 {
                    Json(serde_json::json!({
                        "continue": { "rccontinue": "20260324010202|456" },
                        "query": {
                            "recentchanges": [
                                {
                                    "type": "edit",
                                    "title": "Live route sample",
                                    "ns": 0,
                                    "revid": 123_456,
                                    "old_revid": 123_455,
                                    "user": "192.0.2.44",
                                    "timestamp": "2026-03-24T01:02:02Z",
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
                    }))
                } else {
                    Json(serde_json::json!({
                        "continue": { "rccontinue": "20260324010203|789" },
                        "query": {
                            "recentchanges": [
                                {
                                    "type": "edit",
                                    "title": "Live route sample page 2",
                                    "ns": 0,
                                    "revid": 123_457,
                                    "old_revid": 123_456,
                                    "user": "192.0.2.44",
                                    "timestamp": "2026-03-24T01:02:03Z",
                                    "bot": false,
                                    "minor": false,
                                    "new": false,
                                    "oldlen": 80,
                                    "newlen": 64,
                                    "comment": "sample edit 2",
                                    "tags": ["mw-reverted"]
                                }
                            ]
                        }
                    }))
                }
            }
            (Some("query"), None, _, Some("revisions")) => {
                let revids = params.revids.as_deref().unwrap_or_default();
                let include_second = revids.contains("123457");
                if include_second {
                    Json(serde_json::json!({
                        "query": {
                            "pages": [
                                {
                                    "pageid": 1,
                                    "title": "Live route sample page 2",
                                    "revisions": [
                                        {
                                            "revid": 123_456,
                                            "slots": { "main": { "content": "After text with removal" } }
                                        },
                                        {
                                            "revid": 123_457,
                                            "slots": { "main": { "content": "Page 2 after text" } }
                                        }
                                    ]
                                }
                            ]
                        }
                    }))
                } else {
                    Json(serde_json::json!({
                        "query": {
                            "pages": [
                                {
                                    "pageid": 1,
                                    "title": "Live route sample",
                                    "revisions": [
                                        {
                                            "revid": 123_455,
                                            "slots": { "main": { "content": "Before text" } }
                                        },
                                        {
                                            "revid": 123_456,
                                            "slots": { "main": { "content": "After text with removal" } }
                                        }
                                    ]
                                }
                            ]
                        }
                    }))
                }
            }
            _ => Json(serde_json::json!({
                "error": "unexpected request",
                "params": params
            })),
        }
    }

    let state = MockState {
        recentchanges_calls: Arc::new(AtomicUsize::new(0)),
    };

    let router = Router::new()
        .route("/oauth2/resource/profile", get(profile))
        .route("/w/api.php", get(api))
        .route(
            "/service/lw/inference/v1/models/revertrisk-language-agnostic:predict",
            post(liftwing),
        )
        .with_state(state);

    let listener = TokioTcpListener::bind("127.0.0.1:0")
        .await
        .expect("mock listener should bind");
    let addr = listener.local_addr().expect("mock listener address");
    let handle = tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("mock server should run");
    });

    (format!("http://{addr}"), handle)
}

fn write_local_env(temp_dir: &Path) {
    fs::write(
        temp_dir.join(".env.wikimedia.local"),
        "WIKIMEDIA_CLIENT_APPLICATION_KEY=client-key\nWIKIMEDIA_CLIENT_APPLICATION_SECRET=client-secret\nWIKIMEDIA_ACCESS_TOKEN=token-value\n",
    )
    .expect("local env file should write");
}

fn spawn_server(temp_dir: &Path, bind_addr: &str, mock_base: &str) -> Child {
    let runtime_dir = temp_dir.join(".sp42-runtime");
    fs::create_dir_all(&runtime_dir).expect("runtime dir should create");

    Command::new(env!("CARGO_BIN_EXE_sp42-server"))
        .current_dir(temp_dir)
        .env("SP42_BIND_ADDR", bind_addr)
        .env("SP42_RUNTIME_DIR", &runtime_dir)
        .env(
            "SP42_TEST_PROFILE_URL",
            format!("{mock_base}/oauth2/resource/profile"),
        )
        .env("SP42_TEST_API_URL", format!("{mock_base}/w/api.php"))
        .env(
            "SP42_TEST_LIFTWING_URL",
            format!(
                "{mock_base}/service/lw/inference/v1/models/revertrisk-language-agnostic:predict"
            ),
        )
        .env("SP42_INGESTION_POLL_MS", "50")
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("sp42-server binary should spawn")
}

async fn wait_for_health(client: &Client, base_url: &str, child: &mut Child) -> HealthzResponse {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(status) = child
            .try_wait()
            .expect("server process should be inspectable")
        {
            panic!("sp42-server exited early with status {status}");
        }

        if let Ok(response) = client.get(format!("{base_url}/healthz")).send().await
            && response.status().is_success()
        {
            let status = response
                .json::<HealthzResponse>()
                .await
                .expect("health response should parse");
            if status.ready_for_local_testing {
                return status;
            }
        }

        assert!(
            Instant::now() < deadline,
            "timed out waiting for sp42-server readiness"
        );

        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}


fn dump_runtime_root(runtime_root: &Path) {
    eprintln!("runtime root: {}", runtime_root.display());
    if let Ok(entries) = fs::read_dir(runtime_root) {
        for entry in entries.flatten() {
            let path = entry.path();
            let metadata = entry.metadata().ok();
            eprintln!(
                "runtime entry: {} size={:?}",
                path.display(),
                metadata.as_ref().map(std::fs::Metadata::len)
            );
        }
    }
}

async fn fetch_live(client: &Client, base_url: &str, runtime_root: &Path) -> LiveOperatorView {
    let response = client
        .get(format!("{base_url}/operator/live/frwiki?limit=1"))
        .send()
        .await
        .expect("live operator request should succeed");
    let status = response.status();
    let body = response
        .text()
        .await
        .expect("live operator body should read");
    if !status.is_success() {
        dump_runtime_root(runtime_root);
    }
    assert!(
        status.is_success(),
        "live operator response was {status}: {body}"
    );
    serde_json::from_str::<LiveOperatorView>(&body).expect("live operator payload should parse")
}

async fn wait_for_live_title(
    client: &Client,
    base_url: &str,
    runtime_root: &Path,
    expected_title: &str,
) -> LiveOperatorView {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let view = fetch_live(client, base_url, runtime_root).await;
        if view
            .queue
            .first()
            .is_some_and(|item| item.event.title == expected_title)
        {
            return view;
        }

        assert!(
            Instant::now() < deadline,
            "timed out waiting for live title {expected_title}"
        );

        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[allow(clippy::too_many_lines)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn operator_live_contract_reuses_checkpoints_and_handles_concurrent_requests() {
    let temp_dir = unique_temp_dir("server-operator-live");
    fs::create_dir_all(&temp_dir).expect("temp dir should create");
    write_local_env(&temp_dir);
    let runtime_root = temp_dir.join(".sp42-runtime");

    let (mock_base, mock_handle) = start_mock_backend().await;
    let bind_addr = format!("127.0.0.1:{}", free_port());
    let mut child = spawn_server(&temp_dir, &bind_addr, &mock_base);
    let base_url = format!("http://{bind_addr}");
    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .user_agent(branding::USER_AGENT)
        .build()
        .expect("reqwest client should build");

    let health = wait_for_health(&client, &base_url, &mut child).await;
    assert!(health.ready_for_local_testing);
    assert!(health.bootstrap.bootstrap_ready);
    assert!(health.capability_probe.available);

    let first = fetch_live(&client, &base_url, &runtime_root).await;
    assert_eq!(first.project, branding::PROJECT_NAME);
    assert_eq!(first.wiki_id, "frwiki");
    assert_eq!(first.query.limit, 1);
    assert_eq!(first.queue.len(), 1);
    assert_eq!(first.queue[0].event.title, "Live route sample");
    assert!(first.review_workbench.is_some());
    assert!(first.diff.is_some());
    assert!(first.backlog_status.is_some());
    assert!(first.capabilities.checked);
    assert!(first.backend.bootstrap_ready.is_enabled());
    assert!(
        first
            .notes
            .iter()
            .any(|line| line.contains("Persistent backlog checkpoint"))
    );
    assert_eq!(first.next_continue.as_deref(), Some("20260324010202|456"));

    let stable = fetch_live(&client, &base_url, &runtime_root).await;
    assert_eq!(stable.queue[0].event.title, "Live route sample");
    assert_eq!(stable.next_continue.as_deref(), Some("20260324010202|456"));

    let second = wait_for_live_title(
        &client,
        &base_url,
        &runtime_root,
        "Live route sample page 2",
    )
    .await;
    assert_eq!(second.next_continue.as_deref(), Some("20260324010203|789"));
    assert_eq!(
        second
            .backlog_status
            .as_ref()
            .and_then(|status| status.next_continue.as_deref()),
        Some("20260324010203|789")
    );

    let concurrent = join_all((0..8).map(|_| fetch_live(&client, &base_url, &runtime_root))).await;
    for view in concurrent {
        assert_eq!(view.queue.len(), 1);
        assert_eq!(view.queue[0].event.title, "Live route sample page 2");
        assert_eq!(view.next_continue.as_deref(), Some("20260324010203|789"));
        assert!(view.review_workbench.is_some());
        assert!(view.diff.is_some());
        assert!(view.capabilities.checked);
    }

    let checkpoint_storage = FileStorage::new(runtime_root.clone());
    let checkpoint = checkpoint_storage
        .get("recentchanges.rccontinue.frwiki")
        .await
        .expect("checkpoint should load")
        .expect("checkpoint should persist");
    let checkpoint = String::from_utf8(checkpoint).expect("checkpoint should be valid UTF-8");
    assert_eq!(checkpoint, "20260324010203|789");

    let _ = child.kill();
    let _ = child.wait();
    mock_handle.abort();
    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn auth_logout_clears_bootstrapped_session_state() {
    let temp_dir = unique_temp_dir("server-auth-logout");
    fs::create_dir_all(&temp_dir).expect("temp dir should create");
    write_local_env(&temp_dir);

    let (mock_base, mock_handle) = start_mock_backend().await;
    let bind_addr = format!("127.0.0.1:{}", free_port());
    let mut child = spawn_server(&temp_dir, &bind_addr, &mock_base);
    let base_url = format!("http://{bind_addr}");
    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .user_agent(branding::USER_AGENT)
        .build()
        .expect("reqwest client should build");

    let _health = wait_for_health(&client, &base_url, &mut child).await;

    let bootstrap = client
        .post(format!("{base_url}/dev/auth/session/bootstrap"))
        .json(&serde_json::json!({}))
        .send()
        .await
        .expect("bootstrap request should succeed");
    assert!(bootstrap.status().is_success());
    let session_cookie = bootstrap
        .headers()
        .get(reqwest::header::SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::to_string)
        .expect("bootstrap should return a session cookie");

    let session_after_bootstrap = client
        .get(format!("{base_url}/auth/session"))
        .header(reqwest::header::COOKIE, &session_cookie)
        .send()
        .await
        .expect("auth session request should succeed");
    assert!(session_after_bootstrap.status().is_success());
    let session_json = session_after_bootstrap
        .json::<serde_json::Value>()
        .await
        .expect("auth session response should parse");
    assert_eq!(session_json["authenticated"], serde_json::json!(true));
    assert_eq!(session_json["username"], serde_json::json!("Schiste"));
    assert_eq!(session_json["bridge_mode"], serde_json::json!("local-env-token"));

    let logout = client
        .post(format!("{base_url}/auth/logout"))
        .header(reqwest::header::COOKIE, &session_cookie)
        .send()
        .await
        .expect("logout request should succeed");
    assert!(logout.status().is_success());
    let cleared_cookie = logout
        .headers()
        .get(reqwest::header::SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .expect("logout should return a clearing cookie");
    assert!(cleared_cookie.contains("Max-Age=0"));

    let session_after_logout = client
        .get(format!("{base_url}/auth/session"))
        .header(reqwest::header::COOKIE, &session_cookie)
        .send()
        .await
        .expect("auth session request after logout should succeed");
    assert!(session_after_logout.status().is_success());
    let session_json = session_after_logout
        .json::<serde_json::Value>()
        .await
        .expect("post-logout auth session response should parse");
    assert_eq!(session_json["authenticated"], serde_json::json!(false));
    assert_eq!(session_json["bridge_mode"], serde_json::json!("inactive"));

    let _ = child.kill();
    let _ = child.wait();
    mock_handle.abort();
    let _ = fs::remove_dir_all(&temp_dir);
}
