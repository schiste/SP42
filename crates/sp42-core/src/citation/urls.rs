//! Pure URL helpers for citation verification (ported from wikiharness `urls.ts`).
//!
//! Covers the Parsoid article-HTML endpoint (with an SSRF-guarding wiki-code check
//! before any host interpolation), the `ETag`→revision parse, the Wayback `id_`
//! raw-snapshot rewrite, archive-URL detection, and live-over-archive source resolution.
//! First cut: HTML pages + existing Wayback snapshots only (ADR-0009 §7).

use std::sync::LazyLock;

use regex::Regex;
use url::Url;

use crate::errors::CitationVerificationError;

/// Accepted wiki language/site codes — the SSRF guard before host interpolation.
static WIKI_CODE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^[a-z]{2,3}(-[a-z]{2,8})?$|^(?:simple|test|test2|beta|commons|meta|species|incubator)$",
    )
    .expect("valid regex")
});
/// First run of >=2 digits inside the `ETag` quotes, terminated by `/` or `"`.
static ETAG_REVISION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""(\d{2,})[/"]"#).expect("valid regex"));
/// A `web.archive.org/web/<14-digit-timestamp>/` prefix, anchored at the start.
static WAYBACK_PREFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(https?://web\.archive\.org/web/\d{14})/").expect("valid regex")
});
/// Archive host suffixes (host-based, not substring).
static ARCHIVE_HOST_SUFFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:^|\.)(?:web\.archive\.org|webcitation\.org|archive\.(?:today|ph|is|li))$")
        .expect("valid regex")
});

/// A resolved source URL plus whether it is an archive snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedUrl {
    /// The chosen source URL.
    pub url: String,
    /// `true` if the chosen URL is an archive snapshot (no live URL was available).
    pub is_archive: bool,
}

/// `true` if `wiki` is an accepted wiki code (must hold before host interpolation).
#[must_use]
pub fn is_valid_wiki_code(wiki: &str) -> bool {
    WIKI_CODE.is_match(wiki)
}

/// Build the Parsoid REST HTML URL for an article (optionally pinned to a revision).
///
/// # Errors
///
/// Returns [`CitationVerificationError::InvalidRequest`] if `wiki` is not an accepted
/// wiki code (the SSRF guard).
pub fn build_article_html_url(
    wiki: &str,
    title: &str,
    revision: Option<u64>,
) -> Result<String, CitationVerificationError> {
    if !is_valid_wiki_code(wiki) {
        return Err(CitationVerificationError::InvalidRequest {
            message: format!("invalid wiki code {wiki:?}"),
        });
    }
    let base = format!(
        "https://{wiki}.wikipedia.org/api/rest_v1/page/html/{}",
        encode_uri_component(title)
    );
    Ok(match revision {
        Some(revision) => format!("{base}/{revision}"),
        None => base,
    })
}

/// Parse the `MediaWiki` revision id from a REST `ETag` header, or `None`.
#[must_use]
pub fn parse_revision_from_etag(etag: &str) -> Option<u64> {
    ETAG_REVISION
        .captures(etag)
        .and_then(|caps| caps.get(1))
        .and_then(|m| m.as_str().parse().ok())
}

/// Rewrite a Wayback URL to its raw `id_` form (serving the archived page without
/// Wayback wrapper chrome). Idempotent; non-Wayback URLs pass through unchanged.
#[must_use]
pub fn rewrite_wayback_url(url: &str) -> String {
    WAYBACK_PREFIX.replace(url, "${1}id_/").into_owned()
}

/// `true` if `url` is an archive snapshot (host-based detection; `archive.org` counts
/// only when the path is a `/web/` snapshot).
#[must_use]
pub fn is_archive_url(url: &str) -> bool {
    let Ok(parsed) = Url::parse(url) else {
        return false;
    };
    let Some(host) = parsed.host_str() else {
        return false;
    };
    let host = host.to_lowercase();
    if ARCHIVE_HOST_SUFFIX.is_match(&host) {
        return true;
    }
    (host == "archive.org" || host.ends_with(".archive.org")) && parsed.path().starts_with("/web/")
}

/// Choose which source URL to fetch: prefer the first live (non-archive) URL, else fall
/// back to the first URL (treated as an archive), else `None`.
#[must_use]
pub fn resolve_citation_url(source_urls: &[String]) -> Option<ResolvedUrl> {
    if let Some(live) = source_urls
        .iter()
        .find(|candidate| !is_archive_url(candidate))
    {
        return Some(ResolvedUrl {
            url: live.clone(),
            is_archive: false,
        });
    }
    source_urls.first().map(|archive| ResolvedUrl {
        url: archive.clone(),
        is_archive: true,
    })
}

