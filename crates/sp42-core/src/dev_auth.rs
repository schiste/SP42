//! Development-only browser auth bridge contracts.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::action_executor::SessionActionKind;
use crate::errors::DevAuthError;
use crate::types::{HttpMethod, HttpRequest};

pub const DEV_AUTH_DEFAULT_BASE_URL: &str = "http://127.0.0.1:8788";
pub const DEV_AUTH_SESSION_PATH: &str = "/dev/auth/session";
pub const DEV_AUTH_BOOTSTRAP_SESSION_PATH: &str = "/dev/auth/session/bootstrap";
pub const DEV_AUTH_ACTION_STATUS_PATH: &str = "/dev/actions/status";
pub const DEV_AUTH_ACTION_HISTORY_PATH: &str = "/dev/actions/history";

/// Compatibility payload for the local dev-auth bootstrap POST.
///
/// The server derives identity, effective scopes, and expiry from the
/// Wikimedia token stored in `.env.wikimedia.local`, so the serialized request
/// body is intentionally empty. These fields remain as ignored compatibility
/// shims for older call sites and never cross the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DevAuthBootstrapRequest {
    #[serde(default, skip_serializing)]
    pub username: String,
    #[serde(default, skip_serializing)]
    pub scopes: Vec<String>,
    #[serde(default, skip_serializing)]
    pub expires_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevAuthSessionStatus {
    pub authenticated: bool,
    pub username: Option<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
    pub expires_at_ms: Option<i64>,
    pub token_present: bool,
    pub bridge_mode: String,
    #[serde(default)]
    pub local_token_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LocalOAuthConfigStatus {
    pub client_id_present: bool,
    pub client_secret_present: bool,
    pub access_token_present: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DevAuthCapabilityReadiness {
    pub can_authenticate: bool,
    pub can_query_userinfo: bool,
    pub can_read_recent_changes: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DevAuthEditCapabilities {
    pub can_edit: bool,
    pub can_undo: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DevAuthModerationCapabilities {
    pub can_patrol: bool,
    pub can_rollback: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DevAuthDerivedCapabilities {
    #[serde(flatten)]
    pub read: DevAuthCapabilityReadiness,
    #[serde(flatten)]
    pub editing: DevAuthEditCapabilities,
    #[serde(flatten)]
    pub moderation: DevAuthModerationCapabilities,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DevAuthProbeAcceptance {
    pub profile_accepted: bool,
    pub userinfo_accepted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DevAuthActionTokenAvailability {
    pub csrf_token_available: bool,
    pub patrol_token_available: bool,
    pub rollback_token_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DevAuthCapabilityReport {
    pub checked: bool,
    pub wiki_id: String,
    pub username: Option<String>,
    #[serde(default)]
    pub oauth_grants: Vec<String>,
    #[serde(default)]
    pub wiki_groups: Vec<String>,
    #[serde(default)]
    pub wiki_rights: Vec<String>,
    #[serde(flatten)]
    pub acceptance: DevAuthProbeAcceptance,
    #[serde(flatten)]
    pub token_availability: DevAuthActionTokenAvailability,
    pub capabilities: DevAuthDerivedCapabilities,
    #[serde(default)]
    pub notes: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionExecutionLogEntry {
    pub executed_at_ms: i64,
    pub wiki_id: String,
    pub kind: SessionActionKind,
    pub rev_id: u64,
    pub title: Option<String>,
    pub target_user: Option<String>,
    pub summary: Option<String>,
    pub accepted: bool,
    pub http_status: Option<u16>,
    pub api_code: Option<String>,
    #[serde(default)]
    pub retryable: bool,
    #[serde(default)]
    pub warnings: Vec<String>,
    pub result: Option<String>,
    pub response_preview: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionExecutionStatusReport {
    pub authenticated: bool,
    pub session_id: Option<String>,
    pub username: Option<String>,
    pub total_actions: usize,
    #[serde(default)]
    pub successful_actions: usize,
    #[serde(default)]
    pub failed_actions: usize,
    #[serde(default)]
    pub retryable_failures: usize,
    pub last_execution: Option<ActionExecutionLogEntry>,
    #[serde(default)]
    pub shell_feedback: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionExecutionHistoryReport {
    pub authenticated: bool,
    pub session_id: Option<String>,
    pub username: Option<String>,
    #[serde(default)]
    pub entries: Vec<ActionExecutionLogEntry>,
}

/// Build a localhost-only request that clears the development auth session.
///
/// # Errors
///
/// Returns [`DevAuthError`] when the base URL is invalid.
pub fn build_dev_auth_clear_request(base_url: &str) -> Result<HttpRequest, DevAuthError> {
    Ok(HttpRequest {
        method: HttpMethod::Delete,
        url: session_url(base_url)?,
        headers: BTreeMap::new(),
        body: Vec::new(),
    })
}

/// Build a localhost-only request that installs a dev session from the server's
/// local `.env.wikimedia.local` token.
///
/// The wire payload is canonicalized to `{}` because the server derives the
/// authenticated username, effective scopes, and expiry from the token itself.
///
/// # Errors
///
/// Returns [`DevAuthError`] when the base URL or payload is invalid.
pub fn build_dev_auth_bootstrap_request(
    base_url: &str,
    request: &DevAuthBootstrapRequest,
) -> Result<HttpRequest, DevAuthError> {
    let mut endpoint = Url::parse(base_url).map_err(|error| DevAuthError::InvalidConfig {
        message: error.to_string(),
    })?;
    endpoint = endpoint
        .join(DEV_AUTH_BOOTSTRAP_SESSION_PATH)
        .map_err(|error| DevAuthError::InvalidConfig {
            message: error.to_string(),
        })?;

    let body = serde_json::to_vec(request).map_err(|error| DevAuthError::InvalidPayload {
        message: error.to_string(),
    })?;

    Ok(HttpRequest {
        method: HttpMethod::Post,
        url: endpoint,
        headers: json_headers(),
        body,
    })
}

/// Parse a development auth bridge status response body.
///
/// # Errors
///
/// Returns [`DevAuthError`] when the payload is not a valid session status.
pub fn parse_dev_auth_status(bytes: &[u8]) -> Result<DevAuthSessionStatus, DevAuthError> {
    serde_json::from_slice(bytes).map_err(|error| DevAuthError::InvalidPayload {
        message: error.to_string(),
    })
}

/// Parse a development auth bridge action status response body.
///
/// # Errors
///
/// Returns [`DevAuthError`] when the payload is not a valid action status report.
pub fn parse_action_execution_status(
    bytes: &[u8],
) -> Result<ActionExecutionStatusReport, DevAuthError> {
    serde_json::from_slice(bytes).map_err(|error| DevAuthError::InvalidPayload {
        message: error.to_string(),
    })
}

/// Parse a development auth bridge action history response body.
///
/// # Errors
///
/// Returns [`DevAuthError`] when the payload is not a valid action history report.
pub fn parse_action_execution_history(
    bytes: &[u8],
) -> Result<ActionExecutionHistoryReport, DevAuthError> {
    serde_json::from_slice(bytes).map_err(|error| DevAuthError::InvalidPayload {
        message: error.to_string(),
    })
}

fn session_url(base_url: &str) -> Result<Url, DevAuthError> {
    let base = Url::parse(base_url).map_err(|error| DevAuthError::InvalidConfig {
        message: error.to_string(),
    })?;
    base.join(DEV_AUTH_SESSION_PATH)
        .map_err(|error| DevAuthError::InvalidConfig {
            message: error.to_string(),
        })
}

fn json_headers() -> BTreeMap<String, String> {
    BTreeMap::from([("content-type".to_string(), "application/json".to_string())])
}

#[cfg(test)]
mod tests {
    use super::{
        ActionExecutionHistoryReport, DEV_AUTH_BOOTSTRAP_SESSION_PATH, DEV_AUTH_DEFAULT_BASE_URL,
        DevAuthBootstrapRequest, DevAuthSessionStatus, build_dev_auth_bootstrap_request,
        build_dev_auth_clear_request, parse_action_execution_history,
        parse_action_execution_status, parse_dev_auth_status,
    };
    use crate::action_executor::SessionActionKind;
    use crate::types::HttpMethod;

    #[test]
    fn builds_clear_request() {
        let request =
            build_dev_auth_clear_request(DEV_AUTH_DEFAULT_BASE_URL).expect("request should build");

        assert_eq!(request.method, HttpMethod::Delete);
        assert!(request.url.as_str().ends_with("/dev/auth/session"));
    }

    #[test]
    fn builds_bootstrap_request() {
        let request = build_dev_auth_bootstrap_request(
            DEV_AUTH_DEFAULT_BASE_URL,
            &DevAuthBootstrapRequest::default(),
        )
        .expect("request should build");

        assert_eq!(request.method, HttpMethod::Post);
        assert!(
            request
                .url
                .as_str()
                .ends_with(DEV_AUTH_BOOTSTRAP_SESSION_PATH)
        );
        assert_eq!(request.body, b"{}".to_vec());
    }

    #[test]
    fn bootstrap_request_ignores_legacy_fields_when_serializing() {
        let request = build_dev_auth_bootstrap_request(
            DEV_AUTH_DEFAULT_BASE_URL,
            &DevAuthBootstrapRequest {
                username: "Ignored".to_string(),
                scopes: vec!["rollback".to_string()],
                expires_at_ms: Some(42),
            },
        )
        .expect("request should build");

        assert_eq!(request.body, b"{}".to_vec());
    }

    #[test]
    fn parses_session_status() {
        let status = parse_dev_auth_status(
            br#"{
                "authenticated": true,
                "username": "Example",
                "scopes": ["rollback"],
                "expires_at_ms": 42,
                "token_present": true,
                "bridge_mode": "local-env-token",
                "local_token_available": true
            }"#,
        )
        .expect("status should parse");

        assert_eq!(
            status,
            DevAuthSessionStatus {
                authenticated: true,
                username: Some("Example".to_string()),
                scopes: vec!["rollback".to_string()],
                expires_at_ms: Some(42),
                token_present: true,
                bridge_mode: "local-env-token".to_string(),
                local_token_available: true,
            }
        );
    }

    #[test]
    fn parses_action_status_report() {
        let report = parse_action_execution_status(
            br#"{
                "authenticated": true,
                "session_id": "session-1",
                "username": "Example",
                "total_actions": 2,
                "successful_actions": 1,
                "failed_actions": 1,
                "retryable_failures": 1,
                "last_execution": {
                    "executed_at_ms": 42,
                    "wiki_id": "frwiki",
                    "kind": "Patrol",
                    "rev_id": 123,
                    "title": "Example",
                    "target_user": null,
                    "summary": "review selected item",
                    "accepted": true,
                    "http_status": 200,
                    "api_code": null,
                    "retryable": false,
                    "warnings": [],
                    "result": "patrol=true",
                    "response_preview": "ok",
                    "error": null
                },
                "shell_feedback": ["2 action(s) recorded in this shell session."]
            }"#,
        )
        .expect("status report should parse");

        assert_eq!(report.total_actions, 2);
        assert_eq!(report.successful_actions, 1);
        assert_eq!(report.failed_actions, 1);
        assert_eq!(report.retryable_failures, 1);
        assert_eq!(
            report.last_execution.expect("entry should be present").kind,
            SessionActionKind::Patrol
        );
    }

    #[test]
    fn parses_action_history_report() {
        let report = parse_action_execution_history(
            br#"{
                "authenticated": true,
                "session_id": "session-1",
                "username": "Example",
                "entries": [
                    {
                        "executed_at_ms": 42,
                        "wiki_id": "frwiki",
                        "kind": "Rollback",
                        "rev_id": 123,
                        "title": "Example",
                        "target_user": "192.0.2.1",
                        "summary": "review selected item",
                        "accepted": false,
                        "http_status": 403,
                        "api_code": "badtoken",
                        "retryable": false,
                        "warnings": [],
                        "result": null,
                        "response_preview": null,
                        "error": "forbidden"
                    }
                ]
            }"#,
        )
        .expect("history report should parse");

        assert_eq!(
            report,
            ActionExecutionHistoryReport {
                authenticated: true,
                session_id: Some("session-1".to_string()),
                username: Some("Example".to_string()),
                entries: vec![super::ActionExecutionLogEntry {
                    executed_at_ms: 42,
                    wiki_id: "frwiki".to_string(),
                    kind: SessionActionKind::Rollback,
                    rev_id: 123,
                    title: Some("Example".to_string()),
                    target_user: Some("192.0.2.1".to_string()),
                    summary: Some("review selected item".to_string()),
                    accepted: false,
                    http_status: Some(403),
                    api_code: Some("badtoken".to_string()),
                    retryable: false,
                    warnings: Vec::new(),
                    result: None,
                    response_preview: None,
                    error: Some("forbidden".to_string()),
                }],
            }
        );
    }
}
