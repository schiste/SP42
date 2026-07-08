//! Open Library resolution for book citations (PRD-0009 Layer 1, ADR-0018).
//!
//! Two strictly read-only lookups, both keyed on a validated
//! [`BookIdentifier`] and both pure `build_*`/`parse_*` pairs over the
//! injected `HttpClient` transport, like the Citoid sidecar
//! ([`crate::citation::citoid`]):
//!
//! - **Catalog resolution** via the Books API
//!   (`/api/books?bibkeys={SCHEME}:{value}&jscmd=data&format=json`) — does the
//!   edition exist, and what does its record say. A miss is `None`, never a
//!   create: the `/isbn/{isbn}.json` endpoint is documented under *Import by
//!   ISBN* and can import-on-miss, so this module **must not** address it
//!   (ADR-0018 Decision 2).
//! - **Scan availability** via the Read API
//!   (`/api/volumes/brief/{scheme}/{value}.json`) — consulted only *after*
//!   catalog resolution, and only for readable/borrowable scan discovery.
//!   Items are partitioned by their `match` field: only `exact` matches may
//!   ever feed grounding; a `similar` match is a different edition and is
//!   surfaced as context only (ADR-0018 Decision 3). An empty `items` list
//!   means "no usable scan", **not** "no catalog record".

use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use crate::citation::enrich::EnrichmentCandidate;
use crate::types::{HttpMethod, HttpRequest};
use crate::wikitext_editor::{BookIdentifier, BookSource};
use sp42_types::HttpClient;

/// The side-effect-free Books API catalog lookup (ADR-0018 Decision 2).
pub const OPEN_LIBRARY_BOOKS_API: &str = "https://openlibrary.org/api/books";

/// The Read API scan-availability base (ADR-0018 Decision 3).
pub const OPEN_LIBRARY_READ_API_BASE: &str = "https://openlibrary.org/api/volumes/brief";

/// The Books API endpoint parsed once, for the (unreachable) fallback when a
/// built URL somehow fails to parse — keeps the builders panic-free.
static BOOKS_API_BASE: LazyLock<Url> = LazyLock::new(|| {
    OPEN_LIBRARY_BOOKS_API
        .parse()
        .expect("static endpoint parses")
});

/// The Books API bibkey for an identifier, e.g. `ISBN:9780140328721`.
#[must_use]
pub fn bibkey(identifier: &BookIdentifier) -> String {
    match identifier {
        BookIdentifier::Isbn(value) => format!("ISBN:{value}"),
        BookIdentifier::Oclc(value) => format!("OCLC:{value}"),
        BookIdentifier::Lccn(value) => format!("LCCN:{value}"),
        BookIdentifier::Olid(value) => format!("OLID:{value}"),
    }
}

/// The Read API path scheme for an identifier (`isbn`/`oclc`/`lccn`/`olid`).
#[must_use]
fn read_api_scheme(identifier: &BookIdentifier) -> &'static str {
    match identifier {
        BookIdentifier::Isbn(_) => "isbn",
        BookIdentifier::Oclc(_) => "oclc",
        BookIdentifier::Lccn(_) => "lccn",
        BookIdentifier::Olid(_) => "olid",
    }
}

/// The normalized identifier value, without its scheme.
fn identifier_value(identifier: &BookIdentifier) -> &str {
    match identifier {
        BookIdentifier::Isbn(value)
        | BookIdentifier::Oclc(value)
        | BookIdentifier::Lccn(value)
        | BookIdentifier::Olid(value) => value,
    }
}

/// Build the (read-only GET) Books API catalog lookup for one identifier.
///
/// This is the **only** catalog-resolution request the resolve lane issues:
/// never `/isbn/{isbn}.json` (import-on-miss; ADR-0018 Decision 2).
#[must_use]
pub fn build_catalog_lookup_request(identifier: &BookIdentifier) -> HttpRequest {
    let url = Url::parse_with_params(
        OPEN_LIBRARY_BOOKS_API,
        &[
            ("bibkeys", bibkey(identifier).as_str()),
            ("jscmd", "data"),
            ("format", "json"),
        ],
    )
    .unwrap_or_else(|_| BOOKS_API_BASE.clone());
    HttpRequest {
        method: HttpMethod::Get,
        url,
        headers: BTreeMap::new(),
        body: Vec::new(),
    }
}

