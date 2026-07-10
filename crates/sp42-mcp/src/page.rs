//! `verify_wikipedia_page` — the page-level convenience verb (PRD-0010, Phases 5–6).
//!
//! Fetches a Wikipedia revision through the shared `sp42-parsoid` read path, decomposes it into
//! citation use-sites with `extract_use_sites`, and (unless `estimate_only`) runs the page
//! orchestrator over them. The verdicts ride the unchanged ADR-0007/0008 taxonomy, with the same
//! anti-fabrication guarantee as `verify_claim`: a quote is surfaced only when it grounded.
//!
//! `estimate_only` answers "how big is this job?" without spending any inference — it returns the
//! use-site count and the implied panel-call count (the fan-out width × panel size) so an agent
//! can decide before committing. A default fan-out cap (`DEFAULT_MAX_USE_SITES`, overridable)
//! bounds a single run on a citation-heavy page; when it bites, `truncated` says so.

use serde::{Deserialize, Serialize};
use sp42_citation::{
    CitationFinding, PageVerificationReport, PageVerificationRequest, VerifyOptions,
    extract_use_sites, verify_page,
};
use sp42_platform::{WikiConfig, WikitextPageRef};
use sp42_types::{Clock, HttpClient, ModelClient, ModelRef};

use crate::Verdict;

/// Default cap on use-sites verified in one page run (PRD-0010 open question #2: a sane default
/// limit with a trivial override). `estimate_only` lets a caller measure before exceeding it.
pub const DEFAULT_MAX_USE_SITES: usize = 50;

/// Per-page fan-out concurrency (mirrors `sp42-server`'s `PAGE_VERIFY_CONCURRENCY`).
const PAGE_VERIFY_CONCURRENCY: usize = 8;

/// The Wikipedia page (revision) to verify.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageInput {
    /// The canonical page title, e.g. `Cosmic latte`.
    pub title: String,
    /// The specific revision id to pin verification to.
    pub rev_id: u64,
}

/// One verified citation use-site on the page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageFinding {
    /// The claim sentence this verdict judged.
    pub claim: String,
    /// The source URL the claim was verified against (the really-fetched provenance URL).
    pub ref_url: String,
    /// The four-value support verdict (ADR-0007/0008).
    pub verdict: Verdict,
    /// The supporting quote, re-located verbatim; `None` when none grounded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote: Option<String>,
    /// Document-order position of this use-site on the page.
    pub use_site_ordinal: u32,
    /// The originating ref's marker id, addressable back to the page citation.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ref_id: String,
    /// Set when this verdict was produced against an **archive** fallback because the citation's
    /// live URL was `SourceUnavailable`: the (unreachable) live URL the archive stands in for.
    /// `ref_url` then points at the archive that was actually read, so without this an agent would
    /// see a plain supported finding and miss that the page's live citation is dead and needs
    /// repair. `None` for a verdict from the primary source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_of: Option<String>,
}

/// Result of `verify_wikipedia_page`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageVerifyResult {
    /// Total URL-bearing use-sites found on the page (the full fan-out width).
    pub use_site_count: usize,
    /// `use_site_count` × panel size: the panel calls a full (uncapped) run would make. The cost
    /// estimate an agent screens with before committing.
    pub implied_panel_calls: usize,
    /// Refs that carried no fetchable URL (book/ISBN/offline) and were not verified.
    pub skipped_count: usize,
    /// `true` when this was an `estimate_only` run: no panel was called, `findings` is empty.
    pub estimated: bool,
    /// Use-sites actually verified — equals `use_site_count` unless the fan-out cap truncated it.
    pub verified_count: usize,
    /// `true` when the fan-out cap limited the run below `use_site_count`.
    pub truncated: bool,
    /// One finding per verified use-site, in document order. Empty for an `estimate_only` run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<PageFinding>,
}

