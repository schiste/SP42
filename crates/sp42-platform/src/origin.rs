//! Canonical URL-origin helpers shared by the server and the wasm app.
//!
//! Origin comparisons must never be string prefix / `contains` checks — the URL
//! boundaries (`:`, `.`, `/`) are exactly where edge cases and open-redirect
//! tricks live (`:4173` vs `:41730`, `app.example.org` vs `app.example.org.evil`).
//! These parse into a canonical `scheme://host[:port]` form and compare
//! structurally, covering HTTP(S) and custom app schemes alike (e.g.
//! `tauri://localhost`). This is the single origin primitive; per-call-site
//! copies previously drifted and each regrew the same bug.

use url::Url;

/// The canonical `scheme://host[:port]` origin of a URL string (default ports
/// omitted), or `None` when it has no network authority (e.g. `javascript:`,
/// `mailto:`, a bare path).
#[must_use]
pub fn origin_of(url: &str) -> Option<String> {
    origin_of_url(&Url::parse(url).ok()?)
}

/// Like [`origin_of`] but for an already-parsed [`Url`], avoiding a re-parse.
#[must_use]
pub fn origin_of_url(url: &Url) -> Option<String> {
    let host = url.host_str()?;
    let scheme = url.scheme();
    Some(match url.port() {
        Some(port) => format!("{scheme}://{host}:{port}"),
        None => format!("{scheme}://{host}"),
    })
}

/// Whether two URL strings resolve to the same origin.
#[must_use]
pub fn origins_match(left: &str, right: &str) -> bool {
    match (origin_of(left), origin_of(right)) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{origin_of, origins_match};

    #[test]
    fn extracts_canonical_origin() {
        assert_eq!(
            origin_of("https://app.example.org/path?q=1#x").as_deref(),
            Some("https://app.example.org")
        );
        // explicit default port collapses to the canonical (port-less) origin
        assert_eq!(
            origin_of("https://app.example.org:443/x").as_deref(),
            Some("https://app.example.org")
        );
        assert_eq!(
            origin_of("http://localhost:4173/").as_deref(),
            Some("http://localhost:4173")
        );
        // custom app scheme with an authority (desktop webview)
        assert_eq!(
            origin_of("tauri://localhost/?wiki=dewiki").as_deref(),
            Some("tauri://localhost")
        );
    }

    #[test]
    fn rejects_authority_less_targets() {
        assert!(origin_of("javascript:alert(1)").is_none());
        assert!(origin_of("mailto:a@b.c").is_none());
        assert!(origin_of("/just/a/path").is_none());
        assert!(origin_of("not-a-url").is_none());
    }

    #[test]
    fn origins_match_compares_parsed_origins_not_prefixes() {
        assert!(origins_match(
            "https://app.example.org",
            "https://app.example.org/"
        ));
        // port prefix pitfall: 4173 is a prefix of 41730 but a different origin
        assert!(!origins_match(
            "http://localhost:41730",
            "http://localhost:4173"
        ));
        // host suffix pitfall
        assert!(!origins_match(
            "https://app.example.org.evil",
            "https://app.example.org"
        ));
        assert!(!origins_match(
            "https://api.example.org",
            "https://app.example.org"
        ));
    }
}
