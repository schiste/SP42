//! Page-level verification: request, orchestrator output report, and stats.

use crate::citation::citoid::CitoidMetadata;
use crate::citation::concurrency::map_with_concurrency;
use crate::citation::extract::{
    BlockFailure, BookUseSite, CitationUseSite, ExtractOutcome, SkippedReason, SkippedRef,
};
use crate::citation::openlibrary::{
    BookResolution, BookResolutionOutcome, OpenLibraryEdition, build_catalog_lookup_request,
    resolve_book_source,
};
use crate::citation::search_inside::{
    BookGroundingPreparation, extract_ocaid, prepare_book_grounding, scan_deep_link,
};
use crate::citation::verdict::{CitationVerdict, SupportLevel};
use crate::citation::verify::{
    BookScanProvenance, CitationFinding, CitationVerificationRequest, FetchedSource,
    SourceProvenance, SourceUnavailableReason, VerificationOutcome, VerifyOptions,
    book_scan_unavailable_outcome, book_searched_not_supported_outcome, fetch_source, sha256_hex,
    verify_citation_use_site,
};
use crate::errors::CitationVerificationError;
use sp42_types::{Clock, HttpClient, ModelClient, ModelRef};
use std::collections::{HashMap, HashSet};

/// Cap on the total bytes of prefetched source bodies retained at once during a page run.
/// Beyond this, distinct sources are no longer cached up front (they are fetched per use-site
/// instead), bounding peak memory on citation-heavy pages independent of the citation count.
/// Sized to comfortably hold a typical page's HTML sources while capping a pathological one.
const MAX_PREFETCH_CACHE_BYTES: usize = 64 * 1024 * 1024;

/// Bytes a prefetched source retains in the page cache: the extracted text plus the
/// pre-extraction HTML kept for the usability gate. `raw_html` can dwarf `text` on
/// chrome-heavy pages, so it must count against [`MAX_PREFETCH_CACHE_BYTES`] — otherwise
/// the cache can hold far more than the intended cap.
fn prefetch_retained_bytes(source: &FetchedSource) -> usize {
    source.text.len() + source.raw_html.as_ref().map_or(0, String::len)
}

/// Identity of the page to verify.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PageVerificationRequest {
    pub wiki_id: String,
    pub title: String,
    /// The revision to verify. `0` (or an absent field) means **the latest
    /// revision**: the server resolves it to a concrete id before verifying and
    /// records that id in the report, so the result stays reproducible. `MediaWiki`
    /// revision ids are always `>= 1`, so `0` is an unambiguous sentinel.
    #[serde(default)]
    pub rev_id: u64,
}

/// Identifies one finding to re-verify (PRD-0014): the page it lives on, plus
/// the originating ref's marker id. `rev_id == 0` means "latest", mirroring
/// `PageVerificationRequest` — Re-verify checks the *current* article state,
/// not necessarily the revision the original finding was produced against.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ReverifyFindingRequest {
    pub wiki_id: String,
    pub title: String,
    #[serde(default)]
    pub rev_id: u64,
    pub ref_id: String,
    /// The finding's cited source URL — the **stable** cross-revision use-site
    /// identity. A single `ref_id` can bundle several source URLs, and re-verify
    /// runs against the *latest* revision, where the page-global `use_site_ordinal`
    /// has shifted if any earlier citation changed. Matching on `(ref_id, source_url)`
    /// re-verifies the citation the operator was looking at regardless of position.
    /// This is `finding.archive_of.unwrap_or(finding.provenance.url)` — the original
    /// cited URL, equal to the use-site's `request.source_url`. Preferred over
    /// `use_site_ordinal` when both are present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    /// The finding's document-order use-site position — a positional fallback used
    /// only when `source_url` is absent (a ref bundling several URLs, disambiguated
    /// by ordinal within a single revision). `None` falls back to the first `ref_id`
    /// match (back-compatible with pre-PRD-0014 callers).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub use_site_ordinal: Option<u32>,
}

/// Select the use-site a re-verify request targets from the freshly-extracted
/// use-sites of the current revision.
///
/// Preference order, most stable first:
/// 1. `(ref_id, source_url)` — the citation the operator selected, matched by its
///    cited URL, so it survives the page-global `use_site_ordinal` shifting when an
///    earlier citation changes between the report's revision and the current one.
/// 2. `(ref_id, use_site_ordinal)` — a positional fallback for a multi-source ref
///    within a single revision, when the caller sent no `source_url`.
/// 3. First `ref_id` match — back-compatible with callers that send neither.
///
/// Returns `None` when nothing matches (the ref was removed, or its cited URL
/// changed), which the route surfaces as `ref-not-found`.
#[must_use]
pub fn select_reverify_use_site(
    use_sites: Vec<CitationUseSite>,
    ref_id: &str,
    source_url: Option<&str>,
    use_site_ordinal: Option<u32>,
) -> Option<CitationUseSite> {
    use_sites.into_iter().find(|use_site| {
        use_site.ref_id == ref_id
            && match (source_url, use_site_ordinal) {
                (Some(url), _) => use_site.request.source_url.as_str() == url,
                (None, Some(ordinal)) => use_site.use_site_ordinal == ordinal,
                (None, None) => true,
            }
    })
}

/// Apply a re-verified finding back into the page report: replace the finding for
/// `(ref_id, use_site_ordinal)` with `fresh` and recompute the verdict tallies so the
/// report's stats (and the grouped-by-verdict sections a browser renders from it) stay
/// consistent — a `NotSupported` finding that re-verifies to `Supported` moves to the
/// Supported section and both counts update, instead of lingering under the old header.
///
/// `refs_seen`, `skipped`, and `extraction_failures` are unchanged: re-verify replaces a
/// verdict for an existing use-site, it never adds or removes a ref. The fresh finding's
/// identity/position (`ref_id`, `use_site_ordinal`) is preserved from the slot it replaces
/// (the fresh verdict came from the latest revision, whose ordinal may differ). Returns
/// `true` when a matching finding was found and replaced.
pub fn apply_reverified_finding(
    report: &mut PageVerificationReport,
    ref_id: &str,
    use_site_ordinal: u32,
    mut fresh: CitationFinding,
) -> bool {
    let Some(slot) = report
        .findings
        .iter_mut()
        .find(|finding| finding.ref_id == ref_id && finding.use_site_ordinal == use_site_ordinal)
    else {
        return false;
    };
    fresh.ref_id.clone_from(&slot.ref_id);
    fresh.use_site_ordinal = slot.use_site_ordinal;
    *slot = fresh;
    report.stats = tally_stats(
        report.stats.refs_seen,
        &report.findings,
        report.stats.skipped,
        report.stats.extraction_failures,
        &report.book_resolutions,
    );
    true
}

/// Result of re-verifying one finding: the fresh finding, replacing the
/// operator's card in place (PRD-0014).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ReverifyFindingResponse {
    pub wiki_id: String,
    pub rev_id: u64,
    pub title: String,
    pub finding: CitationFinding,
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
    /// Of `source_unavailable`: the link is dead (missing/non-2xx) — the citation
    /// is actionable. Carried here so a reviewer summary can show the split
    /// without re-aggregating findings (ADR-0011 §7).
    pub source_unavailable_unreachable: usize,
    /// Of `source_unavailable`: fetched 2xx but unreadable (PDF/JS/wrong page) — a
    /// tool limitation, the citation may be fine.
    pub source_unavailable_unusable: usize,
    /// Book citations resolved to an Open Library record (PRD-0009 Layer 1).
    #[serde(default)]
    pub books_resolved: usize,
    /// Book citations whose identifiers were all looked up cleanly with no
    /// catalog record found.
    #[serde(default)]
    pub books_not_found: usize,
    /// Book citations whose catalog lookup failed in transport (existence
    /// unknown — distinct from `books_not_found`).
    #[serde(default)]
    pub book_lookups_failed: usize,
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
    /// Open Library resolutions for the skipped book refs (PRD-0009 Layer 1),
    /// one entry per book source, in skip order.
    #[serde(default)]
    pub book_resolutions: Vec<BookResolution>,
    pub stats: PageVerificationStats,
}

