use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use rand::Rng as _;
use sp42_core::check_fetchable_source_url;
use sp42_inference::guarded_source_client;
use sp42_types::{HttpClient, HttpClientError, HttpMethod, HttpRequest, HttpResponse};
use tracing_subscriber::EnvFilter;

pub(crate) struct ServerRng;

impl sp42_types::Rng for ServerRng {
    fn next_u64(&mut self) -> u64 {
        rand::rng().random()
    }
}

pub(crate) fn build_http_client() -> io::Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .user_agent(sp42_core::branding::USER_AGENT)
        // Use default redirect policy (limited to 10 redirects) for general-purpose clients
        // (e.g., wiki API calls). Source fetches (PlainHttpClient) construct a dedicated
        // guarded client with per-hop SSRF validation (SP42#34).
        .build()
        .map_err(|error| io::Error::other(format!("failed to build reqwest client: {error}")))
}

pub(crate) fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("sp42_server=info,sp42_core=warn"));

    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_ansi(false)
        .try_init();
}

pub(crate) fn runtime_storage_root() -> PathBuf {
    std::env::var_os("SP42_RUNTIME_DIR").map_or_else(
        || {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join(".sp42-runtime")
        },
        PathBuf::from,
    )
}

#[derive(Debug, Clone)]
pub(crate) struct BearerHttpClient {
    client: reqwest::Client,
    access_token: String,
}

impl BearerHttpClient {
    pub(crate) fn new(client: reqwest::Client, access_token: String) -> Self {
        Self {
            client,
            access_token,
        }
    }
}

#[async_trait]
impl HttpClient for BearerHttpClient {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, HttpClientError> {
        let mut builder = match request.method {
            HttpMethod::Get => self.client.get(request.url),
            HttpMethod::Post => self.client.post(request.url),
            HttpMethod::Put => self.client.put(request.url),
            HttpMethod::Patch => self.client.patch(request.url),
            HttpMethod::Delete => self.client.delete(request.url),
        }
        .bearer_auth(&self.access_token);

        for (key, value) in request.headers {
            builder = builder.header(&key, &value);
        }

        let response = if request.body.is_empty() {
            builder.send().await
        } else {
            builder.body(request.body).send().await
        }
        .map_err(|error| HttpClientError::Transport {
            message: error.to_string(),
        })?;

        let status = response.status().as_u16();
        let mut headers = HashMap::new();
        for (key, value) in response.headers() {
            if let Ok(value) = value.to_str() {
                headers.insert(key.to_string(), value.to_string());
            }
        }
        let body = response
            .bytes()
            .await
            .map_err(|error| HttpClientError::InvalidResponse {
                message: error.to_string(),
            })?;

        Ok(HttpResponse {
            status,
            headers: headers.into_iter().collect(),
            body: body.to_vec(),
        })
    }
}

/// Minimal `HttpClient` wrapper around a `reqwest::Client` for read-only source fetches
/// (no bearer auth, no special header handling). Enforces SSRF floor (SP42#34): blocks
/// loopback/private/link-local/localhost hosts and non-http(s) schemes, honors
/// `SP42_FETCH_ALLOW_PRIVATE=1` dev escape hatch, enforces GET-only and `MAX_SOURCE_BYTES` cap.
/// The underlying client is built with a per-hop redirect policy that validates each redirect
/// target against the same SSRF floor (capped at 5 hops).
#[derive(Clone)]
pub(crate) struct PlainHttpClient {
    client: reqwest::Client,
    /// Allow loopback/private source hosts — a dev/test escape hatch for the loopback-serving
    /// benchmark harness (`SP42_FETCH_ALLOW_PRIVATE=1`). Off by default (SP42#34 SSRF floor).
    allow_private: bool,
}

/// Basic source-response size cap, checked against `Content-Length`. Streaming enforcement
/// (for chunked responses with no length) is deferred to the SP42#34 fetch-edge ADR.
const MAX_SOURCE_BYTES: u64 = 8 * 1024 * 1024;

impl PlainHttpClient {
    pub(crate) fn new() -> Result<Self, String> {
        let allow_private =
            std::env::var("SP42_FETCH_ALLOW_PRIVATE").is_ok_and(|value| value == "1");
        let client = guarded_source_client(allow_private)?;
        Ok(Self {
            client,
            allow_private,
        })
    }
}

#[async_trait]
impl HttpClient for PlainHttpClient {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, HttpClientError> {
        // SSRF floor (SP42#34): refuse a non-http(s) / loopback / private / link-local source
        // host unless the dev/test escape hatch is set.
        if !self.allow_private {
            check_fetchable_source_url(&request.url)
                .map_err(|message| HttpClientError::Transport { message })?;
        }
        // Read-only source fetch: GET only.
        let HttpMethod::Get = request.method else {
            return Err(HttpClientError::Transport {
                message: format!("source fetch only allows GET, got {:?}", request.method),
            });
        };

        let mut builder = self.client.get(request.url.clone());
        for (key, value) in &request.headers {
            builder = builder.header(key, value);
        }

        let response = builder
            .send()
            .await
            .map_err(|error| HttpClientError::Transport {
                message: error.to_string(),
            })?;

        if response
            .content_length()
            .is_some_and(|len| len > MAX_SOURCE_BYTES)
        {
            return Err(HttpClientError::Transport {
                message: format!(
                    "source response exceeds {MAX_SOURCE_BYTES}-byte cap (Content-Length)"
                ),
            });
        }

        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (name.as_str().to_lowercase(), value.to_string()))
            })
            .collect();
        let body = response
            .bytes()
            .await
            .map_err(|error| HttpClientError::Transport {
                message: error.to_string(),
            })?
            .to_vec();

        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }
}
