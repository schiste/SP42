//! Content-addressed source-snapshot + verdict-record storage (ADR-0009).
//!
//! Two versioned serde envelopes persist behind the injected `Storage` trait:
//! - [`SnapshotEnvelope`] — the **extracted source body** addressed by the SHA-256 of its
//!   bytes (free dedup + tamper-evidence). Only the body is stored — no headers, cookies,
//!   client IP, or tokens (ADR-0009 §6); bibliographic metadata is never part of the
//!   hashed bytes (ADR-0009 §3).
//! - [`VerdictEnvelope`] — the voted verdict plus the **whole panel**: every per-model
//!   vote tagged with its [`ModelRef`] and the measured [`PanelAgreement`], so a verdict is
//!   reproducible and auditable against the exact models that produced it (ADR-0009 §3).
//!
//! Determinism (Art. 2.1) is achieved by pinning the substrate: given the same stored
//! snapshot and the same N recorded model responses, the pure vote + grounding gate yield
//! the same voted verdict and the same agreement — even though the model is not
//! bit-deterministic (ADR-0009 §1). The records are immutable and read-only; this layer
//! cannot reach the write path (ADR-0009 §4).

use serde::Serialize;
use serde::de::DeserializeOwned;
use url::Url;

use super::verdict::CitationVerdict;
use super::verify::{CitationFinding, LocatedPassage, ModelRef, sha256_hex};
use super::voting::PanelAgreement;
use crate::branding::PROJECT_NAME;
use crate::errors::CitationStorageError;
use crate::traits::Storage;

/// Snapshot schema version (ADR-0009 §3).
pub const SNAPSHOT_SCHEMA_VERSION: u32 = 1;
/// Verdict-record schema version (ADR-0009 §3).
pub const VERDICT_SCHEMA_VERSION: u32 = 1;

/// A content-addressed snapshot of one fetched source's extracted body (ADR-0009 §2).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SnapshotEnvelope {
    /// Provenance marker (the project name).
    pub project: String,
    /// Schema version.
    pub version: u32,
    /// The source URL the body was fetched from.
    pub source_url: Url,
    /// Fetch time in epoch ms (from the injected `Clock`).
    pub fetched_at_ms: i64,
    /// SHA-256 hex of `body_text` — the content-addressing identity.
    pub content_hash: String,
    /// The extracted source body (the grounded bytes; no headers/secrets).
    pub body_text: String,
}

/// One panel member's recorded vote (ADR-0009 §3).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ModelVote {
    /// The model that cast this vote.
    pub model: ModelRef,
    /// Its returned verdict.
    pub verdict: CitationVerdict,
    /// Its located passage, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub located_passage: Option<LocatedPassage>,
}

/// A verdict record persisting the whole panel that produced the voted verdict
/// (ADR-0009 §3). Immutable and read-only.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VerdictEnvelope {
    /// Provenance marker (the project name).
    pub project: String,
    /// Schema version.
    pub version: u32,
    /// The verified claim.
    pub claim: String,
    /// The snapshot this verdict was grounded against (its `content_hash`).
    pub snapshot_hash: String,
    /// Record time in epoch ms (from the injected `Clock`).
    pub recorded_at_ms: i64,
    /// The voted categorical verdict.
    pub verdict: CitationVerdict,
    /// The winning verdict's located passage, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub located_passage: Option<LocatedPassage>,
    /// Every per-model vote (with its `ModelRef`).
    pub panel_votes: Vec<ModelVote>,
    /// Measured agreement among the votes.
    pub agreement: PanelAgreement,
    /// The configured panel members.
    pub panel_ref: Vec<ModelRef>,
}

/// Build a snapshot envelope, computing the content hash over the body bytes.
#[must_use]
pub fn build_snapshot(source_url: Url, body_text: String, fetched_at_ms: i64) -> SnapshotEnvelope {
    let content_hash = sha256_hex(body_text.as_bytes());
    SnapshotEnvelope {
        project: PROJECT_NAME.to_string(),
        version: SNAPSHOT_SCHEMA_VERSION,
        source_url,
        fetched_at_ms,
        content_hash,
        body_text,
    }
}

/// Build a verdict envelope from a finding plus the recorded per-model votes.
#[must_use]
pub fn build_verdict_envelope(
    claim: impl Into<String>,
    finding: &CitationFinding,
    panel_votes: Vec<ModelVote>,
    panel_ref: Vec<ModelRef>,
    recorded_at_ms: i64,
) -> VerdictEnvelope {
    VerdictEnvelope {
        project: PROJECT_NAME.to_string(),
        version: VERDICT_SCHEMA_VERSION,
        claim: claim.into(),
        snapshot_hash: finding.provenance.content_hash.clone(),
        recorded_at_ms,
        verdict: finding.verdict,
        located_passage: finding.passage.clone(),
        panel_votes,
        agreement: finding.agreement,
        panel_ref,
    }
}

/// The storage key for a snapshot, addressed by its content hash.
#[must_use]
pub fn snapshot_storage_key(content_hash: &str) -> String {
    format!("citation:snapshot:{content_hash}")
}

