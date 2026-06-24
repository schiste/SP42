//! Page-level verification: request, orchestrator output report, and stats.

use crate::citation::concurrency::map_with_concurrency;
use crate::citation::extract::{BlockFailure, ExtractOutcome, SkippedRef};
use crate::citation::verdict::{CitationVerdict, SupportLevel};
use crate::citation::verify::{
    CitationFinding, FetchedSource, VerifyOptions, fetch_source, verify_citation_use_site,
};
use sp42_types::{Clock, HttpClient, ModelClient, ModelRef};
use std::collections::HashMap;

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

/// Verify every use-site in `extract` against its source. Fetches each distinct
/// source URL once (shared via the prefetched option), fans the existing
/// per-use-site verifier with bounded concurrency, and assembles a read-only
/// report. A per-use-site error becomes an extraction-failure entry, never a
/// top-level error.
pub async fn verify_page<C, M>(
    fetch_client: &C,
    model_client: &M,
    clock: &dyn Clock,
    panel: &[ModelRef],
    page: &PageVerificationRequest,
    extract: ExtractOutcome,
    options: VerifyOptions,
) -> PageVerificationReport
where
    C: HttpClient + ?Sized,
    M: ModelClient + ?Sized,
{
    let ExtractOutcome {
        use_sites,
        skipped,
        failures,
    } = extract;
    // refs_seen = every ref we encountered: those that became use-sites, those
    // skipped (non-URL), and those that failed extraction.
    let refs_seen = use_sites.len() + skipped.len() + failures.len();

    // Pre-bind shared refs OUTSIDE the closures so the spawned futures capture
    // plain `&`/`&dyn` (Copy) references, not re-borrows of locals — mirrors the
    // panel fan-out in verify.rs and avoids fighting the borrow checker.
    let fetch_client: &C = fetch_client;
    let model_client: &M = model_client;
    let clock: &dyn Clock = clock;
    let panel: &[ModelRef] = panel;

    // 1. Dedupe: distinct source URLs, fetched once each.
    let mut distinct: Vec<String> = Vec::new();
    for us in &use_sites {
        let u = us.request.source_url.to_string();
        if !distinct.contains(&u) {
            distinct.push(u);
        }
    }
    let concurrency = options.concurrency;
    let fetched_list = map_with_concurrency(distinct.clone(), concurrency, |url, _| async move {
        (url.clone(), fetch_source(fetch_client, &url).await.ok())
    })
    .await;
    let mut bodies: HashMap<String, FetchedSource> = HashMap::new();
    for (url, source) in fetched_list {
        if let Some(source) = source {
            bodies.insert(url, source);
        }
    }

    // 2. Fan verify over use-sites, sharing the prefetched body.
    let mut extraction_failures = failures;
    let mut findings = Vec::new();
    let bodies_ref = &bodies;
    let options_ref = &options;
    let results = map_with_concurrency(use_sites, concurrency, |us, _| async move {
        let mut opts = options_ref.clone();
        opts.prefetched = bodies_ref.get(&us.request.source_url.to_string()).cloned();
        let outcome = verify_citation_use_site(
            fetch_client,
            model_client,
            clock,
            panel,
            &us.request,
            Some(&us.context),
            us.use_site_ordinal,
            opts,
        )
        .await;
        (us.ref_id, us.block_ordinal, outcome)
    })
    .await;

    for (ref_id, block_ordinal, outcome) in results {
        match outcome {
            Ok(o) => findings.push(o.finding),
            Err(error) => extraction_failures.push(BlockFailure {
                block_ordinal,
                reason: format!("verify failed for {ref_id}: {error}"),
            }),
        }
    }

    // 3. Stats.
    let mut stats = PageVerificationStats {
        refs_seen,
        use_sites_verified: findings.len(),
        skipped: skipped.len(),
        extraction_failures: extraction_failures.len(),
        ..PageVerificationStats::default()
    };
    for f in &findings {
        match f.verdict {
            CitationVerdict::Judged(SupportLevel::Supported) => stats.supported += 1,
            CitationVerdict::Judged(SupportLevel::Partial) => stats.partial += 1,
            CitationVerdict::Judged(SupportLevel::NotSupported) => stats.not_supported += 1,
            CitationVerdict::SourceUnavailable => stats.source_unavailable += 1,
        }
    }

    PageVerificationReport {
        wiki_id: page.wiki_id.clone(),
        rev_id: page.rev_id,
        title: page.title.clone(),
        findings,
        skipped,
        extraction_failures,
        stats,
    }
}

#[cfg(test)]
mod orchestrator_tests {
    use super::*;
    use crate::citation::extract::extract_use_sites;
    use crate::wikitext_editor::{BlockKind, BlockRef, ParsoidBlock};
    use sp42_types::{FixedClock, HttpResponse, StubHttpClient, StubModelClient};

    fn page() -> PageVerificationRequest {
        PageVerificationRequest {
            wiki_id: "enwiki".into(),
            title: "Cats".into(),
            rev_id: 1,
        }
    }

    fn model_ref() -> ModelRef {
        ModelRef::new("test", "m", "m")
    }

    fn completion(text: &str) -> sp42_types::ModelCompletion {
        sp42_types::ModelCompletion {
            text: text.to_string(),
            served_model: None,
        }
    }

    // Two use-sites citing the SAME url. With dedupe, exactly one fetch.
    fn two_use_sites_same_url() -> ExtractOutcome {
        let b = ParsoidBlock {
            text: "Cats purr. Cats sleep.".into(),
            section_path: vec!["Behaviour".into()],
            refs: vec![
                BlockRef {
                    offset: 10,
                    ref_id: "r1".into(),
                    source_urls: vec![url::Url::parse("https://s.test/x").unwrap()],
                    ref_text: "[1]".into(),
                    named: false,
                },
                BlockRef {
                    offset: 22,
                    ref_id: "r2".into(),
                    source_urls: vec![url::Url::parse("https://s.test/x").unwrap()],
                    ref_text: "[2]".into(),
                    named: false,
                },
            ],
            block_kind: BlockKind::Paragraph,
            block_ordinal: 0,
        };
        extract_use_sites(&[b], &page())
    }

    #[test]
    fn dedupes_fetches_and_verifies_each_use_site() {
        use futures::executor::block_on;
        // EXACTLY ONE http response: proves the shared URL is fetched once.
        let http = StubHttpClient::new([Ok(HttpResponse {
            status: 200,
            headers: std::collections::BTreeMap::new(),
            body: b"cats purr and sleep".to_vec(),
        })]);
        // One model completion per use-site (panel size 1, repair off).
        let model = StubModelClient::new([
            Ok(completion(r#"{"verdict":"SUPPORTED","quote":"cats purr"}"#)),
            Ok(completion(r#"{"verdict":"SUPPORTED","quote":"sleep"}"#)),
        ]);
        let options = VerifyOptions {
            repair_turn: false,
            concurrency: 2,
            ..VerifyOptions::default()
        };
        let report = block_on(verify_page(
            &http,
            &model,
            &FixedClock::new(0),
            &[model_ref()],
            &page(),
            two_use_sites_same_url(),
            options,
        ));
        assert_eq!(report.findings.len(), 2);
        assert_eq!(report.stats.use_sites_verified, 2);
        // If a second fetch had been attempted the queue would be empty → error path.
        assert!(report.extraction_failures.is_empty());
    }
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
        let back: PageVerificationReport = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(report, back);
    }
}
