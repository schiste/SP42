//! Article-level claim↔ref extraction: turning the editor's `ParsoidBlock`
//! intermediate into per-use-site verification inputs. Pure, no DOM, no I/O.

use crate::citation::prompts::ClaimContext;
use crate::citation::verify::CitationVerificationRequest;

/// One citation use-site: a claim sentence, one source URL, and the context
/// passed alongside it. The unit the orchestrator fans the verifier over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CitationUseSite {
    /// Document-order index across the page.
    pub use_site_ordinal: u32,
    /// Document-order index of the block this use-site came from (provenance;
    /// used to attribute verify errors back to a block in the report).
    pub block_ordinal: usize,
    /// Claim + source URL + page identity for the verifier.
    pub request: CitationVerificationRequest,
    /// Section title + preceding sentences passed alongside the claim.
    pub context: ClaimContext,
    /// The originating ref's marker id, for provenance.
    pub ref_id: String,
}

/// Why a ref produced no use-site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkippedReason {
    /// The ref carries no extractable URL (book/ISBN/offline source).
    NonUrlSource,
}

/// A ref that was intentionally not verified.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SkippedRef {
    pub ref_id: String,
    pub reason: SkippedReason,
    pub block_ordinal: usize,
}

/// A block (or ref) that could not be processed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BlockFailure {
    pub block_ordinal: usize,
    pub reason: String,
}

/// Result of extracting use-sites from a page's blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractOutcome {
    pub use_sites: Vec<CitationUseSite>,
    pub skipped: Vec<SkippedRef>,
    pub failures: Vec<BlockFailure>,
}
