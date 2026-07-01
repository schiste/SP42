//! Shared inference edge: the genai-backed `ModelClient` and env-driven
//! construction of an endpoint config + model panel. Also exports the guarded HTTP client
//! builder for source fetches with per-hop SSRF validation (SP42#34).

use std::time::Duration;

use async_trait::async_trait;
use genai::Client;
use genai::adapter::AdapterKind;
use genai::chat::{ChatMessage as GenaiChatMessage, ChatOptions, ChatRequest};
use genai::resolver::{AuthData, Endpoint};
use genai::{Headers, ModelIden, ServiceTarget};
use sp42_types::{
    ChatRole, EndpointMode, ModelClient, ModelClientError, ModelCompletion, ModelCompletionRequest,
    ModelEndpointConfig, ModelRef, SamplingParams,
};

/// Header carrying SP42-specific capability tags on model requests.
const CAPABILITY_TAG_HEADER: &str = "X-SP42-Capability";

/// Wall-clock bound on a single model call so a hung inference endpoint can't wedge the CLI
/// (SP42#34). Applied via `tokio::time::timeout` because `genai` pins its own `reqwest`
/// version, so its client can't be built from this crate's `reqwest`.
const MODEL_CALL_TIMEOUT: Duration = Duration::from_mins(1);

/// Genai-backed model client wrapping the genai `Client` and an endpoint config.
pub struct GenaiModelClient {
    client: Client,
    endpoint: ModelEndpointConfig,
}

impl GenaiModelClient {
    /// Create a new genai model client with the given endpoint config.
    #[must_use]
    pub fn new(endpoint: ModelEndpointConfig) -> Self {
        Self {
            client: Client::default(),
            endpoint,
        }
    }
}

#[async_trait]
impl ModelClient for GenaiModelClient {
    async fn complete(
        &self,
        request: &ModelCompletionRequest,
    ) -> Result<ModelCompletion, ModelClientError> {
        let messages = request
            .messages
            .iter()
            .map(|message| match message.role {
                ChatRole::System => GenaiChatMessage::system(message.content.clone()),
                ChatRole::User => GenaiChatMessage::user(message.content.clone()),
                ChatRole::Assistant => GenaiChatMessage::assistant(message.content.clone()),
            })
            .collect::<Vec<_>>();
        let chat_request = ChatRequest::new(messages);

        let target = ServiceTarget {
            endpoint: Endpoint::from_owned(normalize_base_url(&self.endpoint.base_url)),
            auth: genai_auth_for(&self.endpoint),
            model: ModelIden::new(AdapterKind::OpenAI, request.model.model.clone()),
        };
        let options = genai_chat_options(&request.params, self.endpoint.capability_tag.as_deref());

        let response = tokio::time::timeout(
            MODEL_CALL_TIMEOUT,
            self.client.exec_chat(target, chat_request, Some(&options)),
        )
        .await
        .map_err(|_| ModelClientError::Transport {
            message: format!("model request timed out after {MODEL_CALL_TIMEOUT:?}"),
        })?
        .map_err(|error| ModelClientError::Transport {
            message: error.to_string(),
        })?;

        let text = response
            .into_first_text()
            .ok_or_else(|| ModelClientError::InvalidResponse {
                message: "model response contained no text".to_string(),
            })?;
        Ok(ModelCompletion {
            text,
            served_model: None,
        })
    }
}

/// Translate our neutral [`SamplingParams`] into `genai` `ChatOptions`, attaching the
/// capability tag as a transport header when present.
fn genai_chat_options(params: &SamplingParams, capability_tag: Option<&str>) -> ChatOptions {
    let mut options = ChatOptions::default();
    if let Some(temperature) = params.temperature {
        options = options.with_temperature(temperature);
    }
    if let Some(top_p) = params.top_p {
        options = options.with_top_p(top_p);
    }
    if let Some(max_tokens) = params.max_tokens {
        options = options.with_max_tokens(max_tokens);
    }
    if let Some(tag) = capability_tag {
        options =
            options.with_extra_headers([(CAPABILITY_TAG_HEADER.to_string(), tag.to_string())]);
    }
    options
}

/// Normalize an OpenAI-compatible base URL so `genai`'s adapter can join its
/// `chat/completions` suffix: drop any trailing slash, tolerate a URL that already points at
/// `.../chat/completions` by stripping that segment, then re-append a single trailing slash
/// (reqwest's URL join requires the trailing slash to preserve the base path).
fn normalize_base_url(raw: &str) -> String {
    let trimmed = raw.trim_end_matches('/');
    let base = trimmed.strip_suffix("/chat/completions").unwrap_or(trimmed);
    format!("{base}/")
}

