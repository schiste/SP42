//! Internet Archive search-inside grounding source (PRD-0009 Layer 2,
//! ADR-0024 Decision 4).
//!
//! Turns a resolved exact-edition scan into a **book-snippet source body** the
//! existing verifier can judge: read the item's metadata for its designated
//! server and directory, query the `BookReader` full-text search, and assemble
//! the returned **verbatim OCR snippets** (page-numbered) into a body. The
//! scan's OCR is never downloaded whole, so grounding needs no borrow.
//!
//! **Reach is narrower than originally assumed.** Snippet search was expected
//! to work for lending-restricted scans too; it does not. The endpoint returns
//! 403 for print-disabled/lendable items, so only `full access` scans can be
//! grounded (measured 2026-07-23 across 130 book editions cited by seven
//! en.wikipedia articles: 11 had an exact groundable scan, every one of them
//! `lendable`, all 403). That is a reach limit, not a correctness problem — a
//! 403 is [`BookGroundingPreparation::Unreachable`] and degrades to
//! `SourceUnavailable`, never to `not_supported`.
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

/// The highlight markers the full-text search wraps matched terms in. The
/// live endpoint emits `<IA_FTS_MATCH>…</IA_FTS_MATCH>` (observed 2026-07-23);
/// `{{{…}}}` is the historical form. Both are stripped, because a marker left
/// in the text is markup inside bytes we present as verbatim OCR — it reaches
/// the verifier prompt, and it breaks quote location (and therefore page
/// attribution) for any quote spanning a match boundary.
const HIGHLIGHT_MARKERS: [&str; 4] = ["<IA_FTS_MATCH>", "</IA_FTS_MATCH>", "{{{", "}}}"];

/// Remove every highlight marker form from a snippet.
fn strip_highlight_markers(text: &str) -> String {
    HIGHLIGHT_MARKERS
        .iter()
        .fold(text.to_string(), |acc, marker| acc.replace(marker, ""))
}

/// One search-inside match: the verbatim OCR snippet (match markers stripped)
/// and the scanned page it was found on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchInsideMatch {
    /// Verbatim OCR snippet text, with the highlight markers removed so the
    /// text is exactly what the OCR holds (groundable bytes).
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
                        text: strip_highlight_markers(text),
                        page,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Some(SearchInsideResult { indexed, matches })
}

/// The distinct longer words of a claim (≥ 5 chars, falling back to ≥ 4), in
/// claim order, capped at `limit`. Empty when the claim holds no word that
/// long.
fn claim_terms(claim: &str, limit: usize) -> Vec<&str> {
    let words: Vec<&str> = claim
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    for floor in [5, 4] {
        let mut seen: Vec<&str> = Vec::new();
        for word in &words {
            if word.chars().count() >= floor && !seen.contains(word) {
                seen.push(word);
                if seen.len() == limit {
                    break;
                }
            }
        }
        if !seen.is_empty() {
            return seen;
        }
    }
    Vec::new()
}

/// A conservative full-text query from a claim sentence: the distinct longer
/// words in claim order, capped at `MAX_QUERY_TERMS`.
#[must_use]
pub fn search_query(claim: &str) -> String {
    let terms = claim_terms(claim, MAX_QUERY_TERMS);
    if terms.is_empty() {
        claim.trim().to_string()
    } else {
        terms.join(" ")
    }
}

