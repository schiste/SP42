//! Internet Archive search-inside grounding source (PRD-0009 Layer 2,
//! ADR-0024 Decision 4).
//!
//! Turns a resolved exact-edition scan into a **book-snippet source body** the
//! existing verifier can judge: read the item's metadata for its designated
//! server and directory, query the `BookReader` full-text search, and assemble
//! the returned **verbatim OCR snippets** (page-numbered) into a body. The
//! scan's OCR is never downloaded whole — snippet search typically works even
//! for lending-restricted scans, so grounding needs no borrow.
//!
//! Pure `build_*`/`parse_*` pairs over the injected `HttpClient`, like the
//! Open Library module ([`crate::citation::openlibrary`]). Outcome semantics
//! (ADR-0007 discipline, PRD-0009 resolved Q4):
//! - **No usable body** (not a text item / no index / metadata unusable) →
//!   `SourceUnavailable`. SP42 could not read the book.
//! - **Searched, nothing found** on an indexed scan → `not_supported` (never
//!   `SourceUnavailable`): the source was searched and yielded no passage.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use crate::citation::verify::sha256_hex;
use crate::types::{HttpMethod, HttpRequest};
use sp42_types::HttpClient;

/// The archive.org item-metadata endpoint (read-only; names the item's
/// designated `server` and `dir` the full-text search runs on).
pub const ARCHIVE_METADATA_BASE: &str = "https://archive.org/metadata";

/// The metadata endpoint parsed once, for the (unreachable) fallback when a
/// built URL somehow fails to parse — keeps the builder panic-free.
static METADATA_BASE: LazyLock<Url> = LazyLock::new(|| {
    ARCHIVE_METADATA_BASE
        .parse()
        .expect("static endpoint parses")
});

/// Most matches carried into an assembled snippet body. Bounds the body (and
/// the prompt it feeds) on common-word queries with hundreds of hits.
const MAX_SNIPPET_MATCHES: usize = 8;

/// Most claim terms carried into the search query.
const MAX_QUERY_TERMS: usize = 6;

/// The archive.org item id (`ocaid`) from a Read API `itemURL`, e.g.
/// `https://archive.org/details/matilda00dahl` → `matilda00dahl`.
#[must_use]
pub fn extract_ocaid(item_url: &str) -> Option<String> {
    let url = Url::parse(item_url).ok()?;
    if url.host_str() != Some("archive.org") && url.host_str() != Some("www.archive.org") {
        return None;
    }
    let mut segments = url.path_segments()?;
    if segments.next() != Some("details") {
        return None;
    }
    segments
        .next()
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
}

/// Build the (read-only GET) item-metadata request for a scan.
#[must_use]
pub fn build_item_metadata_request(ocaid: &str) -> HttpRequest {
    let url_string = format!("{ARCHIVE_METADATA_BASE}/{ocaid}");
    let url = url_string.parse().unwrap_or_else(|_| METADATA_BASE.clone());
    HttpRequest {
        method: HttpMethod::Get,
        url,
        headers: BTreeMap::new(),
        body: Vec::new(),
    }
}

/// Where an item's full-text search runs: the metadata-designated server and
/// item directory, plus whether the item is a text item at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemLocation {
    /// Designated server host, e.g. `ia800300.us.archive.org`.
    pub server: String,
    /// Item directory on that server, e.g. `/12/items/matilda00dahl`.
    pub dir: String,
    /// `true` iff `metadata.mediatype` is `texts` — the groundable-item gate.
    pub is_text_item: bool,
}

/// Parse an item-metadata response into the search location. `None` for an
/// unparseable body or one missing the server/dir designation.
#[must_use]
pub fn parse_item_metadata(body: &[u8]) -> Option<ItemLocation> {
    let parsed: Value = serde_json::from_slice(body).ok()?;
    let server = parsed.get("server").and_then(Value::as_str)?;
    let dir = parsed.get("dir").and_then(Value::as_str)?;
    let is_text_item = parsed
        .pointer("/metadata/mediatype")
        .and_then(Value::as_str)
        == Some("texts");
    Some(ItemLocation {
        server: server.to_string(),
        dir: dir.to_string(),
        is_text_item,
    })
}

