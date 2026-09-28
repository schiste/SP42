//! The rules-compliant read-only fetch edge (ADR-0015).
//!
//! One guarded [`sp42_types::HttpClient`] over `reqwest`: SSRF enforced in a
//! custom DNS resolver (resolved-IP `is_global` check, closing the
//! DNS-rebinding gap), a redirect cap, a streamed response-size cap, request
//! timeouts, retry/backoff for transient failures, and the Wikimedia-compliant
//! User-Agent. The untrusted **source** face attaches the guarded resolver; the
//! trusted **Wikimedia** face uses the default resolver.
//!
//! This crate also owns the second half of the same problem: the *content* that
//! comes back over a permitted connection. [`sanitize_rendered_html`] is the
//! allowlist that rendered wiki-revision HTML must pass before it reaches a
//! review surface (ADR-0032). Keeping both halves here is deliberate — a
//! connection allowed by the resolver and markup allowed by the sanitizer are
//! the same trust boundary, and neither is useful without the other.

mod client;
mod html;
mod resolver;

pub use client::{
    GuardedHttpClient, build_source_client, build_wikimedia_client, source_client_from_env,
};
pub use html::{DEFAULT_MAX_BYTES, sanitize_rendered_html, sanitize_with_limits};