/// The storage key for a verdict record (snapshot hash + claim hash, so distinct claims
/// over the same snapshot do not collide).
#[must_use]
pub fn verdict_storage_key(snapshot_hash: &str, claim: &str) -> String {
    format!(
        "citation:verdict:{snapshot_hash}:{}",
        sha256_hex(claim.as_bytes())
    )
}

/// Serialize an envelope to canonical JSON bytes.
///
/// # Errors
///
/// Returns [`CitationStorageError::Serialize`] on a serialization failure.
pub fn serialize_envelope<T: Serialize>(value: &T) -> Result<Vec<u8>, CitationStorageError> {
    serde_json::to_vec(value).map_err(|error| CitationStorageError::Serialize {
        message: error.to_string(),
    })
}

/// Parse an envelope from JSON bytes.
///
/// # Errors
///
/// Returns [`CitationStorageError::InvalidInput`] on a parse failure.
pub fn parse_envelope<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, CitationStorageError> {
    serde_json::from_slice(bytes).map_err(|error| CitationStorageError::InvalidInput {
        message: error.to_string(),
    })
}

/// Persist a snapshot, returning its storage key.
///
/// # Errors
///
/// Returns [`CitationStorageError`] on a serialization or storage-backend failure.
pub async fn store_snapshot<S>(
    storage: &S,
    snapshot: &SnapshotEnvelope,
) -> Result<String, CitationStorageError>
where
    S: Storage + ?Sized,
{
    let key = snapshot_storage_key(&snapshot.content_hash);
    let bytes = serialize_envelope(snapshot)?;
    storage
        .set(key.clone(), bytes)
        .await
        .map_err(|error| CitationStorageError::Storage {
            message: error.to_string(),
        })?;
    Ok(key)
}

/// Load a snapshot by content hash, or `None` if absent.
///
/// # Errors
///
/// Returns [`CitationStorageError`] on a storage-backend or parse failure.
pub async fn load_snapshot<S>(
    storage: &S,
    content_hash: &str,
) -> Result<Option<SnapshotEnvelope>, CitationStorageError>
where
    S: Storage + ?Sized,
{
    let key = snapshot_storage_key(content_hash);
    match storage
        .get(&key)
        .await
        .map_err(|error| CitationStorageError::Storage {
            message: error.to_string(),
        })? {
        Some(bytes) => Ok(Some(parse_envelope(&bytes)?)),
        None => Ok(None),
    }
}

/// Persist a verdict record, returning its storage key.
///
/// # Errors
///
/// Returns [`CitationStorageError`] on a serialization or storage-backend failure.
pub async fn store_verdict<S>(
    storage: &S,
    verdict: &VerdictEnvelope,
) -> Result<String, CitationStorageError>
where
    S: Storage + ?Sized,
{
    let key = verdict_storage_key(&verdict.snapshot_hash, &verdict.claim);
    let bytes = serialize_envelope(verdict)?;
    storage
        .set(key.clone(), bytes)
        .await
        .map_err(|error| CitationStorageError::Storage {
            message: error.to_string(),
        })?;
    Ok(key)
}

