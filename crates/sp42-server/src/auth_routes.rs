use std::collections::HashMap;

use axum::{
    extract::{OriginalUri, Path, Query, State},
    http::{
        header::SET_COOKIE,
        HeaderMap, StatusCode,
    },
    response::{IntoResponse, Redirect, Response},
    Json,
};
use tracing::info;

use crate::{
    auth_session_view, auth_session_view_without_session, bootstrap_status,
    build_authorization_url, capability_report_for_local_token, capability_report_for_request,
    current_status, effective_session_scopes, exchange_authorization_code,
    expired_session_cookie_header, fetch_oauth_profile, generate_oauth_state,
    generate_pkce_verifier, install_session, internal_error, invalid_payload, next_session_id,
    oauth_client_config_for_request, oauth_client_config_from_pending, parse_callback_query,
    prune_expired_sessions, probe_with_targets, redirect_with_status, runtime_debug,
    sanitize_redirect_target, session_cookie_header, session_cookie_value, split_scope_string,
    store_pending_oauth_login, take_pending_oauth_login, to_status, validate_bootstrap_payload,
    AppState, AuthLoginQuery, CachedCapabilityReport, DevAuthBootstrapRequest,
    DevAuthBootstrapStatus, DevAuthCapabilityReport, OAuthCallback, OAuthSessionView,
    PendingOAuthLogin, RuntimeDebugStatus, ServerRng, StoredSession, PENDING_OAUTH_TTL_MS,
    SESSION_IDLE_TIMEOUT_MS,
};

pub(crate) async fn get_runtime_debug(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Json<RuntimeDebugStatus> {
    Json(runtime_debug(&state, &headers).await)
}

pub(crate) async fn get_auth_login(
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
    let now = state.clock.now_ms();
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

pub(crate) fn oauth_redirect_target(pending: Option<&PendingOAuthLogin>) -> String {
    pending.map_or_else(
        || "/".to_string(),
        |entry| entry.redirect_after_login.clone(),
    )
}

pub(crate) fn oauth_error_redirect_response(
    pending: Option<&PendingOAuthLogin>,
    message: &str,
) -> Response {
    Redirect::temporary(&redirect_with_status(
        &oauth_redirect_target(pending),
        "auth_error",
        message,
    ))
    .into_response()
}

pub(crate) async fn complete_auth_callback(
    state: &AppState,
    headers: &HeaderMap,
    pending: PendingOAuthLogin,
    code: String,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    let oauth_config = oauth_client_config_from_pending(state, &pending)?;
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
    let current_ms = state.clock.now_ms();
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
        install_session(state, session_cookie_value(headers), stored, current_ms).await;

    let cookie = session_cookie_header(&session_id)
        .ok_or_else(|| internal_error("failed to build session cookie header"))?;

    Ok((
        [(SET_COOKIE, cookie)],
        Redirect::temporary(&redirect_with_status(
            &pending.redirect_after_login,
            "auth",
            "oauth-ok",
        )),
    )
        .into_response())
}

pub(crate) async fn get_auth_callback(
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
        } => Ok(oauth_error_redirect_response(
            pending.as_ref(),
            &error_description.unwrap_or(error),
        )),
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
            complete_auth_callback(&state, &headers, pending, code).await
        }
    }
}

pub(crate) async fn get_auth_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Json<OAuthSessionView> {
    Json(auth_session_view(&state, &headers, true).await)
}

pub(crate) async fn post_auth_logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    if let Some(session_id) = session_cookie_value(&headers) {
        let mut sessions = state.sessions.write().await;
        sessions.remove(&session_id);
    }

    Ok((
        StatusCode::OK,
        [(SET_COOKIE, expired_session_cookie_header())],
        Json(auth_session_view_without_session(&state)),
    ))
}

pub(crate) async fn get_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    Json(current_status(&state, &headers, true).await)
}

pub(crate) async fn get_capabilities(
    Path(wiki_id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Json<DevAuthCapabilityReport> {
    Json(capability_report_for_request(&state, &headers, &wiki_id, true).await)
}

pub(crate) async fn post_bootstrap_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<DevAuthBootstrapRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    validate_bootstrap_payload(&payload)?;

    let Some(access_token) = state.local_oauth.access_token() else {
        return Err((
            StatusCode::PRECONDITION_FAILED,
            Json(serde_json::json!({
                "error": "WIKIMEDIA_ACCESS_TOKEN is not available in process environment, .env.wikimedia.local, or .env"
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

    let current_ms = state.clock.now_ms();
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
    prune_expired_sessions(&mut sessions, current_ms);
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

    let cookie = session_cookie_header(&session_id)
        .ok_or_else(|| internal_error("failed to build session cookie header"))?;

    Ok((StatusCode::OK, [(SET_COOKIE, cookie)], Json(status)))
}

pub(crate) async fn delete_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    if let Some(session_id) = session_cookie_value(&headers) {
        let mut sessions = state.sessions.write().await;
        sessions.remove(&session_id);
        info!(
            session_id = session_id.as_str(),
            "cleared local dev-auth session"
        );
    }

    Ok((
        StatusCode::OK,
        [(SET_COOKIE, expired_session_cookie_header())],
        Json(to_status(None, &state.local_oauth, state.clock.now_ms())),
    ))
}

pub(crate) async fn get_bootstrap_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Json<DevAuthBootstrapStatus> {
    let auth = current_status(&state, &headers, true).await;

    Json(bootstrap_status(&state, &auth))
}
