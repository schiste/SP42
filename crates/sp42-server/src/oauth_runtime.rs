use axum::http::{HeaderMap, StatusCode, header::HOST};
use axum::Json;

use crate::{
    invalid_payload, AppState, DevAuthBootstrapRequest, LocalOAuthConfig, OAuthClientConfig,
    OAuthProfileResponse, OAuthTokenResponse, PendingOAuthLogin, AUTH_CALLBACK_PATH,
};

pub(crate) fn internal_error(message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "error": message })),
    )
}

pub(crate) fn sanitize_redirect_target(next: Option<&str>) -> String {
    let Some(target) = next.map(str::trim).filter(|value| !value.is_empty()) else {
        return "/".to_string();
    };
    if target.starts_with('/') && !target.starts_with("//") {
        target.to_string()
    } else {
        "/".to_string()
    }
}

pub(crate) fn redirect_with_status(target: &str, key: &str, value: &str) -> String {
    let separator = if target.contains('?') { '&' } else { '?' };
    format!(
        "{target}{separator}{key}={}",
        url::form_urlencoded::byte_serialize(value.as_bytes()).collect::<String>()
    )
}

pub(crate) fn public_base_url(headers: &HeaderMap) -> Result<String, String> {
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
    if !is_local_host(host) {
        return Err(
            "non-local oauth redirects require SP42_PUBLIC_BASE_URL instead of trusting Host"
                .to_string(),
        );
    }
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("http");
    Ok(format!("{scheme}://{host}"))
}

pub(crate) fn is_local_host(host: &str) -> bool {
    let authority = host.trim();
    let host_without_port = if authority.starts_with('[') {
        authority
            .split_once(']')
            .map_or(authority, |(head, _)| &head[1..])
    } else {
        authority.split(':').next().unwrap_or(authority)
    };

    matches!(host_without_port, "localhost" | "127.0.0.1" | "::1")
}

pub(crate) fn oauth_client_config_for_request(
    state: &AppState,
    headers: &HeaderMap,
    wiki_id: &str,
) -> Result<OAuthClientConfig, (StatusCode, Json<serde_json::Value>)> {
    let config =
        crate::resolved_wiki_config(state, wiki_id).map_err(|message| invalid_payload(&message))?;
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

pub(crate) fn oauth_client_config_from_pending(
    state: &AppState,
    pending: &PendingOAuthLogin,
) -> Result<OAuthClientConfig, (StatusCode, Json<serde_json::Value>)> {
    let config = crate::resolved_wiki_config(state, &pending.wiki_id)
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

pub(crate) async fn exchange_authorization_code(
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

pub(crate) async fn fetch_oauth_profile(
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

pub(crate) fn validate_bootstrap_payload(
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
