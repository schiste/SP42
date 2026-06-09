//! The Citoid bibliographic-metadata sidecar (#12 in wikiharness; ADR-0007 Alt (e),
//! ADR-0009 §3).
//!
//! Citoid metadata (title / author / publication / date) is fetched best-effort and
//! rendered to the model as **context only — never quote from here** (see
//! [`crate::citation::prompts`]). It is structurally kept **outside** the grounding
//! boundary: the anti-fabrication gate hashes and locates **only** the fetched source
//! body, never this metadata, so a model can never "ground" a quote in a title/author
//! line (the wikiharness `prependMetadataHeader` failure, deleted; ADR-0007 Alt (e)).
//! Citoid never blocks verification — any failure yields `None`.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use url::Url;

use crate::types::{HttpMethod, HttpRequest};

/// The Citoid REST endpoint (Zotero base-fields shape).
const CITOID_ENDPOINT: &str =
    "https://en.wikipedia.org/api/rest_v1/data/citation/mediawiki-basefields/";

/// The endpoint parsed once, for the (unreachable) fallback when an encoded segment
/// somehow fails to parse — keeps [`build_citoid_request`] panic-free.
static CITOID_BASE: LazyLock<Url> =
    LazyLock::new(|| CITOID_ENDPOINT.parse().expect("static endpoint parses"));

/// Bibliographic metadata for one source — a verification-context sidecar.
///
/// Carried for prompt context and operator display only; **never** part of the
/// content-addressed grounded bytes (ADR-0007 Alt (e), ADR-0009 §3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CitoidMetadata {
    /// Publication / website title (`publicationTitle`, falling back to `websiteTitle`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publication: Option<String>,
    /// Publication date (`date`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published: Option<String>,
    /// Author(s), formatted (`author`, falling back to `creators`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Source title (`title`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The source URL this metadata describes (echoed; not rendered in the prompt).
    pub url: String,
}

/// Build the (read-only GET) Citoid request for a source URL.
///
/// The URL is appended as an `encodeURIComponent`-encoded path segment. Queried on the
/// **original** citation URL, never the Wayback-rewritten form (Citoid resolves the real
/// publication better).
#[must_use]
pub fn build_citoid_request(source_url: &str) -> HttpRequest {
    let url_string = format!("{CITOID_ENDPOINT}{}", encode_uri_component(source_url));
    // The endpoint + an encoded path segment always parses; fall back defensively to
    // the bare endpoint (never panics).
    let url = url_string.parse().unwrap_or_else(|_| CITOID_BASE.clone());
    HttpRequest {
        method: HttpMethod::Get,
        url,
        headers: BTreeMap::new(),
        body: Vec::new(),
    }
}

/// Parse a Citoid response body into the first citation object, or `None` on any
/// failure (invalid JSON, not an array, empty, or first element not an object).
#[must_use]
pub fn parse_citoid_response(body: &[u8]) -> Option<Map<String, Value>> {
    let parsed: Value = serde_json::from_slice(body).ok()?;
    let array = parsed.as_array()?;
    let first = array.first()?;
    first.as_object().cloned()
}

/// Build the metadata sidecar from a parsed Citoid object, or `None` when no
/// meaningful field is present (a bare URL alone is not worth a header).
#[must_use]
pub fn build_citoid_header(raw: &Map<String, Value>, source_url: &str) -> Option<CitoidMetadata> {
    let publication = raw
        .get("publicationTitle")
        .and_then(as_string)
        .or_else(|| raw.get("websiteTitle").and_then(as_string));
    let published = raw.get("date").and_then(as_string);
    let author = raw
        .get("author")
        .and_then(format_authors)
        .or_else(|| raw.get("creators").and_then(format_authors));
    let title = raw.get("title").and_then(as_string);

    if publication.is_none() && published.is_none() && author.is_none() && title.is_none() {
        return None;
    }

    Some(CitoidMetadata {
        publication,
        published,
        author,
        title,
        url: source_url.to_string(),
    })
}