/// Load a verdict record by snapshot hash + claim, or `None` if absent.
///
/// # Errors
///
/// Returns [`CitationStorageError`] on a storage-backend or parse failure.
pub async fn load_verdict<S>(
    storage: &S,
    snapshot_hash: &str,
    claim: &str,
) -> Result<Option<VerdictEnvelope>, CitationStorageError>
where
    S: Storage + ?Sized,
{
    let key = verdict_storage_key(snapshot_hash, claim);
    match storage
        .get(&key)
        .await
        .map_err(|error| CitationStorageError::Storage {
            message: error.to_string(),
        })? {
        Some(bytes) => Ok(Some(parse_envelope(&bytes)?)),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use futures::executor::block_on;

    use super::{
        ModelVote, SnapshotEnvelope, VerdictEnvelope, build_snapshot, build_verdict_envelope,
        load_snapshot, load_verdict, snapshot_storage_key, store_snapshot, store_verdict,
    };
    use crate::citation::parsing::ParsedVerdict;
    use crate::citation::verdict::{CitationFindingKind, CitationVerdict, SupportLevel, Verdict};
    use crate::citation::verify::{
        CitationFinding, GroundingAssertion, LocatedPassage, ModelRef, SourceProvenance,
        assemble_citation_finding, sha256_hex,
    };
    use crate::citation::voting::PanelAgreement;
    use crate::traits::MemoryStorage;

    fn url() -> url::Url {
        "https://example.com/source".parse().expect("url")
    }

    #[test]
    fn snapshot_content_hash_is_over_the_body_only() {
        let snapshot = build_snapshot(url(), "the museum opened in 1850".to_string(), 42);
        assert_eq!(
            snapshot.content_hash,
            sha256_hex(b"the museum opened in 1850")
        );
        // A different body yields a different address (content addressing).
        let other = build_snapshot(url(), "a different body entirely".to_string(), 42);
        assert_ne!(snapshot.content_hash, other.content_hash);
    }

    #[test]
    fn snapshot_round_trips_through_storage() {
        let storage = MemoryStorage::default();
        let snapshot = build_snapshot(url(), "real article body text".to_string(), 100);
        let key = block_on(store_snapshot(&storage, &snapshot)).expect("store");
        assert_eq!(key, snapshot_storage_key(&snapshot.content_hash));
        let loaded: SnapshotEnvelope = block_on(load_snapshot(&storage, &snapshot.content_hash))
            .expect("load")
            .expect("present");
        assert_eq!(loaded, snapshot);
    }

    #[test]
    fn missing_snapshot_loads_as_none() {
        let storage = MemoryStorage::default();
        let loaded = block_on(load_snapshot(&storage, "deadbeef")).expect("load");
        assert!(loaded.is_none());
    }

    fn sample_finding() -> CitationFinding {
        CitationFinding {
            kind: CitationFindingKind::CitationVerdict,
            verdict: CitationVerdict::Judged(SupportLevel::Supported),
            agreement: PanelAgreement::new(2, 2),
            passage: Some(LocatedPassage {
                quote: "opened in 1850".to_string(),
                offset: 11,
            }),
            provenance: SourceProvenance {
                url: url(),
                content_hash: sha256_hex(b"the museum opened in 1850"),
                fetched_at: 42,
            },
            grounding: GroundingAssertion::LocatedQuote {
                quote: "opened in 1850".to_string(),
                source_hash: sha256_hex(b"the museum opened in 1850"),
                offset: 11,
            },
            use_site_ordinal: 0,
            schema_version: crate::citation::verify::SCHEMA_VERSION,
        }
    }

    #[test]
    fn verdict_round_trips_with_the_whole_panel() {
        let storage = MemoryStorage::default();
        let finding = sample_finding();
        let panel = vec![
            ModelRef::new("openrouter", "model-a", "model-a"),
            ModelRef::new("openrouter", "model-b", "model-b"),
        ];
        let votes = vec![
            ModelVote {
                model: panel[0].clone(),
                verdict: CitationVerdict::Judged(SupportLevel::Supported),
                located_passage: finding.passage.clone(),
            },
            ModelVote {
                model: panel[1].clone(),
                verdict: CitationVerdict::Judged(SupportLevel::Supported),
                located_passage: None,
            },
        ];
        let envelope = build_verdict_envelope(
            "the museum opened in 1850",
            &finding,
            votes,
            panel.clone(),
            200,
        );
        let key = block_on(store_verdict(&storage, &envelope)).expect("store");
        let loaded: VerdictEnvelope = block_on(load_verdict(
            &storage,
            &envelope.snapshot_hash,
            "the museum opened in 1850",
        ))
        .expect("load")
        .expect("present");
        assert_eq!(loaded, envelope);
        assert_eq!(loaded.panel_votes.len(), 2);
        assert_eq!(loaded.panel_ref, panel);
        assert!(key.contains("citation:verdict:"));
    }

    #[test]
    fn replay_is_deterministic_over_the_same_snapshot_and_votes() {
        // DoD item 5: same snapshot + same N recorded votes => same voted verdict + agreement.
        let snapshot = build_snapshot(url(), "Acme Corp was established in 1985.".to_string(), 1);
        let provenance = SourceProvenance {
            url: snapshot.source_url.clone(),
            content_hash: snapshot.content_hash.clone(),
            fetched_at: snapshot.fetched_at_ms,
        };
        let votes = vec![
            ParsedVerdict {
                verdict: Verdict::Supported,
                quote: Some("established in 1985".to_string()),
            },
            ParsedVerdict {
                verdict: Verdict::Supported,
                quote: Some("established in 1985".to_string()),
            },
            ParsedVerdict {
                verdict: Verdict::NotSupported,
                quote: None,
            },
        ];
        let first = assemble_citation_finding(&snapshot.body_text, &provenance, &votes, 0);
        let second = assemble_citation_finding(&snapshot.body_text, &provenance, &votes, 0);
        assert_eq!(first, second);
        assert_eq!(
            first.verdict,
            CitationVerdict::Judged(SupportLevel::Supported)
        );
        assert_eq!(first.agreement, PanelAgreement::new(3, 2));
    }

    #[test]
    fn snapshot_of_unusable_body_is_addressable_but_replays_to_abstention() {
        // A snapshot of an empty/unusable body still round-trips, and assembling over it
        // with no usable votes yields an abstention, never a support judgment.
        let snapshot = build_snapshot(url(), String::new(), 1);
        let provenance = SourceProvenance {
            url: snapshot.source_url.clone(),
            content_hash: snapshot.content_hash.clone(),
            fetched_at: 1,
        };
        let votes = vec![ParsedVerdict {
            verdict: Verdict::SourceUnavailable,
            quote: None,
        }];
        let finding = assemble_citation_finding(&snapshot.body_text, &provenance, &votes, 0);
        assert_eq!(finding.verdict, CitationVerdict::SourceUnavailable);
    }
}
