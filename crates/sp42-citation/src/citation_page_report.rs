//! Pure renderer for a page-level citation verification report.
//!
//! [`crate::PageVerificationReport`] (built by the article-level verifier) is
//! data, not presentation. This module turns it into the shared
//! [`ReportDocument`], so the CLI and the app render the same reviewer view
//! through [`render_report_document_text`]/[`render_report_document_markdown`],
//! mirroring `patrol_scenario_report`. The report type lives in `sp42-core`, so
//! the transform is a free function here (no inherent method — orphan rule).

use crate::{
    BookResolution, BookResolutionOutcome, CitationFinding, CitationVerdict, GroundingStatus,
    PageVerificationReport, ScanAvailability, SourceUnavailableReason, SupportLevel,
};

use crate::report_document::{
    ReportDocument, ReportSection, render_report_document_markdown, render_report_document_text,
};

/// Longest claim prefix shown on a one-line finding summary before eliding.
const CLAIM_SUMMARY_MAX_CHARS: usize = 80;

/// Render the page report as the shared plain-text report document.
#[must_use]
pub fn render_page_verification_text(report: &PageVerificationReport) -> String {
    render_report_document_text(&page_verification_report_to_document(report))
}

/// Render the page report as the shared markdown report document.
#[must_use]
pub fn render_page_verification_markdown(report: &PageVerificationReport) -> String {
    render_report_document_markdown(&page_verification_report_to_document(report))
}

/// Transform a page verification report into the shared [`ReportDocument`].
#[must_use]
pub fn page_verification_report_to_document(report: &PageVerificationReport) -> ReportDocument {
    let stats = &report.stats;
    let lead_lines = vec![
        format!("title=\"{}\"", report.title),
        format!("wiki={}", report.wiki_id),
        format!("rev={}", report.rev_id),
        format!(
            "refs_seen={} use_sites_verified={}",
            stats.refs_seen, stats.use_sites_verified
        ),
        format!(
            "verdicts: supported={} partial={} not_supported={} source_unavailable={} (unreachable={} unusable={})",
            stats.supported,
            stats.partial,
            stats.not_supported,
            stats.source_unavailable,
            stats.source_unavailable_unreachable,
            stats.source_unavailable_unusable,
        ),
        format!(
            "skipped={} extraction_failures={}",
            stats.skipped, stats.extraction_failures
        ),
    ];
    let mut lead_lines = lead_lines;
    if !report.book_resolutions.is_empty() {
        lead_lines.push(format!(
            "books: resolved={} not_found={} lookup_failed={}",
            stats.books_resolved, stats.books_not_found, stats.book_lookups_failed
        ));
    }

    ReportDocument::new("Page citation report")
        .with_lead_lines(lead_lines)
        .with_sections(vec![
            findings_section(report),
            skipped_section(report),
            books_section(report),
            extraction_failures_section(report),
        ])
}

fn findings_section(report: &PageVerificationReport) -> ReportSection {
    if report.findings.is_empty() {
        return ReportSection {
            name: "Findings".to_string(),
            available: false,
            summary_lines: vec!["no findings".to_string()],
        };
    }

    // Document order is the reviewer's reading order; sort by use-site ordinal so
    // the render is stable regardless of fan-out completion order.
    let mut findings: Vec<&CitationFinding> = report.findings.iter().collect();
    findings.sort_by_key(|finding| finding.use_site_ordinal);

    ReportSection {
        name: "Findings".to_string(),
        available: true,
        summary_lines: findings.into_iter().map(finding_line).collect(),
    }
}

fn skipped_section(report: &PageVerificationReport) -> ReportSection {
    if report.skipped.is_empty() {
        return ReportSection {
            name: "Skipped".to_string(),
            available: false,
            summary_lines: vec!["none".to_string()],
        };
    }

    ReportSection {
        name: "Skipped".to_string(),
        available: true,
        summary_lines: report
            .skipped
            .iter()
            .map(|skipped| {
                let book_identifiers = skipped.book_identifiers();
                let identifiers = if book_identifiers.is_empty() {
                    String::new()
                } else {
                    let joined = book_identifiers
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(",");
                    format!(" identifiers={joined}")
                };
                format!(
                    "ref={} reason={:?} block={}{identifiers}",
                    skipped.ref_id, skipped.reason, skipped.block_ordinal
                )
            })
            .collect(),
    }
}

