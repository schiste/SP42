//! The rules-compliant read-only fetch edge (ADR-0015).
//!
//! One guarded [`sp42_types::HttpClient`] over `reqwest`: SSRF enforced in a
//! custom DNS resolver (resolved-IP `is_global` check, closing the
//! DNS-rebinding gap), a redirect cap, a streamed response-size cap, request
//! timeouts, retry/backoff for transient failures, and the Wikimedia-compliant
//! User-Agent. The untrusted **source** face attaches the guarded resolver; the
//! trusted **Wikimedia** face uses the default resolver.

mod client;
mod resolver;

pub use client::{
    GuardedHttpClient, build_source_client, build_wikimedia_client, source_client_from_env,
};
