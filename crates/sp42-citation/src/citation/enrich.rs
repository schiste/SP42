//! Open Library enrichment proposals (PRD-0009 Layer 3, ADR-0019).
//!
//! Pure proposal computation — no I/O, no writes. Two stages:
//!
//! 1. **Candidates** ([`enrichment_candidates`]): field-level gaps in a
//!    resolved edition that SP42 can fill from a *deterministic, named
//!    source*. The MVP proposes only identifier completion — an ISBN-13
//!    derived from an ISBN-10 (and vice versa for `978-` ISBN-13s), the
//!    conversion being pure arithmetic on a value the record or the citation
//!    already carries verbatim. Fields needing external sourced context
//!    (subjects, cover, description) are deliberately not produced here; the
//!    synthesized description in particular is gated on rich Wikidata
//!    book-level context per PRD-0009 resolved Q3 and is future work.
//! 2. **Proposals** ([`propose_from_candidate`]): a candidate bound to a raw
//!    record read — the record key and `revision` pinned in (ADR-0019
//!    Decision 3), the exact replacement value for the field, and the edit
//!    comment. A proposal is inert data; only the apply mechanism
//!    ([`crate::citation::openlibrary_apply`]) can carry one to the site,
//!    and only after operator confirmation bound to [`EnrichmentProposal::content_hash`].
//!
//! A candidate whose gap turns out to be closed on the raw record (the
//! Books API view and the raw record can drift) yields **no** proposal — a
//! record already complete is never touched (a PRD-0009 `DoD` item).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::citation::openlibrary::OpenLibraryEdition;
use crate::citation::verify::sha256_hex;
use crate::wikitext_editor::BookIdentifier;

/// A field-level enrichment candidate: what could be added, from where.
/// Display-ready strings — the report's read-only proposal listing
/// (PRD-0009's "read-only proposal listing" surface) renders these directly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnrichmentCandidate {
    /// The raw-record field the candidate fills (e.g. `isbn_13`).
    pub field: String,
    /// The proposed value, verbatim.
    pub proposed: String,
    /// The named source the value traces to, human-readable
    /// (e.g. `derived from ISBN-10 0140328726 (record)`).
    pub source: String,
}

/// The ISBN-13 for an ISBN-10: `978` + the nine data digits + the EAN-13
/// check digit. Pure arithmetic; `None` for anything that is not a
/// checksum-valid ISBN-10.
#[must_use]
pub fn isbn13_from_isbn10(isbn10: &str) -> Option<String> {
    let BookIdentifier::Isbn(valid) = BookIdentifier::isbn(isbn10)? else {
        return None;
    };
    if valid.len() != 10 {
        return None;
    }
    let mut digits = String::with_capacity(13);
    digits.push_str("978");
    digits.push_str(&valid[..9]);
    let check = ean13_check_digit(digits.as_bytes())?;
    digits.push(char::from(b'0' + check));
    Some(digits)
}

/// The ISBN-10 for a `978`-prefixed ISBN-13: the nine data digits + the
/// mod-11 check digit (which can be `X`). `None` for a non-`978` prefix
/// (ISBN-979 books have no ISBN-10 form) or an invalid ISBN-13.
#[must_use]
pub fn isbn10_from_isbn13(isbn13: &str) -> Option<String> {
    let BookIdentifier::Isbn(valid) = BookIdentifier::isbn(isbn13)? else {
        return None;
    };
    if valid.len() != 13 || !valid.starts_with("978") {
        return None;
    }
    let mut digits: String = valid[3..12].to_string();
    let check = isbn10_check_digit(digits.as_bytes())?;
    digits.push(check);
    Some(digits)
}

/// EAN-13 check digit over the first 12 digits.
fn ean13_check_digit(first_twelve: &[u8]) -> Option<u8> {
    if first_twelve.len() != 12 || !first_twelve.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let sum: u32 = first_twelve
        .iter()
        .enumerate()
        .map(|(i, b)| u32::from(b - b'0') * if i % 2 == 0 { 1 } else { 3 })
        .sum();
    Some(u8::try_from((10 - (sum % 10)) % 10).expect("mod 10 fits u8"))
}