/// Project a page report's findings into review-session overlay markers
/// (PRD-0017): one marker per finding, joined to the article by the
/// finding's `ref_id` — the same anchor coordinate review prompts use — so
/// a review surface can show the report's problems in the context of the
/// article instead of as detached text. Verdicts keep their wire labels;
/// claims are truncated to the outline display limit; grounding caveats and
/// unavailable reasons ride along as `detail`. Findings without a `ref_id`
/// (the standalone single-claim path) cannot anchor and are skipped.
#[must_use]
pub fn review_finding_markers(
    report: &PageVerificationReport,
) -> Vec<sp42_platform::ReviewFindingMarker> {
    report
        .findings
        .iter()
        .filter(|finding| !finding.ref_id.is_empty())
        .map(|finding| sp42_platform::ReviewFindingMarker {
            ref_id: finding.ref_id.clone(),
            verdict: finding.verdict.as_wire().to_string(),
            claim: sp42_platform::truncate_outline_text(&finding.claim),
            detail: finding_marker_detail(finding),
        })
        .collect()
}

/// The marker's short qualifier: the unavailable reason for an abstention,
/// or the grounding caveat for a support judgment that did not locate
/// verbatim (SP42#25 layer 6 — surfaced, never silently upgraded).
fn finding_marker_detail(finding: &CitationFinding) -> Option<String> {
    if finding.verdict == CitationVerdict::SourceUnavailable {
        return finding
            .source_unavailable_reason
            .map(|reason| format!("source {}", reason.as_str()));
    }
    match finding.grounding_status {
        crate::citation::verify::GroundingStatus::Located
        | crate::citation::verify::GroundingStatus::NotApplicable => None,
        crate::citation::verify::GroundingStatus::LocatedFuzzy => {
            Some("support located by fuzzy match only".to_string())
        }
        crate::citation::verify::GroundingStatus::Unlocated => {
            Some("support quote not located in source (unverified)".to_string())
        }
    }
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
    // Only fall back to an archive when the live URL is *unreachable* (dead link). An
    // `Unusable` source (fetched 2xx but a PDF/JS shell/wrong page) is a tool limitation, not
    // a dead link (ADR-0011 §7) — archiving it would wrongly stamp `archive_of` and tell the
    // reviewer to repair a live URL that is fine. It would also usually be futile (the archived
    // copy is the same unreadable artifact).
    if let Ok(o) = &outcome
        && matches!(o.finding.verdict, CitationVerdict::SourceUnavailable)
        && o.finding.source_unavailable_reason == Some(SourceUnavailableReason::Unreachable)
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
                if let Ok(mut alt) = verify_citation_use_site(
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
                    // The verdict came from the archive; record the unreachable live
                    // URL it stands in for so the report can flag it for repair.
                    alt.finding.archive_of = Some(us.request.source_url.clone());
                    return Ok(alt);
                }
            }
        }
    }
    outcome
}

/// Verify a single use-site, including archive fallback attempts if the primary
/// URL is unavailable. Returns (`ref_id`, `block_ordinal`, `outcome`).
///
/// Used both by `verify_page`'s fan-out (with a shared prefetched-body cache)
/// and directly by the Re-verify route (PRD-0014, with an empty `bodies` map
/// so the source is always fetched fresh) — the single entry point for
/// per-use-site verification, so Re-verify introduces no new verification
/// logic of its own.
pub async fn verify_one_use_site<C, M, S>(
    fetch_client: &C,
    model_client: &M,
    clock: &dyn Clock,
    panel: &[ModelRef],
    us: crate::citation::extract::CitationUseSite,
    bodies: &HashMap<String, FetchedSource, S>,
    options: &VerifyOptions,
) -> (
    String,
    usize,
    Result<VerificationOutcome, CitationVerificationError>,
)
where
    C: HttpClient + ?Sized,
    M: ModelClient + ?Sized,
    S: std::hash::BuildHasher,
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

    // Stamp the page-provenance the report needs to be self-contained: which ref
    // this verdict belongs to, the claim it judged, and the context it was read
    // against. (`archive_of` is stamped inside try_archive_fallback.)
    let outcome = outcome.map(|mut o| {
        o.finding.ref_id.clone_from(&us.ref_id);
        o.finding.is_bare_url_ref = us.is_bare_url_ref;
        o.finding.claim.clone_from(&us.request.claim);
        o.finding
            .preceding_context
            .clone_from(&us.context.preceding_sentences);
        o
    });

    (us.ref_id, us.block_ordinal, outcome)
}

/// Tally the page-level summary counts from the verified findings. The
/// `source_unavailable` total is further split into `unreachable` (dead link,
/// actionable) vs `unusable` (fetched but unreadable) so a reviewer summary can
/// show the distinction without re-aggregating findings (ADR-0011 §7).
fn tally_stats(
    refs_seen: usize,
    findings: &[CitationFinding],
    skipped: usize,
    extraction_failures: usize,
    book_resolutions: &[BookResolution],
) -> PageVerificationStats {
    let mut stats = PageVerificationStats {
        refs_seen,
        use_sites_verified: findings.len(),
        skipped,
        extraction_failures,
        ..PageVerificationStats::default()
    };
    for resolution in book_resolutions {
        match &resolution.outcome {
            BookResolutionOutcome::Resolved { .. } => stats.books_resolved += 1,
            BookResolutionOutcome::NotFound => stats.books_not_found += 1,
            BookResolutionOutcome::LookupFailed { .. } => stats.book_lookups_failed += 1,
        }
    }
    for f in findings {
        match f.verdict {
            CitationVerdict::Judged(SupportLevel::Supported) => stats.supported += 1,
            CitationVerdict::Judged(SupportLevel::Partial) => stats.partial += 1,
            CitationVerdict::Judged(SupportLevel::NotSupported) => stats.not_supported += 1,
            CitationVerdict::SourceUnavailable => {
                stats.source_unavailable += 1;
                match f.source_unavailable_reason {
                    Some(SourceUnavailableReason::Unreachable) => {
                        stats.source_unavailable_unreachable += 1;
                    }
                    Some(SourceUnavailableReason::Unusable) => {
                        stats.source_unavailable_unusable += 1;
                    }
                    None => {}
                }
            }
        }
    }
    stats
}

/// What one book use-site produced besides its [`BookResolution`]: a finding
/// (resolved book, grounded or honestly `SourceUnavailable`), the refined skip
/// (identifier present, no catalog record / failed lookup), or a verify error.
enum BookVerdict {
    Finding(Box<CitationFinding>),
    Skip(SkippedRef),
    Failure(BlockFailure),
}

/// The finding-side provenance URL for a resolved book: the human-facing Open
/// Library record when the edition names one, else the catalog lookup for the
/// identifier that actually resolved — not the ref's first identifier, which
/// may be one that missed (Codex round 3, PR 147).
fn book_record_url(
    edition: &OpenLibraryEdition,
    resolved_identifier: &crate::wikitext_editor::BookIdentifier,
) -> url::Url {
    edition
        .record_url
        .as_deref()
        .and_then(|record| record.parse().ok())
        .unwrap_or_else(|| build_catalog_lookup_request(resolved_identifier).url)
}

/// A no-model book finding (unavailable / searched-and-nothing-found), with
/// the ref/claim/context provenance stamped like `verify_one_use_site` does.
#[allow(clippy::too_many_arguments)] // https://github.com/schiste/SP42 distinct named provenance inputs, same trade-off as verify_citation_use_site
fn book_no_model_finding(
    site: &BookUseSite,
    url: url::Url,
    content_hash: String,
    http_status: u16,
    clock: &dyn Clock,
    not_supported: bool,
    book_scan: Option<BookScanProvenance>,
) -> CitationFinding {
    let provenance = SourceProvenance {
        url,
        content_hash,
        fetched_at: clock.now_ms(),
        http_status: Some(http_status),
    };
    let outcome = if not_supported {
        book_searched_not_supported_outcome(&provenance, site.use_site_ordinal)
    } else {
        book_scan_unavailable_outcome(&provenance, site.use_site_ordinal)
    };
    let mut finding = outcome.finding;
    finding.ref_id.clone_from(&site.ref_id);
    finding.claim.clone_from(&site.claim);
    finding
        .preceding_context
        .clone_from(&site.context.preceding_sentences);
    finding.book_scan = book_scan;
    finding
}

