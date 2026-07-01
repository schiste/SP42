//! Pure URL helpers for citation verification (ported from wikiharness `urls.ts`).
//!
//! Covers the Parsoid article-HTML endpoint (with an SSRF-guarding wiki-code check
//! before any host interpolation), the `ETag`→revision parse, the Wayback `id_`
//! raw-snapshot rewrite, archive-URL detection, and live-over-archive source resolution.
//! First cut: HTML pages + existing Wayback snapshots only (ADR-0009 §7).

use std::net::{Ipv4Addr, Ipv6Addr};
use std::sync::LazyLock;

use percent_encoding::percent_decode_str;
use regex::Regex;
use url::{Host, Url};

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

/// The basic SSRF floor for fetching an arbitrary cited-source URL (SP42#34): allow only
/// `http`/`https`, and refuse a loopback / private / link-local / unspecified IP literal or
/// `localhost`. Citation URLs come from wiki content (attacker-influenceable), so a source
/// URL pointing at `127.0.0.1`, a private host, or `169.254.169.254` (cloud metadata) must
/// not be fetched.
///
/// This is the *floor*, not the full guard: DNS-resolution-based checks (a hostname that
/// resolves to a private IP) and per-redirect-hop re-checks are deferred to the #34 ADR.
///
/// # Errors
///
/// Returns a human-readable reason when the URL must not be fetched.
pub fn check_fetchable_source_url(url: &Url) -> Result<(), String> {
    match url.scheme() {
        "http" | "https" => {}
        other => return Err(format!("scheme not allowed for source fetch: {other}")),
    }
    match url.host() {
        Some(Host::Ipv4(ip)) if is_blocked_ipv4(ip) => {
            Err(format!("refusing to fetch private/loopback address: {ip}"))
        }
        Some(Host::Ipv6(ip)) if is_blocked_ipv6(ip) => {
            Err(format!("refusing to fetch private/loopback address: {ip}"))
        }
        Some(Host::Domain(domain))
            if {
                let host = domain.trim_end_matches('.').to_ascii_lowercase();
                host == "localhost" || host.ends_with(".localhost")
            } =>
        {
            Err("refusing to fetch localhost".to_string())
        }
        Some(_) => Ok(()),
        None => Err("source URL has no host".to_string()),
    }
}

/// Whether an IPv4 address is in a range that must never be fetched from wiki-supplied input.
fn is_blocked_ipv4(ip: Ipv4Addr) -> bool {
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
}

/// Whether an IPv6 address is loopback / unspecified / unique-local / link-local (or an
/// IPv4-mapped form of a blocked v4 address).
fn is_blocked_ipv6(ip: Ipv6Addr) -> bool {
    if ip.is_loopback() || ip.is_unspecified() {
        return true;
    }
    if let Some(mapped) = ip.to_ipv4_mapped() {
        return is_blocked_ipv4(mapped);
    }
    let first = ip.segments()[0];
    (first & 0xfe00) == 0xfc00 // unique-local fc00::/7
        || (first & 0xffc0) == 0xfe80 // link-local fe80::/10
}

/// A page to verify: the `MediaWiki` page title and a revision (`0` = latest).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageTarget {
    /// The `MediaWiki` page title (namespace-qualified, spaces not underscores).
    pub title: String,
    /// The revision id, or `0` for the latest revision.
    pub rev_id: u64,
}

/// Interpret a user's page reference, accepting either a bare title or a pasted
/// wiki URL. Pasting an article URL is the natural thing to do, but the action
/// API treats a URL as a literal title and reports the page missing, so unwrap it
/// here:
/// - `https://…/wiki/User:Foo/Bar` → title `User:Foo/Bar`;
/// - `https://…/w/index.php?title=Foo&oldid=123` → title `Foo`, `rev_id` `123`.
///
/// Underscores become spaces and percent-escapes are decoded, matching `MediaWiki`
/// title normalization. Anything that is not an `http(s)` URL is treated as a
/// bare title verbatim. The wiki the URL points at is *not* inspected — the caller
/// chooses the wiki separately, and a host mismatch is theirs to reconcile.
#[must_use]
pub fn parse_page_target(input: &str) -> PageTarget {
    let trimmed = input.trim();

    if let Ok(url) = Url::parse(trimmed)
        && matches!(url.scheme(), "http" | "https")
    {
        // `/wiki/<Title>` — the canonical article path; the title may contain
        // slashes (subpages), so take everything after `/wiki/`.
        if let Some(raw) = url.path().strip_prefix("/wiki/")
            && !raw.is_empty()
        {
            return PageTarget {
                title: normalize_title(raw),
                rev_id: 0,
            };
        }

        // `/w/index.php?title=<Title>&oldid=<rev>` — the script path; the title
        // (and an explicit revision) live in the query.
        let mut title = None;
        let mut rev_id = 0;
        for (key, value) in url.query_pairs() {
            match key.as_ref() {
                "title" => title = Some(value.into_owned()),
                "oldid" => rev_id = value.parse().unwrap_or(0),
                _ => {}
            }
        }
        if let Some(title) = title.filter(|title| !title.is_empty()) {
            return PageTarget {
                title: normalize_title(&title),
                rev_id,
            };
        }
    }

    PageTarget {
        title: trimmed.to_string(),
        rev_id: 0,
    }
}