/// Build the (read-only GET) `BookReader` full-text search request against the
/// item's designated server. `None` when the metadata-supplied server does
/// not form a valid URL (treated as no-usable-body by the caller).
#[must_use]
pub fn build_search_inside_request(
    location: &ItemLocation,
    ocaid: &str,
    query: &str,
) -> Option<HttpRequest> {
    let base = format!("https://{}/fulltext/inside.php", location.server);
    let url = Url::parse_with_params(
        &base,
        &[
            ("item_id", ocaid),
            ("doc", ocaid),
            ("path", location.dir.as_str()),
            ("q", query),
        ],
    )
    .ok()?;
    Some(HttpRequest {
        method: HttpMethod::Get,
        url,
        headers: BTreeMap::new(),
        body: Vec::new(),
    })
}

/// One search-inside match: the verbatim OCR snippet (match markers stripped)
/// and the scanned page it was found on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchInsideMatch {
    /// Verbatim OCR snippet text, with the `{{{…}}}` highlight markers
    /// removed so the text is exactly what the OCR holds (groundable bytes).
    pub text: String,
    /// The scanned page number, when the match carries one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
}

/// A parsed search-inside response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchInsideResult {
    /// `false` when the item has no full-text index (search cannot run).
    pub indexed: bool,
    pub matches: Vec<SearchInsideMatch>,
}

/// Parse a search-inside response. `None` for an unparseable body.
#[must_use]
pub fn parse_search_inside(body: &[u8]) -> Option<SearchInsideResult> {
    let parsed: Value = serde_json::from_slice(body).ok()?;
    let object = parsed.as_object()?;
    // The BookReader search reports an un-indexed item either explicitly
    // (`"indexed": false`) or by erroring without a matches array.
    let indexed = object
        .get("indexed")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| object.contains_key("matches"));
    let matches = object
        .get("matches")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| {
                    let text = entry.get("text").and_then(Value::as_str)?;
                    let page = entry
                        .pointer("/par/0/page")
                        .and_then(Value::as_u64)
                        .and_then(|p| u32::try_from(p).ok());
                    Some(SearchInsideMatch {
                        text: text.replace("{{{", "").replace("}}}", ""),
                        page,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Some(SearchInsideResult { indexed, matches })
}

/// A conservative full-text query from a claim sentence: the distinct longer
/// words (≥ 5 chars, falling back to ≥ 4, then the trimmed claim), in claim
/// order, capped at `MAX_QUERY_TERMS`. Deliberately simple — term selection
/// quality only affects recall, never grounding (the snippet bytes are what
/// the gate locates quotes in); smarter selection can come later.
#[must_use]
pub fn search_query(claim: &str) -> String {
    let words: Vec<&str> = claim
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    for floor in [5, 4] {
        let mut seen = Vec::new();
        for word in &words {
            if word.chars().count() >= floor && !seen.contains(word) {
                seen.push(word);
                if seen.len() == MAX_QUERY_TERMS {
                    break;
                }
            }
        }
        if !seen.is_empty() {
            return seen.join(" ");
        }
    }
    claim.trim().to_string()
}

/// A page-anchored deep link into the scan, with the search terms highlighted
/// — the operator's jump to the page that supports (or contradicts) the claim.
#[must_use]
pub fn scan_deep_link(ocaid: &str, page: Option<u32>, query: &str) -> String {
    let base = match page {
        Some(page) => format!("https://archive.org/details/{ocaid}/page/{page}"),
        None => format!("https://archive.org/details/{ocaid}"),
    };
    Url::parse_with_params(&base, &[("q", query)]).map_or(base, |url| url.to_string())
}

/// What the grounding preparation produced for one book claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookGroundingPreparation {
    /// Snippets found: a body for the verifier plus page provenance.
    Body(BookSnippetBody),
    /// The scan is indexed and was searched (both passes), zero snippets →
    /// the caller reports `not_supported` (PRD-0009 resolved Q4).
    NoMatches {
        /// SHA-256 of the search response — the `SourceFetched` grounding.
        response_hash: String,
        /// Deep link into the scan (no page anchor — nothing located).
        deep_link: String,
    },
    /// Search-inside cannot run: not a text item, no full-text index, or the
    /// item metadata was unusable → `SourceUnavailable` (unusable).
    NoUsableBody {
        /// Which precondition failed, for the report.
        detail: &'static str,
    },
    /// A transport failure before an answer → `SourceUnavailable`
    /// (unreachable).
    Unreachable { message: String },
}

/// The assembled book-snippet source body plus its provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookSnippetBody {
    /// The verbatim snippets, joined by blank lines — the groundable bytes.
    pub text: String,
    /// The matches the body was assembled from (page attribution).
    pub matches: Vec<SearchInsideMatch>,
    /// `true` when the cited-page pass matched; `false` means the whole-book
    /// fallback supplied the snippets (pagination-mismatch signal).
    pub cited_page_hit: bool,
    /// The search query that produced the snippets (deep-link highlighting).
    pub query: String,
}

