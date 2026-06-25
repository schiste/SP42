//! Page-level verification: request, orchestrator output report, and stats.

use crate::citation::concurrency::map_with_concurrency;
use crate::citation::extract::{BlockFailure, ExtractOutcome, SkippedRef};
use crate::citation::verdict::{CitationVerdict, SupportLevel};
use crate::citation::verify::{
    CitationFinding, FetchedSource, VerificationOutcome, VerifyOptions, fetch_source,
    verify_citation_use_site,
};
use crate::errors::CitationVerificationError;
use sp42_types::{Clock, HttpClient, ModelClient, ModelRef};
use std::collections::{HashMap, HashSet};

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

/// Try archive fallbacks if the primary URL came back `SourceUnavailable`.
/// Returns the original outcome if no archives work or if no archives exist.
async fn try_archive_fallback<C, M>(
    fetch_client: &C,
    model_client: &M,
    clock: &dyn Clock,
    panel: &[ModelRef],
    us: &crate::citation::extract::CitationUseSite,
    outcome: Result<VerificationOutcome, CitationVerificationError>,
    options: &VerifyOptions,
) -> Result<VerificationOutcome, CitationVerificationError>
where
    C: HttpClient + ?Sized,
    M: ModelClient + ?Sized,
{
    if let Ok(o) = &outcome
        && matches!(o.finding.verdict, CitationVerdict::SourceUnavailable)
        && !us.archive_urls.is_empty()
    {
        for archive in &us.archive_urls {
            if let Ok(body) = fetch_source(fetch_client, archive.as_str()).await {
                let mut alt_request = us.request.clone();
                alt_request.source_url = archive.clone();
                let mut alt_opts = options.clone();
                // Don't fetch Citoid metadata for archive verification; the body fetch is the only
                // network call we want to make for fallback sources.
                alt_opts.include_metadata = false;
                alt_opts.prefetched = Some(body);
                if let Ok(alt) = verify_citation_use_site(
                    fetch_client,
                    model_client,
                    clock,
                    panel,
                    &alt_request,
                    Some(&us.context),
                    us.use_site_ordinal,
                    alt_opts,
                )
                .await
                    && !matches!(alt.finding.verdict, CitationVerdict::SourceUnavailable)
                {
                    return Ok(alt);
                }
            }
        }
    }
    outcome
}