/// The Open Library resolutions for the page's book citations (PRD-0009
/// Layer 1): what each skipped book ref resolved to, or an honest miss.
fn books_section(report: &PageVerificationReport) -> ReportSection {
    if report.book_resolutions.is_empty() {
        return ReportSection {
            name: "Books".to_string(),
            available: false,
            summary_lines: vec!["none".to_string()],
        };
    }

    ReportSection {
        name: "Books".to_string(),
        available: true,
        summary_lines: report.book_resolutions.iter().map(book_line).collect(),
    }
}

fn book_line(resolution: &BookResolution) -> String {
    let identifiers = resolution
        .identifiers
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let base = format!(
        "ref={} block={}",
        resolution.ref_id, resolution.block_ordinal
    );
    match &resolution.outcome {
        BookResolutionOutcome::Resolved {
            identifier,
            edition,
            scan,
        } => {
            let title = edition
                .title
                .as_deref()
                .map(|t| format!(" title=\"{t}\""))
                .unwrap_or_default();
            let authors = if edition.authors.is_empty() {
                String::new()
            } else {
                format!(" authors=\"{}\"", edition.authors.join(", "))
            };
            let url = edition
                .record_url
                .as_deref()
                .map(|u| format!(" url={u}"))
                .unwrap_or_default();
            format!(
                "{base} resolved={identifier}{title}{authors}{url} scan={scan}",
                scan = scan_label(scan.as_ref())
            )
        }
        BookResolutionOutcome::NotFound => {
            format!("{base} not_found identifiers={identifiers}")
        }
        BookResolutionOutcome::LookupFailed { message } => {
            format!("{base} lookup_failed identifiers={identifiers} error={message}")
        }
    }
}

/// One word for the scan-availability outcome: `exact` may feed grounding,
/// `similar_only` must not (different edition), `none` is a record with no
/// usable scan, `unknown` means the availability lookup itself failed.
fn scan_label(scan: Option<&ScanAvailability>) -> &'static str {
    match scan {
        None => "unknown",
        Some(availability) if availability.groundable_scan().is_some() => "exact",
        Some(availability) if !availability.similar.is_empty() => "similar_only",
        Some(_) => "none",
    }
}

fn extraction_failures_section(report: &PageVerificationReport) -> ReportSection {
    if report.extraction_failures.is_empty() {
        return ReportSection {
            name: "Extraction failures".to_string(),
            available: false,
            summary_lines: vec!["none".to_string()],
        };
    }

    ReportSection {
        name: "Extraction failures".to_string(),
        available: true,
        summary_lines: report
            .extraction_failures
            .iter()
            .map(|failure| format!("block={} reason={}", failure.block_ordinal, failure.reason))
            .collect(),
    }
}

fn finding_line(finding: &CitationFinding) -> String {
    let ref_id = if finding.ref_id.is_empty() {
        "-"
    } else {
        finding.ref_id.as_str()
    };

    // A `SourceUnavailable` reason and the grounding axis are mutually exclusive
    // (no quote is grounded for an unavailable source); show whichever applies.
    let status = if let Some(reason) = unavailable_suffix(finding) {
        format!(" {reason}")
    } else if let Some(grounding) = format_grounding(finding.grounding_status) {
        format!(" grounding={grounding}")
    } else {
        String::new()
    };

    let archive = match &finding.archive_of {
        Some(url) => format!(" via archive of {url}"),
        None => String::new(),
    };

    format!(
        "#{ordinal} ref={ref_id} {verdict}{status} url={url}{archive} claim=\"{claim}\"",
        ordinal = finding.use_site_ordinal,
        verdict = format_verdict(finding.verdict),
        url = finding.provenance.url,
        claim = truncate_claim(&finding.claim),
    )
}

/// The decision-aid label for a verdict. Lower-snake-case so it groups cleanly
/// with the `verdicts:` tally line in the lead.
fn format_verdict(verdict: CitationVerdict) -> &'static str {
    match verdict {
        CitationVerdict::Judged(SupportLevel::Supported) => "supported",
        CitationVerdict::Judged(SupportLevel::Partial) => "partial",
        CitationVerdict::Judged(SupportLevel::NotSupported) => "not_supported",
        CitationVerdict::SourceUnavailable => "source_unavailable",
    }
}

