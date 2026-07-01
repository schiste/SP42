//! The guarded read-only `HttpClient` over `reqwest` (ADR-0015).

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use rand::Rng as _;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use sp42_types::{HttpClient, HttpClientError, HttpMethod, HttpRequest, HttpResponse};

use crate::resolver::{GuardedResolver, is_public_ip};

/// Marker embedded in every SSRF-rejection error so the retry strategy can
/// recognise a deterministic refusal and decline to retry it.
const SSRF_MARKER: &str = "SSRF";

/// Whether a URL's host is an IP *literal* in a non-public range. The resolver
/// guard only runs for DNS names, so literal hosts (initial URL or redirect
/// target) need this explicit check.
fn host_is_blocked_literal(url: &reqwest::Url) -> bool {
    match url.host() {
        Some(url::Host::Ipv4(ip)) => !is_public_ip(ip.into()),
        Some(url::Host::Ipv6(ip)) => !is_public_ip(ip.into()),
        _ => false,
    }
}

/// Source-response body cap (#43): enforced against `Content-Length` and while
/// streaming, so a chunked / no-length body cannot return unbounded.
const MAX_SOURCE_BYTES: u64 = 8 * 1024 * 1024;
/// Redirect-hop cap. Each hop is re-validated by the resolver before connecting.
const MAX_REDIRECTS: usize = 5;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_RETRIES: u32 = 3;
/// Base for exponential backoff between retries (doubled per attempt, jittered).
const RETRY_BASE: Duration = Duration::from_millis(500);
/// Ceiling on any single retry wait, so a hostile/buggy `Retry-After` (or a deep
/// backoff) cannot pin the client.
const MAX_RETRY_DELAY: Duration = Duration::from_secs(30);

/// A guarded `HttpClient`: GET-only, redirect-capped, size-capped, timed-out,
/// and retried on transient failures. Whether the SSRF resolver guard is
/// attached is decided by the constructor (source vs. trusted Wikimedia host).
#[derive(Clone)]
pub struct GuardedHttpClient {
    client: reqwest::Client,
    max_source_bytes: u64,
    /// Whether to reject non-public IP-literal hosts pre-flight. Off only for the
    /// dev escape hatch, whose loopback benchmark harness serves on `127.0.0.1`.
    guard_literals: bool,
}

/// Inner resolver delegating to the system resolver via `tokio`. Wrapped by
/// [`GuardedResolver`]; not used directly elsewhere.
struct SystemResolver;

impl Resolve for SystemResolver {
    fn resolve(&self, name: Name) -> Resolving {
        Box::pin(async move {
            let host = name.as_str().to_owned();
            let addrs = tokio::net::lookup_host((host, 0)).await?;
            Ok(Box::new(addrs) as Addrs)
        })
    }
}

/// Transient HTTP statuses worth retrying for an idempotent GET. Permanent 4xx
/// (e.g. 404/403) are not retried; transport/connect errors are not retried
/// either (handled in `execute` — a deterministic SSRF rejection surfaces as a
/// transport error and must fail fast, not burn backoff).
fn is_retryable_status(status: u16) -> bool {
    matches!(status, 408 | 429 | 500 | 502 | 503 | 504)
}

/// The server's `Retry-After` as a duration, if present and a sane delta-seconds
/// value (the common form; HTTP-date form is ignored). Capped at
/// [`MAX_RETRY_DELAY`] so a hostile/buggy header cannot pin the client.
fn retry_after_delay(response: &reqwest::Response) -> Option<Duration> {
    let seconds: u64 = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()?;
    Some(Duration::from_secs(seconds).min(MAX_RETRY_DELAY))
}

