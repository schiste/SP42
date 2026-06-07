use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use rand::Rng as _;
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