/// The string value of a JSON field iff it is a non-empty (after trim) string.
fn as_string(value: &Value) -> Option<String> {
    value
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .map(ToString::to_string)
}

/// Format a Citoid `author` / `creators` value: each entry is either an array of name
/// parts (joined with a space) or a plain string; empties dropped; names joined `", "`.
fn format_authors(value: &Value) -> Option<String> {
    let array = value.as_array()?;
    let mut names = Vec::new();
    for entry in array {
        let name = match entry {
            Value::Array(parts) => parts
                .iter()
                .filter_map(Value::as_str)
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" "),
            Value::String(s) => s.clone(),
            _ => String::new(),
        };
        if !name.is_empty() {
            names.push(name);
        }
    }
    if names.is_empty() {
        None
    } else {
        Some(names.join(", "))
    }
}

/// Encode `input` per JavaScript `encodeURIComponent` (unreserved set
/// `A-Za-z0-9 - _ . ! ~ * ' ( )` pass through; everything else `%XX`).
fn encode_uri_component(input: &str) -> String {
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
        build_citoid_header, build_citoid_request, encode_uri_component, parse_citoid_response,
    };
    use crate::types::HttpMethod;
    use serde_json::json;

    #[test]
    fn build_request_encodes_the_url_as_a_path_segment() {
        let request = build_citoid_request("https://example.com/a?b=c");
        assert_eq!(request.method, HttpMethod::Get);
        let expected = format!(
            "https://en.wikipedia.org/api/rest_v1/data/citation/mediawiki-basefields/{}",
            encode_uri_component("https://example.com/a?b=c")
        );
        assert_eq!(request.url.as_str(), expected);
        assert!(request.url.as_str().contains("https%3A%2F%2Fexample.com"));
    }

    #[test]
    fn parse_returns_first_object_of_the_array() {
        let body = br#"[{"title":"Headline"},{"title":"Other"}]"#;
        let first = parse_citoid_response(body).expect("first object");
        assert_eq!(
            first.get("title").and_then(serde_json::Value::as_str),
            Some("Headline")
        );
    }

    #[test]
    fn parse_rejects_empty_array_non_array_and_garbage() {
        assert!(parse_citoid_response(b"[]").is_none());
        assert!(parse_citoid_response(br#"{"title":"x"}"#).is_none());
        assert!(parse_citoid_response(b"<html>error</html>").is_none());
    }

    #[test]
    fn build_header_maps_all_fields() {
        let raw = json!({
            "publicationTitle": "The Guardian",
            "date": "2020-01-01",
            "author": [["Jane", "Doe"], ["John", "Smith"]],
            "title": "Headline"
        });
        let header = build_citoid_header(raw.as_object().expect("object"), "https://example.com/a")
            .expect("header");
        assert_eq!(header.publication.as_deref(), Some("The Guardian"));
        assert_eq!(header.published.as_deref(), Some("2020-01-01"));
        assert_eq!(header.author.as_deref(), Some("Jane Doe, John Smith"));
        assert_eq!(header.title.as_deref(), Some("Headline"));
        assert_eq!(header.url, "https://example.com/a");
    }

    #[test]
    fn build_header_falls_back_to_website_title_and_creators() {
        let raw = json!({ "websiteTitle": "Example", "creators": [["Ada", "Lovelace"]] });
        let header = build_citoid_header(raw.as_object().expect("object"), "https://example.com")
            .expect("header");
        assert_eq!(header.publication.as_deref(), Some("Example"));
        assert_eq!(header.author.as_deref(), Some("Ada Lovelace"));
    }

    #[test]
    fn build_header_is_none_without_meaningful_fields() {
        let raw = json!({ "accessDate": "2026-06-02" });
        assert!(
            build_citoid_header(raw.as_object().expect("object"), "https://example.com").is_none()
        );
        let empty = json!({});
        assert!(
            build_citoid_header(empty.as_object().expect("object"), "https://example.com")
                .is_none()
        );
    }
}