/// The full OpenAI-compatible chat-completions URL `genai` would POST to, rebuilt here so the
/// tokenless [`genai_auth_for`] `RequestOverride` path (which bypasses genai's URL build)
/// targets the same endpoint. Mirrors genai's `base.join("chat/completions")` over our
/// normalized base (which already carries the single trailing slash that join requires).
fn chat_completions_url(base_url: &str) -> String {
    format!("{}chat/completions", normalize_base_url(base_url))
}

/// Build the `genai` `AuthData` for an endpoint, sending **no** `Authorization` header when
/// no token is configured (SP42#44).
///
/// With a (non-empty) token we use the standard `Authorization: Bearer <token>` path. Without
/// one we cannot simply pass an empty key: `genai` 0.6.5's `OpenAI` chat adapter always emits
/// `Authorization: Bearer {key}` built from the `ServiceTarget` key, so an absent token would
/// otherwise become a literal `Authorization: Bearer ` — which breaks local model servers and
/// sponsor proxies that expect a truly tokenless request. `AuthData::None` is not a way out:
/// it errors inside genai's `get_api_key` before any header is built. The only header-less
/// path is `AuthData::RequestOverride`, which replaces the request URL **and** headers
/// wholesale; we therefore rebuild the chat URL ([`chat_completions_url`]) and re-attach the
/// capability tag here, because the override also bypasses genai's URL construction and the
/// `ChatOptions::extra_headers` merge that normally carries it.
fn genai_auth_for(endpoint: &ModelEndpointConfig) -> AuthData {
    if let Some(token) = endpoint
        .auth_token
        .as_deref()
        .filter(|token| !token.is_empty())
    {
        AuthData::from_single(token.to_string())
    } else {
        let mut headers: Vec<(String, String)> = Vec::new();
        if let Some(tag) = endpoint.capability_tag.as_deref() {
            headers.push((CAPABILITY_TAG_HEADER.to_string(), tag.to_string()));
        }
        AuthData::RequestOverride {
            url: chat_completions_url(&endpoint.base_url),
            headers: Headers::from(headers),
        }
    }
}

/// Parse the optional `SP42_INFERENCE_MODE` env value. Defaults to `local`; the mode is
/// recorded on the endpoint config as advisory metadata (the adapter sends the bearer
/// whenever a token is present, regardless of mode, in this CLI MVP).
///
/// # Errors
///
/// Returns an error if the mode value is not one of `local`, `direct`, `sponsor_proxy`, or `sponsor-proxy`.
pub fn parse_endpoint_mode(value: Option<&str>) -> Result<EndpointMode, String> {
    match value {
        None | Some("local") => Ok(EndpointMode::Local),
        Some("direct") => Ok(EndpointMode::Direct),
        Some("sponsor_proxy" | "sponsor-proxy") => Ok(EndpointMode::SponsorProxy),
        Some(other) => Err(format!("unsupported SP42_INFERENCE_MODE: {other}")),
    }
}

/// Build the model panel from `SP42_INFERENCE_MODELS` (+ `SP42_INFERENCE_PROVIDER`).
///
/// # Errors
///
/// Returns an error if `SP42_INFERENCE_MODELS` is not set or is empty.
pub fn panel_from_env() -> Result<Vec<ModelRef>, String> {
    let provider =
        std::env::var("SP42_INFERENCE_PROVIDER").unwrap_or_else(|_| "configured".to_string());
    let models = std::env::var("SP42_INFERENCE_MODELS").map_err(|_| {
        "set SP42_INFERENCE_MODELS to a comma-separated list of model ids".to_string()
    })?;
    panel_from_models(&provider, &models)
}

/// Build a model panel from an explicit comma-separated id list (the `--models` override),
/// using `provider` for every `ModelRef`. Blank entries are skipped; an all-blank list errors.
///
/// # Errors
///
/// Returns an error if `models` contains no non-empty model id.
pub fn panel_from_models(provider: &str, models: &str) -> Result<Vec<ModelRef>, String> {
    let panel: Vec<ModelRef> = models
        .split(',')
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .map(|m| ModelRef::new(provider.to_string(), m, m))
        .collect();
    if panel.is_empty() {
        return Err("model panel is empty (no model ids given)".to_string());
    }
    Ok(panel)
}