/// Exponential backoff with full jitter for `attempt` (0-based), capped at
/// [`MAX_RETRY_DELAY`]: a uniform random wait in `[0, RETRY_BASE * 2^attempt]`.
fn backoff_delay(attempt: u32) -> Duration {
    let ceiling = RETRY_BASE
        .saturating_mul(1u32 << attempt.min(16))
        .min(MAX_RETRY_DELAY);
    let ceiling_ms = u64::try_from(ceiling.as_millis())
        .unwrap_or(u64::MAX)
        .max(1);
    Duration::from_millis(rand::rng().random_range(0..=ceiling_ms))
}

fn base_builder(user_agent: &str) -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .user_agent(user_agent)
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        // A custom redirect policy: cap the hops AND reject any hop whose target
        // is a non-public IP literal (the resolver guard does not run for
        // literals). DNS-name hops are validated by the resolver at connect.
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= MAX_REDIRECTS {
                return attempt.error(format!("exceeded {MAX_REDIRECTS} redirects"));
            }
            if host_is_blocked_literal(attempt.url()) {
                return attempt.error(format!(
                    "{SSRF_MARKER}: redirect target is a non-public IP literal"
                ));
            }
            attempt.follow()
        }))
}

/// Build the **untrusted source** fetch client: the SSRF resolver guard is
/// attached, so a host resolving to a private/metadata address is refused.
///
/// `allow_private` is the dev/test escape hatch (`SP42_FETCH_ALLOW_PRIVATE`) for
/// the loopback-serving benchmark harness; when set, the guard is omitted.
///
/// # Errors
/// Returns an error string if the underlying `reqwest` client fails to build.
pub fn build_source_client(
    user_agent: &str,
    allow_private: bool,
) -> Result<GuardedHttpClient, String> {
    // Disable system proxy discovery on the source face (ADR-0015): a deployment
    // `HTTP_PROXY`/`HTTPS_PROXY` would move target-host resolution into the proxy
    // and bypass the resolver guard. This stays on even under the escape hatch —
    // the hatch relaxes only the address guard, not the other safety limits.
    let mut builder = base_builder(user_agent).no_proxy();
    if !allow_private {
        builder = builder.dns_resolver(Arc::new(GuardedResolver::new(Arc::new(SystemResolver))));
    }
    let client = builder
        .build()
        .map_err(|error| format!("failed to build source fetch client: {error}"))?;
    Ok(GuardedHttpClient {
        client,
        max_source_bytes: MAX_SOURCE_BYTES,
        guard_literals: !allow_private,
    })
}

/// Build the source fetch client, reading the `SP42_FETCH_ALLOW_PRIVATE` dev
/// escape hatch from the environment. This is the single place the env var is
/// read (ADR-0015).
///
/// # Errors
/// Returns an error string if the underlying `reqwest` client fails to build.
pub fn source_client_from_env(user_agent: &str) -> Result<GuardedHttpClient, String> {
    let allow_private = std::env::var("SP42_FETCH_ALLOW_PRIVATE").is_ok_and(|value| value == "1");
    build_source_client(user_agent, allow_private)
}

/// Build the **trusted Wikimedia** fetch client (e.g. Citoid): same transport,
/// no SSRF guard, because the host is hardcoded and not attacker-influenced.
///
/// # Errors
/// Returns an error string if the underlying `reqwest` client fails to build.
pub fn build_wikimedia_client(user_agent: &str) -> Result<GuardedHttpClient, String> {
    let client = base_builder(user_agent)
        .build()
        .map_err(|error| format!("failed to build wikimedia fetch client: {error}"))?;
    Ok(GuardedHttpClient {
        client,
        max_source_bytes: MAX_SOURCE_BYTES,
        guard_literals: true,
    })
}

#[async_trait]
impl HttpClient for GuardedHttpClient {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, HttpClientError> {
        // GET-only: the `HttpMethod` contract models no HEAD variant, so this is
        // stricter than the ADR's "GET/HEAD accepted" and still rejects every
        // body-bearing / state-changing method.
        let HttpMethod::Get = request.method else {
            return Err(HttpClientError::Transport {
                message: format!("read-only fetch only allows GET, got {:?}", request.method),
            });
        };

