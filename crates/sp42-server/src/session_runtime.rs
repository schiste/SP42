use std::{collections::HashMap, sync::atomic::Ordering};

use axum::http::{
    header::COOKIE,
    HeaderMap, HeaderValue,
};

use crate::{
    AppState, DevAuthBootstrapStatus, DevAuthCapabilityReport, DevAuthSessionStatus, FlagState,
    LocalOAuthConfig, OAuthSessionView, PendingOAuthLogin, SessionSnapshot, StoredSession,
    AUTH_LOGIN_PATH, AUTH_LOGOUT_PATH, SESSION_ABSOLUTE_TIMEOUT_MS,
    SESSION_COOKIE_MAX_AGE_SECONDS, SESSION_COOKIE_NAME, SESSION_IDLE_TIMEOUT_MS,
};

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

pub(crate) fn auth_session_view_without_session(state: &AppState) -> OAuthSessionView {
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
            local_token_available: FlagState::from(state.local_oauth.access_token().is_some()),
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
    let session_id = next_session_id(state, current_ms);
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

pub(crate) fn bootstrap_status(
    state: &AppState,
    auth: &DevAuthSessionStatus,
) -> DevAuthBootstrapStatus {
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

pub(crate) fn next_session_id(state: &AppState, current_ms: i64) -> String {
    let counter = state.next_session_id.fetch_add(1, Ordering::Relaxed);
    format!(
        "{:016x}{:016x}{:08x}",
        u64::try_from(current_ms).unwrap_or(u64::MAX),
        counter,
        std::process::id()
    )
}

pub(crate) fn session_cookie_header(session_id: &str) -> Option<HeaderValue> {
    HeaderValue::from_str(&format!(
        "{SESSION_COOKIE_NAME}={session_id}; HttpOnly; SameSite=Lax; Path=/; Max-Age={SESSION_COOKIE_MAX_AGE_SECONDS}"
    ))
    .ok()
}

pub(crate) fn expired_session_cookie_header() -> HeaderValue {
    HeaderValue::from_static("sp42_dev_session=deleted; HttpOnly; SameSite=Lax; Path=/; Max-Age=0")
}

pub(crate) fn session_expires_at_ms(session: &StoredSession, current_time_ms: i64) -> i64 {
    let idle_deadline = session
        .last_seen_at_ms
        .saturating_add(SESSION_IDLE_TIMEOUT_MS);
    let absolute_deadline = session
        .created_at_ms
        .saturating_add(SESSION_ABSOLUTE_TIMEOUT_MS);
    let deadline = idle_deadline.min(absolute_deadline);
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
            local_token_available: state.local_oauth.access_token().is_some(),
        },
        None => to_status(None, &state.local_oauth, state.clock.now_ms()),
    }
}