/// Build a genai model client from `SP42_INFERENCE_URL`/`TOKEN`/`CAPABILITY`/`MODE`.
///
/// # Errors
///
/// Returns an error if `SP42_INFERENCE_URL` is not set or if endpoint mode parsing fails.
pub fn client_from_env() -> Result<GenaiModelClient, String> {
    let base_url = std::env::var("SP42_INFERENCE_URL").map_err(|_| {
        "set SP42_INFERENCE_URL to the model's OpenAI-compatible base URL".to_string()
    })?;
    let auth_token = std::env::var("SP42_INFERENCE_TOKEN").ok();
    let capability_tag = std::env::var("SP42_INFERENCE_CAPABILITY").ok();
    let mode = parse_endpoint_mode(std::env::var("SP42_INFERENCE_MODE").ok().as_deref())?;
    Ok(GenaiModelClient::new(ModelEndpointConfig {
        mode,
        base_url,
        auth_token,
        capability_tag,
    }))
}

/// Check whether a URL host is safe to fetch from, honoring the `allow_private` escape hatch
/// (SP42#34 SSRF floor). Used as the per-hop predicate in the redirect policy.
///
/// Delegates to the canonical SSRF check in `sp42_core::check_fetchable_source_url` to avoid
/// duplicating and drifting the security logic.
///
/// # Arguments
///
/// * `url` - The URL to validate (typically from a redirect Location header).
/// * `allow_private` - If `true`, allow private/loopback/link-local addresses (dev escape hatch).
///
/// # Returns
///
/// `true` if the URL host passes the SSRF check, `false` if it should be blocked.
#[must_use]
pub fn redirect_host_allowed(url: &reqwest::Url, allow_private: bool) -> bool {
    if allow_private {
        // Dev escape hatch: allow private/loopback hosts, but still only http(s).
        return matches!(url.scheme(), "http" | "https");
    }
    sp42_core::check_fetchable_source_url(url).is_ok()
}

