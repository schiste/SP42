//! The SSRF-guarded source HTTP client used by the running server (PRD-0010, Phase 4).
//!
//! Reuses `sp42-fetch`'s shared source-fetch edge (SSRF resolver guard, redirect cap, streamed
//! body cap, timeouts, retry policy, and Wikimedia-compliant user agent). The MCP wrapper only
//! keeps the dev/test private-address flag visible for validating caller-supplied Parsoid
//! overrides; the verb logic above it is stub-tested in `probe`/`verify`.

use async_trait::async_trait;
use sp42_types::{HttpClient, HttpClientError, HttpRequest, HttpResponse};

/// SSRF-guarded, GET-only, size-capped source-fetch client for the running MCP server.
pub struct GuardedHttpClient {
    client: sp42_fetch::GuardedHttpClient,
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
            client: sp42_fetch::build_source_client(
                sp42_platform::branding::USER_AGENT,
                allow_private,
            )?,
            allow_private,
        })
    }

    /// Whether the dev/test private-address escape hatch is active.
    #[must_use]
    pub fn allows_private_addresses(&self) -> bool {
        self.allow_private
    }
}

#[async_trait]
impl HttpClient for GuardedHttpClient {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, HttpClientError> {
        self.client.execute(request).await
    }
}