/// Verify a single use-site, including archive fallback attempts if the primary
/// URL is unavailable. Returns (`ref_id`, `block_ordinal`, `outcome`).
async fn verify_one_use_site<C, M>(
    fetch_client: &C,
    model_client: &M,
    clock: &dyn Clock,
    panel: &[ModelRef],
    us: crate::citation::extract::CitationUseSite,
    bodies: &HashMap<String, FetchedSource>,
    options: &VerifyOptions,
) -> (
    String,
    usize,
    Result<VerificationOutcome, CitationVerificationError>,
)
where
    C: HttpClient + ?Sized,
    M: ModelClient + ?Sized,
{
    let mut opts = options.clone();
    opts.prefetched = bodies.get(&us.request.source_url.to_string()).cloned();
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

    // Try archive fallbacks if primary came back unavailable.
    let outcome = try_archive_fallback(
        fetch_client,
        model_client,
        clock,
        panel,
        &us,
        outcome,
        options,
    )
    .await;

    (us.ref_id, us.block_ordinal, outcome)
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
    // refs_seen = every ref we encountered: those that became use-sites (count by distinct ref_id,
    // since extract_use_sites emits one use-site per source URL), those skipped (non-URL),
    // and those that failed extraction.
    let distinct_use_site_refs: HashSet<&str> =
        use_sites.iter().map(|u| u.ref_id.as_str()).collect();
    let refs_seen = distinct_use_site_refs.len() + skipped.len() + failures.len();

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
        (url.clone(), fetch_source(fetch_client, &url).await)
    })
    .await;
    let mut bodies: HashMap<String, FetchedSource> = HashMap::new();
    for (url, result) in fetched_list {
        let source = match result {
            Ok(source) => source,
            Err(_) => {
                // Transport error: insert a sentinel (empty text, status 0) so no use-site
                // re-fetches. The empty-text path routes to SourceUnavailable via the
                // body-usability gate (body_classifier.rs).
                FetchedSource {
                    text: String::new(),
                    status: 0,
                }
            }
        };
        bodies.insert(url, source);
    }

    // 2. Fan verify over use-sites, sharing the prefetched body.
    // Every distinct URL is now in `bodies` (including sentinel entries for failed fetches),
    // so every use-site finds a prefetched body and never re-fetches.
    // Archive URLs are consulted on-demand only if the primary URL returns SourceUnavailable.
    let mut extraction_failures = failures;
    let mut findings = Vec::new();
    let results = map_with_concurrency(use_sites, concurrency, |us, _| {
        verify_one_use_site(
            fetch_client,
            model_client,
            clock,
            panel,
            us,
            &bodies,
            &options,
        )
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
                    sources: vec![crate::wikitext_editor::CitedSource {
                        url: url::Url::parse("https://s.test/x").unwrap(),
                        archive_urls: vec![],
                    }],
                    ref_text: "[1]".into(),
                    named: false,
                },
                BlockRef {
                    offset: 22,
                    ref_id: "r2".into(),
                    sources: vec![crate::wikitext_editor::CitedSource {
                        url: url::Url::parse("https://s.test/x").unwrap(),
                        archive_urls: vec![],
                    }],
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

    #[test]
    fn failed_fetch_is_not_refetched_per_use_site() {
        use futures::executor::block_on;
        use sp42_types::HttpClientError;

        // EXACTLY ONE fetch error in the queue: a transport-layer failure.
        // Both use-sites will encounter this single failing fetch, which gets converted
        // to an empty-text sentinel. No use-site should ever re-fetch (the queue is consumed).
        let http = StubHttpClient::new([Err(HttpClientError::Transport {
            message: "network timeout".to_string(),
        })]);
        // No model calls should occur since the body is unavailable (SourceUnavailable
        // short-circuits before model invocation).
        let model = StubModelClient::new([]);
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
        // Both use-sites should produce SourceUnavailable findings (not errors),
        // routed through the empty-text/status-0 sentinel → body-usability gate.
        assert_eq!(report.findings.len(), 2);
        assert_eq!(report.stats.use_sites_verified, 2);
        assert_eq!(report.stats.source_unavailable, 2);
        // No model invocations occurred, so no extraction failures from the model side.
        assert!(report.extraction_failures.is_empty());
        // Verify both findings are SourceUnavailable.
        for finding in &report.findings {
            assert_eq!(
                finding.verdict,
                super::CitationVerdict::SourceUnavailable,
                "both findings should be SourceUnavailable"
            );
        }
    }

    #[test]
    fn refs_seen_counts_distinct_refs_not_use_sites() {
        use futures::executor::block_on;
        // Regression test for Issue 2: refs_seen should count DISTINCT refs
        // (by ref_id), not use-sites. A single ref with multiple source URLs
        // produces multiple use-sites but should be counted as one ref.
        let block = ParsoidBlock {
            text: "Cats are animals.".into(),
            section_path: vec!["Facts".into()],
            refs: vec![BlockRef {
                offset: 5,
                ref_id: "ref_multi_url".into(),
                // ONE ref with TWO cited sources → TWO use-sites in extract_use_sites
                sources: vec![
                    crate::wikitext_editor::CitedSource {
                        url: url::Url::parse("https://s.test/a").unwrap(),
                        archive_urls: vec![],
                    },
                    crate::wikitext_editor::CitedSource {
                        url: url::Url::parse("https://s.test/b").unwrap(),
                        archive_urls: vec![],
                    },
                ],
                ref_text: "[1]".into(),
                named: false,
            }],
            block_kind: BlockKind::Paragraph,
            block_ordinal: 0,
        };
        let extract = extract_use_sites(&[block], &page());
        // Verify that extract_use_sites produces 2 use-sites (one per URL)
        assert_eq!(extract.use_sites.len(), 2, "should have 2 use-sites");
        assert!(extract.skipped.is_empty(), "no skipped refs");
        assert!(extract.failures.is_empty(), "no failures");

        // Now verify via verify_page
        let http = StubHttpClient::new([
            Ok(HttpResponse {
                status: 200,
                headers: std::collections::BTreeMap::new(),
                body: b"test content a".to_vec(),
            }),
            Ok(HttpResponse {
                status: 200,
                headers: std::collections::BTreeMap::new(),
                body: b"test content b".to_vec(),
            }),
        ]);
        let model = StubModelClient::new([
            Ok(completion(r#"{"verdict":"SUPPORTED","quote":"are"}"#)),
            Ok(completion(r#"{"verdict":"SUPPORTED","quote":"are"}"#)),
        ]);
        let options = VerifyOptions::default();
        let report = block_on(verify_page(
            &http,
            &model,
            &FixedClock::new(0),
            &[model_ref()],
            &page(),
            extract,
            options,
        ));

        // refs_seen should be 1 (one ref), but use_sites_verified should be 2 (two use-sites)
        assert_eq!(
            report.stats.refs_seen, 1,
            "refs_seen should count 1 distinct ref, not 2 use-sites"
        );
        assert_eq!(
            report.stats.use_sites_verified, 2,
            "use_sites_verified should count 2 use-sites"
        );
    }

    #[test]
    fn archive_fallback_used_when_primary_unavailable() {
        use futures::executor::block_on;

        // Build a use-site with primary and archive URLs.
        let primary = url::Url::parse("https://s.test/primary").unwrap();
        let archive = url::Url::parse("https://archive.org/web/20240101/s.test/primary").unwrap();
        let b = ParsoidBlock {
            text: "Test claim here.".into(),
            section_path: vec!["Section".into()],
            refs: vec![BlockRef {
                offset: 5,
                ref_id: "r1".into(),
                sources: vec![crate::wikitext_editor::CitedSource {
                    url: primary.clone(),
                    archive_urls: vec![archive.clone()],
                }],
                ref_text: "[1]".into(),
                named: false,
            }],
            block_kind: BlockKind::Paragraph,
            block_ordinal: 0,
        };
        let extract = extract_use_sites(&[b], &page());
        assert_eq!(extract.use_sites.len(), 1);

        // HTTP queue: primary returns 503 (unavailable), archive returns good body.
        // The primary fetch is done in verify_page's dedupe pass; the archive fetch
        // is on-demand in try_archive_fallback.
        // Note: body must be >= 300 chars to pass the body-usability gate.
        let archive_body = b"This is the archived version of the page with substantial content \
that exceeds the minimum body length threshold. It contains enough text to demonstrate that \
the archive was successfully fetched and verified. The content discusses various topics and \
provides context about the cited material. This ensures that the body classifier will not \
reject it as too short, allowing the verification process to proceed normally."
            .to_vec();
        let http = StubHttpClient::new([
            // Primary fetch (dedupe pass in verify_page)
            Ok(HttpResponse {
                status: 503,
                headers: std::collections::BTreeMap::new(),
                body: vec![],
            }),
            // Archive fetch (fallback in try_archive_fallback)
            Ok(HttpResponse {
                status: 200,
                headers: std::collections::BTreeMap::new(),
                body: archive_body,
            }),
        ]);
        // Model panel: one completion for the archive verify (SUPPORTED)
        let model =
            StubModelClient::new([Ok(completion(r#"{"verdict":"SUPPORTED","quote":"test"}"#))]);
        let options = VerifyOptions::default();
        let report = block_on(verify_page(
            &http,
            &model,
            &FixedClock::new(0),
            &[model_ref()],
            &page(),
            extract,
            options,
        ));

        // Should have one finding (the archive verify), not SourceUnavailable.
        assert_eq!(report.findings.len(), 1);
        let finding = &report.findings[0];
        assert!(
            !matches!(finding.verdict, super::CitationVerdict::SourceUnavailable),
            "finding should not be SourceUnavailable (archive was used)"
        );
        assert_eq!(
            finding.provenance.url, archive,
            "finding should cite the archive URL"
        );
        assert_eq!(report.stats.use_sites_verified, 1);
        assert!(report.extraction_failures.is_empty());
    }

    #[test]
    fn archive_not_fetched_when_primary_available() {
        use futures::executor::block_on;

        // Build a use-site with primary and archive URLs.
        let primary = url::Url::parse("https://s.test/primary").unwrap();
        let archive = url::Url::parse("https://archive.org/web/20240101/s.test/primary").unwrap();
        let b = ParsoidBlock {
            text: "Test claim here.".into(),
            section_path: vec!["Section".into()],
            refs: vec![BlockRef {
                offset: 5,
                ref_id: "r1".into(),
                sources: vec![crate::wikitext_editor::CitedSource {
                    url: primary.clone(),
                    archive_urls: vec![archive.clone()],
                }],
                ref_text: "[1]".into(),
                named: false,
            }],
            block_kind: BlockKind::Paragraph,
            block_ordinal: 0,
        };
        let extract = extract_use_sites(&[b], &page());

        // HTTP queue: EXACTLY ONE response (primary succeeds). If archive is fetched,
        // the queue will be drained and cause an error. This proves the archive was NOT fetched.
        // Note: body must be >= 300 chars to pass the body-usability gate.
        let primary_body = b"This is the primary page with substantial content that exceeds the \
minimum body length threshold for the classifier. It contains enough text to demonstrate that \
the primary source was successfully fetched and verified. The content provides context about \
the cited material and shows that the archive fallback was not needed. This ensures that if \
archive is fetched, the queue will be empty and the test will fail, proving the archive was not accessed.".to_vec();
        let http = StubHttpClient::new([Ok(HttpResponse {
            status: 200,
            headers: std::collections::BTreeMap::new(),
            body: primary_body,
        })]);
        // Model panel: one completion (SUPPORTED from primary)
        let model =
            StubModelClient::new([Ok(completion(r#"{"verdict":"SUPPORTED","quote":"test"}"#))]);
        let options = VerifyOptions::default();
        let report = block_on(verify_page(
            &http,
            &model,
            &FixedClock::new(0),
            &[model_ref()],
            &page(),
            extract,
            options,
        ));

        // Should have one successful finding citing the primary URL.
        assert_eq!(report.findings.len(), 1);
        let finding = &report.findings[0];
        assert_eq!(
            finding.provenance.url, primary,
            "finding should cite the primary URL"
        );
        assert_eq!(
            finding.verdict,
            super::CitationVerdict::Judged(SupportLevel::Supported)
        );
        // No extraction failures (if archive had been fetched, queue drain would cause error).
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
