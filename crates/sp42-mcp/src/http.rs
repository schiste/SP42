//! The SSRF-guarded source HTTP client used by the running server (PRD-0010, Phase 4).
//!
//! Reuses `sp42-inference`'s `guarded_source_client` (the hardened reqwest builder with a per-hop
//! redirect SSRF policy) and `sp42-core`'s `check_fetchable_source_url` (the initial-URL SSRF
//! floor), adding only the thin GET-only, size-capped [`HttpClient`] adapter — mirroring
//! `sp42-server`'s `PlainHttpClient`. Not exercised by unit tests (it performs real network I/O);
//! the verb logic above it is stub-tested in `probe`/`verify`.

use async_trait::async_trait;
use sp42_citation::check_fetchable_source_url;
use sp42_inference::guarded_source_client;
use sp42_types::{HttpClient, HttpClientError, HttpMethod, HttpRequest, HttpResponse};

/// Source-response size cap (mirrors `sp42-server`'s `PlainHttpClient`, SP42#34).
const MAX_SOURCE_BYTES: u64 = 8 * 1024 * 1024;

/// SSRF-guarded, GET-only, size-capped source-fetch client for the running MCP server.
pub struct GuardedHttpClient {
    client: reqwest::Client,
    allow_private: bool,
}

impl GuardedHttpClient {
    /// Build from the environment, honoring `SP42_FETCH_ALLOW_PRIVATE=1` (the dev/test escape
    /// hatch that allows loopback/private source hosts).
    ///
    /// # Errors
    ///
    /// Returns an error string if the guarded reqwest client cannot be constructed.
    pub fn from_env() -> Result<Self, String> {
        let allow_private =
            std::env::var("SP42_FETCH_ALLOW_PRIVATE").is_ok_and(|value| value == "1");
        Ok(Self {
            client: guarded_source_client(allow_private)?,
            allow_private,
        })
    }
}

#[async_trait]
impl HttpClient for GuardedHttpClient {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, HttpClientError> {
        // SSRF floor (SP42#34): refuse non-http(s) / loopback / private / link-local source hosts
        // unless the dev/test escape hatch is set.
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

        let mut response = builder
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

        // Enforce the cap while streaming: a chunked / length-less response slips past the
        // header check above.
        let cap = usize::try_from(MAX_SOURCE_BYTES).unwrap_or(usize::MAX);
        let mut body = Vec::new();
        while let Some(chunk) =
            response
                .chunk()
                .await
                .map_err(|error| HttpClientError::Transport {
                    message: error.to_string(),
                })?
        {
            if body.len() + chunk.len() > cap {
                return Err(HttpClientError::Transport {
                    message: format!(
                        "source response exceeds {MAX_SOURCE_BYTES}-byte cap (streamed)"
                    ),
                });
            }
            body.extend_from_slice(&chunk);
        }

        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }
}