        // Pre-flight: an IP-literal initial host skips DNS, so the resolver guard
        // never sees it — reject a non-public literal here.
        if self.guard_literals && host_is_blocked_literal(&request.url) {
            return Err(HttpClientError::Transport {
                message: format!(
                    "{SSRF_MARKER}: refusing non-public IP literal {}",
                    request.url
                ),
            });
        }

        // Retry idempotent GETs on transient HTTP statuses, waiting the server's
        // `Retry-After` when present, else exponential backoff + jitter. Transport
        // errors are NOT retried (`send().await?` returns immediately) — a
        // deterministic SSRF rejection surfaces as a transport error and must fail
        // fast rather than burn backoff.
        let mut attempt = 0u32;
        let response = loop {
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
            if attempt < MAX_RETRIES && is_retryable_status(response.status().as_u16()) {
                let delay = retry_after_delay(&response).unwrap_or_else(|| backoff_delay(attempt));
                tokio::time::sleep(delay).await;
                attempt += 1;
                continue;
            }
            break response;
        };

        if response
            .content_length()
            .is_some_and(|len| len > self.max_source_bytes)
        {
            return Err(HttpClientError::Transport {
                message: format!(
                    "response exceeds {}-byte cap (Content-Length)",
                    self.max_source_bytes
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

        let mut response = response;
        let mut body: Vec<u8> = Vec::new();
        while let Some(chunk) =
            response
                .chunk()
                .await
                .map_err(|error| HttpClientError::Transport {
                    message: error.to_string(),
                })?
        {
            if body.len() as u64 + chunk.len() as u64 > self.max_source_bytes {
                return Err(HttpClientError::Transport {
                    message: format!(
                        "response exceeds {}-byte cap (streamed)",
                        self.max_source_bytes
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

#[cfg(test)]
mod tests {
    use std::io::{Read as _, Write as _};
    use std::net::{SocketAddr, TcpListener};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use sp42_types::{HttpClient, HttpMethod, HttpRequest};
    use url::Url;

    use super::GuardedHttpClient;
    use crate::resolver::GuardedResolver;

    fn get(url: &str) -> HttpRequest {
        HttpRequest {
            method: HttpMethod::Get,
            url: Url::parse(url).expect("valid url"),
            headers: std::collections::BTreeMap::new(),
            body: Vec::new(),
        }
    }

    /// A loopback server that replies with `responses[i]` for the i-th connection,
    /// repeating the last entry thereafter. Returns its address.
    fn spawn_sequenced_server(responses: Vec<Vec<u8>>) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        std::thread::spawn(move || {
            let count = AtomicUsize::new(0);
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf);
                let i = count.fetch_add(1, Ordering::SeqCst);
                let reply = responses
                    .get(i)
                    .or_else(|| responses.last())
                    .cloned()
                    .unwrap_or_default();
                let _ = stream.write_all(&reply);
                let _ = stream.flush();
            }
        });
        addr
    }

    /// A client over a pass-through (no SSRF guard) reqwest client that resolves
    /// `host` to a loopback `addr` — for exercising the transport mechanics
    /// (retry, caps) against a loopback server.
    fn loopback_client(host: &str, addr: SocketAddr, max_source_bytes: u64) -> GuardedHttpClient {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::limited(5))
            .resolve(host, addr)
            .build()
            .expect("build client");
        GuardedHttpClient {
            client,
            max_source_bytes,
            guard_literals: true,
        }
    }

    #[tokio::test]
    async fn retries_a_transient_503_then_succeeds() {
        let addr = spawn_sequenced_server(vec![
            b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n".to_vec(),
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok".to_vec(),
        ]);
        let client = loopback_client("flaky.test", addr, 8 * 1024 * 1024);

        let response = client
            .execute(get(&format!("http://flaky.test:{}/", addr.port())))
            .await
            .expect("transient 503 should be retried into a success");

        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"ok");
    }

    #[tokio::test]
    async fn guard_blocks_a_literal_private_ip_pointing_at_a_live_server() {
        // An IP-literal host skips DNS, so the resolver never runs. The client must
        // still refuse a private literal pointing at a live loopback server.
        let server = spawn_sequenced_server(vec![
            b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nleak".to_vec(),
        ]);
        let client = super::build_source_client("test-agent", false).expect("build");
        let error = client
            .execute(get(&format!("http://127.0.0.1:{}/", server.port())))
            .await
            .expect_err("a private IP-literal host must be refused, not connected to");
        assert!(
            format!("{error:?}").contains("SSRF"),
            "rejection must carry the SSRF reason, got: {error:?}"
        );
    }

    #[tokio::test]
    async fn honors_retry_after_over_default_backoff() {
        // 429 with `Retry-After: 3`, then 200. The client must wait ~3s (the
        // server's instruction), well past the sub-second default backoff.
        let addr = spawn_sequenced_server(vec![
            b"HTTP/1.1 429 Too Many Requests\r\nRetry-After: 3\r\nContent-Length: 0\r\n\r\n"
                .to_vec(),
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok".to_vec(),
        ]);
        let client = loopback_client("paced.test", addr, 8 * 1024 * 1024);
        let start = std::time::Instant::now();
        let response = client
            .execute(get(&format!("http://paced.test:{}/", addr.port())))
            .await
            .expect("429 then 200 should succeed");
        let waited = start.elapsed();
        assert_eq!(response.status, 200);
        assert!(
            waited >= std::time::Duration::from_millis(2500),
            "should have waited for Retry-After (~3s), waited {waited:?}"
        );
    }

    #[tokio::test]
    async fn rejects_non_get_methods() {
        let client = loopback_client("x.test", "127.0.0.1:9".parse().expect("addr"), 1024);
        let mut request = get("http://x.test/");
        request.method = HttpMethod::Post;
        let error = client
            .execute(request)
            .await
            .expect_err("POST must be refused");
        assert!(format!("{error:?}").contains("only allows GET"));
    }

    #[tokio::test]
    async fn enforces_size_cap_on_chunked_body() {
        let addr = spawn_sequenced_server(vec![
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n10\r\nAAAAAAAAAAAAAAAA\r\n0\r\n\r\n".to_vec(),
        ]);
        let client = loopback_client("big.test", addr, 8);
        let error = client
            .execute(get(&format!("http://big.test:{}/", addr.port())))
            .await
            .expect_err("oversized chunked body must be rejected");
        assert!(format!("{error:?}").contains("cap"));
    }

    #[tokio::test]
    async fn guard_blocks_a_host_that_resolves_to_loopback() {
        // The SSRF guard, wrapping an inner resolver that returns a loopback
        // address, must refuse the connection — reqwest never reaches the server.
        let server = spawn_sequenced_server(vec![
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nno".to_vec(),
        ]);
        let guard = GuardedResolver::new(Arc::new(super::SystemResolver));
        let client = reqwest::Client::builder()
            .dns_resolver(Arc::new(guard))
            .build()
            .expect("build client");
        let client = GuardedHttpClient {
            client,
            max_source_bytes: 1024,
            guard_literals: true,
        };
        // "localhost" resolves (via the system resolver inside the guard) to a
        // loopback address, which the guard drops — so reqwest never connects to
        // the server and the fetch fails.
        let _ = server;
        let result = client
            .execute(get(&format!("http://localhost:{}/", server.port())))
            .await;
        // Security property: the connection is refused. (The descriptive reason
        // does not survive — reqwest discards the resolver's error detail; see
        // FINDINGS. The literal-host path below keeps a clean reason.)
        assert!(
            result.is_err(),
            "a host resolving to loopback must be blocked by the guard"
        );
    }
}