/// An Open Library edition record, from the Books API `jscmd=data` shape.
///
/// Every field is best-effort: a thin record simply has absences, which is
/// exactly what the enrichment lane (PRD-0009 Layer 3) later proposes to fill.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenLibraryEdition {
    /// Edition key, e.g. `/books/OL7826547M`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Human-facing record URL on openlibrary.org.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Author display names, in record order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub publishers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publish_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number_of_pages: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub isbn_10: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub isbn_13: Vec<String>,
    /// Subject display names (thinness here drives enrichment proposals).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subjects: Vec<String>,
    /// Largest cover image URL present, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover_url: Option<String>,
}

/// Parse a Books API `jscmd=data` response for the identifier it was queried
/// with. `None` is a **catalog miss** ("no record found" — never a create) or
/// an unparseable body; per ADR-0018 Decision 2 no import or write follows.
#[must_use]
pub fn parse_catalog_lookup(
    identifier: &BookIdentifier,
    body: &[u8],
) -> Option<OpenLibraryEdition> {
    let parsed: Value = serde_json::from_slice(body).ok()?;
    let record = parsed.get(bibkey(identifier))?.as_object()?;

    let string_of = |key: &str| {
        record
            .get(key)
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .map(ToString::to_string)
    };
    let names_of = |value: Option<&Value>| -> Vec<String> {
        value
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|entry| entry.get("name").and_then(Value::as_str))
                    .filter(|s| !s.trim().is_empty())
                    .map(ToString::to_string)
                    .collect()
            })
            .unwrap_or_default()
    };
    let identifier_list = |key: &str| -> Vec<String> {
        record
            .get("identifiers")
            .and_then(|ids| ids.get(key))
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(ToString::to_string)
                    .collect()
            })
            .unwrap_or_default()
    };
    let cover_url = record.get("cover").and_then(|cover| {
        ["large", "medium", "small"]
            .iter()
            .find_map(|size| cover.get(size).and_then(Value::as_str))
            .map(ToString::to_string)
    });

    Some(OpenLibraryEdition {
        key: string_of("key"),
        record_url: string_of("url"),
        title: string_of("title"),
        authors: names_of(record.get("authors")),
        publishers: names_of(record.get("publishers")),
        publish_date: string_of("publish_date"),
        number_of_pages: record.get("number_of_pages").and_then(Value::as_u64),
        isbn_10: identifier_list("isbn_10"),
        isbn_13: identifier_list("isbn_13"),
        subjects: names_of(record.get("subjects")),
        cover_url,
    })
}

/// Build the (read-only GET) Read API scan-availability request.
///
/// Consulted only **after** catalog resolution (ADR-0018 Decision 3): its
/// answer is "is there a usable online scan", never "does the record exist".
#[must_use]
pub fn build_scan_availability_request(identifier: &BookIdentifier) -> HttpRequest {
    let url_string = format!(
        "{OPEN_LIBRARY_READ_API_BASE}/{}/{}.json",
        read_api_scheme(identifier),
        identifier_value(identifier)
    );
    // Normalized identifier values are plain alphanumerics; fall back
    // defensively to the Books API base (never panics).
    let url = url_string
        .parse()
        .unwrap_or_else(|_| BOOKS_API_BASE.clone());
    HttpRequest {
        method: HttpMethod::Get,
        url,
        headers: BTreeMap::new(),
        body: Vec::new(),
    }
}

/// One readable/borrowable scan the Read API reported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanItem {
    /// Access level as reported (`full access`, `lendable`, …).
    pub status: String,
    /// The archive.org item URL for the scan.
    pub item_url: String,
    /// Open Library edition id of the scanned volume, when reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ol_edition_id: Option<String>,
}