/// Encode `input` per JavaScript `encodeURIComponent` (unreserved set
/// `A-Za-z0-9 - _ . ! ~ * ' ( )` pass through; everything else `%XX`).
#[must_use]
pub(crate) fn encode_uri_component(input: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(input.len());
    for &byte in input.as_bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')'
            )
        {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{
        build_article_html_url, is_archive_url, is_valid_wiki_code, parse_revision_from_etag,
        resolve_citation_url, rewrite_wayback_url,
    };

    #[test]
    fn builds_article_html_url() {
        assert_eq!(
            build_article_html_url("en", "Markdown", None).expect("valid"),
            "https://en.wikipedia.org/api/rest_v1/page/html/Markdown"
        );
        assert_eq!(
            build_article_html_url("en", "Foo Bar", Some(123)).expect("valid"),
            "https://en.wikipedia.org/api/rest_v1/page/html/Foo%20Bar/123"
        );
    }

    #[test]
    fn invalid_wiki_code_is_rejected() {
        assert!(build_article_html_url("evil.com/", "X", None).is_err());
    }

    #[test]
    fn wiki_code_validation() {
        for ok in ["en", "simple", "pt", "zh-yue", "commons"] {
            assert!(is_valid_wiki_code(ok), "{ok} should be valid");
        }
        for bad in [
            "evil.com/",
            "x.attacker.com#",
            "en.wikipedia.org.evil",
            "a/b",
            "../../etc",
        ] {
            assert!(!is_valid_wiki_code(bad), "{bad} should be invalid");
        }
    }

    #[test]
    fn parse_revision_from_etag_cases() {
        assert_eq!(
            parse_revision_from_etag("W/\"1353541055/f8b582cf-5e1a-11f1/view/html\""),
            Some(1_353_541_055)
        );
        assert_eq!(parse_revision_from_etag("\"no-digits/here\""), None);
        assert_eq!(
            parse_revision_from_etag("\"2024-06-01/no-revid-here\""),
            None
        );
    }

    #[test]
    fn rewrite_wayback_adds_id_flag() {
        assert_eq!(
            rewrite_wayback_url("https://web.archive.org/web/20200101120000/http://example.com/a"),
            "https://web.archive.org/web/20200101120000id_/http://example.com/a"
        );
        assert_eq!(
            rewrite_wayback_url("http://web.archive.org/web/20200101120000/http://example.com/a"),
            "http://web.archive.org/web/20200101120000id_/http://example.com/a"
        );
    }

    #[test]
    fn rewrite_wayback_is_idempotent_and_anchored() {
        let already = "https://web.archive.org/web/20200101120000id_/http://example.com/a";
        assert_eq!(rewrite_wayback_url(already), already);
        let other_flag = "https://web.archive.org/web/20200101120000im_/http://example.com/a";
        assert_eq!(rewrite_wayback_url(other_flag), other_flag);
        let mimic = "https://example.com/web/20200101120000/x";
        assert_eq!(rewrite_wayback_url(mimic), mimic);
    }

    #[test]
    fn archive_url_detection() {
        for archive in [
            "https://web.archive.org/web/20200101/http://example.com",
            "https://archive.today/abc",
            "https://archive.ph/xyz",
            "https://archive.org/web/20200101/http://example.com",
        ] {
            assert!(is_archive_url(archive), "{archive} should be archive");
        }
        for live in [
            "https://archive.org/details/somebook",
            "https://example.org/web.archive.org/notreally",
            "https://archive.org.evil.example/x",
            "not a url",
        ] {
            assert!(!is_archive_url(live), "{live} should not be archive");
        }
    }

    #[test]
    fn resolve_prefers_live_then_archive_then_none() {
        let mixed = vec![
            "https://web.archive.org/web/2020/http://x.com".to_string(),
            "https://live.example.com/a".to_string(),
        ];
        let resolved = resolve_citation_url(&mixed).expect("resolved");
        assert_eq!(resolved.url, "https://live.example.com/a");
        assert!(!resolved.is_archive);

        let only_archive = vec!["https://web.archive.org/web/2020/http://x.com".to_string()];
        let resolved = resolve_citation_url(&only_archive).expect("resolved");
        assert!(resolved.is_archive);

        assert!(resolve_citation_url(&[]).is_none());
    }
}