/// ISBN-10 check digit (mod 11, `X` for 10) over the first nine digits.
fn isbn10_check_digit(first_nine: &[u8]) -> Option<char> {
    if first_nine.len() != 9 || !first_nine.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let sum: u32 = first_nine
        .iter()
        .enumerate()
        .map(|(i, b)| (10 - u32::try_from(i).unwrap_or(0)) * u32::from(b - b'0'))
        .sum();
    let check = (11 - (sum % 11)) % 11;
    Some(if check == 10 {
        'X'
    } else {
        char::from(b'0' + u8::try_from(check).expect("mod 11 fits u8"))
    })
}

/// Compute the enrichment candidates for a resolved edition (PRD-0009
/// Layer 3, identifier completion only). `cited` is the citation's own
/// validated identifiers — a source SP42 already trusts for resolution.
/// A record that already carries both ISBN forms yields no candidates.
#[must_use]
pub fn enrichment_candidates(
    edition: &OpenLibraryEdition,
    cited: &[BookIdentifier],
) -> Vec<EnrichmentCandidate> {
    let cited_isbns = cited.iter().filter_map(|identifier| match identifier {
        BookIdentifier::Isbn(value) => Some(value.as_str()),
        _ => None,
    });

    // Known ISBN-10s / ISBN-13s, record values first (prefer the record's own
    // data as the derivation source), then the citation's.
    let mut known_10: Vec<(&str, &str)> = Vec::new();
    let mut known_13: Vec<(&str, &str)> = Vec::new();
    for value in &edition.isbn_10 {
        known_10.push((value.as_str(), "record"));
    }
    for value in &edition.isbn_13 {
        known_13.push((value.as_str(), "record"));
    }
    for value in cited_isbns {
        match value.len() {
            10 => known_10.push((value, "citation")),
            13 => known_13.push((value, "citation")),
            _ => {}
        }
    }

    let mut candidates = Vec::new();
    if edition.isbn_13.is_empty()
        && let Some((converted, source_value, origin)) = known_10
            .iter()
            .find_map(|(value, origin)| isbn13_from_isbn10(value).map(|c| (c, *value, *origin)))
    {
        candidates.push(EnrichmentCandidate {
            field: "isbn_13".to_string(),
            proposed: converted,
            source: format!("derived from ISBN-10 {source_value} ({origin})"),
        });
    }
    if edition.isbn_10.is_empty()
        && let Some((converted, source_value, origin)) = known_13
            .iter()
            .find_map(|(value, origin)| isbn10_from_isbn13(value).map(|c| (c, *value, *origin)))
    {
        candidates.push(EnrichmentCandidate {
            field: "isbn_10".to_string(),
            proposed: converted,
            source: format!("derived from ISBN-13 {source_value} ({origin})"),
        });
    }
    candidates
}

/// A raw Open Library record read (`/books/OL…M.json`): the revision the
/// drift discipline pins against, plus the full document for verbatim
/// replay by the apply lanes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenLibraryRecord {
    /// Record key, e.g. `/books/OL7826547M`.
    pub key: String,
    /// The record revision this read observed.
    pub revision: u64,
    /// The full raw document.
    pub raw: Value,
}

/// Parse a raw record read. `None` for an unparseable body or one missing
/// the key/revision the discipline depends on.
#[must_use]
pub fn parse_record(body: &[u8]) -> Option<OpenLibraryRecord> {
    let raw: Value = serde_json::from_slice(body).ok()?;
    let key = raw.get("key")?.as_str()?.to_string();
    let revision = raw.get("revision")?.as_u64()?;
    Some(OpenLibraryRecord { key, revision, raw })
}

/// One confirmed-or-confirmable field change, pinned to the record revision
/// it was computed against (ADR-0019 Decision 3). Inert data: nothing here
/// performs I/O, and the apply mechanism replays exactly this.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnrichmentProposal {
    /// The record the proposal targets, e.g. `/books/OL7826547M`.
    pub record_key: String,
    /// The revision the proposal was computed against; apply refuses if the
    /// record has moved past it.
    pub base_revision: u64,
    /// The raw-record field the proposal sets.
    pub field: String,
    /// The field's current raw value (`None` = absent), echoed so the
    /// operator sees exactly what changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<Value>,
    /// The complete replacement value for the field (not a delta) — the
    /// apply writes exactly this.
    pub proposed: Value,
    /// The named source of the proposed value (provenance, shown to the
    /// operator).
    pub source: String,
    /// The edit comment the apply carries as `_comment`.
    pub comment: String,
}

