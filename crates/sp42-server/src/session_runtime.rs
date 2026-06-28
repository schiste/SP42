use std::collections::HashMap;

use axum::{
    Json,
    http::{HeaderMap, HeaderValue, StatusCode, header::COOKIE},
};
use sp42_core::{
    DevAuthCapabilityReport, DevAuthSessionStatus, FlagState,
    routes::{AUTH_LOGIN_PATH, AUTH_LOGOUT_PATH},
};

use crate::http_errors::{forbidden_error, unauthorized_error};
use crate::runtime_status::DevAuthBootstrapStatus;
use crate::state::{AppState, PendingOAuthLogin, SessionSnapshot, StoredSession};

pub(crate) const SESSION_COOKIE_NAME: &str = "sp42_dev_session";
pub(crate) use sp42_core::routes::CSRF_HEADER_NAME;
pub(crate) const SESSION_IDLE_TIMEOUT_MS: i64 = 30 * 60 * 1000;
const SESSION_ABSOLUTE_TIMEOUT_MS: i64 = 8 * 60 * 60 * 1000;
// The cookie must live as long as the session possibly can — the absolute
// timeout, not the (sliding) idle timeout. Pinning it to the idle window expired
// the browser cookie 30 min after login even for an active user whose session
// the server keeps alive via touches, bouncing them to login. The server still
// enforces the finer sliding-idle policy; an idle-expired session just yields a
// cookie the server rejects. Codex review #90.
const SESSION_COOKIE_MAX_AGE_SECONDS: i64 = SESSION_ABSOLUTE_TIMEOUT_MS / 1000;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct OAuthSessionView {
    pub(crate) authenticated: FlagState,
    pub(crate) username: Option<String>,
    pub(crate) scopes: Vec<String>,
    pub(crate) expires_at_ms: Option<i64>,
    pub(crate) upstream_access_expires_at_ms: Option<i64>,
    pub(crate) refresh_available: FlagState,
    pub(crate) bridge_mode: String,
    pub(crate) csrf_token: Option<String>,
    pub(crate) local_token_available: FlagState,
    pub(crate) oauth_client_ready: FlagState,
    pub(crate) login_path: String,
    pub(crate) logout_path: String,
}

pub(crate) fn effective_session_scopes(report: &DevAuthCapabilityReport) -> Vec<String> {
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

pub(crate) fn auth_session_view_without_session(state: &AppState) -> OAuthSessionView {
    OAuthSessionView {
        authenticated: FlagState::Disabled,
        username: None,
        scopes: Vec::new(),
        expires_at_ms: None,
        upstream_access_expires_at_ms: None,
        refresh_available: FlagState::Disabled,
        bridge_mode: "inactive".to_string(),
        csrf_token: None,
        local_token_available: FlagState::from(state.shared_local_access_token().is_some()),
        oauth_client_ready: FlagState::from(state.local_oauth.has_confidential_oauth_client()),
        login_path: AUTH_LOGIN_PATH.to_string(),
        logout_path: AUTH_LOGOUT_PATH.to_string(),
    }
}

pub(crate) async fn auth_session_view(
    state: &AppState,
    headers: &HeaderMap,
    touch: bool,
) -> OAuthSessionView {
    match current_session_snapshot(state, headers, touch).await {
        Some(session) => OAuthSessionView {
            authenticated: FlagState::Enabled,
            username: Some(session.username),
            scopes: session.scopes,
            expires_at_ms: session.expires_at_ms,
            upstream_access_expires_at_ms: sessions_upstream_access_expiry(state, headers).await,
            refresh_available: FlagState::from(sessions_refresh_available(state, headers).await),
            bridge_mode: session.bridge_mode,
            csrf_token: Some(session.csrf_token),
            local_token_available: FlagState::from(state.shared_local_access_token().is_some()),
            oauth_client_ready: FlagState::from(state.local_oauth.has_confidential_oauth_client()),
            login_path: AUTH_LOGIN_PATH.to_string(),
            logout_path: AUTH_LOGOUT_PATH.to_string(),
        },
        None => auth_session_view_without_session(state),
    }
}

pub(crate) async fn store_pending_oauth_login(state: &AppState, pending: PendingOAuthLogin) {
    let mut pending_logins = state.pending_oauth_logins.write().await;
    prune_expired_pending_oauth_logins(&mut pending_logins, state.clock.now_ms());
    pending_logins.insert(pending.state.clone(), pending);
}

pub(crate) async fn take_pending_oauth_login(
    state: &AppState,
    state_token: &str,
) -> Option<PendingOAuthLogin> {
    let mut pending_logins = state.pending_oauth_logins.write().await;
    prune_expired_pending_oauth_logins(&mut pending_logins, state.clock.now_ms());
    pending_logins.remove(state_token)
}

pub(crate) fn prune_expired_pending_oauth_logins(
    pending_logins: &mut HashMap<String, PendingOAuthLogin>,
    current_time_ms: i64,
) {
    pending_logins.retain(|_, pending| pending.expires_at_ms > current_time_ms);
}

pub(crate) async fn install_session(
    state: &AppState,
    prior_session_id: Option<String>,
    stored: StoredSession,
    current_ms: i64,
) -> String {
    let session_id = next_session_id();
    let mut sessions = state.sessions.write().await;
    prune_expired_sessions(&mut sessions, current_ms);
    if let Some(prior_session_id) = prior_session_id {
        sessions.remove(&prior_session_id);
    }
    sessions.insert(session_id.clone(), stored);
    session_id
}

pub(crate) fn to_status(
    session: Option<&StoredSession>,
    local_token_available: bool,
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
        csrf_token: session.map(|entry| entry.csrf_token.clone()),
        // Callers pass `state.shared_local_access_token().is_some()` so this is
        // false outside local mode (the env token can't act as identity there).
        local_token_available,
    }
}

