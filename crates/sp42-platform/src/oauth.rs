//! OAuth 2 and PKCE helpers shared by browser-oriented targets.

use std::collections::BTreeMap;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;
use url::form_urlencoded::{Serializer, parse};

use crate::errors::OAuthError;
use crate::traits::Rng;
use crate::types::{HttpMethod, HttpRequest};

const CODE_VERIFIER_MIN_LEN: usize = 43;
const CODE_VERIFIER_MAX_LEN: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthClientConfig {
    pub client_id: String,
    pub authorize_url: Url,
    pub token_url: Url,
    pub redirect_uri: Url,
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthTokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: Option<u64>,
    pub refresh_token: Option<String>,
    pub scope: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthLaunchContext {
    pub state: String,
    pub verifier: String,
    pub challenge: String,
    pub authorization_url: Url,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OAuthCallback {
    AuthorizationCode {
        code: String,
        state: String,
    },
    AuthorizationError {
        error: String,
        error_description: Option<String>,
        state: Option<String>,
    },
}

/// Validate a PKCE code verifier according to the RFC length and character set.
///
/// # Errors
///
/// Returns [`OAuthError`] when the verifier is empty, too short, too long, or
/// contains characters outside the unreserved URL-safe set.
pub fn validate_code_verifier(verifier: &str) -> Result<(), OAuthError> {
    let length = verifier.len();
    if !(CODE_VERIFIER_MIN_LEN..=CODE_VERIFIER_MAX_LEN).contains(&length) {
        return Err(OAuthError::InvalidVerifier {
            message: format!(
                "expected verifier length between {CODE_VERIFIER_MIN_LEN} and {CODE_VERIFIER_MAX_LEN}, got {length}"
            ),
        });
    }

    if verifier.chars().any(
        |character| !matches!(character, 'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '.' | '_' | '~'),
    ) {
        return Err(OAuthError::InvalidVerifier {
            message: "verifier contains characters outside the PKCE unreserved set".to_string(),
        });
    }

    Ok(())
}

/// Generate a PKCE code verifier using the injected RNG trait.
#[must_use]
pub fn generate_pkce_verifier<R>(rng: &mut R) -> String
where
    R: Rng + ?Sized,
{
    generate_token(rng, 64)
}

/// Generate an OAuth state token using the injected RNG trait.
#[must_use]
pub fn generate_oauth_state<R>(rng: &mut R) -> String
where
    R: Rng + ?Sized,
{
    generate_token(rng, 32)
}

/// Prepare the browser launch context for the OAuth 2 PKCE flow.
///
/// # Errors
///
/// Returns [`OAuthError`] when URL construction or PKCE preparation fails.
pub fn prepare_oauth_launch<R>(
    config: &OAuthClientConfig,
    rng: &mut R,
) -> Result<OAuthLaunchContext, OAuthError>
where
    R: Rng + ?Sized,
{
    let verifier = generate_pkce_verifier(rng);
    let state = generate_oauth_state(rng);
    let challenge = code_challenge_from_verifier(&verifier)?;
    let authorization_url = build_authorization_url(config, &state, &challenge)?;

    Ok(OAuthLaunchContext {
        state,
        verifier,
        challenge,
        authorization_url,
    })
}

/// Derive an `S256` code challenge from a validated PKCE verifier.
///
/// # Errors
///
/// Returns [`OAuthError`] when the verifier fails validation.
pub fn code_challenge_from_verifier(verifier: &str) -> Result<String, OAuthError> {
    validate_code_verifier(verifier)?;

    let digest = Sha256::digest(verifier.as_bytes());
    Ok(URL_SAFE_NO_PAD.encode(digest))
}

/// Build the browser authorization URL for a public OAuth 2 client using PKCE.
///
/// # Errors
///
/// Returns [`OAuthError`] when required configuration values are missing.
pub fn build_authorization_url(
    config: &OAuthClientConfig,
    state: &str,
    code_challenge: &str,
) -> Result<Url, OAuthError> {
    ensure_non_empty("client_id", &config.client_id)?;
    ensure_non_empty("state", state)?;
    ensure_non_empty("code_challenge", code_challenge)?;

    let mut url = config.authorize_url.clone();
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &config.client_id)
        .append_pair("redirect_uri", config.redirect_uri.as_ref())
        .append_pair("code_challenge", code_challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state);

    if !config.scopes.is_empty() {
        url.query_pairs_mut()
            .append_pair("scope", &config.scopes.join(" "));
    }

    Ok(url)
}

/// Build the token-exchange request for the OAuth 2 authorization code flow.
///
/// # Errors
///
/// Returns [`OAuthError`] when required input values are missing or the PKCE
/// verifier is invalid.
pub fn build_access_token_request(
    config: &OAuthClientConfig,
    code: &str,
    verifier: &str,
) -> Result<HttpRequest, OAuthError> {
    ensure_non_empty("client_id", &config.client_id)?;
    ensure_non_empty("authorization code", code)?;
    validate_code_verifier(verifier)?;

    Ok(HttpRequest {
        method: HttpMethod::Post,
        url: config.token_url.clone(),
        headers: BTreeMap::from([(
            "content-type".to_string(),
            "application/x-www-form-urlencoded".to_string(),
        )]),
        body: encode_form(&[
            ("grant_type", "authorization_code"),
            ("client_id", config.client_id.as_str()),
            ("redirect_uri", config.redirect_uri.as_ref()),
            ("code", code),
            ("code_verifier", verifier),
        ]),
    })
}

/// Parse the query string returned to the OAuth callback route.
///
/// # Errors
///
/// Returns [`OAuthError`] when the query does not contain a valid `code` or an
/// OAuth error payload.
pub fn parse_callback_query(query: &str) -> Result<OAuthCallback, OAuthError> {
    let params: BTreeMap<String, String> = parse(query.trim_start_matches('?').as_bytes())
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();

    if let Some(error) = params.get("error") {
        return Ok(OAuthCallback::AuthorizationError {
            error: error.clone(),
            error_description: params.get("error_description").cloned(),
            state: params.get("state").cloned(),
        });
    }

    let code = params
        .get("code")
        .cloned()
        .ok_or_else(|| OAuthError::InvalidCallback {
            message: "callback query does not contain code".to_string(),
        })?;
    let state = params
        .get("state")
        .cloned()
        .ok_or_else(|| OAuthError::InvalidCallback {
            message: "callback query does not contain state".to_string(),
        })?;

    Ok(OAuthCallback::AuthorizationCode { code, state })
}

/// Validate an OAuth callback query and prepare the token-exchange request.
///
/// # Errors
///
/// Returns [`OAuthError`] when the callback indicates authorization failure,
/// the callback state does not match, or the token request cannot be built.
pub fn prepare_token_exchange_from_callback(
    config: &OAuthClientConfig,
    callback_query: &str,
    expected_state: &str,
    verifier: &str,
) -> Result<HttpRequest, OAuthError> {
    match parse_callback_query(callback_query)? {
        OAuthCallback::AuthorizationCode { code, state } => {
            if state != expected_state {
                return Err(OAuthError::StateMismatch);
            }

            build_access_token_request(config, &code, verifier)
        }
        OAuthCallback::AuthorizationError {
            error,
            error_description,
            ..
        } => Err(OAuthError::AuthorizationFailed {
            message: error_description.unwrap_or(error),
        }),
    }
}

fn ensure_non_empty(label: &str, value: &str) -> Result<(), OAuthError> {
    if value.trim().is_empty() {
        return Err(OAuthError::InvalidConfig {
            message: format!("{label} is required"),
        });
    }

    Ok(())
}

fn encode_form(fields: &[(&str, &str)]) -> Vec<u8> {
    let mut serializer = Serializer::new(String::new());
    for (key, value) in fields {
        serializer.append_pair(key, value);
    }

    serializer.finish().into_bytes()
}

fn generate_token<R>(rng: &mut R, byte_count: usize) -> String
where
    R: Rng + ?Sized,
{
    let mut bytes = Vec::with_capacity(byte_count);
    while bytes.len() < byte_count {
        bytes.extend_from_slice(&rng.next_u64().to_le_bytes());
    }
    bytes.truncate(byte_count);

    URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::{
        OAuthCallback, OAuthClientConfig, build_access_token_request, build_authorization_url,
        code_challenge_from_verifier, generate_oauth_state, generate_pkce_verifier,
        parse_callback_query, prepare_oauth_launch, prepare_token_exchange_from_callback,
        validate_code_verifier,
    };
    use crate::errors::OAuthError;
    use crate::traits::SequenceRng;
    use crate::types::HttpMethod;
    use url::Url;

    fn config() -> OAuthClientConfig {
        OAuthClientConfig {
            client_id: "sp42-dev".to_string(),
            authorize_url: Url::parse("https://meta.wikimedia.org/w/rest.php/oauth2/authorize")
                .expect("authorize url should parse"),
            token_url: Url::parse("https://meta.wikimedia.org/w/rest.php/oauth2/access_token")
                .expect("token url should parse"),
            redirect_uri: Url::parse("http://localhost:4173/oauth/callback")
                .expect("redirect url should parse"),
            scopes: vec!["basic".to_string(), "patrol".to_string()],
        }
    }

    #[test]
    fn rejects_short_verifier() {
        assert!(validate_code_verifier("short").is_err());
    }

    #[test]
    fn derives_url_safe_code_challenge() {
        let verifier = "abcdefghijklmnopqrstuvwxyz0123456789-._~ABCDEFG";
        let challenge =
            code_challenge_from_verifier(verifier).expect("challenge should be derived");

        assert!(!challenge.contains('='));
        assert!(!challenge.contains('+'));
        assert!(!challenge.contains('/'));
    }

    #[test]
    fn generates_valid_pkce_verifier_from_rng() {
        let mut rng = SequenceRng::new([1, 2, 3, 4, 5, 6, 7, 8]);
        let verifier = generate_pkce_verifier(&mut rng);

        assert!(validate_code_verifier(&verifier).is_ok());
    }

    #[test]
    fn generates_url_safe_state_from_rng() {
        let mut rng = SequenceRng::new([11, 12, 13, 14]);
        let state = generate_oauth_state(&mut rng);

        assert!(!state.contains('='));
        assert!(!state.contains('+'));
        assert!(!state.contains('/'));
    }

    #[test]
    fn builds_authorization_url_with_pkce_parameters() {
        let url = build_authorization_url(&config(), "state-123", "challenge-456")
            .expect("authorization url should build");
        let url_string = url.to_string();

        assert!(url_string.contains("response_type=code"));
        assert!(url_string.contains("client_id=sp42-dev"));
        assert!(url_string.contains("code_challenge=challenge-456"));
        assert!(url_string.contains("scope=basic+patrol"));
    }

    #[test]
    fn prepares_oauth_launch_context() {
        let mut rng = SequenceRng::new([1, 2, 3, 4, 5, 6, 7, 8]);
        let launch = prepare_oauth_launch(&config(), &mut rng).expect("launch should prepare");

        assert!(
            launch
                .authorization_url
                .as_str()
                .contains("response_type=code")
        );
        assert!(validate_code_verifier(&launch.verifier).is_ok());
        assert!(!launch.state.is_empty());
        assert!(!launch.challenge.is_empty());
    }

    #[test]
    fn builds_access_token_request_body() {
        let verifier = "abcdefghijklmnopqrstuvwxyz0123456789-._~ABCDEFG";
        let request = build_access_token_request(&config(), "code-123", verifier)
            .expect("token request should build");
        let body = String::from_utf8(request.body).expect("body should be utf-8");

        assert_eq!(request.method, HttpMethod::Post);
        assert!(body.contains("grant_type=authorization_code"));
        assert!(body.contains("code=code-123"));
        assert!(body.contains("code_verifier="));
    }

    #[test]
    fn parses_success_callback() {
        let callback = parse_callback_query("?code=abc&state=xyz").expect("callback should parse");

        assert_eq!(
            callback,
            OAuthCallback::AuthorizationCode {
                code: "abc".to_string(),
                state: "xyz".to_string(),
            }
        );
    }

    #[test]
    fn parses_error_callback() {
        let callback =
            parse_callback_query("?error=access_denied&state=xyz").expect("callback should parse");

        assert_eq!(
            callback,
            OAuthCallback::AuthorizationError {
                error: "access_denied".to_string(),
                error_description: None,
                state: Some("xyz".to_string()),
            }
        );
    }

    #[test]
    fn prepares_token_exchange_from_success_callback() {
        let request = prepare_token_exchange_from_callback(
            &config(),
            "?code=abc123&state=expected-state",
            "expected-state",
            "abcdefghijklmnopqrstuvwxyz0123456789-._~ABCDEFG",
        )
        .expect("token exchange should prepare");
        let body = String::from_utf8(request.body).expect("body should be utf-8");

        assert!(body.contains("code=abc123"));
    }

    #[test]
    fn rejects_callback_with_wrong_state() {
        let result = prepare_token_exchange_from_callback(
            &config(),
            "?code=abc123&state=wrong-state",
            "expected-state",
            "abcdefghijklmnopqrstuvwxyz0123456789-._~ABCDEFG",
        );

        assert!(matches!(result, Err(OAuthError::StateMismatch)));
    }
}