/// Judge one book claim against its assembled snippet body: the existing
/// verifier runs over the snippets (scoped short-body bypass), the resolved
/// record rides the metadata sidecar as prompt context (context only, never
/// grounded), and the finding gains the book-scan provenance — including the
/// **scanned** page the passage was actually found on, so a pagination
/// mismatch surfaces instead of a false `not_supported` (PRD-0009 Q5).
#[allow(clippy::too_many_arguments)] // https://github.com/schiste/SP42 injected edges + site, same trade-off as verify_citation_use_site
async fn judge_book_snippets<C, M>(
    fetch_client: &C,
    model_client: &M,
    clock: &dyn Clock,
    panel: &[ModelRef],
    page: &PageVerificationRequest,
    site: &BookUseSite,
    edition: &OpenLibraryEdition,
    ocaid: &str,
    options: &VerifyOptions,
    body: &crate::citation::search_inside::BookSnippetBody,
) -> BookVerdict
where
    C: HttpClient + ?Sized,
    M: ModelClient + ?Sized,
{
    let source_url: url::Url = scan_deep_link(ocaid, body.page_of_passage(None), &body.query)
        .parse()
        .unwrap_or_else(|_| {
            format!("https://archive.org/details/{ocaid}")
                .parse()
                .expect("archive details url parses")
        });
    let request = CitationVerificationRequest {
        wiki_id: page.wiki_id.clone(),
        rev_id: page.rev_id,
        title: page.title.clone(),
        claim: site.claim.clone(),
        source_url,
    };
    let mut opts = options.clone();
    opts.include_metadata = false;
    // The resolved record is legitimate prompt context (context only, never
    // grounded) — reuse the metadata sidecar slot.
    opts.metadata_sidecar = Some(CitoidMetadata {
        publication: (!edition.publishers.is_empty()).then(|| edition.publishers.join(", ")),
        published: edition.publish_date.clone(),
        author: (!edition.authors.is_empty()).then(|| edition.authors.join(", ")),
        title: edition.title.clone(),
        url: request.source_url.to_string(),
    });
    opts.prefetched = Some(FetchedSource {
        text: body.text.clone(),
        status: 200,
        content_type: "text/plain".to_string(),
        raw_html: None,
        book_snippet: true,
    });
    match verify_citation_use_site(
        fetch_client,
        model_client,
        clock,
        panel,
        &request,
        Some(&site.context),
        site.use_site_ordinal,
        opts,
    )
    .await
    {
        Ok(outcome) => {
            let mut finding = outcome.finding;
            finding.ref_id.clone_from(&site.ref_id);
            finding.claim.clone_from(&site.claim);
            finding
                .preceding_context
                .clone_from(&site.context.preceding_sentences);
            let scanned_page =
                body.page_of_passage(finding.passage.as_ref().map(|p| p.quote.as_str()));
            // The pre-verdict request URL was built with page_of_passage(None)
            // (first snippet); now that the verifier has picked the passage,
            // re-anchor the provenance deep link to the page that actually
            // carries it (Codex P2, PR 147). The grounded bytes are the
            // assembled snippet body, so content_hash is unaffected.
            if let Ok(anchored) =
                scan_deep_link(ocaid, scanned_page, &body.query).parse::<url::Url>()
            {
                finding.provenance.url = anchored;
            }
            finding.book_scan = Some(BookScanProvenance {
                ocaid: ocaid.to_string(),
                scanned_page,
                cited_page: site.book.cited_page.clone(),
                note: (!body.cited_page_hit && site.book.cited_page.is_some())
                    .then(|| "cited page had no match; whole-book search".to_string()),
            });
            BookVerdict::Finding(Box::new(finding))
        }
        Err(error) => BookVerdict::Failure(BlockFailure {
            block_ordinal: site.block_ordinal,
            reason: format!("book verify failed for {}: {error}", site.ref_id),
        }),
    }
}

/// Ground and judge one resolved book claim against its exact-edition scan:
/// search-inside snippets become the source body for the existing verifier
/// (with the scoped short-body bypass), and every degraded outcome maps onto
/// the ADR-0007 split (`SourceUnavailable` vs `not_supported`).
#[allow(clippy::too_many_arguments)] // https://github.com/schiste/SP42 injected edges + site, same trade-off as verify_citation_use_site
async fn ground_resolved_book<C, M>(
    fetch_client: &C,
    model_client: &M,
    clock: &dyn Clock,
    panel: &[ModelRef],
    page: &PageVerificationRequest,
    site: &BookUseSite,
    edition: &OpenLibraryEdition,
    ocaid: &str,
    options: &VerifyOptions,
) -> BookVerdict
where
    C: HttpClient + ?Sized,
    M: ModelClient + ?Sized,
{
    let preparation = prepare_book_grounding(
        fetch_client,
        ocaid,
        &site.claim,
        site.book.cited_page.as_deref(),
    )
    .await;
    let scan_url = |page_number: Option<u32>, query: &str| -> url::Url {
        scan_deep_link(ocaid, page_number, query)
            .parse()
            .unwrap_or_else(|_| {
                format!("https://archive.org/details/{ocaid}")
                    .parse()
                    .expect("archive details url parses")
            })
    };
    match preparation {
        BookGroundingPreparation::Body(body) => {
            judge_book_snippets(
                fetch_client,
                model_client,
                clock,
                panel,
                page,
                site,
                edition,
                ocaid,
                options,
                &body,
            )
            .await
        }
        BookGroundingPreparation::NoMatches {
            response_hash,
            deep_link,
        } => {
            let url = deep_link
                .parse()
                .unwrap_or_else(|_| scan_url(None, &site.claim));
            let book_scan = Some(BookScanProvenance {
                ocaid: ocaid.to_string(),
                scanned_page: None,
                cited_page: site.book.cited_page.clone(),
                note: Some("searched, no matching passage".to_string()),
            });
            BookVerdict::Finding(Box::new(book_no_model_finding(
                site,
                url,
                response_hash,
                200,
                clock,
                true,
                book_scan,
            )))
        }
        BookGroundingPreparation::NoUsableBody { detail } => {
            let book_scan = Some(BookScanProvenance {
                ocaid: ocaid.to_string(),
                scanned_page: None,
                cited_page: site.book.cited_page.clone(),
                note: Some(detail.to_string()),
            });
            BookVerdict::Finding(Box::new(book_no_model_finding(
                site,
                scan_url(None, &site.claim),
                sha256_hex(b""),
                200,
                clock,
                false,
                book_scan,
            )))
        }
        BookGroundingPreparation::Unreachable { message } => {
            let book_scan = Some(BookScanProvenance {
                ocaid: ocaid.to_string(),
                scanned_page: None,
                cited_page: site.book.cited_page.clone(),
                note: Some(message),
            });
            BookVerdict::Finding(Box::new(book_no_model_finding(
                site,
                scan_url(None, &site.claim),
                sha256_hex(b""),
                0,
                clock,
                false,
                book_scan,
            )))
        }
    }
}