/// The grounding axis, shown only when it carries information; `NotApplicable`
/// (no quote expected) adds noise, so it is elided.
fn format_grounding(status: GroundingStatus) -> Option<&'static str> {
    match status {
        GroundingStatus::Located => Some("located"),
        GroundingStatus::LocatedFuzzy => Some("located_fuzzy"),
        GroundingStatus::Unlocated => Some("unlocated"),
        GroundingStatus::NotApplicable => None,
    }
}

/// For a `SourceUnavailable` verdict, the parenthetical reason — `unreachable`
/// (dead link, actionable) vs `unusable` (fetched but unreadable), the latter
/// carrying the body-classifier detail (`PdfBody`, `ViewerShell`, …) when known.
/// `None` for any other verdict.
fn unavailable_suffix(finding: &CitationFinding) -> Option<String> {
    match finding.source_unavailable_reason? {
        SourceUnavailableReason::Unreachable => Some("(unreachable)".to_string()),
        SourceUnavailableReason::Unusable => Some(match finding.unusable_reason {
            Some(reason) => format!("(unusable: {reason:?})"),
            None => "(unusable)".to_string(),
        }),
    }
}

fn truncate_claim(claim: &str) -> String {
    if claim.chars().count() <= CLAIM_SUMMARY_MAX_CHARS {
        return claim.to_string();
    }
    let prefix: String = claim.chars().take(CLAIM_SUMMARY_MAX_CHARS).collect();
    format!("{prefix}…")
}

#[cfg(test)]
mod tests {
    use crate::{
        BlockFailure, BodyUsabilityReason, CitationFinding, CitationFindingKind, CitationVerdict,
        GroundingAssertion, GroundingStatus, LocatedPassage, PageVerificationReport,
        PageVerificationStats, PanelAgreement, SkippedReason, SkippedRef, SourceProvenance,
        SourceUnavailableReason, SupportLevel,
    };

    use super::{
        page_verification_report_to_document, render_page_verification_markdown,
        render_page_verification_text,
    };

    fn provenance(url: &str) -> SourceProvenance {
        SourceProvenance {
            url: url::Url::parse(url).expect("test url should parse"),
            content_hash: "deadbeef".to_string(),
            fetched_at: 42,
            http_status: Some(200),
        }
    }

    fn base_finding(ordinal: u32, ref_id: &str, url: &str, claim: &str) -> CitationFinding {
        CitationFinding {
            kind: CitationFindingKind::CitationVerdict,
            verdict: CitationVerdict::Judged(SupportLevel::Supported),
            grounding_status: GroundingStatus::Located,
            source_unavailable_reason: None,
            unusable_reason: None,
            agreement: PanelAgreement::new(2, 2),
            passage: Some(LocatedPassage {
                quote: "opened in 1850".to_string(),
                offset: 11,
            }),
            provenance: provenance(url),
            source_excerpt: None,
            metadata: None,
            grounding: GroundingAssertion::SourceFetched {
                source_hash: "deadbeef".to_string(),
            },
            use_site_ordinal: ordinal,
            ref_id: ref_id.to_string(),
            claim: claim.to_string(),
            preceding_context: Vec::new(),
            archive_of: None,
            schema_version: 1,
        }
    }

