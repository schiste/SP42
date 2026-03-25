use serde_json::Value;
use sp42_core::traits::Rng;
use sp42_core::{
    ActionExecutionHistoryReport, ActionExecutionStatusReport, DEV_AUTH_ACTION_HISTORY_PATH,
    DEV_AUTH_ACTION_STATUS_PATH, DEV_AUTH_BOOTSTRAP_SESSION_PATH, DEV_AUTH_DEFAULT_BASE_URL,
    DEV_AUTH_SESSION_PATH, DevAuthBootstrapRequest, DevAuthSessionStatus, OAuthCallback,
    OAuthClientConfig, SessionActionExecutionRequest, SessionActionExecutionResponse,
    parse_action_execution_history, parse_action_execution_status, parse_callback_query,
    parse_dev_auth_status, prepare_oauth_launch,
};
use url::Url;

#[cfg(target_arch = "wasm32")]
use super::{
    globals,
    http::{delete_bytes, get_bytes, post_json_bytes},
};

const DEV_AUTH_BOOTSTRAP_STATUS_PATH: &str = "/dev/auth/bootstrap/status";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserAuthPreview {
    pub redirect_uri: String,
    pub authorization_url: Option<String>,
    pub dev_bridge_url: String,
    pub callback_preview: String,
    pub launch_state_preview: String,
    pub notes: Vec<String>,
}