impl EnrichmentProposal {
    /// Content hash the operator's confirmation binds to (ADR-0019
    /// Decision 3): any change to what would be written changes the hash,
    /// so a confirmation can never authorize anything but this proposal.
    #[must_use]
    pub fn content_hash(&self) -> String {
        let identity = serde_json::json!({
            "record_key": self.record_key,
            "base_revision": self.base_revision,
            "field": self.field,
            "proposed": self.proposed,
            "comment": self.comment,
        });
        sha256_hex(identity.to_string().as_bytes())
    }

    /// The single-value string form of the proposed value, for the form
    /// lane's input field. `None` when the value has no such form (the form
    /// lane then refuses rather than guessing).
    #[must_use]
    pub fn proposed_form_value(&self) -> Option<String> {
        match &self.proposed {
            Value::String(value) => Some(value.clone()),
            Value::Array(values) if values.len() == 1 => {
                values[0].as_str().map(ToString::to_string)
            }
            _ => None,
        }
    }
}

/// Bind a candidate to a raw record read, re-checking the gap against the
/// raw document (the Books API view can lag). `None` when the field is
/// already populated — a closed gap is never re-proposed.
#[must_use]
pub fn propose_from_candidate(
    record: &OpenLibraryRecord,
    candidate: &EnrichmentCandidate,
) -> Option<EnrichmentProposal> {
    let current = record.raw.get(&candidate.field);
    let occupied = current.is_some_and(|value| match value {
        Value::Array(items) => !items.is_empty(),
        Value::Null => false,
        Value::String(s) => !s.trim().is_empty(),
        _ => true,
    });
    if occupied {
        return None;
    }
    Some(EnrichmentProposal {
        record_key: record.key.clone(),
        base_revision: record.revision,
        field: candidate.field.clone(),
        current: current.cloned(),
        proposed: Value::Array(vec![Value::String(candidate.proposed.clone())]),
        source: candidate.source.clone(),
        comment: format!(
            "Add {field} {value} ({source}) — SP42 assisted edit, operator-confirmed",
            field = candidate.field,
            value = candidate.proposed,
            source = candidate.source,
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::{
        EnrichmentCandidate, OpenLibraryRecord, enrichment_candidates, isbn10_from_isbn13,
        isbn13_from_isbn10, parse_record, propose_from_candidate,
    };
    use crate::citation::openlibrary::OpenLibraryEdition;
    use crate::wikitext_editor::BookIdentifier;
    use serde_json::json;

    #[test]
    fn isbn_conversion_round_trips_with_correct_check_digits() {
        // Matilda: ISBN-10 0140328726 <-> ISBN-13 9780140328721.
        assert_eq!(
            isbn13_from_isbn10("0-14-032872-6").as_deref(),
            Some("9780140328721")
        );
        assert_eq!(
            isbn10_from_isbn13("978-0-14-032872-1").as_deref(),
            Some("0140328726")
        );
        // An X check digit survives the 13 -> 10 direction.
        let thirteen = isbn13_from_isbn10("080442957X").expect("valid isbn-10");
        assert_eq!(isbn10_from_isbn13(&thirteen).as_deref(), Some("080442957X"));
        // Invalid inputs and 979-prefixed ISBN-13s convert to nothing.
        assert_eq!(isbn13_from_isbn10("0140328720"), None, "bad checksum");
        assert_eq!(isbn10_from_isbn13("9791036700248"), None, "979 has no -10");
    }

    fn edition(isbn_10: &[&str], isbn_13: &[&str]) -> OpenLibraryEdition {
        OpenLibraryEdition {
            isbn_10: isbn_10.iter().map(ToString::to_string).collect(),
            isbn_13: isbn_13.iter().map(ToString::to_string).collect(),
            ..OpenLibraryEdition::default()
        }
    }

    #[test]
    fn missing_isbn13_is_proposed_from_the_record_isbn10() {
        let candidates = enrichment_candidates(&edition(&["0140328726"], &[]), &[]);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].field, "isbn_13");
        assert_eq!(candidates[0].proposed, "9780140328721");
        assert!(candidates[0].source.contains("0140328726"));
        assert!(candidates[0].source.contains("(record)"));
    }

    #[test]
    fn missing_isbn10_is_proposed_from_the_cited_isbn13() {
        // The record has no ISBNs at all; the citation's validated ISBN-13
        // supplies the ISBN-10 derivation, marked as citation-sourced.
        // (Only conversions are proposed — never a verbatim relay of the
        // citation's own value into the record.)
        let cited = [BookIdentifier::isbn("978-0-14-032872-1").expect("valid")];
        let candidates = enrichment_candidates(&edition(&[], &[]), &cited);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].field, "isbn_10");
        assert_eq!(candidates[0].proposed, "0140328726");
        assert!(candidates[0].source.contains("(citation)"));
    }

    #[test]
    fn complete_record_yields_no_candidates() {
        // PRD-0009 DoD: a record already complete yields no proposal.
        let candidates = enrichment_candidates(
            &edition(&["0140328726"], &["9780140328721"]),
            &[BookIdentifier::isbn("9780140328721").expect("valid")],
        );
        assert!(candidates.is_empty());
    }

    #[test]
    fn record_parse_pins_key_and_revision() {
        let record = parse_record(
            br#"{"key": "/books/OL7826547M", "revision": 7, "title": "Matilda", "isbn_10": ["0140328726"]}"#,
        )
        .expect("parses");
        assert_eq!(record.key, "/books/OL7826547M");
        assert_eq!(record.revision, 7);
        // Missing revision or key is unusable for the drift discipline.
        assert_eq!(parse_record(br#"{"key": "/books/OL1M"}"#), None);
        assert_eq!(parse_record(b"not json"), None);
    }

    fn candidate() -> EnrichmentCandidate {
        EnrichmentCandidate {
            field: "isbn_13".to_string(),
            proposed: "9780140328721".to_string(),
            source: "derived from ISBN-10 0140328726 (record)".to_string(),
        }
    }

    #[test]
    fn proposal_binds_revision_and_replacement_value() {
        let record = OpenLibraryRecord {
            key: "/books/OL7826547M".to_string(),
            revision: 7,
            raw: json!({"key": "/books/OL7826547M", "revision": 7, "isbn_10": ["0140328726"]}),
        };
        let proposal = propose_from_candidate(&record, &candidate()).expect("open gap");
        assert_eq!(proposal.base_revision, 7);
        assert_eq!(proposal.field, "isbn_13");
        assert_eq!(proposal.proposed, json!(["9780140328721"]));
        assert_eq!(proposal.current, None);
        assert!(proposal.comment.contains("9780140328721"));
        assert_eq!(
            proposal.proposed_form_value().as_deref(),
            Some("9780140328721")
        );
        // The confirmation hash moves with anything that would be written.
        let mut other = proposal.clone();
        other.proposed = json!(["9999999999999"]);
        assert_ne!(proposal.content_hash(), other.content_hash());
        assert_eq!(proposal.content_hash(), proposal.clone().content_hash());
    }

    #[test]
    fn closed_gap_on_the_raw_record_yields_no_proposal() {
        // The Books API view said the field was missing, but the raw record
        // already has it (view lag): never re-propose a closed gap.
        let record = OpenLibraryRecord {
            key: "/books/OL7826547M".to_string(),
            revision: 8,
            raw: json!({"key": "/books/OL7826547M", "revision": 8, "isbn_13": ["9780140328721"]}),
        };
        assert_eq!(propose_from_candidate(&record, &candidate()), None);
        // An explicitly empty array is an OPEN gap.
        let record_empty = OpenLibraryRecord {
            key: "/books/OL7826547M".to_string(),
            revision: 8,
            raw: json!({"key": "/books/OL7826547M", "revision": 8, "isbn_13": []}),
        };
        assert!(propose_from_candidate(&record_empty, &candidate()).is_some());
    }
}