pub(crate) fn bootstrap_status(
    state: &AppState,
    auth: &DevAuthSessionStatus,
) -> DevAuthBootstrapStatus {
    let source_report = state.local_oauth.source_report();

    DevAuthBootstrapStatus {
        bootstrap_ready: state.shared_local_access_token().is_some(),
        oauth: state.local_oauth.status(),
        session: auth.clone(),
        source_path: source_report
            .loaded_from_source
            .then_some(source_report.file_name.clone()),
        source_report,
    }
}

pub(crate) fn session_cookie_value(headers: &HeaderMap) -> Option<String> {
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

pub(crate) fn next_session_id() -> String {
    // CSPRNG session identifier (256 bits). It gates the stored Wikimedia bearer
    // token, so it must be unguessable — never derive it from time/pid/counter,
    // which an attacker who can approximate the login time could reconstruct.
    // Codex review #90 (P1).
    use crate::runtime_adapters::ServerRng;
    use sp42_types::Rng as _;

    let mut rng = ServerRng;
    format!(
        "{:016x}{:016x}{:016x}{:016x}",
        rng.next_u64(),
        rng.next_u64(),
        rng.next_u64(),
        rng.next_u64()
    )
}

pub(crate) fn session_cookie_header(state: &AppState, session_id: &str) -> Option<HeaderValue> {
    session_cookie_header_value(state, session_id, SESSION_COOKIE_MAX_AGE_SECONDS)
}

pub(crate) fn expired_session_cookie_header(state: &AppState) -> HeaderValue {
    session_cookie_header_value(state, "deleted", 0).unwrap_or_else(|| {
        HeaderValue::from_static(
            "sp42_dev_session=deleted; HttpOnly; SameSite=Lax; Path=/; Max-Age=0",
        )
    })
}

fn session_cookie_header_value(
    state: &AppState,
    session_id: &str,
    max_age_seconds: i64,
) -> Option<HeaderValue> {
    let same_site = state.deployment.mode.session_cookie_same_site();
    // Browsers only honor `SameSite=None` when it is paired with `Secure`, so
    // force `Secure` for the cross-site modes regardless of `uses_secure_cookies`
    // (loopback/localhost is a secure context in the webview). Codex review #90.
    let secure = if same_site == "None" || state.deployment.mode.uses_secure_cookies() {
        "; Secure"
    } else {
        ""
    };
    HeaderValue::from_str(&format!(
        "{SESSION_COOKIE_NAME}={session_id}; HttpOnly; SameSite={same_site}; Path=/; Max-Age={max_age_seconds}{secure}"
    ))
    .ok()
}

pub(crate) fn session_expires_at_ms(session: &StoredSession, current_time_ms: i64) -> i64 {
    let idle_deadline = session
        .last_seen_at_ms
        .saturating_add(SESSION_IDLE_TIMEOUT_MS);
    let absolute_deadline = session
        .created_at_ms
        .saturating_add(SESSION_ABSOLUTE_TIMEOUT_MS);
    let mut deadline = idle_deadline.min(absolute_deadline);
    // The session cannot outlive the upstream OAuth access token: once it
    // expires, every wiki API call fails with an expired bearer while the user
    // still looks authenticated. Cap the session at that deadline so it re-gates
    // instead (token refresh is out of scope, ADR-0014). Codex review #90.
    if let Some(upstream) = session.upstream_access_expires_at_ms {
        deadline = deadline.min(upstream);
    }
    deadline.max(current_time_ms)
}

pub(crate) fn session_is_expired(session: &StoredSession, current_time_ms: i64) -> bool {
    current_time_ms >= session_expires_at_ms(session, current_time_ms)
}

pub(crate) fn prune_expired_sessions(
    sessions: &mut HashMap<String, StoredSession>,
    current_time_ms: i64,
) {
    sessions.retain(|_, session| !session_is_expired(session, current_time_ms));
}

pub(crate) async fn current_session_snapshot(
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
        csrf_token: session.csrf_token.clone(),
    })
}

pub(crate) async fn sessions_upstream_access_expiry(
    state: &AppState,
    headers: &HeaderMap,
) -> Option<i64> {
    let session_id = session_cookie_value(headers)?;
    let sessions = state.sessions.read().await;
    sessions
        .get(&session_id)
        .and_then(|session| session.upstream_access_expires_at_ms)
}

pub(crate) async fn sessions_refresh_available(state: &AppState, headers: &HeaderMap) -> bool {
    let Some(session_id) = session_cookie_value(headers) else {
        return false;
    };
    let sessions = state.sessions.read().await;
    sessions
        .get(&session_id)
        .and_then(|session| session.refresh_token.as_ref())
        .is_some_and(|token| !token.is_empty())
}

pub(crate) async fn current_status(
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
            csrf_token: Some(session.csrf_token),
            local_token_available: state.shared_local_access_token().is_some(),
        },
        None => to_status(
            None,
            state.shared_local_access_token().is_some(),
            state.clock.now_ms(),
        ),
    }
}