impl BookSnippetBody {
    /// The scanned page of the match containing `passage`, falling back to
    /// the first match's page. Scan pagination often differs from the cited
    /// edition, so the report shows where the passage was actually found.
    ///
    /// Matching uses the grounding locator (exact, then normalized fuzzy),
    /// not a raw `contains`: the verifier grounds quotes
    /// case/punctuation-insensitively, so page attribution must accept the
    /// same matches or it silently points at the wrong page (Codex P2,
    /// PR #147).
    #[must_use]
    pub fn page_of_passage(&self, passage: Option<&str>) -> Option<u32> {
        use super::locate_quote::{locate_quote, locate_quote_fuzzy};
        passage
            .and_then(|quote| {
                self.matches
                    .iter()
                    .find(|entry| {
                        locate_quote(quote, &entry.text).is_some()
                            || locate_quote_fuzzy(quote, &entry.text).is_some()
                    })
                    .and_then(|entry| entry.page)
            })
            .or_else(|| self.matches.first().and_then(|entry| entry.page))
    }
}

/// Prepare the grounding body for one book claim against one scan: metadata →
/// search-inside → cited-page-first snippet selection. Strictly read-only and
/// best-effort; every failure maps onto the ADR-0007 outcome split rather
/// than an error.
pub async fn prepare_book_grounding<C>(
    client: &C,
    ocaid: &str,
    claim: &str,
    cited_page: Option<&str>,
) -> BookGroundingPreparation
where
    C: HttpClient + ?Sized,
{
    // 1. Item metadata: where does this item's full-text search run?
    let metadata_response = match client.execute(build_item_metadata_request(ocaid)).await {
        Ok(response) => response,
        Err(error) => {
            return BookGroundingPreparation::Unreachable {
                message: error.to_string(),
            };
        }
    };
    if !(200..300).contains(&metadata_response.status) {
        return BookGroundingPreparation::Unreachable {
            message: format!("item metadata returned {}", metadata_response.status),
        };
    }
    let Some(location) = parse_item_metadata(&metadata_response.body) else {
        return BookGroundingPreparation::NoUsableBody {
            detail: "item metadata unusable",
        };
    };
    if !location.is_text_item {
        return BookGroundingPreparation::NoUsableBody {
            detail: "not a text item",
        };
    }

    // 2. Full-text search on the designated server.
    let query = search_query(claim);
    let Some(request) = build_search_inside_request(&location, ocaid, &query) else {
        return BookGroundingPreparation::NoUsableBody {
            detail: "search server unusable",
        };
    };
    let search_response = match client.execute(request).await {
        Ok(response) => response,
        Err(error) => {
            return BookGroundingPreparation::Unreachable {
                message: error.to_string(),
            };
        }
    };
    if !(200..300).contains(&search_response.status) {
        return BookGroundingPreparation::Unreachable {
            message: format!("search-inside returned {}", search_response.status),
        };
    }
    let Some(result) = parse_search_inside(&search_response.body) else {
        return BookGroundingPreparation::NoUsableBody {
            detail: "search response unusable",
        };
    };
    if !result.indexed {
        return BookGroundingPreparation::NoUsableBody {
            detail: "no full-text index",
        };
    }
    if result.matches.is_empty() {
        return BookGroundingPreparation::NoMatches {
            response_hash: sha256_hex(&search_response.body),
            deep_link: scan_deep_link(ocaid, None, &query),
        };
    }

    // 3. Cited page first, then fall back to the whole book (PRD-0009
    // resolved Q5). The search returns page-numbered matches for the whole
    // book, so the page-first pass is a filter, not a second request.
    let cited_page_number = cited_page.and_then(leading_page_number);
    let on_cited_page: Vec<SearchInsideMatch> = cited_page_number
        .map(|page| {
            result
                .matches
                .iter()
                .filter(|entry| entry.page == Some(page))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let cited_page_hit = !on_cited_page.is_empty();
    let mut selected = if cited_page_hit {
        on_cited_page
    } else {
        result.matches
    };
    selected.truncate(MAX_SNIPPET_MATCHES);

    let text = selected
        .iter()
        .map(|entry| entry.text.trim())
        .filter(|snippet| !snippet.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    if text.is_empty() {
        return BookGroundingPreparation::NoUsableBody {
            detail: "empty snippets",
        };
    }
    BookGroundingPreparation::Body(BookSnippetBody {
        text,
        matches: selected,
        cited_page_hit,
        query,
    })
}

/// The leading page number of a cite `page=`/`pages=` value (`"42"`,
/// `"42–45"`, `"42, 44"` all yield 42). `None` when it does not start with a
/// number (e.g. roman numerals — the whole-book pass covers those).
fn leading_page_number(cited_page: &str) -> Option<u32> {
    let digits: String = cited_page
        .trim()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::{
        BookGroundingPreparation, ItemLocation, build_item_metadata_request,
        build_search_inside_request, extract_ocaid, parse_item_metadata, parse_search_inside,
        prepare_book_grounding, scan_deep_link, search_query,
    };
    use crate::types::HttpResponse;
    use futures::executor::block_on;
    use sp42_types::{HttpClientError, StubHttpClient};

    fn ok(body: &str) -> HttpResponse {
        HttpResponse {
            status: 200,
            headers: std::collections::BTreeMap::new(),
            body: body.as_bytes().to_vec(),
        }
    }

    const ITEM_METADATA: &str = r#"{
        "server": "ia800300.us.archive.org",
        "dir": "/12/items/matilda00dahl",
        "metadata": {"identifier": "matilda00dahl", "mediatype": "texts"}
    }"#;

    const SEARCH_HITS: &str = r#"{
        "indexed": true,
        "matches": [
            {"text": "Matilda longed for her parents to be {{{good}}} and {{{loving}}}.",
             "par": [{"page": 42, "boxes": []}]},
            {"text": "a {{{good}}} and {{{loving}}} child somewhere else in the book",
             "par": [{"page": 7, "boxes": []}]}
        ]
    }"#;

    #[test]
    fn ocaid_extraction_requires_an_archive_details_url() {
        assert_eq!(
            extract_ocaid("https://archive.org/details/matilda00dahl").as_deref(),
            Some("matilda00dahl")
        );
        assert_eq!(
            extract_ocaid("https://archive.org/details/matilda00dahl/page/8").as_deref(),
            Some("matilda00dahl")
        );
        assert_eq!(extract_ocaid("https://example.org/details/x"), None);
        assert_eq!(extract_ocaid("https://archive.org/download/x"), None);
        assert_eq!(extract_ocaid("not a url"), None);
    }

    #[test]
    fn metadata_request_and_parse_designate_the_search_server() {
        let request = build_item_metadata_request("matilda00dahl");
        assert_eq!(
            request.url.as_str(),
            "https://archive.org/metadata/matilda00dahl"
        );
        let location = parse_item_metadata(ITEM_METADATA.as_bytes()).expect("parses");
        assert_eq!(location.server, "ia800300.us.archive.org");
        assert_eq!(location.dir, "/12/items/matilda00dahl");
        assert!(location.is_text_item);
        // A movie item is not groundable.
        let movie = r#"{"server": "s", "dir": "/d", "metadata": {"mediatype": "movies"}}"#;
        assert!(
            !parse_item_metadata(movie.as_bytes())
                .expect("parses")
                .is_text_item
        );
        assert_eq!(parse_item_metadata(b"{}"), None);
    }

    #[test]
    fn search_request_targets_the_designated_server() {
        let location = ItemLocation {
            server: "ia800300.us.archive.org".to_string(),
            dir: "/12/items/matilda00dahl".to_string(),
            is_text_item: true,
        };
        let request = build_search_inside_request(&location, "matilda00dahl", "good loving")
            .expect("valid server forms a url");
        let url = request.url.as_str();
        assert!(url.starts_with("https://ia800300.us.archive.org/fulltext/inside.php?"));
        assert!(url.contains("item_id=matilda00dahl"));
        assert!(url.contains("path=%2F12%2Fitems%2Fmatilda00dahl"));
        assert!(url.contains("q=good+loving"));
    }

    #[test]
    fn parse_search_strips_highlight_markers_and_reads_pages() {
        let result = parse_search_inside(SEARCH_HITS.as_bytes()).expect("parses");
        assert!(result.indexed);
        assert_eq!(result.matches.len(), 2);
        assert_eq!(
            result.matches[0].text,
            "Matilda longed for her parents to be good and loving."
        );
        assert_eq!(result.matches[0].page, Some(42));
        // Unindexed and garbage responses are distinguishable.
        assert!(
            !parse_search_inside(br#"{"indexed": false}"#)
                .expect("parses")
                .indexed
        );
        assert_eq!(parse_search_inside(b"<html>"), None);
    }

    #[test]
    fn query_takes_distinct_longer_words_in_claim_order() {
        assert_eq!(
            search_query("Matilda longed for her parents to be good and loving."),
            "Matilda longed parents loving"
        );
        // Short-word claims fall back to the 4-char floor, then the raw claim.
        assert_eq!(search_query("The cat sat down"), "down");
        assert_eq!(search_query("a b c"), "a b c");
    }

    #[test]
    fn deep_link_anchors_the_page_and_highlights_the_query() {
        assert_eq!(
            scan_deep_link("matilda00dahl", Some(42), "good loving"),
            "https://archive.org/details/matilda00dahl/page/42?q=good+loving"
        );
        assert_eq!(
            scan_deep_link("matilda00dahl", None, "good"),
            "https://archive.org/details/matilda00dahl?q=good"
        );
    }

    #[test]
    fn grounding_selects_the_cited_page_first() {
        let client = StubHttpClient::new(vec![Ok(ok(ITEM_METADATA)), Ok(ok(SEARCH_HITS))]);
        let prep = block_on(prepare_book_grounding(
            &client,
            "matilda00dahl",
            "Matilda longed for her parents to be good and loving.",
            Some("42"),
        ));
        let BookGroundingPreparation::Body(body) = prep else {
            panic!("expected a body, got {prep:?}");
        };
        assert!(body.cited_page_hit, "page-42 match should be selected");
        assert_eq!(body.matches.len(), 1, "only the cited-page match");
        assert_eq!(body.matches[0].page, Some(42));
        assert!(body.text.contains("good and loving"));
        assert_eq!(body.page_of_passage(Some("good and loving")), Some(42));
    }

    #[test]
    fn grounding_falls_back_to_the_whole_book_on_a_page_miss() {
        // Cited page 99 has no match: pagination differs — fall back to every
        // match and record that the cited-page pass missed.
        let client = StubHttpClient::new(vec![Ok(ok(ITEM_METADATA)), Ok(ok(SEARCH_HITS))]);
        let prep = block_on(prepare_book_grounding(
            &client,
            "matilda00dahl",
            "Matilda longed for her parents to be good and loving.",
            Some("99"),
        ));
        let BookGroundingPreparation::Body(body) = prep else {
            panic!("expected a body, got {prep:?}");
        };
        assert!(!body.cited_page_hit);
        assert_eq!(
            body.matches.len(),
            2,
            "whole-book fallback keeps all matches"
        );
    }

    #[test]
    fn indexed_scan_with_zero_matches_is_no_matches_not_unusable() {
        let client = StubHttpClient::new(vec![
            Ok(ok(ITEM_METADATA)),
            Ok(ok(r#"{"indexed": true, "matches": []}"#)),
        ]);
        let prep = block_on(prepare_book_grounding(
            &client,
            "matilda00dahl",
            "claim text here",
            None,
        ));
        assert!(
            matches!(prep, BookGroundingPreparation::NoMatches { .. }),
            "searched-and-found-nothing must not read as unusable: {prep:?}"
        );
    }

    #[test]
    fn non_text_item_and_missing_index_are_no_usable_body() {
        let movie =
            r#"{"server": "s.archive.org", "dir": "/d", "metadata": {"mediatype": "movies"}}"#;
        let client = StubHttpClient::new(vec![Ok(ok(movie))]);
        let prep = block_on(prepare_book_grounding(
            &client,
            "x",
            "claim text here",
            None,
        ));
        assert_eq!(
            prep,
            BookGroundingPreparation::NoUsableBody {
                detail: "not a text item"
            }
        );

        let client =
            StubHttpClient::new(vec![Ok(ok(ITEM_METADATA)), Ok(ok(r#"{"indexed": false}"#))]);
        let prep = block_on(prepare_book_grounding(
            &client,
            "x",
            "claim text here",
            None,
        ));
        assert_eq!(
            prep,
            BookGroundingPreparation::NoUsableBody {
                detail: "no full-text index"
            }
        );
    }

    #[test]
    fn transport_failure_is_unreachable() {
        let client = StubHttpClient::new(vec![Err(HttpClientError::Transport {
            message: "archive.org unreachable".to_string(),
        })]);
        let prep = block_on(prepare_book_grounding(
            &client,
            "x",
            "claim text here",
            None,
        ));
        let BookGroundingPreparation::Unreachable { message } = prep else {
            panic!("expected Unreachable, got {prep:?}");
        };
        assert!(message.contains("archive.org unreachable"));
    }
}