/// Verify every URL-bearing citation on a Wikipedia revision.
///
/// `config` resolves the wiki (its `parsoid_url` and `wiki_id`); the caller is responsible for
/// having selected/overridden it. When `estimate_only`, the page is fetched and decomposed but no
/// panel runs — the result reports the counts only. `max_use_sites` overrides the default fan-out
/// cap (`DEFAULT_MAX_USE_SITES`).
///
/// # Errors
///
/// Returns the stringified fetch/parse error if the revision cannot be fetched or decomposed
/// (e.g. the wiki has no `parsoid_url`, or the revision is missing).
#[allow(clippy::too_many_arguments)] // https://github.com/schiste/SP42/pull/103 keeps MCP deps explicit.
pub async fn verify_wikipedia_page<C, M>(
    fetch_client: &C,
    model_client: &M,
    clock: &dyn Clock,
    panel: &[ModelRef],
    config: &WikiConfig,
    page: &PageInput,
    estimate_only: bool,
    max_use_sites: Option<usize>,
) -> Result<PageVerifyResult, String>
where
    C: HttpClient + ?Sized,
    M: ModelClient + ?Sized,
{
    let page_ref = WikitextPageRef {
        title: page.title.clone(),
        rev_id: page.rev_id,
    };
    let blocks = sp42_parsoid::fetch_page_blocks(config, &page_ref)
        .await
        .map_err(|error| error.to_string())?;
    let request = PageVerificationRequest {
        wiki_id: config.wiki_id.clone(),
        title: page.title.clone(),
        rev_id: page.rev_id,
    };
    Ok(verify_extracted(
        fetch_client,
        model_client,
        clock,
        panel,
        blocks,
        &request,
        estimate_only,
        max_use_sites,
    )
    .await)
}

/// The post-fetch half: extract use-sites from `blocks`, then estimate or verify. Split out so it
/// is stub-testable with hand-built blocks (no network).
#[allow(clippy::too_many_arguments)] // https://github.com/schiste/SP42/pull/103 keeps testable deps explicit.
async fn verify_extracted<C, M>(
    fetch_client: &C,
    model_client: &M,
    clock: &dyn Clock,
    panel: &[ModelRef],
    blocks: Vec<sp42_platform::ParsoidBlock>,
    request: &PageVerificationRequest,
    estimate_only: bool,
    max_use_sites: Option<usize>,
) -> PageVerifyResult
where
    C: HttpClient + ?Sized,
    M: ModelClient + ?Sized,
{
    let mut extract = extract_use_sites(&blocks, request);
    let use_site_count = extract.use_sites.len();
    let skipped_count = extract.skipped.len();
    let implied_panel_calls = use_site_count * panel.len();

    if estimate_only {
        return PageVerifyResult {
            use_site_count,
            implied_panel_calls,
            skipped_count,
            estimated: true,
            verified_count: 0,
            truncated: false,
            findings: Vec::new(),
        };
    }

    let limit = max_use_sites.unwrap_or(DEFAULT_MAX_USE_SITES);
    let truncated = use_site_count > limit;
    if truncated {
        extract.use_sites.truncate(limit);
    }

    let report: PageVerificationReport = verify_page(
        fetch_client,
        model_client,
        clock,
        panel,
        request,
        extract,
        VerifyOptions::default(),
        PAGE_VERIFY_CONCURRENCY,
    )
    .await;

    let findings: Vec<PageFinding> = report.findings.into_iter().map(to_page_finding).collect();
    let verified_count = findings.len();

    PageVerifyResult {
        use_site_count,
        implied_panel_calls,
        skipped_count,
        estimated: false,
        verified_count,
        truncated,
        findings,
    }
}