pub fn preview_browser_auth() -> BrowserAuthPreview {
    let config = OAuthClientConfig {
        client_id: configured_client_id(),
        authorize_url: Url::parse("https://meta.wikimedia.org/w/rest.php/oauth2/authorize")
            .expect("authorize url should parse"),
        token_url: Url::parse("https://meta.wikimedia.org/w/rest.php/oauth2/access_token")
            .expect("token url should parse"),
        redirect_uri: Url::parse(&configured_redirect_uri()).expect("redirect uri should parse"),
        scopes: vec![
            "basic".to_string(),
            "editpage".to_string(),
            "patrol".to_string(),
            "rollback".to_string(),
        ],
    };
    let mut rng = PreviewRng::default();
    let launch = prepare_oauth_launch(&config, &mut rng).ok();
    let authorization_url = launch
        .as_ref()
        .map(|launch| launch.authorization_url.to_string());

    BrowserAuthPreview {
        redirect_uri: config.redirect_uri.to_string(),
        authorization_url,
        dev_bridge_url: format!("{DEV_AUTH_DEFAULT_BASE_URL}{DEV_AUTH_BOOTSTRAP_SESSION_PATH}"),
        callback_preview: current_callback_preview(),
        launch_state_preview: launch
            .map(|launch| {
                format!(
                    "state={}, verifier_len={}",
                    launch.state,
                    launch.verifier.len()
                )
            })
            .unwrap_or_else(|| "OAuth launch context unavailable.".to_string()),
        notes: build_notes(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevAuthBootstrapStatus {
    pub bootstrap_ready: bool,
    pub oauth: sp42_core::LocalOAuthConfigStatus,
    pub session: DevAuthSessionStatus,
    pub source_path: Option<String>,
}

#[must_use]
pub fn preview_local_oauth_config_status() -> sp42_core::LocalOAuthConfigStatus {
    sp42_core::LocalOAuthConfigStatus::default()
}

#[must_use]
pub fn preview_dev_auth_session_status() -> DevAuthSessionStatus {
    DevAuthSessionStatus {
        authenticated: false,
        username: None,
        scopes: Vec::new(),
        expires_at_ms: None,
        token_present: false,
        bridge_mode: "browser-preview".to_string(),
        local_token_available: false,
    }
}

#[must_use]
pub fn preview_dev_auth_bootstrap_status() -> DevAuthBootstrapStatus {
    DevAuthBootstrapStatus {
        bootstrap_ready: false,
        oauth: preview_local_oauth_config_status(),
        session: preview_dev_auth_session_status(),
        source_path: None,
    }
}

fn build_notes() -> Vec<String> {
    let mut notes = vec![
        "Tokens must stay in memory only; no localStorage or persisted browser cache.".to_string(),
        "The localhost bridge installs a dev session from .env.wikimedia.local with an empty bootstrap payload; the server derives username, scopes, and expiry from the token and keeps the raw token server-side.".to_string(),
        "The browser client_id is used only to build OAuth URLs; the live token handoff stays on the localhost bridge.".to_string(),
    ];

    notes.insert(
        0,
        "The browser client_id is read from the build environment when available; otherwise a local-dev fallback is used for browser-side URL generation.".to_string(),
    );

    notes
}

fn configured_client_id() -> String {
    option_env!("SP42_WIKIMEDIA_CLIENT_APPLICATION_KEY")
        .unwrap_or("local-dev-client-id")
        .to_string()
}

fn configured_redirect_uri() -> String {
    option_env!("SP42_WIKIMEDIA_OAUTH_CALLBACK_URL")
        .unwrap_or("http://localhost:4173/oauth/callback")
        .to_string()
}

#[cfg(target_arch = "wasm32")]
pub async fn fetch_dev_auth_session_status() -> Result<DevAuthSessionStatus, String> {
    let bytes = get_bytes(
        &format!("{DEV_AUTH_DEFAULT_BASE_URL}{DEV_AUTH_SESSION_PATH}"),
        "fetch dev auth session status",
    )
    .await?;
    parse_dev_auth_status(&bytes).map_err(|error| error.to_string())
}

#[cfg(target_arch = "wasm32")]
pub async fn fetch_dev_auth_bootstrap_status() -> Result<DevAuthBootstrapStatus, String> {
    let bytes = get_bytes(
        &format!("{DEV_AUTH_DEFAULT_BASE_URL}{DEV_AUTH_BOOTSTRAP_STATUS_PATH}"),
        "fetch dev auth bootstrap status",
    )
    .await?;
    parse_bootstrap_status(&bytes)
}

#[cfg(target_arch = "wasm32")]
pub async fn bootstrap_dev_auth_session(
    request: &DevAuthBootstrapRequest,
) -> Result<DevAuthSessionStatus, String> {
    let body = serde_json::to_string(request).map_err(|error| error.to_string())?;
    let bytes = post_json_bytes(
        &format!("{DEV_AUTH_DEFAULT_BASE_URL}{DEV_AUTH_BOOTSTRAP_SESSION_PATH}"),
        body,
        "bootstrap dev auth session",
    )
    .await?;
    parse_dev_auth_status(&bytes).map_err(|error| error.to_string())
}

#[cfg(target_arch = "wasm32")]
pub async fn execute_dev_auth_action(
    request: &SessionActionExecutionRequest,
) -> Result<SessionActionExecutionResponse, String> {
    let body = serde_json::to_string(request).map_err(|error| error.to_string())?;
    let bytes = post_json_bytes(
        &format!("{DEV_AUTH_DEFAULT_BASE_URL}/dev/actions/execute"),
        body,
        "execute dev auth action",
    )
    .await?;
    serde_json::from_slice(&bytes).map_err(|error| error.to_string())
}

#[cfg(target_arch = "wasm32")]
pub async fn fetch_dev_auth_action_status() -> Result<ActionExecutionStatusReport, String> {
    let bytes = get_bytes(
        &format!("{DEV_AUTH_DEFAULT_BASE_URL}{DEV_AUTH_ACTION_STATUS_PATH}"),
        "fetch dev auth action status",
    )
    .await?;
    parse_action_execution_status(&bytes).map_err(|error| error.to_string())
}

#[cfg(target_arch = "wasm32")]
pub async fn fetch_dev_auth_action_history(
    limit: usize,
) -> Result<ActionExecutionHistoryReport, String> {
    let bytes = get_bytes(
        &format!(
            "{DEV_AUTH_DEFAULT_BASE_URL}{DEV_AUTH_ACTION_HISTORY_PATH}?limit={}",
            limit.max(1)
        ),
        "fetch dev auth action history",
    )
    .await?;
    parse_action_execution_history(&bytes).map_err(|error| error.to_string())
}

#[cfg(target_arch = "wasm32")]
pub async fn clear_dev_auth_session() -> Result<DevAuthSessionStatus, String> {
    let bytes = delete_bytes(
        &format!("{DEV_AUTH_DEFAULT_BASE_URL}{DEV_AUTH_SESSION_PATH}"),
        "clear dev auth session",
    )
    .await?;
    parse_dev_auth_status(&bytes).map_err(|error| error.to_string())
}

#[cfg(target_arch = "wasm32")]
fn current_callback_preview() -> String {
    let Ok(Some(search)) = globals::browser_location_search() else {
        return "OAuth callback query not available yet.".to_string();
    };
    if search.is_empty() {
        return "OAuth callback query not present in the current URL.".to_string();
    }

    callback_preview(&search)
}

#[cfg(not(target_arch = "wasm32"))]
fn current_callback_preview() -> String {
    "OAuth callback preview is only available in the browser runtime.".to_string()
}

fn mask_code(code: &str) -> String {
    let visible = code.chars().take(6).collect::<String>();
    format!("{visible}...")
}

#[must_use]
pub fn callback_preview(search: &str) -> String {
    match parse_callback_query(search) {
        Ok(OAuthCallback::AuthorizationCode { code, state }) => {
            format!(
                "OAuth callback detected: code={}, state={state}",
                mask_code(&code)
            )
        }
        Ok(OAuthCallback::AuthorizationError {
            error,
            error_description,
            state,
        }) => format!(
            "OAuth callback error: error={error}, state={}, description={}",
            state.unwrap_or_else(|| "missing".to_string()),
            error_description.unwrap_or_else(|| "missing".to_string())
        ),
        Err(error) => format!("OAuth callback parse error: {error}"),
    }
}

#[must_use]
pub fn bootstrap_status_lines(status: &DevAuthBootstrapStatus) -> Vec<String> {
    vec![
        format!("bootstrap_ready={}", status.bootstrap_ready),
        format!("client_id_present={}", status.oauth.client_id_present),
        format!(
            "client_secret_present={}",
            status.oauth.client_secret_present
        ),
        format!("access_token_present={}", status.oauth.access_token_present),
        format!("authenticated={}", status.session.authenticated),
        format!(
            "local_token_available={}",
            status.session.local_token_available
        ),
        format!("bootstrap_source_loaded={}", status.source_path.is_some()),
        format!(
            "source_path={}",
            status.source_path.as_deref().unwrap_or("not configured")
        ),
    ]
}

#[must_use]
pub fn dev_auth_session_lines(status: &DevAuthSessionStatus) -> Vec<String> {
    vec![
        format!("authenticated={}", status.authenticated),
        format!(
            "username={}",
            status.username.as_deref().unwrap_or("anonymous")
        ),
        format!("token_present={}", status.token_present),
        format!("bridge_mode={}", status.bridge_mode),
        format!("local_token_available={}", status.local_token_available),
    ]
}

fn parse_bootstrap_status(bytes: &[u8]) -> Result<DevAuthBootstrapStatus, String> {
    let value: Value = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    parse_bootstrap_status_value(value)
}

pub(crate) fn parse_bootstrap_status_value(value: Value) -> Result<DevAuthBootstrapStatus, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "bootstrap status response must be a JSON object".to_string())?;

    let bootstrap_ready = object
        .get("bootstrap_ready")
        .and_then(Value::as_bool)
        .ok_or_else(|| "bootstrap_ready field is missing or invalid".to_string())?;

    let oauth_value = object
        .get("oauth")
        .ok_or_else(|| "oauth field is missing".to_string())?
        .clone();
    let oauth = serde_json::from_value(oauth_value).map_err(|error| error.to_string())?;

    let session_value = object
        .get("session")
        .ok_or_else(|| "session field is missing".to_string())?
        .clone();
    let session = serde_json::from_value(session_value).map_err(|error| error.to_string())?;

    let source_path = match object.get("source_path") {
        Some(Value::String(value)) => Some(value.clone()),
        Some(Value::Null) | None => None,
        Some(_) => return Err("source_path field is invalid".to_string()),
    };

    Ok(DevAuthBootstrapStatus {
        bootstrap_ready,
        oauth,
        session,
        source_path,
    })
}

#[derive(Debug, Default)]
struct PreviewRng {
    next: u64,
}

impl Rng for PreviewRng {
    fn next_u64(&mut self) -> u64 {
        let value = self.next;
        self.next = self.next.saturating_add(1);
        value
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DevAuthBootstrapStatus, bootstrap_status_lines, callback_preview, configured_redirect_uri,
        preview_browser_auth,
    };
    use sp42_core::{DevAuthSessionStatus, LocalOAuthConfigStatus};

    #[test]
    fn preview_contains_redirect_uri() {
        let preview = preview_browser_auth();

        assert_eq!(preview.redirect_uri, configured_redirect_uri());
        assert!(preview.authorization_url.is_some());
        assert_eq!(
            preview.dev_bridge_url,
            "http://127.0.0.1:8788/dev/auth/session/bootstrap"
        );
        assert!(!preview.callback_preview.is_empty());
        assert!(preview.launch_state_preview.contains("verifier_len="));
        assert_eq!(preview.notes.len(), 3);
    }

    #[test]
    fn callback_preview_masks_codes() {
        let preview = callback_preview("?code=supersecret&state=abc");

        assert!(preview.contains("code=super..."));
        assert!(preview.contains("state=abc"));
    }

    #[test]
    fn bootstrap_status_lines_include_the_core_fields() {
        let lines = bootstrap_status_lines(&DevAuthBootstrapStatus {
            bootstrap_ready: true,
            oauth: LocalOAuthConfigStatus {
                client_id_present: true,
                client_secret_present: false,
                access_token_present: true,
            },
            session: DevAuthSessionStatus {
                authenticated: true,
                username: Some("Tester".to_string()),
                scopes: vec!["rollback".to_string()],
                expires_at_ms: None,
                token_present: true,
                bridge_mode: "local".to_string(),
                local_token_available: true,
            },
            source_path: Some(".env.wikimedia.local".to_string()),
        });

        assert!(
            lines
                .iter()
                .any(|line| line.contains("bootstrap_ready=true"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("client_id_present=true"))
        );
        assert!(lines.iter().any(|line| line.contains("authenticated=true")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("source_path=.env.wikimedia.local"))
        );
    }

    #[test]
    fn parse_bootstrap_status_value_accepts_the_server_shape() {
        let status = super::parse_bootstrap_status_value(serde_json::json!({
            "bootstrap_ready": true,
            "oauth": {
                "client_id_present": true,
                "client_secret_present": true,
                "access_token_present": true
            },
            "session": {
                "authenticated": true,
                "username": "Schiste",
                "scopes": ["basic", "editpage", "patrol"],
                "expires_at_ms": 123,
                "token_present": true,
                "bridge_mode": "local-env-token",
                "local_token_available": true
            },
            "source_path": ".env.wikimedia.local"
        }))
        .expect("bootstrap status should parse");

        assert!(status.bootstrap_ready);
        assert_eq!(status.session.username.as_deref(), Some("Schiste"));
        assert_eq!(status.source_path.as_deref(), Some(".env.wikimedia.local"));
    }

    #[test]
    fn parse_bootstrap_status_value_rejects_invalid_objects() {
        let err = super::parse_bootstrap_status_value(serde_json::json!({
            "bootstrap_ready": true,
            "oauth": {},
            "session": {},
            "source_path": null
        }))
        .expect_err("payload should be rejected");

        assert!(err.contains("client_id_present") || err.contains("authenticated"));
    }
}