/// Scan availability for a resolved edition, partitioned by match quality.
///
/// Only `exact` items may feed grounding (PRD-0009 / ADR-0018 Decision 3): a
/// `similar` item is a scan of a *different edition* of the same work and is
/// surfaced to the operator as context only — never verified against. Both
/// lists empty means the catalog record has no usable online scan, which is
/// **not** a resolution failure.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanAvailability {
    pub exact: Vec<ScanItem>,
    pub similar: Vec<ScanItem>,
}

impl ScanAvailability {
    /// The scan eligible to enter grounding: the first **exact** match.
    /// `None` when only similar-edition scans (or none) exist — grounding
    /// then degrades to `SourceUnavailable` rather than verifying a
    /// page-specific citation against a different edition.
    #[must_use]
    pub fn groundable_scan(&self) -> Option<&ScanItem> {
        self.exact.first()
    }
}

/// Parse a Read API brief response into partitioned scan availability.
/// `None` only for an unparseable body; a well-formed response with no items
/// is `Some` with both lists empty ("record may exist, no scan").
#[must_use]
pub fn parse_scan_availability(body: &[u8]) -> Option<ScanAvailability> {
    let parsed: Value = serde_json::from_slice(body).ok()?;
    let object = parsed.as_object()?;
    let mut availability = ScanAvailability::default();
    let Some(items) = object.get("items") else {
        return Some(availability);
    };
    for item in items.as_array()? {
        let Some(item_url) = item.get("itemURL").and_then(Value::as_str) else {
            continue;
        };
        let scan = ScanItem {
            status: item
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            item_url: item_url.to_string(),
            ol_edition_id: item
                .get("ol-edition-id")
                .and_then(Value::as_str)
                .map(ToString::to_string),
        };
        match item.get("match").and_then(Value::as_str) {
            Some("exact") => availability.exact.push(scan),
            // An unlabeled match is treated as similar: never ground on it.
            _ => availability.similar.push(scan),
        }
    }
    Some(availability)
}

/// Outcome of resolving one book source against the Open Library catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum BookResolutionOutcome {
    /// A catalog record was found under one of the ref's identifiers.
    Resolved {
        /// The identifier that keyed the successful catalog lookup.
        identifier: BookIdentifier,
        edition: Box<OpenLibraryEdition>,
        /// Scan availability for the resolved identifier. `None` means the
        /// availability lookup itself failed (unknown); an empty
        /// [`ScanAvailability`] means "record exists, no usable scan".
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scan: Option<ScanAvailability>,
    },
    /// Every identifier was looked up cleanly and none has a catalog record.
    /// Never a create (ADR-0018 Decision 2).
    NotFound,
    /// A lookup failed in transport before a clean answer; whether a record
    /// exists is unknown, which is different from `NotFound`.
    LookupFailed { message: String },
}

/// One book citation's Layer-1 resolution, with page provenance
/// (PRD-0009): which ref named the book, what identifiers it carried, and
/// what the catalog said.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookResolution {
    /// The skipped ref this resolution belongs to.
    pub ref_id: String,
    pub block_ordinal: usize,
    /// The identifiers the ref carried, in template order (what was tried).
    pub identifiers: Vec<BookIdentifier>,
    /// Verbatim cited page from the template, for the future search-inside
    /// pass (ADR-0018 Decision 4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cited_page: Option<String>,
    pub outcome: BookResolutionOutcome,
    /// Read-only enrichment candidates for a resolved record (PRD-0009
    /// Layer 3's proposal listing): deterministic field-gap fills the
    /// operator could confirm once the write lane is enabled (ADR-0019).
    /// Always empty for unresolved outcomes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enrichment_candidates: Vec<EnrichmentCandidate>,
}