/// Build a `reqwest::Client` configured for source fetches with per-hop SSRF validation (SP42#34).
/// The client enforces a redirect policy that checks each hop against the SSRF floor and caps
/// the total redirect count.
///
/// # Arguments
///
/// * `allow_private` - If `true`, allow private/loopback/link-local addresses (dev escape hatch).
///
/// # Returns
///
/// A configured `reqwest::Client` ready for source fetches.
///
/// # Errors
///
/// Returns an error if the client fails to build (e.g., I/O error).
pub fn guarded_source_client(allow_private: bool) -> Result<reqwest::Client, String> {
    let max_redirects = 5;
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .user_agent(sp42_core::branding::USER_AGENT)
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            // Check the redirect target host against the SSRF floor.
            if redirect_host_allowed(attempt.url(), allow_private) {
                // Host is allowed. Check hop count.
                if attempt.previous().len() < max_redirects {
                    attempt.follow()
                } else {
                    attempt.stop()
                }
            } else {
                // Host is blocked. Return error so the policy closure reports the failure.
                attempt.error("SSRF: redirect target host is not allowed")
            }
        }))
        .build()
        .map_err(|error| format!("failed to build guarded source client: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_base_url_ensures_single_trailing_slash() {
        assert_eq!(
            normalize_base_url("https://openrouter.ai/api/v1"),
            "https://openrouter.ai/api/v1/"
        );
        assert_eq!(
            normalize_base_url("https://openrouter.ai/api/v1/"),
            "https://openrouter.ai/api/v1/"
        );
    }

    #[test]
    fn normalize_base_url_strips_completions_suffix() {
        assert_eq!(
            normalize_base_url("https://openrouter.ai/api/v1/chat/completions"),
            "https://openrouter.ai/api/v1/"
        );
        assert_eq!(
            normalize_base_url("https://openrouter.ai/api/v1/chat/completions/"),
            "https://openrouter.ai/api/v1/"
        );
    }

    #[test]
    fn panel_from_models_builds_one_ref_per_trimmed_id() {
        let panel = panel_from_models("configured", "alpha, beta ,, gamma").expect("panel");
        assert_eq!(panel.len(), 3);
        assert_eq!(panel[0].provider, "configured");
        assert_eq!(panel[0].model, "alpha");
        assert_eq!(panel[0].version, "alpha");
        assert_eq!(panel[1].model, "beta");
        assert_eq!(panel[2].model, "gamma");
    }

    #[test]
    fn panel_from_models_rejects_an_all_blank_list() {
        assert!(panel_from_models("configured", "  , ,").is_err());
    }

    #[test]
    fn parse_endpoint_mode_defaults_to_local() {
        assert_eq!(parse_endpoint_mode(None), Ok(EndpointMode::Local));
        assert_eq!(parse_endpoint_mode(Some("local")), Ok(EndpointMode::Local));
    }

    #[test]
    fn parse_endpoint_mode_recognizes_variants() {
        assert_eq!(
            parse_endpoint_mode(Some("direct")),
            Ok(EndpointMode::Direct)
        );
        assert_eq!(
            parse_endpoint_mode(Some("sponsor_proxy")),
            Ok(EndpointMode::SponsorProxy)
        );
        assert_eq!(
            parse_endpoint_mode(Some("sponsor-proxy")),
            Ok(EndpointMode::SponsorProxy)
        );
    }

    #[test]
    fn parse_endpoint_mode_rejects_unknown() {
        assert!(parse_endpoint_mode(Some("unknown")).is_err());
    }

    #[test]
    fn redirect_host_allowed_with_allow_private_false() {
        use url::Url;

        // Loopback IPv4 should be blocked
        assert!(
            !redirect_host_allowed(
                &Url::parse("http://127.0.0.1/admin").expect("valid URL"),
                false
            ),
            "127.0.0.1 should be blocked with allow_private=false"
        );

        // Private IPv4 ranges should be blocked
        assert!(
            !redirect_host_allowed(&Url::parse("http://10.0.0.1/").expect("valid URL"), false),
            "10.0.0.1 should be blocked"
        );
        assert!(
            !redirect_host_allowed(
                &Url::parse("http://192.168.1.1/").expect("valid URL"),
                false
            ),
            "192.168.1.1 should be blocked"
        );

        // Link-local IPv4 should be blocked
        assert!(
            !redirect_host_allowed(
                &Url::parse("http://169.254.1.1/").expect("valid URL"),
                false
            ),
            "169.254.x.x link-local should be blocked"
        );

        // Cloud metadata endpoint should be blocked
        assert!(
            !redirect_host_allowed(
                &Url::parse("http://169.254.169.254/latest/meta-data/").expect("valid URL"),
                false
            ),
            "metadata endpoint should be blocked"
        );

        // localhost domain should be blocked
        assert!(
            !redirect_host_allowed(
                &Url::parse("http://localhost/admin").expect("valid URL"),
                false
            ),
            "localhost should be blocked"
        );

        // .localhost subdomain should be blocked
        assert!(
            !redirect_host_allowed(
                &Url::parse("http://foo.localhost/").expect("valid URL"),
                false
            ),
            ".localhost should be blocked"
        );

        // Trailing-dot localhost (FQDN notation, resolves to loopback) should be blocked
        // This is a regression test for the drift bug: old code didn't trim_end_matches('.')
        assert!(
            !redirect_host_allowed(&Url::parse("http://localhost./").expect("valid URL"), false),
            "localhost. with trailing dot should be blocked"
        );

        // Trailing-dot loopback IPv4 should be blocked
        assert!(
            !redirect_host_allowed(&Url::parse("http://127.0.0.1./").expect("valid URL"), false),
            "127.0.0.1. with trailing dot should be blocked"
        );

        // Loopback IPv6 should be blocked
        assert!(
            !redirect_host_allowed(&Url::parse("http://[::1]/").expect("valid URL"), false),
            "::1 loopback should be blocked"
        );

        // IPv6 link-local should be blocked
        assert!(
            !redirect_host_allowed(&Url::parse("http://[fe80::1]/").expect("valid URL"), false),
            "fe80:: link-local should be blocked"
        );

        // IPv6 unique-local should be blocked
        assert!(
            !redirect_host_allowed(&Url::parse("http://[fc00::1]/").expect("valid URL"), false),
            "fc00:: unique-local should be blocked"
        );

        // IPv4-mapped IPv6 loopback should be blocked
        assert!(
            !redirect_host_allowed(
                &Url::parse("http://[::ffff:127.0.0.1]/").expect("valid URL"),
                false
            ),
            "IPv4-mapped loopback should be blocked"
        );

        // Non-http(s) scheme should be blocked
        assert!(
            !redirect_host_allowed(&Url::parse("file:///etc/passwd").expect("valid URL"), false),
            "file:// scheme should be blocked"
        );

        // Normal public domain should be allowed
        assert!(
            redirect_host_allowed(
                &Url::parse("https://example.com/page").expect("valid URL"),
                false
            ),
            "public domain should be allowed"
        );

        // Public IPv4 should be allowed
        assert!(
            redirect_host_allowed(&Url::parse("http://8.8.8.8/dns").expect("valid URL"), false),
            "public IPv4 should be allowed"
        );
    }

    #[test]
    fn redirect_host_allowed_with_allow_private_true() {
        use url::Url;

        // With allow_private=true, loopback/private/link-local should be allowed
        assert!(
            redirect_host_allowed(
                &Url::parse("http://127.0.0.1/admin").expect("valid URL"),
                true
            ),
            "127.0.0.1 should be allowed with allow_private=true"
        );
        assert!(
            redirect_host_allowed(&Url::parse("http://10.0.0.1/").expect("valid URL"), true),
            "10.0.0.1 should be allowed with allow_private=true"
        );
        assert!(
            redirect_host_allowed(&Url::parse("http://192.168.1.1/").expect("valid URL"), true),
            "192.168.1.1 should be allowed with allow_private=true"
        );
        assert!(
            redirect_host_allowed(
                &Url::parse("http://169.254.169.254/latest/meta-data/").expect("valid URL"),
                true
            ),
            "metadata endpoint should be allowed with allow_private=true"
        );

        // But non-http(s) schemes should still be blocked
        assert!(
            !redirect_host_allowed(&Url::parse("file:///etc/passwd").expect("valid URL"), true),
            "file:// scheme should be blocked even with allow_private=true"
        );

        // And normal public domains should still work
        assert!(
            redirect_host_allowed(
                &Url::parse("https://example.com/page").expect("valid URL"),
                true
            ),
            "public domain should be allowed with allow_private=true"
        );

        // Even with allow_private=true, localhost should be allowed (it's private)
        assert!(
            redirect_host_allowed(&Url::parse("http://localhost/").expect("valid URL"), true),
            "localhost should be allowed with allow_private=true"
        );

        // But trailing-dot localhost should still be allowed with allow_private=true (it's still private)
        assert!(
            redirect_host_allowed(&Url::parse("http://localhost./").expect("valid URL"), true),
            "localhost. should be allowed with allow_private=true"
        );
    }

    fn endpoint_config(
        auth_token: Option<&str>,
        capability_tag: Option<&str>,
    ) -> ModelEndpointConfig {
        ModelEndpointConfig {
            mode: EndpointMode::Local,
            base_url: "http://localhost:11434/v1".to_string(),
            auth_token: auth_token.map(ToString::to_string),
            capability_tag: capability_tag.map(ToString::to_string),
        }
    }

    #[test]
    fn genai_auth_for_uses_bearer_key_when_token_present() {
        let auth = genai_auth_for(&endpoint_config(Some("secret-token"), None));
        // A present token must keep the standard `Authorization: Bearer <token>` path.
        assert!(matches!(auth, AuthData::Key(_)));
        assert_eq!(
            auth.single_key_value().ok().as_deref(),
            Some("secret-token")
        );
    }

    #[test]
    fn genai_auth_for_sends_no_authorization_header_when_token_absent() {
        // genai 0.6.5 always emits `Authorization: Bearer {key}` from a ServiceTarget key, so
        // a tokenless endpoint must use `RequestOverride` to omit the header entirely (#44).
        let auth = genai_auth_for(&endpoint_config(None, None));
        let AuthData::RequestOverride { url, headers } = auth else {
            panic!("tokenless endpoint must use RequestOverride, got {auth:?}");
        };
        assert_eq!(url, "http://localhost:11434/v1/chat/completions");
        assert!(
            !headers
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("authorization")),
            "no Authorization header without a token"
        );
    }

    #[test]
    fn genai_auth_for_treats_an_empty_token_as_absent() {
        // `SP42_INFERENCE_TOKEN=` (set but empty) must not become `Authorization: Bearer `.
        let auth = genai_auth_for(&endpoint_config(Some(""), None));
        assert!(matches!(auth, AuthData::RequestOverride { .. }));
    }

    #[test]
    fn genai_auth_for_carries_capability_tag_in_override_headers() {
        // RequestOverride replaces all headers, so the capability tag must ride here, not via
        // the (discarded) ChatOptions extra_headers.
        let auth = genai_auth_for(&endpoint_config(None, Some("citation-verify")));
        let AuthData::RequestOverride { headers, .. } = auth else {
            panic!("expected RequestOverride");
        };
        assert!(
            headers
                .iter()
                .any(|(name, value)| name.as_str() == CAPABILITY_TAG_HEADER
                    && value.as_str() == "citation-verify")
        );
    }
}