    /// A report exercising every section: findings out of order, two skipped
    /// refs (one a book), a resolved book, and an extraction failure.
    fn sample_report() -> PageVerificationReport {
        let supported = base_finding(
            0,
            "cite_1",
            "https://example.org/a",
            "The museum opened in 1850.",
        );
        let mut unreachable = base_finding(
            2,
            "cite_3",
            "https://dead.example/x",
            "Population was 5000.",
        );
        unreachable.verdict = CitationVerdict::SourceUnavailable;
        unreachable.grounding_status = GroundingStatus::NotApplicable;
        unreachable.source_unavailable_reason = Some(SourceUnavailableReason::Unreachable);

        PageVerificationReport {
            wiki_id: "enwiki".to_string(),
            rev_id: 12345,
            title: "Museum".to_string(),
            // Out of document order on purpose: render must sort by ordinal.
            findings: vec![unreachable, supported],
            skipped: vec![
                SkippedRef {
                    ref_id: "cite_book".to_string(),
                    reason: SkippedReason::NonUrlSource,
                    block_ordinal: 4,
                    book_sources: vec![],
                },
                SkippedRef {
                    ref_id: "cite_isbn".to_string(),
                    reason: SkippedReason::BookSource,
                    block_ordinal: 5,
                    book_sources: vec![crate::BookSource {
                        identifiers: vec![crate::BookIdentifier::Isbn("9780140328721".to_string())],
                        cited_page: Some("42".to_string()),
                    }],
                },
            ],
            extraction_failures: vec![BlockFailure {
                block_ordinal: 7,
                reason: "no claim sentence".to_string(),
            }],
            book_resolutions: vec![crate::BookResolution {
                ref_id: "cite_isbn".to_string(),
                block_ordinal: 5,
                identifiers: vec![crate::BookIdentifier::Isbn("9780140328721".to_string())],
                cited_page: Some("42".to_string()),
                outcome: crate::BookResolutionOutcome::Resolved {
                    identifier: crate::BookIdentifier::Isbn("9780140328721".to_string()),
                    edition: Box::new(crate::OpenLibraryEdition {
                        title: Some("Matilda".to_string()),
                        authors: vec!["Roald Dahl".to_string()],
                        record_url: Some(
                            "https://openlibrary.org/books/OL7826547M/Matilda".to_string(),
                        ),
                        ..crate::OpenLibraryEdition::default()
                    }),
                    scan: Some(crate::ScanAvailability {
                        exact: vec![crate::ScanItem {
                            status: "full access".to_string(),
                            item_url: "https://archive.org/details/matilda00dahl".to_string(),
                            ol_edition_id: None,
                        }],
                        similar: vec![],
                    }),
                },
            }],
            stats: PageVerificationStats {
                refs_seen: 4,
                use_sites_verified: 2,
                skipped: 2,
                extraction_failures: 1,
                supported: 1,
                partial: 0,
                not_supported: 0,
                source_unavailable: 1,
                source_unavailable_unreachable: 1,
                source_unavailable_unusable: 0,
                books_resolved: 1,
                books_not_found: 0,
                book_lookups_failed: 0,
            },
        }
    }

    #[test]
    fn renders_stats_findings_skipped_and_failures() {
        let report = sample_report();
        let document = page_verification_report_to_document(&report);
        assert_eq!(document.title, "Page citation report");
        assert!(
            document
                .lead_lines
                .contains(&"title=\"Museum\"".to_string())
        );
        assert!(document.lead_lines.contains(&"wiki=enwiki".to_string()));
        assert!(
            document
                .lead_lines
                .iter()
                .any(|line| line.contains("supported=1") && line.contains("unreachable=1"))
        );

        let findings = &document.sections[0];
        assert_eq!(findings.name, "Findings");
        assert!(findings.available);
        // Sorted by ordinal: supported (#0) before unreachable (#2).
        assert!(findings.summary_lines[0].starts_with("#0 ref=cite_1 supported"));
        assert!(findings.summary_lines[0].contains("grounding=located"));
        assert!(
            findings.summary_lines[1].contains("#2 ref=cite_3 source_unavailable (unreachable)")
        );
        // NotApplicable grounding is elided.
        assert!(!findings.summary_lines[1].contains("grounding="));

        let text = render_page_verification_text(&report);
        assert!(text.contains("Page citation report"));
        assert!(text.contains("[Skipped] available=true"));
        assert!(text.contains("ref=cite_book reason=NonUrlSource block=4"));
        assert!(
            text.contains("ref=cite_isbn reason=BookSource block=5 identifiers=isbn:9780140328721")
        );
        assert!(text.contains("block=7 reason=no claim sentence"));

        let markdown = render_page_verification_markdown(&report);
        assert!(markdown.contains("# Page citation report"));
        assert!(markdown.contains("## Findings"));
    }