/// Normalize a title extracted from a URL: percent-decode, then map underscores
/// to spaces (`MediaWiki` treats them as equivalent and the action API expects
/// spaces).
fn normalize_title(raw: &str) -> String {
    percent_decode_str(raw)
        .decode_utf8_lossy()
        .replace('_', " ")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        build_article_html_url, check_fetchable_source_url, is_archive_url, is_valid_wiki_code,
        parse_page_target, parse_revision_from_etag, resolve_citation_url, rewrite_wayback_url,
    };
    use url::Url;

    #[test]
    fn parse_page_target_passes_through_a_bare_title() {
        let target = parse_page_target("  User:LuisVilla/SP42 smoke  ");
        assert_eq!(target.title, "User:LuisVilla/SP42 smoke");
        assert_eq!(target.rev_id, 0);
    }

    #[test]
    fn parse_page_target_unwraps_a_wiki_url() {
        // The reported failure: a pasted /wiki/ URL with an underscore subpage.
        let target = parse_page_target("https://en.wikipedia.org/wiki/User:LuisVilla/SP42_smoke");
        assert_eq!(target.title, "User:LuisVilla/SP42 smoke");
        assert_eq!(target.rev_id, 0);
    }

    #[test]
    fn parse_page_target_decodes_percent_escapes() {
        let target = parse_page_target("https://en.wikipedia.org/wiki/Saint-%C3%89tienne");
        assert_eq!(target.title, "Saint-Étienne");
    }

    #[test]
    fn parse_page_target_reads_index_php_title_and_oldid() {
        let target = parse_page_target(
            "https://en.wikipedia.org/w/index.php?title=Samin_Nosrat&oldid=1359520049",
        );
        assert_eq!(target.title, "Samin Nosrat");
        assert_eq!(target.rev_id, 1_359_520_049);
    }

    #[test]
    fn parse_page_target_leaves_non_http_input_alone() {
        // A title that merely looks scheme-ish must not be mistaken for a URL.
        let target = parse_page_target("Template:Citation needed");
        assert_eq!(target.title, "Template:Citation needed");
        assert_eq!(target.rev_id, 0);
    }

    fn blocked(u: &str) -> bool {
        check_fetchable_source_url(&Url::parse(u).expect("valid url")).is_err()
    }

    #[test]
    fn ssrf_floor_blocks_loopback_private_linklocal_and_localhost() {
        for u in [
            "http://127.0.0.1/x",
            "http://127.0.0.1:8080/admin",
            "https://localhost/x",
            "http://localhost:9000/x",
            "http://sub.localhost/x",
            "http://10.0.0.1/x",
            "http://192.168.1.1/x",
            "http://172.16.0.5/x",
            "http://169.254.169.254/latest/meta-data/", // cloud metadata
            "http://0.0.0.0/x",
            "http://[::1]/x",
            "http://[fc00::1]/x",
            "http://[fe80::1]/x",
        ] {
            assert!(blocked(u), "should block {u}");
        }
    }

    #[test]
    fn ssrf_floor_blocks_non_http_schemes() {
        assert!(blocked("ftp://example.com/x"));
        assert!(blocked("file:///etc/passwd"));
    }

    #[test]
    fn ssrf_floor_allows_public_hosts_and_ips() {
        for u in [
            "https://en.wikipedia.org/wiki/Foo",
            "http://example.com/page",
            "https://8.8.8.8/x",
            "https://93.184.216.34/x", // a public IP
        ] {
            assert!(
                check_fetchable_source_url(&Url::parse(u).expect("url")).is_ok(),
                "should allow {u}"
            );
        }
    }

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