/// Resolve one book source: try its identifiers in template order against the
/// Books API; on the first catalog hit, additionally look up scan
/// availability. Strictly read-only and best-effort — the only endpoints
/// addressed are the two builders above, a miss issues no import or retry
/// against a write path, and any transport failure degrades to
/// [`BookResolutionOutcome::LookupFailed`] rather than an error.
pub async fn resolve_book_source<C>(client: &C, book: &BookSource) -> BookResolutionOutcome
where
    C: HttpClient + ?Sized,
{
    let mut failure: Option<String> = None;
    for identifier in &book.identifiers {
        let response = match client
            .execute(build_catalog_lookup_request(identifier))
            .await
        {
            Ok(response) => response,
            Err(error) => {
                failure.get_or_insert_with(|| error.to_string());
                continue;
            }
        };
        if !(200..300).contains(&response.status) {
            failure.get_or_insert_with(|| format!("catalog lookup returned {}", response.status));
            continue;
        }
        // An unparseable 2xx body is a failure, not a miss: "NotFound" is
        // reserved for a clean catalog answer with no record under our bibkey.
        if serde_json::from_slice::<Value>(&response.body).is_err() {
            failure.get_or_insert_with(|| "catalog lookup body was not JSON".to_string());
            continue;
        }
        if let Some(edition) = parse_catalog_lookup(identifier, &response.body) {
            let scan = fetch_scan_availability(client, identifier).await;
            return BookResolutionOutcome::Resolved {
                identifier: identifier.clone(),
                edition: Box::new(edition),
                scan,
            };
        }
        // A clean miss under this identifier — try the next one.
    }
    match failure {
        Some(message) => BookResolutionOutcome::LookupFailed { message },
        None => BookResolutionOutcome::NotFound,
    }
}

/// Best-effort Read API availability check; any failure yields `None`
/// (availability unknown — never blocks or fails the resolution).
async fn fetch_scan_availability<C>(
    client: &C,
    identifier: &BookIdentifier,
) -> Option<ScanAvailability>
where
    C: HttpClient + ?Sized,
{
    let response = client
        .execute(build_scan_availability_request(identifier))
        .await
        .ok()?;
    if !(200..300).contains(&response.status) {
        return None;
    }
    parse_scan_availability(&response.body)
}

#[cfg(test)]
mod tests {
    use super::{
        BookIdentifier, BookResolutionOutcome, BookSource, HttpClient, ScanAvailability, bibkey,
        build_catalog_lookup_request, build_scan_availability_request, parse_catalog_lookup,
        parse_scan_availability, resolve_book_source,
    };
    use crate::errors::HttpClientError;
    use crate::types::{HttpMethod, HttpRequest, HttpResponse};
    use futures::executor::block_on;
    use sp42_types::StubHttpClient;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    fn isbn() -> BookIdentifier {
        BookIdentifier::isbn("978-0-14-032872-1").expect("valid isbn")
    }

    fn book(identifiers: Vec<BookIdentifier>) -> BookSource {
        BookSource {
            identifiers,
            cited_page: Some("42".to_string()),
        }
    }

    fn ok(body: &str) -> HttpResponse {
        HttpResponse {
            status: 200,
            headers: std::collections::BTreeMap::new(),
            body: body.as_bytes().to_vec(),
        }
    }

    const CATALOG_HIT: &str =
        r#"{"ISBN:9780140328721": {"key": "/books/OL7826547M", "title": "Matilda"}}"#;
    const READ_API_EXACT: &str = r#"{"items": [{"match": "exact", "status": "full access", "itemURL": "https://archive.org/details/matilda00dahl"}]}"#;

    /// A stub that also records every requested URL, so tests can assert the
    /// resolve lane addresses only the two read endpoints (ADR-0018 Decision 2).
    struct RecordingHttpClient {
        urls: Mutex<Vec<String>>,
        responses: Mutex<VecDeque<Result<HttpResponse, HttpClientError>>>,
    }

    impl RecordingHttpClient {
        fn new<I>(responses: I) -> Self
        where
            I: IntoIterator<Item = Result<HttpResponse, HttpClientError>>,
        {
            Self {
                urls: Mutex::new(Vec::new()),
                responses: Mutex::new(responses.into_iter().collect()),
            }
        }
    }

    #[async_trait::async_trait]
    impl HttpClient for RecordingHttpClient {
        async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, HttpClientError> {
            self.urls
                .lock()
                .expect("urls lock")
                .push(request.url.to_string());
            self.responses
                .lock()
                .expect("responses lock")
                .pop_front()
                .expect("a queued response for every request")
        }
    }