/// Resolve and judge one book use-site (PRD-0009 Layers 1+2): Open Library
/// resolution always yields a [`BookResolution`] for the report; the verdict
/// side is a finding for a resolved book (grounded when an exact-edition scan
/// is searchable, honest `SourceUnavailable` otherwise) or the refined skip
/// when no catalog record was found.
async fn verify_book_use_site<C, M>(
    fetch_client: &C,
    model_client: &M,
    clock: &dyn Clock,
    panel: &[ModelRef],
    page: &PageVerificationRequest,
    site: BookUseSite,
    options: &VerifyOptions,
) -> (BookResolution, BookVerdict)
where
    C: HttpClient + ?Sized,
    M: ModelClient + ?Sized,
{
    let outcome = resolve_book_source(fetch_client, &site.book).await;
    let mut resolution = BookResolution {
        ref_id: site.ref_id.clone(),
        block_ordinal: site.block_ordinal,
        identifiers: site.book.identifiers.clone(),
        cited_page: site.book.cited_page.clone(),
        outcome,
        enrichment_candidates: Vec::new(),
    };
    if let BookResolutionOutcome::Resolved {
        edition,
        identifier,
        ..
    } = &resolution.outcome
    {
        // Read-only proposal listing (PRD-0009 Layer 3): what an operator
        // could confirm once ADR-0025's write lane is enabled. PR 148 P2: use
        // only the resolved ISBN for enrichment proposals, not all cited
        // identifiers (which haven't been verified against the resolved record).
        resolution.enrichment_candidates = crate::citation::enrich::enrichment_candidates(
            edition,
            std::slice::from_ref(identifier),
        );
    }

    let verdict = match &resolution.outcome {
        BookResolutionOutcome::Resolved {
            identifier,
            edition,
            scan,
        } => {
            let groundable_ocaid = scan
                .as_ref()
                .and_then(|availability| availability.groundable_scan())
                .and_then(|item| {
                    // Parse-time ocaid first (live 2026-07 Read API shape);
                    // itemURL derivation kept for replayed pre-drift records.
                    item.ocaid.clone().or_else(|| extract_ocaid(&item.item_url))
                });
            if let Some(ocaid) = groundable_ocaid {
                ground_resolved_book(
                    fetch_client,
                    model_client,
                    clock,
                    panel,
                    page,
                    &site,
                    edition,
                    &ocaid,
                    options,
                )
                .await
            } else {
                // Resolved, but no exact-edition scan (similar-only, none, or
                // availability unknown): grounding degrades to
                // `SourceUnavailable` (ADR-0024 Decision 3); the Books section
                // carries the scan state.
                BookVerdict::Finding(Box::new(book_no_model_finding(
                    &site,
                    book_record_url(edition, identifier),
                    sha256_hex(b""),
                    200,
                    clock,
                    false,
                    None,
                )))
            }
        }
        BookResolutionOutcome::NotFound | BookResolutionOutcome::LookupFailed { .. } => {
            BookVerdict::Skip(SkippedRef {
                ref_id: site.ref_id.clone(),
                reason: SkippedReason::BookSource,
                block_ordinal: site.block_ordinal,
                book_sources: vec![site.book.clone()],
            })
        }
    };
    (resolution, verdict)
}

/// Prefetch each distinct source URL once, into the shared body cache the
/// use-site fan-out reads — but cap the retained bytes so a citation-heavy
/// page can't OOM the server. Fetches run in concurrency-sized chunks and
/// *caching* stops once retained bodies reach `MAX_PREFETCH_CACHE_BYTES`; any
/// URL past the budget is simply not prefetched, so `verify_one_use_site`
/// lazily (re)fetches it on demand (un-deduped). Peak retained ≈ budget +
/// `page_concurrency` * source cap. A proper evict-after-last-use fix is
/// tracked in SP42#59. A transport error inserts a sentinel (empty text,
/// status 0) so no use-site re-fetches; the empty-text path routes to
/// `SourceUnavailable` via the body-usability gate.
async fn prefetch_bodies<C>(
    fetch_client: &C,
    use_sites: &[crate::citation::extract::CitationUseSite],
    page_concurrency: usize,
) -> HashMap<String, FetchedSource>
where
    C: HttpClient + ?Sized,
{
    let mut distinct: Vec<String> = Vec::new();
    for us in use_sites {
        let u = us.request.source_url.to_string();
        if !distinct.contains(&u) {
            distinct.push(u);
        }
    }
    let mut bodies: HashMap<String, FetchedSource> = HashMap::new();
    let mut retained_bytes: usize = 0;
    let mut budget_hit = false;
    for chunk in distinct.chunks(page_concurrency) {
        if retained_bytes >= MAX_PREFETCH_CACHE_BYTES {
            budget_hit = true;
            break;
        }
        let fetched = map_with_concurrency(chunk.to_vec(), page_concurrency, |url, _| async move {
            (url.clone(), fetch_source(fetch_client, &url).await)
        })
        .await;
        for (url, result) in fetched {
            let source = result.unwrap_or_else(|_| FetchedSource {
                text: String::new(),
                status: 0,
                content_type: String::new(),
                raw_html: None,
                book_snippet: false,
            });
            retained_bytes += prefetch_retained_bytes(&source);
            bodies.insert(url, source);
        }
    }
    if budget_hit {
        tracing::warn!(
            distinct_sources = distinct.len(),
            cached_sources = bodies.len(),
            retained_bytes,
            "verify_page source-body cache hit its byte budget; remaining sources will be \
             re-fetched per use-site (un-deduped) to bound memory"
        );
    }
    bodies
}