/// Map the engine's finding onto the agent-facing finding. The quote rides only when it grounded
/// (`passage` is `Some`), so an unlocated/fabricated quote is never surfaced.
fn to_page_finding(finding: CitationFinding) -> PageFinding {
    PageFinding {
        claim: finding.claim,
        ref_url: finding.provenance.url.to_string(),
        verdict: Verdict::from(finding.verdict),
        quote: finding.passage.map(|passage| passage.quote),
        use_site_ordinal: finding.use_site_ordinal,
        ref_id: finding.ref_id,
        archive_of: finding.archive_of.map(|url| url.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use sp42_citation::PageVerificationRequest;
    use sp42_platform::{BlockKind, BlockRef, CitedSource, ParsoidBlock};
    use sp42_types::{
        FixedClock, HttpResponse, ModelCompletion, ModelRef, StubHttpClient, StubModelClient,
    };

    use super::verify_extracted;
    use crate::Verdict;

    // Extracted-text body (≥300 chars) carrying the claim verbatim.
    const BODY_TEXT: &str = "The Foo Bridge is a suspension bridge spanning the River Bar in the \
        city of Bazton. Construction began in 1994 and the bridge opened in 1998 after several \
        delays caused by funding shortfalls. At completion it was the longest single-span \
        crossing in the region, carrying four lanes of traffic and a separated pedestrian and \
        cycle path along its eastern edge.";

    fn model() -> ModelRef {
        ModelRef::new("test", "test-model", "test-model")
    }

    fn completion(text: &str) -> ModelCompletion {
        ModelCompletion {
            text: text.to_owned(),
            served_model: None,
        }
    }

    fn html_body() -> Vec<u8> {
        format!("<html><body><p>{BODY_TEXT}</p></body></html>").into_bytes()
    }

    /// A one-paragraph block whose single ref points at `url`.
    fn block_with(ordinal: usize, ref_id: &str, url: &str) -> ParsoidBlock {
        let text = "The bridge opened in 1998.".to_owned();
        ParsoidBlock {
            refs: vec![BlockRef {
                offset: text.len(),
                ref_id: ref_id.to_owned(),
                sources: vec![CitedSource {
                    url: url.parse().expect("url parses"),
                    archive_urls: vec![],
                }],
                ref_text: "Example".to_owned(),
                named: false,
                is_bare_url_ref: false,
            }],
            text,
            block_kind: BlockKind::Paragraph,
            block_ordinal: ordinal,
        }
    }

    /// Like `block_with`, but the ref also carries an archive fallback URL, so an unreachable
    /// live `url` can be re-verified against `archive_url`.
    fn block_with_archive(
        ordinal: usize,
        ref_id: &str,
        url: &str,
        archive_url: &str,
    ) -> ParsoidBlock {
        let text = "The bridge opened in 1998.".to_owned();
        ParsoidBlock {
            refs: vec![BlockRef {
                offset: text.len(),
                ref_id: ref_id.to_owned(),
                sources: vec![CitedSource {
                    url: url.parse().expect("url parses"),
                    archive_urls: vec![archive_url.parse().expect("archive url parses")],
                }],
                ref_text: "Example".to_owned(),
                named: false,
            }],
            text,
            block_kind: BlockKind::Paragraph,
            block_ordinal: ordinal,
        }
    }

    fn page_request() -> PageVerificationRequest {
        PageVerificationRequest {
            wiki_id: "testwiki".to_owned(),
            title: "Bridge".to_owned(),
            rev_id: 1,
        }
    }

    #[tokio::test]
    async fn estimate_only_counts_without_calling_the_panel() {
        // Empty stub queues: any fetch or model call would error.
        let fetch = StubHttpClient::new([]);
        let models = StubModelClient::new([]);
        let result = verify_extracted(
            &fetch,
            &models,
            &FixedClock::new(0),
            &[model(), model()],
            vec![block_with(0, "cite_ref-1", "https://example.com/a")],
            &page_request(),
            true,
            None,
        )
        .await;
        assert_eq!(result.use_site_count, 1);
        // 1 use-site × 2-model panel.
        assert_eq!(result.implied_panel_calls, 2);
        assert!(result.estimated);
        assert_eq!(result.verified_count, 0);
        assert!(!result.truncated);
        assert!(result.findings.is_empty());
    }

    #[tokio::test]
    async fn full_run_verifies_use_site_with_grounded_quote() {
        let fetch = StubHttpClient::new([Ok(HttpResponse {
            status: 200,
            headers: BTreeMap::from([("content-type".to_owned(), "text/html".to_owned())]),
            body: html_body(),
        })]);
        let models = StubModelClient::new([Ok(completion(
            r#"{"verdict": "SUPPORTED", "quote": "the bridge opened in 1998"}"#,
        ))]);
        let result = verify_extracted(
            &fetch,
            &models,
            &FixedClock::new(0),
            &[model()],
            vec![block_with(0, "cite_ref-1", "https://example.com/bridge")],
            &page_request(),
            false,
            None,
        )
        .await;
        assert_eq!(result.use_site_count, 1);
        assert_eq!(result.verified_count, 1);
        assert!(!result.estimated);
        assert!(!result.truncated);
        let finding = &result.findings[0];
        assert_eq!(finding.verdict, Verdict::Supported);
        assert_eq!(finding.ref_url, "https://example.com/bridge");
        assert_eq!(finding.quote.as_deref(), Some("the bridge opened in 1998"));
        assert_eq!(finding.ref_id, "cite_ref-1");
        // A primary-source verdict carries no archive provenance.
        assert_eq!(finding.archive_of, None);
    }

    #[tokio::test]
    async fn archive_fallback_surfaces_dead_live_url_in_finding() {
        // Live URL is unreachable (404), so verification falls back to the archive. The archive is
        // what was read (`ref_url`), but the finding must still name the dead live URL in
        // `archive_of` so an agent knows the page citation needs repair.
        let fetch = StubHttpClient::new([
            Ok(HttpResponse {
                status: 404,
                headers: BTreeMap::from([("content-type".to_owned(), "text/html".to_owned())]),
                body: Vec::new(),
            }),
            Ok(HttpResponse {
                status: 200,
                headers: BTreeMap::from([("content-type".to_owned(), "text/html".to_owned())]),
                body: html_body(),
            }),
        ]);
        let models = StubModelClient::new([Ok(completion(
            r#"{"verdict": "SUPPORTED", "quote": "the bridge opened in 1998"}"#,
        ))]);
        let result = verify_extracted(
            &fetch,
            &models,
            &FixedClock::new(0),
            &[model()],
            vec![block_with_archive(
                0,
                "cite_ref-1",
                "https://example.com/dead",
                "https://web.archive.org/web/2020/https://example.com/dead",
            )],
            &page_request(),
            false,
            None,
        )
        .await;
        let finding = &result.findings[0];
        assert_eq!(finding.verdict, Verdict::Supported);
        assert_eq!(
            finding.ref_url,
            "https://web.archive.org/web/2020/https://example.com/dead"
        );
        assert_eq!(
            finding.archive_of.as_deref(),
            Some("https://example.com/dead")
        );
    }

    #[tokio::test]
    async fn fan_out_cap_truncates_and_flags() {
        // Two use-sites, cap of 1: only the first is verified, and `truncated` says so.
        let fetch = StubHttpClient::new([Ok(HttpResponse {
            status: 200,
            headers: BTreeMap::from([("content-type".to_owned(), "text/html".to_owned())]),
            body: html_body(),
        })]);
        let models = StubModelClient::new([Ok(completion(
            r#"{"verdict": "SUPPORTED", "quote": "the bridge opened in 1998"}"#,
        ))]);
        let result = verify_extracted(
            &fetch,
            &models,
            &FixedClock::new(0),
            &[model()],
            vec![
                block_with(0, "cite_ref-1", "https://example.com/a"),
                block_with(1, "cite_ref-2", "https://example.com/b"),
            ],
            &page_request(),
            false,
            Some(1),
        )
        .await;
        assert_eq!(result.use_site_count, 2);
        assert!(result.truncated);
        assert_eq!(result.verified_count, 1);
        assert_eq!(result.findings.len(), 1);
    }
}