/// Progressively narrower queries, tried in order until one returns matches.
///
/// This ladder is a correctness requirement, not a recall optimization. The
/// full-text endpoint returns zero matches for the **entire** query when any
/// single term is absent from that item's OCR index. Because an empty result
/// is what [`BookGroundingPreparation::NoMatches`] reports — and the caller
/// turns that into `not_supported` (PRD-0009 resolved Q4) — a one-shot query
/// lets a single unindexed word turn a well-supported claim into a false
/// accusation against the citation.
///
/// Observed live on `onoriginofspeci00darw` (2026-07-23), the 1871 D. Appleton
/// printing, which spells it *favored*: `natural selection preservation slight`
/// returns 493 matches, adding `favourable` returns **0**, and `favourable`
/// alone returns 0 — while `preservation` alone returns 25. Spelling variants,
/// OCR damage and hyphenation breaks all trigger the same collapse.
///
/// Falling back to the few most distinctive terms, and finally to the single
/// most distinctive one, makes "no matches" mean the scan genuinely lacks the
/// claim's rarest word.
#[must_use]
pub fn search_query_ladder(claim: &str) -> Vec<String> {
    let terms = claim_terms(claim, MAX_QUERY_TERMS);
    if terms.is_empty() {
        return vec![claim.trim().to_string()];
    }

    let mut rungs: Vec<String> = Vec::new();
    for width in [MAX_QUERY_TERMS, 3, 1] {
        // Narrower rungs keep the longest (rarest) terms, in claim order.
        let mut kept: Vec<&str> = terms.clone();
        if width < kept.len() {
            kept.sort_by_key(|term| std::cmp::Reverse(term.chars().count()));
            kept.truncate(width);
            kept.sort_by_key(|term| terms.iter().position(|t| t == term));
        }
        let query = kept.join(" ");
        if !rungs.contains(&query) {
            rungs.push(query);
        }
    }
    rungs
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
    /// PR 147).
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

/// What the query ladder ended on: the last response parsed, the query that
/// produced it, and that response's raw bytes (for the `SourceFetched` hash).
struct SearchLadderOutcome {
    result: SearchInsideResult,
    query: String,
    response_body: Vec<u8>,
}

/// Walk [`search_query_ladder`], stopping at the first rung that returns
/// matches. `Err` carries the [`BookGroundingPreparation`] the caller should
/// return directly — every failure mode is an outcome, never an error.
async fn run_search_ladder<C>(
    client: &C,
    location: &ItemLocation,
    ocaid: &str,
    claim: &str,
) -> Result<SearchLadderOutcome, BookGroundingPreparation>
where
    C: HttpClient + ?Sized,
{
    let mut outcome = SearchLadderOutcome {
        result: SearchInsideResult {
            indexed: true,
            matches: Vec::new(),
        },
        query: String::new(),
        response_body: Vec::new(),
    };

    for rung in search_query_ladder(claim) {
        outcome.query = rung;
        let Some(request) = build_search_inside_request(location, ocaid, &outcome.query) else {
            return Err(BookGroundingPreparation::NoUsableBody {
                detail: "search server unusable",
            });
        };
        let response = match client.execute(request).await {
            Ok(response) => response,
            Err(error) => {
                return Err(BookGroundingPreparation::Unreachable {
                    message: error.to_string(),
                });
            }
        };
        // A 403 here is the lending-restricted case (see the module header):
        // a failure to read, never a finding about the book.
        if !(200..300).contains(&response.status) {
            return Err(BookGroundingPreparation::Unreachable {
                message: format!("search-inside returned {}", response.status),
            });
        }
        let Some(parsed) = parse_search_inside(&response.body) else {
            return Err(BookGroundingPreparation::NoUsableBody {
                detail: "search response unusable",
            });
        };
        if !parsed.indexed {
            return Err(BookGroundingPreparation::NoUsableBody {
                detail: "no full-text index",
            });
        }
        outcome.response_body = response.body;
        outcome.result = parsed;
        if !outcome.result.matches.is_empty() {
            break;
        }
    }

    Ok(outcome)
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

    // 2. Full-text search on the designated server, narrowing the query until
    //    something comes back (see `search_query_ladder` for why that is a
    //    correctness requirement rather than a recall tweak).
    let search = match run_search_ladder(client, &location, ocaid, claim).await {
        Ok(search) => search,
        Err(outcome) => return outcome,
    };
    let SearchLadderOutcome {
        result,
        query,
        response_body,
    } = search;
    if result.matches.is_empty() {
        // Every rung came back empty, down to the single most distinctive
        // term — the scan genuinely lacks the claim's rarest word.
        return BookGroundingPreparation::NoMatches {
            response_hash: sha256_hex(&response_body),
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
        prepare_book_grounding, scan_deep_link, search_query, search_query_ladder,
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

    // Live marker form (`<IA_FTS_MATCH>`); the historical `{{{…}}}` form is
    // covered separately in `parse_search_strips_highlight_markers_and_reads_pages`.
    const SEARCH_HITS: &str = r#"{
        "indexed": true,
        "matches": [
            {"text": "Matilda longed for her parents to be <IA_FTS_MATCH>good</IA_FTS_MATCH> and <IA_FTS_MATCH>loving</IA_FTS_MATCH>.",
             "par": [{"page": 42, "boxes": []}]},
            {"text": "a <IA_FTS_MATCH>good</IA_FTS_MATCH> and <IA_FTS_MATCH>loving</IA_FTS_MATCH> child somewhere else in the book",
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
        // A marker left in the text is markup inside bytes we present as
        // verbatim OCR: it reaches the verifier prompt and breaks quote
        // location for any quote spanning a match boundary.
        for entry in &result.matches {
            assert!(
                !entry.text.contains("IA_FTS_MATCH") && !entry.text.contains("{{{"),
                "marker survived into supposedly verbatim OCR: {}",
                entry.text
            );
        }
        // The historical marker form is still stripped.
        let legacy = parse_search_inside(
            br#"{"indexed": true, "matches": [{"text": "a {{{good}}} book", "par": [{"page": 1}]}]}"#,
        )
        .expect("parses");
        assert_eq!(legacy.matches[0].text, "a good book");
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
    fn query_ladder_narrows_to_the_single_most_distinctive_term() {
        // The safety property: one word missing from a scan's OCR index zeroes
        // the whole query, so "no matches" must never be concluded from a wide
        // query alone.
        assert_eq!(
            search_query_ladder(
                "Natural selection acts by the preservation of slight favourable variations."
            ),
            vec![
                "Natural selection preservation slight favourable variations",
                "preservation favourable variations",
                "preservation",
            ]
        );
        // A claim with few long words collapses to fewer distinct rungs.
        assert_eq!(search_query_ladder("The cat sat down"), vec!["down"]);
    }

    #[test]
    fn grounding_retries_a_narrower_query_when_the_wide_one_is_empty() {
        // Regression for the false-`not_supported` bug: the wide query and the
        // three-term rung both come back empty (as they do live, because the
        // 1871 D. Appleton printing spells it "favored"), and the single-term
        // rung finds the passage.
        let client = StubHttpClient::new(vec![
            Ok(ok(ITEM_METADATA)),
            Ok(ok(r#"{"indexed": true, "matches": []}"#)),
            Ok(ok(r#"{"indexed": true, "matches": []}"#)),
            Ok(ok(
                r#"{"indexed": true, "matches": [{"text": "the <IA_FTS_MATCH>preservation</IA_FTS_MATCH> of favored races", "par": [{"page": 3}]}]}"#,
            )),
        ]);
        let prep = block_on(prepare_book_grounding(
            &client,
            "onoriginofspeci00darw",
            "Natural selection acts by the preservation of slight favourable variations.",
            None,
        ));
        let BookGroundingPreparation::Body(body) = prep else {
            panic!("expected a body, got {prep:?}");
        };
        assert_eq!(body.query, "preservation", "the narrowest rung grounded it");
        assert_eq!(body.text, "the preservation of favored races");
    }

    #[test]
    fn no_matches_only_after_every_rung_is_exhausted() {
        let client = StubHttpClient::new(vec![
            Ok(ok(ITEM_METADATA)),
            Ok(ok(r#"{"indexed": true, "matches": []}"#)),
            Ok(ok(r#"{"indexed": true, "matches": []}"#)),
            Ok(ok(r#"{"indexed": true, "matches": []}"#)),
        ]);
        let prep = block_on(prepare_book_grounding(
            &client,
            "onoriginofspeci00darw",
            "Natural selection acts by the preservation of slight favourable variations.",
            None,
        ));
        let BookGroundingPreparation::NoMatches { deep_link, .. } = prep else {
            panic!("expected NoMatches, got {prep:?}");
        };
        assert!(
            deep_link.ends_with("?q=preservation"),
            "the deep link reflects the query actually exhausted: {deep_link}"
        );
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