    #[test]
    fn renders_books_section_with_resolution_and_scan() {
        let report = sample_report();
        let text = render_page_verification_text(&report);
        assert!(text.contains("books: resolved=1 not_found=0 lookup_failed=0"));
        assert!(text.contains("[Books] available=true"));
        assert!(text.contains(
            "ref=cite_isbn block=5 resolved=isbn:9780140328721 title=\"Matilda\" \
             authors=\"Roald Dahl\" url=https://openlibrary.org/books/OL7826547M/Matilda \
             scan=exact"
        ));
    }

    #[test]
    fn book_line_reports_misses_and_failures_distinctly() {
        let mut report = sample_report();
        report.book_resolutions = vec![
            crate::BookResolution {
                ref_id: "cite_isbn".to_string(),
                block_ordinal: 5,
                identifiers: vec![crate::BookIdentifier::Isbn("9780140328721".to_string())],
                cited_page: None,
                outcome: crate::BookResolutionOutcome::NotFound,
            },
            crate::BookResolution {
                ref_id: "cite_lccn".to_string(),
                block_ordinal: 6,
                identifiers: vec![crate::BookIdentifier::Lccn("n78890351".to_string())],
                cited_page: None,
                outcome: crate::BookResolutionOutcome::LookupFailed {
                    message: "openlibrary unreachable".to_string(),
                },
            },
        ];
        let text = render_page_verification_text(&report);
        assert!(text.contains("ref=cite_isbn block=5 not_found identifiers=isbn:9780140328721"));
        assert!(text.contains(
            "ref=cite_lccn block=6 lookup_failed identifiers=lccn:n78890351 \
             error=openlibrary unreachable"
        ));
    }

    #[test]
    fn empty_report_marks_sections_unavailable() {
        let report = PageVerificationReport {
            wiki_id: "enwiki".to_string(),
            rev_id: 1,
            title: "Empty".to_string(),
            findings: Vec::new(),
            skipped: Vec::new(),
            extraction_failures: Vec::new(),
            book_resolutions: Vec::new(),
            stats: PageVerificationStats::default(),
        };

        let document = page_verification_report_to_document(&report);
        assert!(document.sections.iter().all(|section| !section.available));

        let text = render_page_verification_text(&report);
        assert!(text.contains("[Findings] available=false"));
        assert!(text.contains("no findings"));
    }

    #[test]
    fn unusable_detail_and_archive_fallback_render() {
        let mut unusable = base_finding(0, "cite_1", "https://example.org/a.pdf", "A claim.");
        unusable.verdict = CitationVerdict::SourceUnavailable;
        unusable.grounding_status = GroundingStatus::NotApplicable;
        unusable.source_unavailable_reason = Some(SourceUnavailableReason::Unusable);
        unusable.unusable_reason = Some(BodyUsabilityReason::PdfBody);

        let mut archived = base_finding(
            1,
            "cite_2",
            "https://web.archive.org/snap",
            "Another claim.",
        );
        archived.archive_of =
            Some(url::Url::parse("https://dead.example/live").expect("url parses"));

        let report = PageVerificationReport {
            wiki_id: "enwiki".to_string(),
            rev_id: 9,
            title: "Mixed".to_string(),
            findings: vec![unusable, archived],
            skipped: Vec::new(),
            extraction_failures: Vec::new(),
            book_resolutions: Vec::new(),
            stats: PageVerificationStats::default(),
        };

        let document = page_verification_report_to_document(&report);
        let lines = &document.sections[0].summary_lines;
        assert!(lines[0].contains("source_unavailable (unusable: PdfBody)"));
        assert!(lines[1].contains("supported"));
        assert!(lines[1].contains("via archive of https://dead.example/live"));
    }

    #[test]
    fn long_claim_is_truncated_with_ellipsis() {
        let long = "x".repeat(200);
        let finding = base_finding(0, "cite_1", "https://example.org/a", &long);
        let report = PageVerificationReport {
            wiki_id: "enwiki".to_string(),
            rev_id: 1,
            title: "Long".to_string(),
            findings: vec![finding],
            skipped: Vec::new(),
            extraction_failures: Vec::new(),
            book_resolutions: Vec::new(),
            stats: PageVerificationStats::default(),
        };

        let document = page_verification_report_to_document(&report);
        let line = &document.sections[0].summary_lines[0];
        assert!(line.contains('…'));
        assert!(!line.contains(&"x".repeat(200)));
    }
}