/// Verify every use-site in `extract` against its source. Fetches each distinct
/// source URL once (shared via the prefetched option), fans the existing
/// per-use-site verifier with bounded concurrency, and assembles a read-only
/// report. A per-use-site error becomes an extraction-failure entry, never a
/// top-level error.
///
/// Concurrency is two-level and multiplicative: `page_concurrency` bounds how
/// many use-sites (and distinct source fetches) are in flight at once, while
/// `options.concurrency` bounds the model panel *within* each use-site. So the
/// peak number of concurrent model calls is roughly
/// `page_concurrency * options.concurrency` — size the two against the model
/// endpoint's rate limit (e.g. 8 use-sites x a 3-model panel = 24 in flight).
// `page_concurrency` is page-orchestrator-specific and deliberately separate from
// the per-use-site `VerifyOptions` (where it would be a meaningless field on the
// single-claim path), so it stays a parameter — same trade-off as
// `verify_citation_use_site`.
#[allow(clippy::too_many_arguments)]
pub async fn verify_page<C, M>(
    fetch_client: &C,
    model_client: &M,
    clock: &dyn Clock,
    panel: &[ModelRef],
    page: &PageVerificationRequest,
    extract: ExtractOutcome,
    options: VerifyOptions,
    page_concurrency: usize,
) -> PageVerificationReport
where
    C: HttpClient + ?Sized,
    M: ModelClient + ?Sized,
{
    let ExtractOutcome {
        use_sites,
        book_use_sites,
        skipped,
        failures,
    } = extract;
    let mut skipped = skipped;
    // refs_seen = every ref we encountered: those that became use-sites (count by distinct
    // ref_id across URL and book use-sites, since extract emits one use-site per source),
    // those skipped (non-URL, no identifier), and those that failed extraction.
    let distinct_use_site_refs: HashSet<&str> = use_sites
        .iter()
        .map(|u| u.ref_id.as_str())
        .chain(book_use_sites.iter().map(|b| b.ref_id.as_str()))
        .collect();
    // A partially-resolved bundled ref appears BOTH as a use-site and as an
    // unresolved-short-cite skip (the disclosure); count the physical ref
    // once (Codex round 11, PR 153).
    let skipped_only = skipped
        .iter()
        .filter(|skip| !distinct_use_site_refs.contains(skip.ref_id.as_str()))
        .count();
    let refs_seen = distinct_use_site_refs.len() + skipped_only + failures.len();

    // Pre-bind shared refs OUTSIDE the closures so the spawned futures capture
    // plain `&`/`&dyn` (Copy) references, not re-borrows of locals — mirrors the
    // panel fan-out in verify.rs and avoids fighting the borrow checker.
    let fetch_client: &C = fetch_client;
    let model_client: &M = model_client;
    let clock: &dyn Clock = clock;
    let panel: &[ModelRef] = panel;

    // 1. Prefetch each distinct source URL once, into a shared body cache.
    // Page-level concurrency: distinct fetches in flight. The per-use-site panel
    // concurrency stays `options.concurrency` (applied inside verify_one_use_site).
    let page_concurrency = page_concurrency.max(1);
    let bodies = prefetch_bodies(fetch_client, &use_sites, page_concurrency).await;

    // 2. Fan verify over use-sites, sharing the prefetched body.
    // Every distinct URL is now in `bodies` (including sentinel entries for failed fetches),
    // so every use-site finds a prefetched body and never re-fetches.
    // Archive URLs are consulted on-demand only if the primary URL returns SourceUnavailable.
    let mut extraction_failures = failures;
    let mut findings = Vec::new();
    let results = map_with_concurrency(use_sites, page_concurrency, |us, _| {
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

    // 3. Book citations (PRD-0009 Layers 1+2): resolve each against Open
    // Library, ground resolved books in their exact-edition scan's
    // search-inside snippets, and refine the skip for unresolved ones.
    // Read-only against two third-party hosts; concurrency stays low out of
    // REST politeness (ADR-0015 sizes third-party REST fan-out at <= 3).
    let page_ref = page;
    let book_options = &options;
    let book_results =
        map_with_concurrency(book_use_sites, page_concurrency.clamp(1, 3), |site, _| {
            verify_book_use_site(
                fetch_client,
                model_client,
                clock,
                panel,
                page_ref,
                site,
                book_options,
            )
        })
        .await;
    let mut book_resolutions = Vec::new();
    for (resolution, verdict) in book_results {
        book_resolutions.push(resolution);
        match verdict {
            BookVerdict::Finding(finding) => findings.push(*finding),
            BookVerdict::Skip(skip) => skipped.push(skip),
            BookVerdict::Failure(failure) => extraction_failures.push(failure),
        }
    }
    // Book findings were appended after the URL lane; restore document order
    // (the MCP wrapper documents `findings` as document-ordered, and book and
    // URL use-sites share one ordinal sequence). Stable sort keeps each
    // lane's internal order (Codex P2, PR 147).
    findings.sort_by_key(|finding| finding.use_site_ordinal);

    // 4. Stats.
    let stats = tally_stats(
        refs_seen,
        &findings,
        skipped.len(),
        extraction_failures.len(),
        &book_resolutions,
    );

    PageVerificationReport {
        wiki_id: page.wiki_id.clone(),
        rev_id: page.rev_id,
        title: page.title.clone(),
        findings,
        skipped,
        extraction_failures,
        book_resolutions,
        stats,
    }
}

#[cfg(test)]
mod orchestrator_tests {
    use super::*;
    use crate::citation::body_classifier::BodyUsabilityReason;
    use crate::citation::extract::extract_use_sites;
    use crate::citation::verdict::CitationFindingKind;
    use crate::citation::verify::{GroundingAssertion, GroundingStatus, SourceProvenance};
    use crate::citation::voting::PanelAgreement;
    use crate::wikitext_editor::{BlockKind, BlockRef, ParsoidBlock};
    use sp42_types::{FixedClock, HttpResponse, StubHttpClient, StubModelClient};

    /// A minimal finding fixture: only `ref_id`, `use_site_ordinal`, and `verdict` matter for the
    /// re-group logic; the rest carry inert defaults.
    fn mk_finding(ref_id: &str, ordinal: u32, verdict: CitationVerdict) -> CitationFinding {
        CitationFinding {
            kind: CitationFindingKind::CitationVerdict,
            verdict,
            grounding_status: GroundingStatus::Located,
            source_unavailable_reason: None,
            unusable_reason: None,
            agreement: PanelAgreement::new(3, 3),
            passage: None,
            provenance: SourceProvenance {
                url: url::Url::parse("https://s.test/x").unwrap(),
                content_hash: "h".into(),
                fetched_at: 0,
                http_status: Some(200),
            },
            source_excerpt: None,
            metadata: None,
            grounding: GroundingAssertion::SourceFetched {
                source_hash: "h".into(),
            },
            use_site_ordinal: ordinal,
            ref_id: ref_id.into(),
            claim: "A claim.".into(),
            preceding_context: Vec::new(),
            archive_of: None,
            is_bare_url_ref: false,
            book_scan: None,
            schema_version: crate::citation::verify::SCHEMA_VERSION,
        }
    }

    #[test]
    fn review_finding_markers_project_the_report_onto_review_anchors() {
        let judged = |level| CitationVerdict::Judged(level);
        let mut unlocated = mk_finding("r_unlocated", 1, judged(SupportLevel::Supported));
        unlocated.grounding_status = GroundingStatus::Unlocated;
        let mut dead = mk_finding("r_dead", 2, CitationVerdict::SourceUnavailable);
        dead.source_unavailable_reason = Some(SourceUnavailableReason::Unreachable);
        let mut long_claim = mk_finding("r_long", 3, judged(SupportLevel::Partial));
        long_claim.claim = "x".repeat(500);
        // The standalone single-claim path has no page ref: nothing to anchor to.
        let standalone = mk_finding("", 4, judged(SupportLevel::NotSupported));
        let report = PageVerificationReport {
            wiki_id: "enwiki".into(),
            title: "Cats".into(),
            rev_id: 7,
            findings: vec![
                mk_finding("r_ok", 0, judged(SupportLevel::Supported)),
                unlocated,
                dead,
                long_claim,
                standalone,
            ],
            skipped: Vec::new(),
            extraction_failures: Vec::new(),
            book_resolutions: Vec::new(),
            stats: PageVerificationStats::default(),
        };

        let markers = super::review_finding_markers(&report);

        assert_eq!(markers.len(), 4, "ref-less standalone finding is skipped");
        assert_eq!(markers[0].ref_id, "r_ok");
        assert_eq!(markers[0].verdict, "supported");
        assert_eq!(markers[0].detail, None);
        assert_eq!(
            markers[1].detail.as_deref(),
            Some("support quote not located in source (unverified)")
        );
        assert_eq!(markers[2].verdict, "source_unavailable");
        assert_eq!(markers[2].detail.as_deref(), Some("source unreachable"));
        assert!(
            markers[3].claim.chars().count() <= 200,
            "claims are truncated to the outline display limit"
        );
    }

    #[test]
    fn apply_reverified_finding_moves_verdict_and_updates_tallies() {
        let judged = |level| CitationVerdict::Judged(level);
        let mut report = PageVerificationReport {
            wiki_id: "enwiki".into(),
            title: "Cats".into(),
            rev_id: 1,
            findings: vec![
                mk_finding("r1", 0, judged(SupportLevel::NotSupported)),
                mk_finding("r2", 1, judged(SupportLevel::Supported)),
            ],
            skipped: Vec::new(),
            extraction_failures: Vec::new(),
            book_resolutions: Vec::new(),
            stats: tally_stats(
                2,
                &[
                    mk_finding("r1", 0, judged(SupportLevel::NotSupported)),
                    mk_finding("r2", 1, judged(SupportLevel::Supported)),
                ],
                0,
                0,
                &[],
            ),
        };
        assert_eq!(report.stats.not_supported, 1);
        assert_eq!(report.stats.supported, 1);

        // r1 re-verifies from NotSupported to Supported.
        let replaced = apply_reverified_finding(
            &mut report,
            "r1",
            0,
            mk_finding("r1", 0, judged(SupportLevel::Supported)),
        );
        assert!(replaced);
        // The finding now reads Supported, and the tallies moved with it.
        let r1 = report.findings.iter().find(|f| f.ref_id == "r1").unwrap();
        assert_eq!(r1.verdict, judged(SupportLevel::Supported));
        assert_eq!(report.stats.not_supported, 0);
        assert_eq!(report.stats.supported, 2);
        assert_eq!(
            report.stats.refs_seen, 2,
            "refs_seen is preserved across a re-verify"
        );
    }

    #[test]
    fn apply_reverified_finding_is_a_noop_when_the_use_site_is_gone() {
        let mut report = PageVerificationReport {
            wiki_id: "enwiki".into(),
            title: "Cats".into(),
            rev_id: 1,
            findings: vec![mk_finding(
                "r1",
                0,
                CitationVerdict::Judged(SupportLevel::Partial),
            )],
            skipped: Vec::new(),
            extraction_failures: Vec::new(),
            book_resolutions: Vec::new(),
            stats: PageVerificationStats::default(),
        };
        let replaced = apply_reverified_finding(
            &mut report,
            "r1",
            9, // wrong ordinal
            mk_finding("r1", 9, CitationVerdict::Judged(SupportLevel::Supported)),
        );
        assert!(!replaced);
        assert_eq!(
            report.findings[0].verdict,
            CitationVerdict::Judged(SupportLevel::Partial),
            "an unmatched re-verify leaves the report untouched"
        );
    }

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
            refs: vec![
                BlockRef {
                    offset: 10,
                    ref_id: "r1".into(),
                    sources: vec![crate::wikitext_editor::CitedSource {
                        url: url::Url::parse("https://s.test/x").unwrap(),
                        archive_urls: vec![],
                    }],
                    book_sources: vec![],
                    ref_text: "[1]".into(),
                    named: false,
                    is_bare_url_ref: false,
                    short_cite_unresolved: false,
                },
                BlockRef {
                    offset: 22,
                    ref_id: "r2".into(),
                    sources: vec![crate::wikitext_editor::CitedSource {
                        url: url::Url::parse("https://s.test/x").unwrap(),
                        archive_urls: vec![],
                    }],
                    book_sources: vec![],
                    ref_text: "[2]".into(),
                    named: false,
                    is_bare_url_ref: false,
                    short_cite_unresolved: false,
                },
            ],
            block_kind: BlockKind::Paragraph,
            block_ordinal: 0,
        };
        extract_use_sites(&[b], &page())
    }

    // A page where an EARLIER citation precedes a two-source ref `rX`, so `rX`'s two use-sites
    // carry *shifted* page-global ordinals (1 and 2, not 0 and 1) — the exact drift a re-verify
    // against a later revision hits when a citation was inserted ahead of the card's ref.
    fn shifted_page_use_sites() -> Vec<CitationUseSite> {
        let source = |u: &str| crate::wikitext_editor::CitedSource {
            url: url::Url::parse(u).unwrap(),
            archive_urls: vec![],
        };
        let b = ParsoidBlock {
            text: "Intro. Cats purr.".into(),
            refs: vec![
                BlockRef {
                    offset: 6,
                    ref_id: "early".into(),
                    sources: vec![source("https://s.test/early")],
                    book_sources: vec![],
                    ref_text: "[1]".into(),
                    named: false,
                    is_bare_url_ref: false,
                    short_cite_unresolved: false,
                },
                BlockRef {
                    offset: 17,
                    ref_id: "rX".into(),
                    sources: vec![source("https://s.test/a"), source("https://s.test/b")],
                    book_sources: vec![],
                    ref_text: "[2]".into(),
                    named: false,
                    is_bare_url_ref: false,
                    short_cite_unresolved: false,
                },
            ],
            block_kind: BlockKind::Paragraph,
            block_ordinal: 0,
        };
        extract_use_sites(&[b], &page()).use_sites
    }

    #[test]
    fn reverify_matches_stable_source_url_across_ordinal_shift() {
        // `rX`'s /b source is at page-global ordinal 2 here; the operator's card recorded ordinal 1
        // from the old report (before the earlier citation was inserted). `source_url` must win so
        // re-verify targets /b, not the /a source now sitting at ordinal 1.
        let picked = select_reverify_use_site(
            shifted_page_use_sites(),
            "rX",
            Some("https://s.test/b"),
            Some(1),
        )
        .expect("the /b use-site is found by (ref_id, source_url)");
        assert_eq!(picked.ref_id, "rX");
        assert_eq!(picked.request.source_url.as_str(), "https://s.test/b");
    }

    #[test]
    fn reverify_ordinal_is_only_a_fallback_when_no_source_url() {
        // With no source_url, ordinal 1 (page-global) is rX's /a source (early ref took ordinal 0).
        let picked = select_reverify_use_site(shifted_page_use_sites(), "rX", None, Some(1))
            .expect("ordinal 1 matches");
        assert_eq!(picked.request.source_url.as_str(), "https://s.test/a");
    }

    #[test]
    fn reverify_first_ref_match_when_neither_given() {
        let picked = select_reverify_use_site(shifted_page_use_sites(), "rX", None, None)
            .expect("first rX use-site");
        assert_eq!(picked.request.source_url.as_str(), "https://s.test/a");
    }

    #[test]
    fn reverify_absent_source_url_is_not_found() {
        assert!(
            select_reverify_use_site(
                shifted_page_use_sites(),
                "rX",
                Some("https://s.test/gone"),
                Some(1)
            )
            .is_none()
        );
    }

    fn book_block(cited_page: Option<&str>) -> ParsoidBlock {
        use crate::wikitext_editor::{BookIdentifier, BookSource};
        ParsoidBlock {
            text: "Matilda longed for her parents to be good and loving.".into(),
            refs: vec![BlockRef {
                offset: 54,
                ref_id: "cite_book".into(),
                sources: vec![],
                book_sources: vec![BookSource {
                    identifiers: vec![
                        BookIdentifier::isbn("978-0-14-032872-1").expect("valid isbn"),
                    ],
                    cited_page: cited_page.map(ToString::to_string),
                }],
                ref_text: "[1]".into(),
                named: false,
                is_bare_url_ref: false,
                short_cite_unresolved: false,
            }],
            block_kind: BlockKind::Paragraph,
            block_ordinal: 0,
        }
    }

    fn response(body: &str) -> HttpResponse {
        HttpResponse {
            status: 200,
            headers: std::collections::BTreeMap::new(),
            body: body.as_bytes().to_vec(),
        }
    }

    #[test]
    fn book_ref_is_resolved_grounded_and_judged() {
        use crate::citation::openlibrary::BookResolutionOutcome;
        use futures::executor::block_on;

        // One url-less book ref, no URL use-sites. The whole chain over the
        // stub, in order: catalog lookup → Read API (exact scan) → item
        // metadata → search-inside → model panel over the snippet body.
        let extract = extract_use_sites(&[book_block(Some("42"))], &page());
        assert!(extract.skipped.is_empty());
        assert_eq!(extract.book_use_sites.len(), 1);

        let http = StubHttpClient::new([
            Ok(response(
                r#"{"ISBN:9780140328721": {"title": "Matilda", "url": "https://openlibrary.org/books/OL7826547M/Matilda", "authors": [{"name": "Roald Dahl"}]}}"#,
            )),
            Ok(response(
                r#"{"items": [{"match": "exact", "status": "full access", "itemURL": "https://archive.org/details/matilda00dahl"}]}"#,
            )),
            Ok(response(
                r#"{"server": "ia800300.us.archive.org", "dir": "/12/items/matilda00dahl", "metadata": {"mediatype": "texts"}}"#,
            )),
            Ok(response(
                r#"{"indexed": true, "matches": [{"text": "Matilda longed for her {{{parents}}} to be good and loving.", "par": [{"page": 42}]}]}"#,
            )),
        ]);
        // Panel size 1, repair off: exactly one model call over the snippet.
        let model = StubModelClient::new([Ok(completion(
            r#"{"verdict":"SUPPORTED","quote":"good and loving"}"#,
        ))]);
        let options = VerifyOptions {
            repair_turn: false,
            concurrency: 1,
            ..VerifyOptions::default()
        };
        let report = block_on(verify_page(
            &http,
            &model,
            &FixedClock::new(0),
            &[model_ref()],
            &page(),
            extract,
            options,
            1,
        ));

        // The book ref produced a real grounded finding, not a skip.
        assert!(report.skipped.is_empty());
        assert_eq!(report.findings.len(), 1);
        let finding = &report.findings[0];
        assert_eq!(finding.ref_id, "cite_book");
        assert_eq!(
            finding.verdict,
            crate::CitationVerdict::Judged(crate::SupportLevel::Supported)
        );
        assert_eq!(finding.grounding_status, crate::GroundingStatus::Located);
        let scan = finding.book_scan.as_ref().expect("book-scan provenance");
        assert_eq!(scan.ocaid, "matilda00dahl");
        assert_eq!(scan.scanned_page, Some(42));
        assert_eq!(scan.cited_page.as_deref(), Some("42"));
        assert!(
            finding
                .provenance
                .url
                .as_str()
                .starts_with("https://archive.org/details/matilda00dahl/page/42"),
            "deep link should anchor the scanned page: {}",
            finding.provenance.url
        );

        // The resolution record still feeds the Books section.
        assert_eq!(report.book_resolutions.len(), 1);
        let BookResolutionOutcome::Resolved { edition, .. } = &report.book_resolutions[0].outcome
        else {
            panic!("expected Resolved");
        };
        assert_eq!(edition.title.as_deref(), Some("Matilda"));
        assert_eq!(report.stats.books_resolved, 1);
        assert_eq!(report.stats.supported, 1);
        assert_eq!(report.stats.skipped, 0);
        assert_eq!(report.stats.refs_seen, 1);
    }

    #[test]
    fn indexed_scan_with_no_snippets_is_not_supported_never_unavailable() {
        use futures::executor::block_on;

        let extract = extract_use_sites(&[book_block(None)], &page());
        let http = StubHttpClient::new([
            Ok(response(r#"{"ISBN:9780140328721": {"title": "Matilda"}}"#)),
            Ok(response(
                r#"{"items": [{"match": "exact", "status": "full access", "itemURL": "https://archive.org/details/matilda00dahl"}]}"#,
            )),
            Ok(response(
                r#"{"server": "ia800300.us.archive.org", "dir": "/12/items/matilda00dahl", "metadata": {"mediatype": "texts"}}"#,
            )),
            // Every rung of the query ladder comes back empty — only then is
            // "the scan does not contain this claim" a safe conclusion.
            Ok(response(r#"{"indexed": true, "matches": []}"#)),
            Ok(response(r#"{"indexed": true, "matches": []}"#)),
            Ok(response(r#"{"indexed": true, "matches": []}"#)),
        ]);
        // Zero snippets → deterministic not_supported, no model call.
        let model = StubModelClient::new([]);
        let report = block_on(verify_page(
            &http,
            &model,
            &FixedClock::new(0),
            &[model_ref()],
            &page(),
            extract,
            VerifyOptions::default(),
            1,
        ));
        assert_eq!(report.findings.len(), 1);
        let finding = &report.findings[0];
        assert_eq!(
            finding.verdict,
            crate::CitationVerdict::Judged(crate::SupportLevel::NotSupported)
        );
        assert_eq!(finding.source_unavailable_reason, None);
        assert_eq!(
            finding
                .book_scan
                .as_ref()
                .expect("book scan provenance")
                .note
                .as_deref(),
            Some("searched, no matching passage")
        );
        assert_eq!(report.stats.not_supported, 1);
        assert_eq!(report.stats.source_unavailable, 0);
    }

    #[test]
    fn similar_only_scan_degrades_to_source_unavailable() {
        use futures::executor::block_on;

        let extract = extract_use_sites(&[book_block(None)], &page());
        let http = StubHttpClient::new([
            Ok(response(
                r#"{"ISBN:9780140328721": {"title": "Matilda", "url": "https://openlibrary.org/books/OL7826547M/Matilda"}}"#,
            )),
            Ok(response(
                r#"{"items": [{"match": "similar", "status": "lendable", "itemURL": "https://archive.org/details/other-edition"}]}"#,
            )),
        ]);
        let model = StubModelClient::new([]);
        let report = block_on(verify_page(
            &http,
            &model,
            &FixedClock::new(0),
            &[model_ref()],
            &page(),
            extract,
            VerifyOptions::default(),
            1,
        ));
        // Never verified against a different edition: honest SourceUnavailable,
        // pointing the operator at the resolved record.
        assert_eq!(report.findings.len(), 1);
        let finding = &report.findings[0];
        assert_eq!(finding.verdict, crate::CitationVerdict::SourceUnavailable);
        assert_eq!(
            finding.source_unavailable_reason,
            Some(crate::SourceUnavailableReason::Unusable)
        );
        assert_eq!(
            finding.provenance.url.as_str(),
            "https://openlibrary.org/books/OL7826547M/Matilda"
        );
        assert_eq!(report.stats.source_unavailable_unusable, 1);
        assert_eq!(report.stats.books_resolved, 1);
    }

    #[test]
    fn book_lookup_failure_never_fails_the_page() {
        use crate::citation::openlibrary::BookResolutionOutcome;
        use futures::executor::block_on;
        use sp42_types::HttpClientError;

        let extract = extract_use_sites(&[book_block(None)], &page());
        let http = StubHttpClient::new([Err(HttpClientError::Transport {
            message: "openlibrary unreachable".to_string(),
        })]);
        let model = StubModelClient::new([]);
        let report = block_on(verify_page(
            &http,
            &model,
            &FixedClock::new(0),
            &[model_ref()],
            &page(),
            extract,
            VerifyOptions::default(),
            1,
        ));
        assert_eq!(report.book_resolutions.len(), 1);
        assert!(matches!(
            report.book_resolutions[0].outcome,
            BookResolutionOutcome::LookupFailed { .. }
        ));
        assert_eq!(report.stats.book_lookups_failed, 1);
        // The failure stays inside the books lane: no extraction failure, no
        // finding, and the ref lands in skipped with the refined reason.
        assert!(report.extraction_failures.is_empty());
        assert!(report.findings.is_empty());
        assert_eq!(report.skipped.len(), 1);
        assert_eq!(
            report.skipped[0].reason,
            crate::citation::extract::SkippedReason::BookSource
        );
        assert_eq!(report.stats.skipped, 1);
    }

    #[test]
    fn partially_resolved_ref_counts_once_in_refs_seen() {
        use futures::executor::block_on;
        // Codex round 11 (PR 153): a ref that is BOTH a URL use-site and an
        // unresolved-short-cite skip (partially-resolved bundled ref) is one
        // physical ref in refs_seen, while the disclosure stays in skipped.
        let b = ParsoidBlock {
            text: "Cats purr.".into(),
            refs: vec![BlockRef {
                offset: 10,
                ref_id: "r1".into(),
                sources: vec![crate::wikitext_editor::CitedSource {
                    url: url::Url::parse("https://s.test/x").unwrap(),
                    archive_urls: vec![],
                }],
                book_sources: vec![],
                ref_text: "[1]".into(),
                named: false,
                is_bare_url_ref: false,
                short_cite_unresolved: true,
            }],
            block_kind: BlockKind::Paragraph,
            block_ordinal: 0,
        };
        let extract = extract_use_sites(&[b], &page());
        let http = StubHttpClient::new([Ok(HttpResponse {
            status: 200,
            headers: std::collections::BTreeMap::new(),
            body: b"cats purr and sleep".to_vec(),
        })]);
        let model = StubModelClient::new([Ok(completion(
            "{\"verdict\": \"supported\", \"quote\": \"cats purr\"}",
        ))]);
        let report = block_on(verify_page(
            &http,
            &model,
            &FixedClock::new(0),
            &[model_ref()],
            &page(),
            extract,
            VerifyOptions::default(),
            1,
        ));
        assert_eq!(report.skipped.len(), 1, "disclosure survives");
        assert_eq!(
            report.skipped[0].reason,
            crate::citation::extract::SkippedReason::UnresolvedShortCite
        );
        assert_eq!(report.stats.refs_seen, 1, "one physical ref");
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
            1,
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
            1,
        ));
        // Both use-sites should produce SourceUnavailable findings (not errors),
        // routed through the empty-text/status-0 sentinel → body-usability gate.
        assert_eq!(report.findings.len(), 2);
        assert_eq!(report.stats.use_sites_verified, 2);
        assert_eq!(report.stats.source_unavailable, 2);
        // A transport failure → status-0 sentinel → Unreachable, so the summary
        // split must attribute both to unreachable, none to unusable.
        assert_eq!(report.stats.source_unavailable_unreachable, 2);
        assert_eq!(report.stats.source_unavailable_unusable, 0);
        // No model invocations occurred, so no extraction failures from the model side.
        assert!(report.extraction_failures.is_empty());
        // Verify both findings are SourceUnavailable and addressable back to their ref,
        // each carrying the claim sentence it judged.
        let by_ref: HashMap<&str, &str> = report
            .findings
            .iter()
            .map(|f| (f.ref_id.as_str(), f.claim.as_str()))
            .collect();
        assert_eq!(
            by_ref,
            HashMap::from([("r1", "Cats purr."), ("r2", "Cats sleep.")]),
            "each finding should carry its ref_id and the claim sentence it judged"
        );
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
                book_sources: vec![],
                ref_text: "[1]".into(),
                named: false,
                is_bare_url_ref: false,
                short_cite_unresolved: false,
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
            1,
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
            refs: vec![BlockRef {
                offset: 5,
                ref_id: "r1".into(),
                sources: vec![crate::wikitext_editor::CitedSource {
                    url: primary.clone(),
                    archive_urls: vec![archive.clone()],
                }],
                book_sources: vec![],
                ref_text: "[1]".into(),
                named: false,
                is_bare_url_ref: false,
                short_cite_unresolved: false,
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
            1,
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
        assert_eq!(
            finding.archive_of.as_ref(),
            Some(&primary),
            "finding should record the unreachable live URL the archive stands in for"
        );
        assert_eq!(
            finding.claim, "Test claim here.",
            "finding should echo the claim it judged so the report is self-contained"
        );
        assert_eq!(report.stats.use_sites_verified, 1);
        assert!(report.extraction_failures.is_empty());
    }

    #[test]
    fn archive_not_consulted_for_unusable_primary() {
        use futures::executor::block_on;

        // Primary returns 2xx but an unusably short body → SourceUnavailable (Unusable).
        // The archive must NOT be consulted (Unusable is a tool limitation, not a dead link),
        // so the finding keeps `archive_of == None` and is not stamped for repair.
        let primary = url::Url::parse("https://s.test/primary").unwrap();
        let archive = url::Url::parse("https://archive.org/web/20240101/s.test/primary").unwrap();
        let b = ParsoidBlock {
            text: "Test claim here.".into(),
            refs: vec![BlockRef {
                offset: 5,
                ref_id: "r1".into(),
                sources: vec![crate::wikitext_editor::CitedSource {
                    url: primary.clone(),
                    archive_urls: vec![archive.clone()],
                }],
                book_sources: vec![],
                ref_text: "[1]".into(),
                named: false,
                is_bare_url_ref: false,
                short_cite_unresolved: false,
            }],
            block_kind: BlockKind::Paragraph,
            block_ordinal: 0,
        };
        let extract = extract_use_sites(&[b], &page());

        // EXACTLY ONE HTTP response (primary, 200 but too short to be usable). If the archive
        // were fetched the queue would drain — proving the fallback did not run.
        let http = StubHttpClient::new([Ok(HttpResponse {
            status: 200,
            headers: std::collections::BTreeMap::new(),
            body: b"too short".to_vec(),
        })]);
        // No model call: the body-usability gate short-circuits before model invocation.
        let model = StubModelClient::new([]);
        let report = block_on(verify_page(
            &http,
            &model,
            &FixedClock::new(0),
            &[model_ref()],
            &page(),
            extract,
            VerifyOptions::default(),
            1,
        ));

        assert_eq!(report.findings.len(), 1);
        let finding = &report.findings[0];
        assert_eq!(finding.verdict, super::CitationVerdict::SourceUnavailable);
        assert_eq!(
            finding.source_unavailable_reason,
            Some(super::SourceUnavailableReason::Unusable),
            "a 2xx-but-short body is Unusable, not Unreachable"
        );
        assert_eq!(
            finding.archive_of, None,
            "an Unusable primary must not be archive-repaired"
        );
        assert_eq!(report.stats.source_unavailable_unusable, 1);
    }

    #[test]
    fn archive_not_fetched_when_primary_available() {
        use futures::executor::block_on;

        // Build a use-site with primary and archive URLs.
        let primary = url::Url::parse("https://s.test/primary").unwrap();
        let archive = url::Url::parse("https://archive.org/web/20240101/s.test/primary").unwrap();
        let b = ParsoidBlock {
            text: "Test claim here.".into(),
            refs: vec![BlockRef {
                offset: 5,
                ref_id: "r1".into(),
                sources: vec![crate::wikitext_editor::CitedSource {
                    url: primary.clone(),
                    archive_urls: vec![archive.clone()],
                }],
                book_sources: vec![],
                ref_text: "[1]".into(),
                named: false,
                is_bare_url_ref: false,
                short_cite_unresolved: false,
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
            1,
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

    #[test]
    fn pdf_citation_carries_unusable_reason_through_page_report() {
        use futures::executor::block_on;

        // A single citation to a PDF. The finding should have unusable_reason == Some(PdfBody)
        // and the PageVerificationReport should serialize/deserialize with the field intact.
        let b = ParsoidBlock {
            text: "Test claim here.".into(),
            refs: vec![BlockRef {
                offset: 5,
                ref_id: "r1".into(),
                sources: vec![crate::wikitext_editor::CitedSource {
                    url: url::Url::parse("https://example.com/report.pdf").unwrap(),
                    archive_urls: vec![],
                }],
                book_sources: vec![],
                ref_text: "[1]".into(),
                named: false,
                is_bare_url_ref: false,
                short_cite_unresolved: false,
            }],
            block_kind: BlockKind::Paragraph,
            block_ordinal: 0,
        };
        let extract = extract_use_sites(&[b], &page());

        // Fetch returns 200 with PDF body.
        let http = StubHttpClient::new([Ok(HttpResponse {
            status: 200,
            headers: {
                let mut m = std::collections::BTreeMap::new();
                m.insert("content-type".to_string(), "application/pdf".to_string());
                m
            },
            body: b"%PDF-1.7 body".to_vec(),
        })]);
        // No model call: body-usability gate short-circuits before model invocation.
        let model = StubModelClient::new([]);
        let report = block_on(verify_page(
            &http,
            &model,
            &FixedClock::new(0),
            &[model_ref()],
            &page(),
            extract,
            VerifyOptions::default(),
            1,
        ));

        assert_eq!(report.findings.len(), 1);
        let finding = &report.findings[0];
        assert_eq!(finding.verdict, super::CitationVerdict::SourceUnavailable);
        assert_eq!(
            finding.unusable_reason,
            Some(BodyUsabilityReason::PdfBody),
            "PDF source should have PdfBody as unusable_reason"
        );

        // The report serializes the per-finding reason (the reviewer-facing surface).
        let json = serde_json::to_string(&report).expect("serialize report");
        let back: PageVerificationReport = serde_json::from_str(&json).expect("deserialize report");
        assert_eq!(
            back.findings[0].unusable_reason,
            Some(BodyUsabilityReason::PdfBody),
            "unusable_reason should survive the page report round-trip"
        );

        // Confirm the stat still increments (no new stats fields are added).
        assert_eq!(report.stats.source_unavailable_unusable, 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefetch_retained_bytes_counts_raw_html() {
        // The retained HTML can dwarf the extracted text on chrome-heavy pages, so
        // the page-cache budget must count it — not just `text`.
        let html_heavy = FetchedSource {
            text: "abc".to_string(),
            status: 200,
            content_type: "text/html".to_string(),
            raw_html: Some("x".repeat(100)),
            book_snippet: false,
        };
        assert_eq!(prefetch_retained_bytes(&html_heavy), 3 + 100);

        // No retained HTML → just the text length (non-HTML body, or the sentinel).
        let text_only = FetchedSource {
            text: "hello".to_string(),
            status: 200,
            content_type: String::new(),
            raw_html: None,
            book_snippet: false,
        };
        assert_eq!(prefetch_retained_bytes(&text_only), 5);
    }

    #[test]
    fn request_rev_id_defaults_to_zero_meaning_latest() {
        // An absent rev_id deserializes to 0, the "latest revision" sentinel the
        // server resolves before verifying.
        let request: PageVerificationRequest =
            serde_json::from_str(r#"{"wiki_id":"enwiki","title":"Example"}"#)
                .expect("request without rev_id should deserialize");
        assert_eq!(request.rev_id, 0);

        // An explicit rev_id is preserved.
        let pinned: PageVerificationRequest =
            serde_json::from_str(r#"{"wiki_id":"enwiki","title":"Example","rev_id":123}"#)
                .expect("request with rev_id should deserialize");
        assert_eq!(pinned.rev_id, 123);
    }

    #[test]
    fn report_round_trips_through_serde() {
        let report = PageVerificationReport {
            wiki_id: "frwiki".to_string(),
            rev_id: 42,
            title: "Exemple".to_string(),
            findings: Vec::new(),
            skipped: Vec::new(),
            extraction_failures: Vec::new(),
            book_resolutions: Vec::new(),
            stats: PageVerificationStats::default(),
        };
        let json = serde_json::to_string(&report).expect("serialize");
        let back: PageVerificationReport = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(report, back);
    }

    #[test]
    fn report_without_book_fields_still_deserializes() {
        // A report produced before the book-resolution slice (no
        // `book_resolutions`, no book stats) must keep deserializing.
        let json = r#"{
            "wiki_id": "frwiki", "rev_id": 42, "title": "Exemple",
            "findings": [], "skipped": [], "extraction_failures": [],
            "stats": {
                "refs_seen": 0, "use_sites_verified": 0, "skipped": 0,
                "extraction_failures": 0, "supported": 0, "partial": 0,
                "not_supported": 0, "source_unavailable": 0,
                "source_unavailable_unreachable": 0,
                "source_unavailable_unusable": 0
            }
        }"#;
        let report: PageVerificationReport =
            serde_json::from_str(json).expect("older report deserializes");
        assert!(report.book_resolutions.is_empty());
        assert_eq!(report.stats.books_resolved, 0);
    }
}
