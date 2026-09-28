//! Response security headers (ADR-0033).
//!
//! The browser shell and the localhost server share an origin, and the shell
//! holds the operator's session. These headers are defense in depth: they do not
//! replace the rendered-HTML allowlist (ADR-0032) — they contain the blast radius
//! if markup or a dependency ever produces script that the allowlist did not
//! intend to allow.
//!
//! # Why the inline-script source is still present
//!
//! The policy keeps the CSP inline-script source in `script-src` because the
//! Leptos/Trunk bundle bootstraps through an inline module script. Removing it
//! is blocked on a bundler-level change, not a policy decision, so it is tracked
//! as the primary follow-up in ADR-0033 rather than silently shipped. The
//! consequential parts — `object-src 'none'`, `frame-ancestors 'none'`,
//! `base-uri 'self'`, `form-action 'self'`, and a `connect-src` that does not
//! blanket-allow `https:` — are all in place now.

use axum::http::{HeaderValue, header};
use tower_http::set_header::SetResponseHeaderLayer;

/// `Content-Security-Policy` for the browser shell and API responses.
///
/// Kept as a function returning a fresh `HeaderValue` because the value is
/// built once at router construction and `HeaderValue` is not `Clone`-cheap to
/// share across concurrent responses.
#[must_use]
pub fn content_security_policy() -> HeaderValue {
    // The inline-script source is required by the current Trunk/Leptos
    // bootstrap and is a known, tracked gap (ADR-0033). Everything else is
    // restrictive: no plugins, no framing, no form hijacking, no base-URI
    // rewriting, and connect-src limited to the local API plus the Wikimedia
    // and inference hosts the tool actually talks to.
    const POLICY: &str = "\
default-src 'self'; \
script-src 'self' 'unsafe-inline'; \
style-src 'self' 'unsafe-inline'; \
img-src 'self' data: blob: https:; \
font-src 'self' data:; \
connect-src 'self' https://*.wikimedia.org https://*.wikipedia.org wss://*.wikimedia.org ws://127.0.0.1:* ws://localhost:*; \
object-src 'none'; \
frame-src 'none'; \
frame-ancestors 'none'; \
base-uri 'self'; \
form-action 'self'";

    HeaderValue::from_static(POLICY)
}

/// The layer that applies [`content_security_policy`] to every response.
#[must_use]
pub fn csp_layer() -> SetResponseHeaderLayer<HeaderValue> {
    SetResponseHeaderLayer::if_not_present(
        header::CONTENT_SECURITY_POLICY,
        content_security_policy(),
    )
}

/// Response headers that are not CSP but belong to the same layer.
///
/// `X-Content-Type-Options: nosniff` stops a browser from re-interpreting a
/// response as an executable type; `Referrer-Policy: no-referrer` keeps wiki
/// revision paths out of outbound `Referer` headers.
///
/// `if_not_present` throughout: nothing here may clobber a header a route set
/// deliberately.
#[must_use]
pub fn hardening_layers() -> Vec<SetResponseHeaderLayer<HeaderValue>> {
    vec![
        SetResponseHeaderLayer::if_not_present(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ),
        SetResponseHeaderLayer::if_not_present(
            header::REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::content_security_policy;
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use tower::ServiceExt;

    /// Build a router with only the security-header layer, and return the
    /// response for a GET.
    async fn response_for_request() -> axum::response::Response {
        let app = axum::Router::new()
            .fallback(|| async { "ok" })
            .layer(super::csp_layer());
        app.oneshot(
            Request::builder()
                .uri("/")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("router responds")
    }

    fn policy_of(response: &axum::response::Response) -> String {
        response
            .headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .expect("CSP header is set")
            .to_str()
            .expect("CSP is ascii")
            .to_string()
    }

    #[tokio::test]
    async fn every_response_carries_a_csp() {
        let response = response_for_request().await;
        assert_eq!(response.status(), StatusCode::OK);
        let policy = policy_of(&response);
        assert!(policy.contains("default-src 'self'"), "{policy}");
    }

    #[tokio::test]
    async fn csp_blocks_the_consequential_active_content_vectors() {
        let policy = policy_of(&response_for_request().await);
        // Plugins, framing, and base-URI rewriting are the three that turn a
        // markup gap into script execution or a takeover, and all three are off.
        assert!(policy.contains("object-src 'none'"), "{policy}");
        assert!(policy.contains("frame-src 'none'"), "{policy}");
        assert!(policy.contains("frame-ancestors 'none'"), "{policy}");
        assert!(policy.contains("base-uri 'self'"), "{policy}");
        assert!(policy.contains("form-action 'self'"), "{policy}");
    }

    #[test]
    fn connect_src_does_not_blanket_allow_https() {
        // The desktop Tauri config allows `connect-src ... https:`, which lets
        // injected script exfiltrate to any host. The server policy must not.
        // Wildcard *subdomains* are fine and necessary (per-wiki API hosts);
        // a bare `https:` scheme-source is not.
        let policy = content_security_policy()
            .to_str()
            .expect("ascii")
            .to_string();
        let connect = policy
            .split("connect-src ")
            .nth(1)
            .and_then(|rest| rest.split(';').next())
            .expect("connect-src directive present");
        for scheme_source in connect.split_whitespace() {
            assert_ne!(
                scheme_source, "https:",
                "connect-src must not allow all https"
            );
            assert_ne!(
                scheme_source, "http:",
                "connect-src must not allow all http"
            );
            assert_ne!(scheme_source, "*", "connect-src must not allow any host");
        }
        assert!(connect.contains("https://*.wikimedia.org"), "{connect}");
    }

    #[test]
    fn policy_is_a_valid_single_header_value() {
        // `to_str` succeeding proves the value is visible ASCII, so it can be
        // sent verbatim without multi-header splitting.
        let policy = content_security_policy()
            .to_str()
            .expect("ascii")
            .to_string();
        assert!(!policy.contains('\n'));
        assert!(!policy.contains('\r'));
    }
}
