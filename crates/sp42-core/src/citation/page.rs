//! Page-level verification: request, orchestrator output report, and stats.

use crate::citation::extract::{BlockFailure, SkippedRef};
use crate::citation::verify::CitationFinding;

/// Identity of the page to verify.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PageVerificationRequest {
    pub wiki_id: String,
    pub title: String,
    pub rev_id: u64,
}

/// Counts summarising a page run.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PageVerificationStats {
    pub refs_seen: usize,
    pub use_sites_verified: usize,
    pub skipped: usize,
    pub extraction_failures: usize,
    pub supported: usize,
    pub partial: usize,
    pub not_supported: usize,
    pub source_unavailable: usize,
}

/// Read-only result of verifying every URL-bearing citation on a page.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PageVerificationReport {
    pub wiki_id: String,
    pub rev_id: u64,
    pub title: String,
    pub findings: Vec<CitationFinding>,
    pub skipped: Vec<SkippedRef>,
    pub extraction_failures: Vec<BlockFailure>,
    pub stats: PageVerificationStats,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_round_trips_through_serde() {
        let report = PageVerificationReport {
            wiki_id: "frwiki".to_string(),
            rev_id: 42,
            title: "Exemple".to_string(),
            findings: Vec::new(),
            skipped: Vec::new(),
            extraction_failures: Vec::new(),
            stats: PageVerificationStats::default(),
        };
        let json = serde_json::to_string(&report).expect("serialize");
        let back: PageVerificationReport =
            serde_json::from_str(&json).expect("deserialize");
        assert_eq!(report, back);
    }
}
