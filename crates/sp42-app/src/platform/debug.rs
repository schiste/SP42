use serde_json::Value;
use sp42_core::{
    CoordinationSnapshot, DevAuthActionTokenAvailability, DevAuthCapabilityReport,
    DevAuthDerivedCapabilities, DevAuthProbeAcceptance, DevAuthSessionStatus,
    LocalOAuthConfigStatus, ServerDebugSummary,
};

#[cfg(target_arch = "wasm32")]
use super::{auth, coordination, http::get_bytes};

const DEBUG_SUMMARY_URL: &str = "http://127.0.0.1:8788/debug/summary";
const DEBUG_RUNTIME_URL: &str = "http://127.0.0.1:8788/debug/runtime";
const CAPABILITIES_URL_PREFIX: &str = "http://127.0.0.1:8788/dev/auth/capabilities";
const ACTION_HISTORY_URL: &str = "http://127.0.0.1:8788/dev/actions/history";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevActionHistoryRecord {
    pub timestamp_ms: Option<u64>,
    pub wiki_id: String,
    pub rev_id: Option<u64>,
    pub kind: String,
    pub actor: Option<String>,
    pub status: String,
    pub summary: Option<String>,
    pub result: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeDebugStatus {
    pub project: String,
    pub uptime_ms: u64,
    pub auth: DevAuthSessionStatus,
    pub oauth: LocalOAuthConfigStatus,
    pub bootstrap: auth::DevAuthBootstrapStatus,
    pub coordination: CoordinationSnapshot,
}

#[must_use]
pub fn preview_dev_auth_capability_report(wiki_id: &str) -> DevAuthCapabilityReport {
    DevAuthCapabilityReport {
        checked: false,
        wiki_id: wiki_id.to_string(),
        username: None,
        oauth_grants: Vec::new(),
        wiki_groups: Vec::new(),
        wiki_rights: Vec::new(),
        acceptance: DevAuthProbeAcceptance {
            profile_accepted: false,
            userinfo_accepted: false,
        },
        token_availability: DevAuthActionTokenAvailability {
            csrf_token_available: false,
            patrol_token_available: false,
            rollback_token_available: false,
        },
        capabilities: DevAuthDerivedCapabilities::default(),
        notes: vec!["Local debug services are unavailable.".to_string()],
        error: Some("Local debug services are unavailable.".to_string()),
    }
}

#[must_use]
pub fn preview_runtime_debug_status() -> RuntimeDebugStatus {
    RuntimeDebugStatus {
        project: "SP42".to_string(),
        uptime_ms: 0,
        auth: auth::preview_dev_auth_session_status(),
        oauth: auth::preview_local_oauth_config_status(),
        bootstrap: auth::preview_dev_auth_bootstrap_status(),
        coordination: coordination::preview_coordination_snapshot(),
    }
}

#[must_use]
pub fn preview_server_debug_summary() -> ServerDebugSummary {
    ServerDebugSummary {
        project: "SP42".to_string(),
        auth: auth::preview_dev_auth_session_status(),
        oauth: auth::preview_local_oauth_config_status(),
        capabilities: preview_dev_auth_capability_report("frwiki"),
        coordination: coordination::preview_coordination_snapshot(),
    }
}

#[must_use]
pub fn preview_dev_action_history() -> Vec<DevActionHistoryRecord> {
    Vec::new()
}

#[cfg(target_arch = "wasm32")]
pub async fn fetch_server_debug_summary() -> Result<ServerDebugSummary, String> {
    let bytes = get_bytes(DEBUG_SUMMARY_URL, "fetch server debug summary").await?;
    serde_json::from_slice(&bytes).map_err(|error| error.to_string())
}

#[cfg(target_arch = "wasm32")]
pub async fn fetch_runtime_debug_status() -> Result<RuntimeDebugStatus, String> {
    let bytes = get_bytes(DEBUG_RUNTIME_URL, "fetch runtime debug status").await?;
    parse_runtime_debug_status(&bytes)
}

#[cfg(target_arch = "wasm32")]
pub async fn fetch_dev_auth_capabilities(wiki_id: &str) -> Result<DevAuthCapabilityReport, String> {
    let bytes = get_bytes(
        &format!("{CAPABILITIES_URL_PREFIX}/{wiki_id}"),
        "fetch dev auth capabilities",
    )
    .await?;
    serde_json::from_slice(&bytes).map_err(|error| error.to_string())
}

#[cfg(target_arch = "wasm32")]
pub async fn fetch_dev_action_history() -> Result<Vec<DevActionHistoryRecord>, String> {
    let bytes = get_bytes(ACTION_HISTORY_URL, "fetch dev action history").await?;
    parse_dev_action_history(&bytes)
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn fetch_server_debug_summary() -> Result<ServerDebugSummary, String> {
    Err("Server debug summary fetch is only available in the browser runtime.".to_string())
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn fetch_runtime_debug_status() -> Result<RuntimeDebugStatus, String> {
    Err("Runtime debug status fetch is only available in the browser runtime.".to_string())
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn fetch_dev_auth_capabilities(
    _wiki_id: &str,
) -> Result<DevAuthCapabilityReport, String> {
    Err("Dev auth capability fetch is only available in the browser runtime.".to_string())
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn fetch_dev_action_history() -> Result<Vec<DevActionHistoryRecord>, String> {
    Err("Dev action history fetch is only available in the browser runtime.".to_string())
}

#[must_use]
pub fn runtime_debug_status_lines(status: &RuntimeDebugStatus) -> Vec<String> {
    vec![
        format!("project={}", status.project),
        format!("uptime_ms={}", status.uptime_ms),
        format!("authenticated={}", status.auth.authenticated),
        format!("bootstrap_ready={}", status.bootstrap.bootstrap_ready),
        format!("room_count={}", status.coordination.rooms.len()),
    ]
}

#[must_use]
pub fn server_debug_summary_lines(summary: &ServerDebugSummary) -> Vec<String> {
    vec![
        format!("project={}", summary.project),
        format!("authenticated={}", summary.auth.authenticated),
        format!(
            "can_rollback={}",
            summary.capabilities.capabilities.moderation.can_rollback
        ),
        format!("room_count={}", summary.coordination.rooms.len()),
    ]
}

#[must_use]
pub fn dev_action_history_lines(history: &[DevActionHistoryRecord]) -> Vec<String> {
    let mut lines = vec![format!("action_history count={}", history.len())];

    if history.is_empty() {
        lines.push("No action history has been recorded yet.".to_string());
        return lines;
    }

    for (index, record) in history.iter().enumerate().take(6) {
        lines.push(format!("entry_index={}", index + 1));
        lines.push(format!(
            "kind={} wiki={} rev={} actor={} status={}",
            record.kind,
            record.wiki_id,
            record
                .rev_id
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            record.actor.as_deref().unwrap_or("unknown"),
            record.status
        ));
        if let Some(summary) = &record.summary {
            lines.push(format!("summary={summary}"));
        }
        if let Some(result) = &record.result {
            lines.push(format!("result={result}"));
        }
        if let Some(timestamp_ms) = record.timestamp_ms {
            lines.push(format!("timestamp_ms={timestamp_ms}"));
        }
    }

    lines
}

fn parse_runtime_debug_status(bytes: &[u8]) -> Result<RuntimeDebugStatus, String> {
    let value: Value = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    let object = value
        .as_object()
        .ok_or_else(|| "runtime debug response must be a JSON object".to_string())?;

    let project = object
        .get("project")
        .and_then(Value::as_str)
        .ok_or_else(|| "project field is missing or invalid".to_string())?
        .to_string();
    let uptime_ms = object
        .get("uptime_ms")
        .and_then(Value::as_u64)
        .ok_or_else(|| "uptime_ms field is missing or invalid".to_string())?;

    let auth = object
        .get("auth")
        .ok_or_else(|| "auth field is missing".to_string())?
        .clone();
    let auth = serde_json::from_value(auth).map_err(|error| error.to_string())?;

    let oauth = object
        .get("oauth")
        .ok_or_else(|| "oauth field is missing".to_string())?
        .clone();
    let oauth = serde_json::from_value(oauth).map_err(|error| error.to_string())?;

    let bootstrap = object
        .get("bootstrap")
        .ok_or_else(|| "bootstrap field is missing".to_string())?
        .clone();
    let bootstrap = auth::parse_bootstrap_status_value(bootstrap)?;

    let coordination = object
        .get("coordination")
        .ok_or_else(|| "coordination field is missing".to_string())?
        .clone();
    let coordination = serde_json::from_value(coordination).map_err(|error| error.to_string())?;

    Ok(RuntimeDebugStatus {
        project,
        uptime_ms,
        auth,
        oauth,
        bootstrap,
        coordination,
    })
}

fn parse_dev_action_history(bytes: &[u8]) -> Result<Vec<DevActionHistoryRecord>, String> {
    let value: Value = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;

    match value {
        Value::Array(entries) => entries
            .into_iter()
            .map(parse_history_record)
            .collect::<Result<Vec<_>, _>>(),
        Value::Object(object) => {
            if let Some(history) = object.get("history") {
                parse_history_value(history.clone())
            } else if let Some(entries) = object.get("entries") {
                parse_history_value(entries.clone())
            } else {
                Err(
                    "action history payload must contain a `history` or `entries` array"
                        .to_string(),
                )
            }
        }
        _ => Err("action history response must be a JSON array or object".to_string()),
    }
}

fn parse_history_value(value: Value) -> Result<Vec<DevActionHistoryRecord>, String> {
    match value {
        Value::Array(entries) => entries
            .into_iter()
            .map(parse_history_record)
            .collect::<Result<Vec<_>, _>>(),
        _ => Err("action history collection must be an array".to_string()),
    }
}

fn parse_history_record(value: Value) -> Result<DevActionHistoryRecord, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "action history entry must be a JSON object".to_string())?;

    Ok(DevActionHistoryRecord {
        timestamp_ms: read_u64(object, &["timestamp_ms", "timestamp", "created_at_ms"]),
        wiki_id: read_string(object, &["wiki_id", "wiki"]).unwrap_or_else(|| "unknown".to_string()),
        rev_id: read_u64(object, &["rev_id", "revision_id"]),
        kind: read_string(object, &["kind", "action", "type"])
            .unwrap_or_else(|| "unknown".to_string()),
        actor: read_string(object, &["actor", "username", "user"]),
        status: read_string(object, &["status", "state"]).unwrap_or_else(|| "unknown".to_string()),
        summary: read_string(object, &["summary", "message", "note"]),
        result: read_string(object, &["result", "response_preview", "response"]),
    })
}

fn read_u64(object: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|key| {
        object.get(*key).and_then(|value| match value {
            Value::Number(number) => number.as_u64(),
            Value::String(text) => text.parse::<u64>().ok(),
            _ => None,
        })
    })
}

fn read_string(object: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        object.get(*key).and_then(|value| match value {
            Value::String(text) => Some(text.clone()),
            Value::Number(number) => Some(number.to_string()),
            Value::Bool(flag) => Some(flag.to_string()),
            _ => None,
        })
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        DevActionHistoryRecord, RuntimeDebugStatus, dev_action_history_lines,
        parse_dev_action_history, parse_runtime_debug_status, runtime_debug_status_lines,
    };
    use sp42_core::{CoordinationSnapshot, DevAuthSessionStatus, LocalOAuthConfigStatus};

    #[test]
    fn runtime_debug_status_lines_include_key_statuses() {
        let lines = runtime_debug_status_lines(&RuntimeDebugStatus {
            project: "SP42".to_string(),
            uptime_ms: 42,
            auth: DevAuthSessionStatus {
                authenticated: true,
                username: Some("Tester".to_string()),
                scopes: vec![],
                expires_at_ms: None,
                token_present: true,
                bridge_mode: "local".to_string(),
                local_token_available: true,
            },
            oauth: LocalOAuthConfigStatus {
                client_id_present: true,
                client_secret_present: false,
                access_token_present: true,
            },
            bootstrap: super::auth::DevAuthBootstrapStatus {
                bootstrap_ready: true,
                oauth: LocalOAuthConfigStatus {
                    client_id_present: true,
                    client_secret_present: false,
                    access_token_present: true,
                },
                session: DevAuthSessionStatus {
                    authenticated: true,
                    username: Some("Tester".to_string()),
                    scopes: vec![],
                    expires_at_ms: None,
                    token_present: true,
                    bridge_mode: "local".to_string(),
                    local_token_available: true,
                },
                source_path: Some(".env.wikimedia.local".to_string()),
            },
            coordination: CoordinationSnapshot { rooms: vec![] },
        });

        assert!(lines.iter().any(|line| line.contains("project=SP42")));
        assert!(lines.iter().any(|line| line.contains("uptime_ms=42")));
        assert!(lines.iter().any(|line| line.contains("authenticated=true")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("bootstrap_ready=true"))
        );
    }

    #[test]
    fn runtime_debug_status_parse_requires_core_fields() {
        let value = json!({
            "project": "SP42",
            "uptime_ms": 99,
            "auth": {
                "authenticated": true,
                "username": "Tester",
                "scopes": ["rollback"],
                "expires_at_ms": null,
                "token_present": true,
                "bridge_mode": "local",
                "local_token_available": true
            },
            "oauth": {
                "client_id_present": true,
                "client_secret_present": false,
                "access_token_present": true
            },
            "bootstrap": {
                "bootstrap_ready": true,
                "oauth": {
                    "client_id_present": true,
                    "client_secret_present": false,
                    "access_token_present": true
                },
                "session": {
                    "authenticated": true,
                    "username": "Tester",
                    "scopes": ["rollback"],
                    "expires_at_ms": null,
                    "token_present": true,
                    "bridge_mode": "local",
                    "local_token_available": true
                },
                "source_path": ".env.wikimedia.local"
            },
            "coordination": {
                "rooms": []
            }
        });

        let parsed = parse_runtime_debug_status(
            &serde_json::to_vec(&value).expect("fixture should serialize"),
        )
        .expect("runtime debug status should parse");

        assert_eq!(parsed.project, "SP42");
        assert!(parsed.bootstrap.bootstrap_ready);
        assert_eq!(parsed.coordination.rooms.len(), 0);
    }

    #[test]
    fn dev_action_history_lines_include_key_fields() {
        let lines = dev_action_history_lines(&[DevActionHistoryRecord {
            timestamp_ms: Some(42),
            wiki_id: "frwiki".to_string(),
            rev_id: Some(123456),
            kind: "Rollback".to_string(),
            actor: Some("Tester".to_string()),
            status: "success".to_string(),
            summary: Some("rolled back edit".to_string()),
            result: Some("ok".to_string()),
        }]);

        assert!(
            lines
                .iter()
                .any(|line| line.contains("action_history count=1"))
        );
        assert!(lines.iter().any(|line| line.contains("kind=Rollback")));
        assert!(lines.iter().any(|line| line.contains("result=ok")));
    }

    #[test]
    fn dev_action_history_parse_accepts_wrapped_entries() {
        let value = json!({
            "history": [
                {
                    "timestamp_ms": 7,
                    "wiki_id": "frwiki",
                    "rev_id": 1,
                    "kind": "Patrol",
                    "actor": "Tester",
                    "status": "success",
                    "summary": "patrolled",
                    "result": "ok"
                }
            ]
        });

        let parsed = parse_dev_action_history(
            &serde_json::to_vec(&value).expect("history fixture should serialize"),
        )
        .expect("history should parse");

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].kind, "Patrol");
        assert_eq!(parsed[0].wiki_id, "frwiki");
    }
}