    #[test]
    fn catalog_lookup_addresses_only_the_books_api() {
        let request = build_catalog_lookup_request(&isbn());
        assert_eq!(request.method, HttpMethod::Get);
        assert_eq!(
            request.url.as_str(),
            "https://openlibrary.org/api/books?bibkeys=ISBN%3A9780140328721&jscmd=data&format=json"
        );
        // The side-effect-free rule (ADR-0018 Decision 2): the import-on-miss
        // endpoint must never be addressed by the resolve lane.
        assert!(!request.url.path().starts_with("/isbn/"));
    }

    #[test]
    fn bibkeys_cover_all_schemes() {
        assert_eq!(bibkey(&isbn()), "ISBN:9780140328721");
        assert_eq!(
            bibkey(&BookIdentifier::oclc("ocm12345678").expect("valid oclc")),
            "OCLC:12345678"
        );
        assert_eq!(
            bibkey(&BookIdentifier::lccn("n 78-890351").expect("valid lccn")),
            "LCCN:n78890351"
        );
        assert_eq!(
            bibkey(&BookIdentifier::olid("7030731M").expect("valid olid")),
            "OLID:OL7030731M"
        );
    }

    #[test]
    fn parse_catalog_lookup_reads_the_data_shape() {
        // Trimmed replay of a real Books API jscmd=data response shape.
        let body = br#"{
            "ISBN:9780140328721": {
                "url": "https://openlibrary.org/books/OL7826547M/Matilda",
                "key": "/books/OL7826547M",
                "title": "Matilda",
                "authors": [{"url": "https://openlibrary.org/authors/OL34184A/Roald_Dahl", "name": "Roald Dahl"}],
                "number_of_pages": 240,
                "identifiers": {
                    "isbn_10": ["0140328726"],
                    "isbn_13": ["9780140328721"],
                    "openlibrary": ["OL7826547M"]
                },
                "publishers": [{"name": "Puffin"}],
                "publish_date": "October 1, 1988",
                "subjects": [{"name": "School stories", "url": "https://openlibrary.org/subjects/school_stories"}],
                "cover": {
                    "small": "https://covers.openlibrary.org/b/id/8314135-S.jpg",
                    "medium": "https://covers.openlibrary.org/b/id/8314135-M.jpg",
                    "large": "https://covers.openlibrary.org/b/id/8314135-L.jpg"
                }
            }
        }"#;
        let edition = parse_catalog_lookup(&isbn(), body).expect("record present");
        assert_eq!(edition.key.as_deref(), Some("/books/OL7826547M"));
        assert_eq!(edition.title.as_deref(), Some("Matilda"));
        assert_eq!(edition.authors, vec!["Roald Dahl"]);
        assert_eq!(edition.publishers, vec!["Puffin"]);
        assert_eq!(edition.publish_date.as_deref(), Some("October 1, 1988"));
        assert_eq!(edition.number_of_pages, Some(240));
        assert_eq!(edition.isbn_10, vec!["0140328726"]);
        assert_eq!(edition.isbn_13, vec!["9780140328721"]);
        assert_eq!(edition.subjects, vec!["School stories"]);
        assert_eq!(
            edition.cover_url.as_deref(),
            Some("https://covers.openlibrary.org/b/id/8314135-L.jpg")
        );
    }

    #[test]
    fn parse_catalog_lookup_miss_is_none_never_a_create() {
        // A Books API miss is an empty object: "no record found", full stop.
        assert!(parse_catalog_lookup(&isbn(), b"{}").is_none());
        assert!(parse_catalog_lookup(&isbn(), b"not json").is_none());
        // A response keyed by a different bibkey is also a miss for ours.
        assert!(parse_catalog_lookup(&isbn(), br#"{"ISBN:0000000000": {}}"#).is_none());
    }

    #[test]
    fn parse_catalog_lookup_tolerates_a_thin_record() {
        let body = br#"{"ISBN:9780140328721": {"title": "Matilda"}}"#;
        let edition = parse_catalog_lookup(&isbn(), body).expect("thin record still resolves");
        assert_eq!(edition.title.as_deref(), Some("Matilda"));
        assert!(edition.authors.is_empty());
        assert!(edition.subjects.is_empty());
        assert_eq!(edition.number_of_pages, None);
        assert_eq!(edition.cover_url, None);
    }

    #[test]
    fn scan_availability_request_uses_the_brief_read_api() {
        let request = build_scan_availability_request(&isbn());
        assert_eq!(request.method, HttpMethod::Get);
        assert_eq!(
            request.url.as_str(),
            "https://openlibrary.org/api/volumes/brief/isbn/9780140328721.json"
        );
        let by_olid =
            build_scan_availability_request(&BookIdentifier::olid("7030731M").expect("valid olid"));
        assert_eq!(
            by_olid.url.as_str(),
            "https://openlibrary.org/api/volumes/brief/olid/OL7030731M.json"
        );
    }

    #[test]
    fn scan_items_partition_by_match_and_only_exact_grounds() {
        let body = br#"{
            "records": {"/books/OL7826547M": {"recordURL": "https://openlibrary.org/books/OL7826547M"}},
            "items": [
                {"match": "similar", "status": "lendable", "itemURL": "https://archive.org/details/matilda00dahl_1", "ol-edition-id": "OL999M"},
                {"match": "exact", "status": "full access", "itemURL": "https://archive.org/details/matilda00dahl", "ol-edition-id": "OL7826547M"}
            ]
        }"#;
        let availability = parse_scan_availability(body).expect("parses");
        assert_eq!(availability.exact.len(), 1);
        assert_eq!(availability.similar.len(), 1);
        let scan = availability.groundable_scan().expect("exact scan grounds");
        assert_eq!(scan.item_url, "https://archive.org/details/matilda00dahl");
        assert_eq!(scan.status, "full access");
        assert_eq!(scan.ol_edition_id.as_deref(), Some("OL7826547M"));
    }

    #[test]
    fn similar_only_availability_never_grounds() {
        let body = br#"{"items": [{"match": "similar", "status": "lendable", "itemURL": "https://archive.org/details/other-edition"}]}"#;
        let availability = parse_scan_availability(body).expect("parses");
        assert!(availability.groundable_scan().is_none());
        assert_eq!(availability.similar.len(), 1);
    }

    #[test]
    fn empty_items_is_no_scan_not_no_record() {
        // A well-formed response with no items parses to an empty availability:
        // the catalog record still exists (ADR-0018 Decision 3).
        let availability = parse_scan_availability(br#"{"records": {}, "items": []}"#)
            .expect("well-formed response parses");
        assert_eq!(availability, ScanAvailability::default());
        // Same for a response omitting items entirely.
        assert_eq!(
            parse_scan_availability(b"{}").expect("parses"),
            ScanAvailability::default()
        );
        // Garbage does not parse.
        assert!(parse_scan_availability(b"<html>error</html>").is_none());
    }

    #[test]
    fn resolve_hit_returns_edition_and_scan() {
        let client = StubHttpClient::new(vec![Ok(ok(CATALOG_HIT)), Ok(ok(READ_API_EXACT))]);
        let outcome = block_on(resolve_book_source(&client, &book(vec![isbn()])));
        let BookResolutionOutcome::Resolved {
            identifier,
            edition,
            scan,
        } = outcome
        else {
            panic!("expected Resolved, got {outcome:?}");
        };
        assert_eq!(identifier, isbn());
        assert_eq!(edition.title.as_deref(), Some("Matilda"));
        let scan = scan.expect("availability was looked up");
        assert_eq!(scan.exact.len(), 1);
        assert!(scan.groundable_scan().is_some());
    }

    #[test]
    fn resolve_tries_identifiers_in_order_until_a_hit() {
        // ISBN misses cleanly; OCLC hits. The resolved identifier records
        // which key actually found the record.
        let oclc = BookIdentifier::oclc("12345678").expect("valid oclc");
        let oclc_hit = r#"{"OCLC:12345678": {"title": "Matilda"}}"#;
        let client = StubHttpClient::new(vec![
            Ok(ok("{}")),
            Ok(ok(oclc_hit)),
            Ok(ok(r#"{"items": []}"#)),
        ]);
        let outcome = block_on(resolve_book_source(
            &client,
            &book(vec![isbn(), oclc.clone()]),
        ));
        let BookResolutionOutcome::Resolved {
            identifier, scan, ..
        } = outcome
        else {
            panic!("expected Resolved, got {outcome:?}");
        };
        assert_eq!(identifier, oclc);
        // Empty items = record exists, no usable scan (not unknown).
        assert_eq!(scan, Some(ScanAvailability::default()));
    }

    #[test]
    fn resolve_clean_miss_is_not_found() {
        let client = StubHttpClient::new(vec![Ok(ok("{}"))]);
        let outcome = block_on(resolve_book_source(&client, &book(vec![isbn()])));
        assert_eq!(outcome, BookResolutionOutcome::NotFound);
    }

    #[test]
    fn resolve_transport_failure_is_lookup_failed_not_not_found() {
        // A failed lookup means "unknown", which must not masquerade as a
        // clean catalog miss.
        let client = StubHttpClient::new(vec![Err(HttpClientError::InvalidResponse {
            message: "connection reset".to_string(),
        })]);
        let outcome = block_on(resolve_book_source(&client, &book(vec![isbn()])));
        let BookResolutionOutcome::LookupFailed { message } = outcome else {
            panic!("expected LookupFailed, got {outcome:?}");
        };
        assert!(message.contains("connection reset"));
    }

    #[test]
    fn resolve_non_2xx_catalog_answer_is_lookup_failed() {
        let client = StubHttpClient::new(vec![Ok(HttpResponse {
            status: 503,
            headers: std::collections::BTreeMap::new(),
            body: Vec::new(),
        })]);
        let outcome = block_on(resolve_book_source(&client, &book(vec![isbn()])));
        let BookResolutionOutcome::LookupFailed { message } = outcome else {
            panic!("expected LookupFailed, got {outcome:?}");
        };
        assert!(message.contains("503"));
    }

    #[test]
    fn resolve_scan_availability_failure_degrades_to_unknown() {
        // The catalog hit stands even when the availability check fails; the
        // scan is simply unknown (None), not absent (Some(empty)).
        let client = StubHttpClient::new(vec![
            Ok(ok(CATALOG_HIT)),
            Err(HttpClientError::InvalidResponse {
                message: "read api down".to_string(),
            }),
        ]);
        let outcome = block_on(resolve_book_source(&client, &book(vec![isbn()])));
        let BookResolutionOutcome::Resolved { scan, .. } = outcome else {
            panic!("expected Resolved, got {outcome:?}");
        };
        assert_eq!(scan, None);
    }

    #[test]
    fn resolve_addresses_only_the_read_endpoints() {
        // The PRD-0009 DoD assertion: a full resolve (hit) and a miss both
        // address only /api/books and /api/volumes/brief — never the
        // import-on-miss /isbn/{isbn}.json path, and no write follows a miss.
        let client = RecordingHttpClient::new(vec![
            Ok(ok("{}")),        // isbn: clean miss
            Ok(ok(CATALOG_HIT)), // (second book) isbn: hit
            Ok(ok(READ_API_EXACT)),
        ]);
        let miss = block_on(resolve_book_source(&client, &book(vec![isbn()])));
        assert_eq!(miss, BookResolutionOutcome::NotFound);
        let hit = block_on(resolve_book_source(&client, &book(vec![isbn()])));
        assert!(matches!(hit, BookResolutionOutcome::Resolved { .. }));

        let urls = client.urls.lock().expect("urls lock");
        assert_eq!(urls.len(), 3, "miss issues exactly one read, no follow-up");
        for url in urls.iter() {
            assert!(
                url.starts_with("https://openlibrary.org/api/books?")
                    || url.starts_with("https://openlibrary.org/api/volumes/brief/"),
                "unexpected endpoint addressed: {url}"
            );
            // The import path sits at the site root (`openlibrary.org/isbn/…`),
            // distinct from the Read API's `/api/volumes/brief/isbn/…` segment.
            assert!(
                !url.starts_with("https://openlibrary.org/isbn/"),
                "import path must never be hit: {url}"
            );
        }
    }
}
