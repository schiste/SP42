//! Pure builder: `PageVerificationReport` → plain-wikitext GA evidence appendix.
//! (PRD-0016). No I/O, no inference; deterministic given the report
//! plus the shell-injected render timestamp.

/// Escape one verbatim field for safe embedding in wikitext (PRD-0016 hard
/// safety rule): entity-encode `&`, `<`, `>` inside the content — which makes
/// an embedded `</nowiki>` terminator inert — then wrap in `<nowiki>` so
/// braces, brackets, and pipes stay display-only.
fn escape_verbatim(text: &str) -> String {
    let inner = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!("<nowiki>{inner}</nowiki>")
}

/// Reader-facing ref label derived from the stable cite id (PRD-0016: the
/// report carries no rendered marker; never print the raw `cite_ref-…` id).
/// Named `MediaWiki` refs produce `cite_ref-<name>_<seq>-<use>`; unnamed refs
/// produce `cite_ref-<n>`. The `ordinal` is the finding's `use_site_ordinal`.
fn ref_label(ref_id: &str, ordinal: u32) -> String {
    let fallback = format!("ref #{}", ordinal + 1);
    let Some(rest) = ref_id.strip_prefix("cite_ref-") else {
        return fallback;
    };
    if rest.chars().all(|c| c.is_ascii_digit()) {
        return fallback;
    }
    // Strip the trailing `-<use>` then the trailing `_<seq>`; what remains is
    // the ref name. Any parse miss falls back to the ordinal.
    let Some((rest, use_idx)) = rest.rsplit_once('-') else {
        return fallback;
    };
    if !use_idx.chars().all(|c| c.is_ascii_digit()) {
        return fallback;
    }
    let Some((name, seq)) = rest.rsplit_once('_') else {
        return fallback;
    };
    if name.is_empty() || !seq.chars().all(|c| c.is_ascii_digit()) {
        return fallback;
    }
    format!("ref \"{}\"", name.replace('_', " "))
}

/// `YYYY-MM-DD` (UTC) from epoch milliseconds. Civil-from-days per Howard
/// Hinnant's algorithm — the workspace carries no date crate, and the footer
/// needs only a date (cf. the private helpers in `sp42-live`).
fn format_utc_date(epoch_ms: i64) -> String {
    let days = epoch_ms.div_euclid(86_400_000);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!("{year:04}-{month:02}-{day:02}")
}

/// Count-matched noun for the stats line ("1 dead link" / "2 dead links").
const fn plural<'a>(count: usize, singular: &'a str, plural: &'a str) -> &'a str {
    if count == 1 { singular } else { plural }
}

/// The consequence-ordered criterion-2 sublists (PRD-0016). Verdict
/// partitions; grounding and `archive_of` annotate. Every finding lands in
/// exactly one bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bucket {
    Disagreement,
    Recovered,
    DeadLink,
    Unreadable,
    Unconfirmed,
    Supported,
}

fn bucket_for(finding: &sp42_citation::CitationFinding) -> Bucket {
    use sp42_citation::{CitationVerdict, GroundingStatus, SourceUnavailableReason, SupportLevel};

    match finding.verdict {
        CitationVerdict::Judged(SupportLevel::NotSupported | SupportLevel::Partial) => {
            Bucket::Disagreement
        }
        CitationVerdict::Judged(SupportLevel::Supported) => {
            if finding.grounding_status == GroundingStatus::Located {
                if finding.archive_of.is_some() {
                    Bucket::Recovered
                } else {
                    Bucket::Supported
                }
            } else {
                Bucket::Unconfirmed
            }
        }
        CitationVerdict::SourceUnavailable => match finding.source_unavailable_reason {
            Some(SourceUnavailableReason::Unusable) => Bucket::Unreadable,
            Some(SourceUnavailableReason::Unreachable) | None => Bucket::DeadLink,
        },
    }
}

/// Render the GA evidence appendix (PRD-0016) — pure and deterministic.
///
/// `rendered_at_ms` is the shell-injected render time (the report contract
/// carries no verification timestamp today; the footer labels the date as the
/// render date). `sp42_version` is the shell's crate version.
#[must_use]
pub fn render_ga_appendix(
    report: &sp42_citation::PageVerificationReport,
    rendered_at_ms: i64,
    sp42_version: &str,
) -> String {
    use sp42_citation::{CitationVerdict, GroundingStatus, SupportLevel};

    let mut output = String::new();

    // Header
    output.push_str(crate::copy::APPENDIX_HEADING);
    output.push_str("\n\n");
    output.push_str(crate::copy::ASSESSED_LINE);
    output.push('\n');

    // Criterion-2 heading and stats line
    output.push('\n');
    output.push_str(crate::copy::CRITERION_2_HEADING);
    output.push('\n');

    // Calculate supported split
    let unconfirmed_supported = report
        .findings
        .iter()
        .filter(|f| {
            matches!(f.verdict, CitationVerdict::Judged(SupportLevel::Supported))
                && f.grounding_status != GroundingStatus::Located
        })
        .count();

    let stats_line = format!(
        "Of {} references, {} citation use-sites were machine-checked: {} supported ({} of them unconfirmed), {} partially supported, {} where claim and source disagree, {} dead {}, {} {} the tool could not read; {} book/offline {} and {} unprocessable {} were not checked.",
        report.stats.refs_seen,
        report.stats.use_sites_verified,
        report.stats.supported,
        unconfirmed_supported,
        report.stats.partial,
        report.stats.not_supported,
        report.stats.source_unavailable_unreachable,
        plural(report.stats.source_unavailable_unreachable, "link", "links"),
        report.stats.source_unavailable_unusable,
        plural(
            report.stats.source_unavailable_unusable,
            "source",
            "sources"
        ),
        report.stats.skipped,
        plural(report.stats.skipped, "ref", "refs"),
        report.stats.extraction_failures,
        plural(report.stats.extraction_failures, "ref", "refs")
    );
    output.push('\'');
    output.push_str(&stats_line);
    output.push_str("'\n");

    // Render buckets
    render_bucket(&mut output, report, Bucket::Disagreement);
    render_bucket(&mut output, report, Bucket::Recovered);
    render_bucket(&mut output, report, Bucket::DeadLink);
    render_bucket(&mut output, report, Bucket::Unreadable);
    render_bucket(&mut output, report, Bucket::Unconfirmed);
    render_bucket(&mut output, report, Bucket::Supported);

    render_skipped_section(&mut output, report);

    render_books_section(&mut output, report);

    // Extraction failures
    if !report.extraction_failures.is_empty() {
        output.push('\n');
        output.push_str(crate::copy::BUCKET_EXTRACTION_FAILURES);
        output.push('\n');
        for failure in &report.extraction_failures {
            output.push_str("* ");
            let rewritten = sanitize_reason(&failure.reason, failure.block_ordinal);
            output.push_str(&escape_verbatim(&rewritten));
            output.push('\n');
        }
    }

    // Footer
    output.push_str("\n----\n");
    output.push_str("''");
    output.push_str(&report.title);
    output.push_str(" at rev ");
    output.push_str(&report.rev_id.to_string());
    output.push_str(" · rendered ");
    output.push_str(&format_utc_date(rendered_at_ms));
    output.push_str(" (render date, not verification date) · SP42 ");
    output.push_str(sp42_version);
    output.push_str(" · ");
    output.push_str(crate::copy::FRAMING_LINE);
    output.push_str(" [");
    output.push_str(crate::copy::EXPLAINER_URL);
    output.push_str(" What is this?]''");

    output
}

/// Label for a skipped ref: named cite ids keep the derived-name path; an
/// unnamed id must NOT be numbered from `block_ordinal` (a paragraph index,
/// not a citation number — Codex round 5, PR 154), so it renders
/// block-anchored instead.
fn skip_label(ref_id: &str, block_ordinal: usize) -> String {
    let unnamed = ref_id
        .strip_prefix("cite_ref-")
        .is_none_or(|rest| rest.chars().all(|c| c.is_ascii_digit()));
    if unnamed {
        format!("an unnamed ref (block {block_ordinal})")
    } else {
        ref_label(ref_id, 0)
    }
}

/// Book-scan provenance annotation for a finding verified against an
/// Internet Archive scan: disclose a scanned-vs-cited page difference and
/// any search note the report carries (Codex round 6, PR 154).
fn book_scan_annotation(finding: &sp42_citation::CitationFinding) -> Option<String> {
    let scan = finding.book_scan.as_ref()?;
    let mut parts: Vec<String> = Vec::new();
    if let (Some(scanned), Some(cited)) = (scan.scanned_page, scan.cited_page.as_deref())
        && scanned.to_string() != cited
    {
        parts.push(crate::copy::book_scan_pages(scanned, cited));
    }
    if let Some(note) = &scan.note {
        parts.push(escape_verbatim(note));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("; "))
    }
}

/// The skipped-refs section: reason-matched reader copy, with `BookSource`
/// skips joined against `book_resolutions` so failed lookups are disclosed
/// as tool failures (Codex rounds 2–3, PR 154).
fn render_skipped_section(output: &mut String, report: &sp42_citation::PageVerificationReport) {
    // A BookSource skip's detail (catalog miss vs failed lookup) lives in
    // the report's book_resolutions. Join by ref id AND identifiers (not just
    // ref id) so a URL-less ref citing two books (one LookupFailed, one NotFound)
    // renders each with its matching outcome, not both as failures (Codex round 3, PR 154).
    let skip_outcome =
        |skip: &sp42_citation::SkippedRef| -> Option<&sp42_citation::BookResolutionOutcome> {
            let skip_identifiers = skip.book_identifiers();
            if skip_identifiers.is_empty() {
                // No identifiers to match; use ref_id-only fallback for legacy
                // or non-book skips (Codex round 3, PR 154).
                return report
                    .book_resolutions
                    .iter()
                    .find(|resolution| resolution.ref_id == skip.ref_id)
                    .map(|resolution| &resolution.outcome);
            }
            // Per-source join: find a resolution matching this skip's ref AND at least one identifier.
            report
                .book_resolutions
                .iter()
                .find(|resolution| {
                    resolution.ref_id == skip.ref_id
                        && resolution
                            .identifiers
                            .iter()
                            .any(|id| skip_identifiers.contains(&id))
                })
                .map(|resolution| &resolution.outcome)
        };
    if !report.skipped.is_empty() {
        output.push('\n');
        output.push_str(crate::copy::BUCKET_SKIPPED);
        output.push('\n');
        for skip in &report.skipped {
            output.push_str("* ");
            output.push_str(&skip_label(&skip.ref_id, skip.block_ordinal));
            output.push_str(": ");
            output.push_str(match skip.reason {
                sp42_citation::SkippedReason::NonUrlSource => crate::copy::SKIPPED_NON_URL,
                sp42_citation::SkippedReason::BookSource => {
                    if let Some(sp42_citation::BookResolutionOutcome::LookupFailed { .. }) =
                        skip_outcome(skip)
                    {
                        crate::copy::SKIPPED_BOOK_LOOKUP_FAILED
                    } else {
                        crate::copy::SKIPPED_BOOK_UNRESOLVED
                    }
                }
            });
            // Which book: identifiers (and cited page) make multiple books
            // under one ref distinguishable (Codex round 4, PR 154).
            let identifiers = skip.book_identifiers();
            if !identifiers.is_empty() {
                let labels: Vec<String> = identifiers
                    .iter()
                    .map(|identifier| crate::copy::book_identifier(identifier))
                    .collect();
                output.push_str(" (");
                output.push_str(&labels.join(", "));
                if let Some(page) = skip
                    .book_sources
                    .iter()
                    .find_map(|book| book.cited_page.as_deref())
                {
                    output.push_str(", p. ");
                    output.push_str(&escape_verbatim(page));
                }
                output.push(')');
            }
            output.push('\n');
        }
    }
}

/// The books-consulted section: shows books that were resolved but had no
/// searchable scan (`SourceUnavailable` outcomes) — distinct from unresolved
/// skips. Renders edition title and scan state for each book (Codex round 3, PR 154).
fn render_books_section(output: &mut String, report: &sp42_citation::PageVerificationReport) {
    let resolvable = report
        .book_resolutions
        .iter()
        .filter(|resolution| {
            matches!(
                resolution.outcome,
                sp42_citation::BookResolutionOutcome::Resolved { .. }
            )
        })
        .collect::<Vec<_>>();

    if resolvable.is_empty() {
        return;
    }

    output.push('\n');
    output.push_str(crate::copy::BUCKET_BOOKS_CONSULTED);
    output.push('\n');

    for resolution in resolvable {
        output.push_str("* ");
        output.push_str(&skip_label(&resolution.ref_id, resolution.block_ordinal));
        output.push_str(": ");

        if let sp42_citation::BookResolutionOutcome::Resolved { edition, scan, .. } =
            &resolution.outcome
        {
            // Render the edition title (preferred) or a generic fallback.
            if let Some(title) = &edition.title {
                output.push_str(&escape_verbatim(title));
            } else {
                output.push_str("(untitled book)");
            }

            output.push_str(" — ");

            // Render scan availability state (Codex round 3, PR 154).
            let scan_state = match scan {
                Some(availability) => {
                    if !availability.exact.is_empty()
                        && availability.exact.iter().any(|item| item.ocaid.is_some())
                    {
                        crate::copy::BOOK_SCAN_EXACT
                    } else if !availability.similar.is_empty() {
                        crate::copy::BOOK_SCAN_SIMILAR_ONLY
                    } else {
                        crate::copy::BOOK_SCAN_NONE
                    }
                }
                None => crate::copy::BOOK_SCAN_UNKNOWN,
            };
            output.push_str(scan_state);
        }
        output.push('\n');
    }
}

fn render_bucket(
    output: &mut String,
    report: &sp42_citation::PageVerificationReport,
    bucket: Bucket,
) {
    let findings: Vec<_> = report
        .findings
        .iter()
        .filter(|f| bucket_for(f) == bucket)
        .collect();

    if findings.is_empty() {
        return;
    }

    output.push('\n');
    match bucket {
        Bucket::Disagreement => output.push_str(crate::copy::BUCKET_DISAGREEMENTS),
        Bucket::Recovered => output.push_str(crate::copy::BUCKET_RECOVERED),
        Bucket::DeadLink => output.push_str(crate::copy::BUCKET_DEAD_LINKS),
        Bucket::Unreadable => output.push_str(crate::copy::BUCKET_UNREADABLE),
        Bucket::Unconfirmed => output.push_str(crate::copy::BUCKET_UNCONFIRMED),
        Bucket::Supported => output.push_str(crate::copy::BUCKET_SUPPORTED),
    }
    output.push('\n');

    for finding in findings {
        match bucket {
            Bucket::Disagreement => render_disagreement_line(output, finding),
            Bucket::Recovered => render_recovered_line(output, finding),
            Bucket::DeadLink => render_dead_link_line(output, finding),
            Bucket::Unreadable => render_unreadable_line(output, finding),
            Bucket::Unconfirmed => render_unconfirmed_line(output, finding),
            Bucket::Supported => render_supported_line(output, finding),
        }
    }
}

fn render_disagreement_line(output: &mut String, finding: &sp42_citation::CitationFinding) {
    use sp42_citation::{CitationVerdict, SupportLevel};

    output.push_str("* ");
    output.push_str(&ref_label(&finding.ref_id, finding.use_site_ordinal));
    output.push_str(": ");

    if let CitationVerdict::Judged(level) = finding.verdict {
        output.push_str(crate::copy::disagreement_verdict(level));
    }

    output.push_str(". Claim: ");
    output.push_str(&escape_verbatim(&truncate_claim(&finding.claim, 200)));
    output.push('.');

    if let Some(passage) = &finding.passage {
        output.push_str(" The panel located: ");
        output.push_str(&escape_verbatim(&passage.quote));
        output.push('.');
    } else if let Some(excerpt) = &finding.source_excerpt {
        output.push(' ');
        output.push_str(crate::copy::EXCERPT_CONTEXT_LABEL);
        output.push_str(": ");
        output.push_str(&escape_verbatim(&truncate_claim(excerpt, 200)));
        output.push('.');
    } else {
        output.push(' ');
        output.push_str(crate::copy::NO_PASSAGE_LINE);
        output.push('.');
    }

    if let Some(annotation) = crate::copy::grounding_annotation(finding.grounding_status)
        && matches!(
            finding.verdict,
            CitationVerdict::Judged(SupportLevel::Partial)
        )
    {
        output.push(' ');
        output.push_str(annotation);
        output.push('.');
    }

    // Only a real panel can split: no-model findings (deterministic book
    // search-inside outcomes) carry PanelAgreement(0, 0) and must not be
    // labeled low-confidence (Codex round 2, PR 154).
    if finding.agreement.panel_size > 0
        && u32::from(finding.agreement.winner_votes) * 2 <= u32::from(finding.agreement.panel_size)
    {
        output.push_str(" (");
        output.push_str(crate::copy::PANEL_SPLIT_LINE);
        output.push(')');
    }

    output.push_str(" — [");
    output.push_str(finding.provenance.url.as_str());
    output.push(']');

    if let Some(archive) = &finding.archive_of {
        output.push_str(" (");
        output.push_str(crate::copy::ARCHIVE_HANDLE_PREFIX);
        output.push_str(" [");
        output.push_str(archive.as_str());
        output.push_str("])");
    }

    output.push('\n');
}

fn render_recovered_line(output: &mut String, finding: &sp42_citation::CitationFinding) {
    output.push_str("* ");
    output.push_str(&ref_label(&finding.ref_id, finding.use_site_ordinal));
    // Contract semantics: `provenance.url` is the archive copy that was
    // actually read; `archive_of` is the dead live URL it stands in for
    // (Codex P1, PR 154 — the fields were previously swapped here).
    output.push_str(": supported via an archive copy — update the citation to [");
    output.push_str(finding.provenance.url.as_str());
    output.push_str("] (replacing the dead link: [");
    if let Some(dead_live) = &finding.archive_of {
        output.push_str(dead_live.as_str());
    }
    output.push_str("]). Claim: ");
    output.push_str(&escape_verbatim(&truncate_claim(&finding.claim, 120)));
    output.push('.');
    output.push('\n');
}

fn render_dead_link_line(output: &mut String, finding: &sp42_citation::CitationFinding) {
    output.push_str("* ");
    output.push_str(&ref_label(&finding.ref_id, finding.use_site_ordinal));
    output.push_str(": the source could not be fetched (link may be dead): [");
    output.push_str(finding.provenance.url.as_str());
    output.push(']');
    if let Some(annotation) = book_scan_annotation(finding) {
        output.push_str(" (");
        output.push_str(&annotation);
        output.push(')');
    }
    output.push('\n');
}

fn render_unreadable_line(output: &mut String, finding: &sp42_citation::CitationFinding) {
    output.push_str("* ");
    output.push_str(&ref_label(&finding.ref_id, finding.use_site_ordinal));
    output.push_str(": the tool fetched [");
    output.push_str(finding.provenance.url.as_str());
    output.push_str("] but read ");

    let reason_text = if let Some(reason) = finding.unusable_reason {
        crate::copy::unusable_reason(reason)
    } else {
        crate::copy::UNUSABLE_GENERIC
    };
    output.push_str(reason_text);
    output.push_str(" — the citation may be fine.");
    if let Some(annotation) = book_scan_annotation(finding) {
        output.push_str(" (");
        output.push_str(&annotation);
        output.push(')');
    }
    output.push('\n');
}

fn render_unconfirmed_line(output: &mut String, finding: &sp42_citation::CitationFinding) {
    output.push_str("* ");
    output.push_str(&ref_label(&finding.ref_id, finding.use_site_ordinal));
    output.push_str(": judged supported, but ");

    if let Some(annotation) = crate::copy::grounding_annotation(finding.grounding_status) {
        output.push_str(annotation);
    }

    output.push_str(". Claim: ");
    output.push_str(&escape_verbatim(&truncate_claim(&finding.claim, 80)));
    output.push_str(". — [");
    output.push_str(finding.provenance.url.as_str());
    output.push(']');

    if let Some(archive) = &finding.archive_of {
        output.push_str(" (");
        output.push_str(crate::copy::ARCHIVE_HANDLE_PREFIX);
        output.push_str(" [");
        output.push_str(archive.as_str());
        output.push_str("])");
    }

    output.push('\n');
}

fn render_supported_line(output: &mut String, finding: &sp42_citation::CitationFinding) {
    output.push_str("* ");
    output.push_str(&ref_label(&finding.ref_id, finding.use_site_ordinal));
    output.push_str(": supported — ");
    output.push_str(&escape_verbatim(&truncate_claim(&finding.claim, 80)));
    output.push_str(" — [");
    output.push_str(finding.provenance.url.as_str());
    output.push_str("] (quote located)");
    if let Some(annotation) = book_scan_annotation(finding) {
        output.push_str(" (");
        output.push_str(&annotation);
        output.push(')');
    }
    output.push('\n');
}

fn truncate_claim(text: &str, max_len: usize) -> String {
    let char_count = text.chars().count();
    let truncated: String = text.chars().take(max_len).collect();
    if char_count > max_len {
        truncated + "…"
    } else {
        truncated
    }
}

/// Rewrite `cite_ref` IDs in an extraction-failure reason to reader-facing labels.
/// For unnamed refs (`cite_ref-<digits>`), use a block-anchored phrase to keep
/// failures distinct; named refs use the existing derived-name path
/// (Codex round 3, PR 154).
fn sanitize_reason(reason: &str, block_ordinal: usize) -> String {
    let mut result = String::new();
    let mut remaining = reason;

    while let Some(pos) = remaining.find("cite_ref-") {
        result.push_str(&remaining[..pos]);
        let after_prefix = &remaining[pos + "cite_ref-".len()..];

        // Find end of token (whitespace or end of string)
        let token_end = after_prefix
            .find(|c: char| c.is_whitespace())
            .unwrap_or(after_prefix.len());

        let cite_id = &remaining[pos..pos + "cite_ref-".len() + token_end];
        let rest = cite_id.strip_prefix("cite_ref-").unwrap_or("");
        // Reason strings punctuate ids ("verify … for cite_ref-64: …"); strip
        // trailing punctuation before classifying (Codex round 5, PR 154).
        let trailing_punct: usize = rest
            .chars()
            .rev()
            .take_while(char::is_ascii_punctuation)
            .map(char::len_utf8)
            .sum();
        let rest = &rest[..rest.len() - trailing_punct];
        let cite_id = &cite_id[..cite_id.len() - trailing_punct];

        // For unnamed refs (all digits), use block-anchored label to distinguish
        // multiple failed extractions in the same output (Codex round 3, PR 154).
        let label = if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
            format!("an unnamed ref (block {block_ordinal})")
        } else {
            ref_label(cite_id, 0)
        };
        result.push_str(&label);

        // Re-emit stripped trailing punctuation ("cite_ref-64:" keeps its
        // colon after the label).
        remaining = &remaining[pos + "cite_ref-".len() + token_end - trailing_punct..];
    }

    result.push_str(remaining);
    result
}

#[cfg(test)]
mod fixtures {
    use sp42_citation::{
        BlockFailure, CitationFinding, CitationFindingKind, CitationVerdict, GroundingAssertion,
        GroundingStatus, LocatedPassage, PageVerificationReport, PageVerificationStats,
        PanelAgreement, SkippedReason, SkippedRef, SourceProvenance, SourceUnavailableReason,
        SupportLevel,
    };

    fn base_finding(
        verdict: CitationVerdict,
        grounding: GroundingStatus,
        archived: bool,
        use_site_ordinal: u32,
        ref_id: &str,
        claim: &str,
    ) -> CitationFinding {
        CitationFinding {
            kind: CitationFindingKind::default(),
            verdict,
            grounding_status: grounding,
            source_unavailable_reason: None,
            unusable_reason: None,
            agreement: PanelAgreement::new(3, 3),
            passage: if grounding == GroundingStatus::Located {
                Some(LocatedPassage {
                    quote: "supporting quote text".to_string(),
                    offset: 0,
                })
            } else {
                None
            },
            provenance: SourceProvenance {
                url: if archived {
                    url::Url::parse("https://web.archive.org/x").unwrap()
                } else {
                    url::Url::parse("https://example.org/source").unwrap()
                },
                content_hash: String::new(),
                fetched_at: 0,
                http_status: Some(200),
            },
            source_excerpt: None,
            metadata: None,
            grounding: GroundingAssertion::SourceFetched {
                source_hash: String::new(),
            },
            use_site_ordinal,
            ref_id: ref_id.to_string(),
            claim: claim.to_string(),
            preceding_context: Vec::new(),
            archive_of: if archived {
                Some(url::Url::parse("https://example.org/dead-live").unwrap())
            } else {
                None
            },
            is_bare_url_ref: false,
            book_scan: None,
            schema_version: 1,
        }
    }

    #[allow(clippy::too_many_lines)] // https://github.com/schiste/SP42/blob/main/docs/domains/assessment/prd/0016-ga-evidence-appendix-renderer.md full-appendix fixture spans every bucket by design
    pub fn full_report() -> PageVerificationReport {
        let mut findings = vec![];

        // Disagreement with archive and minority panel
        let mut f = base_finding(
            CitationVerdict::Judged(SupportLevel::NotSupported),
            GroundingStatus::NotApplicable,
            true,
            0,
            "cite_ref-1",
            "This claim is disputed",
        );
        f.agreement = PanelAgreement::new(3, 1);
        findings.push(f);

        // Disagreement without archive, with excerpt
        let mut f = base_finding(
            CitationVerdict::Judged(SupportLevel::NotSupported),
            GroundingStatus::NotApplicable,
            false,
            1,
            "cite_ref-1b",
            "Another disputed claim",
        );
        f.source_excerpt = Some("source text excerpt".to_string());
        findings.push(f);

        // Disagreement without archive (Partial)
        findings.push(base_finding(
            CitationVerdict::Judged(SupportLevel::Partial),
            GroundingStatus::Located,
            false,
            2,
            "cite_ref-2",
            "Partially supported claim",
        ));

        // Recovered (Supported + Located + Archive)
        findings.push(base_finding(
            CitationVerdict::Judged(SupportLevel::Supported),
            GroundingStatus::Located,
            true,
            3,
            "cite_ref-3",
            "This was supported via archive",
        ));

        // Dead link
        let mut f = base_finding(
            CitationVerdict::SourceUnavailable,
            GroundingStatus::NotApplicable,
            false,
            4,
            "cite_ref-4",
            "Dead link claim",
        );
        f.source_unavailable_reason = Some(SourceUnavailableReason::Unreachable);
        findings.push(f);

        // Unreadable (PdfBody)
        let mut f = base_finding(
            CitationVerdict::SourceUnavailable,
            GroundingStatus::NotApplicable,
            false,
            5,
            "cite_ref-5",
            "PDF claim",
        );
        f.source_unavailable_reason = Some(SourceUnavailableReason::Unusable);
        f.unusable_reason = Some(sp42_citation::BodyUsabilityReason::PdfBody);
        findings.push(f);

        // Unreadable (ViewerShell)
        let mut f = base_finding(
            CitationVerdict::SourceUnavailable,
            GroundingStatus::NotApplicable,
            false,
            6,
            "cite_ref-6",
            "Viewer shell claim",
        );
        f.source_unavailable_reason = Some(SourceUnavailableReason::Unusable);
        f.unusable_reason = Some(sp42_citation::BodyUsabilityReason::ViewerShell);
        findings.push(f);

        // Unconfirmed (Supported + Unlocated + Archive)
        findings.push(base_finding(
            CitationVerdict::Judged(SupportLevel::Supported),
            GroundingStatus::Unlocated,
            true,
            7,
            "cite_ref-7",
            "Unconfirmed with archive",
        ));

        // Supported (exact match, no archive)
        findings.push(base_finding(
            CitationVerdict::Judged(SupportLevel::Supported),
            GroundingStatus::Located,
            false,
            8,
            "cite_ref-8",
            "Cleanly supported claim",
        ));

        PageVerificationReport {
            wiki_id: "test_page".to_string(),
            rev_id: 12345,
            title: "Test Article".to_string(),
            findings,
            skipped: vec![SkippedRef {
                ref_id: "cite_ref-9".to_string(),
                reason: SkippedReason::NonUrlSource,
                block_ordinal: 0,
                book_sources: Vec::new(),
            }],
            extraction_failures: vec![BlockFailure {
                block_ordinal: 1,
                reason: "ref cite_ref-64 has no resolvable claim text".to_string(),
            }],
            book_resolutions: Vec::new(),
            stats: PageVerificationStats {
                refs_seen: 10,
                use_sites_verified: 8,
                skipped: 1,
                extraction_failures: 1,
                supported: 3,
                partial: 1,
                not_supported: 1,
                source_unavailable: 3,
                source_unavailable_unreachable: 1,
                books_resolved: 0,
                books_not_found: 0,
                book_lookups_failed: 0,
                source_unavailable_unusable: 2,
            },
        }
    }

    pub fn hostile_report() -> PageVerificationReport {
        let mut findings = vec![];

        // Finding with hostile claim
        let f = base_finding(
            CitationVerdict::Judged(SupportLevel::Partial),
            GroundingStatus::Located,
            false,
            0,
            "cite_ref-10",
            "Claim with {{Infobox}} template",
        );
        findings.push(f);

        // Finding with hostile quote
        let mut f = base_finding(
            CitationVerdict::Judged(SupportLevel::Partial),
            GroundingStatus::Located,
            false,
            1,
            "cite_ref-11",
            "Normal claim",
        );
        if let Some(ref mut p) = f.passage {
            p.quote = "Quote with <ref>tag</ref> inside".to_string();
        }
        findings.push(f);

        // Finding with hostile excerpt
        let mut f = base_finding(
            CitationVerdict::Judged(SupportLevel::NotSupported),
            GroundingStatus::NotApplicable,
            false,
            2,
            "cite_ref-12",
            "Normal claim",
        );
        f.source_excerpt = Some("Excerpt with </nowiki> terminator".to_string());
        findings.push(f);

        PageVerificationReport {
            wiki_id: "hostile_page".to_string(),
            rev_id: 99999,
            title: "Hostile Test".to_string(),
            findings,
            skipped: Vec::new(),
            extraction_failures: Vec::new(),
            book_resolutions: Vec::new(),
            stats: PageVerificationStats {
                refs_seen: 3,
                use_sites_verified: 3,
                skipped: 0,
                extraction_failures: 0,
                supported: 0,
                partial: 2,
                not_supported: 1,
                source_unavailable: 0,
                source_unavailable_unreachable: 0,
                books_resolved: 0,
                books_not_found: 0,
                book_lookups_failed: 0,
                source_unavailable_unusable: 0,
            },
        }
    }

    pub fn bundled_ref_report() -> PageVerificationReport {
        let mut findings = vec![];

        // Two supported findings with same ref_id but different URLs
        let mut f = base_finding(
            CitationVerdict::Judged(SupportLevel::Supported),
            GroundingStatus::Located,
            false,
            0,
            "cite_ref-shared",
            "First use of shared ref",
        );
        f.provenance.url = url::Url::parse("https://example.org/one").unwrap();
        findings.push(f);

        let mut f = base_finding(
            CitationVerdict::Judged(SupportLevel::Supported),
            GroundingStatus::Located,
            false,
            1,
            "cite_ref-shared",
            "Second use of shared ref",
        );
        f.provenance.url = url::Url::parse("https://example.org/two").unwrap();
        findings.push(f);

        PageVerificationReport {
            wiki_id: "bundled_page".to_string(),
            rev_id: 77777,
            title: "Bundled Refs Test".to_string(),
            findings,
            skipped: Vec::new(),
            extraction_failures: Vec::new(),
            book_resolutions: Vec::new(),
            stats: PageVerificationStats {
                refs_seen: 1,
                use_sites_verified: 2,
                skipped: 0,
                extraction_failures: 0,
                supported: 2,
                partial: 0,
                not_supported: 0,
                source_unavailable: 0,
                source_unavailable_unreachable: 0,
                books_resolved: 0,
                books_not_found: 0,
                book_lookups_failed: 0,
                source_unavailable_unusable: 0,
            },
        }
    }
}

#[cfg(test)]
mod helper_tests {
    use super::{escape_verbatim, format_utc_date, ref_label};

    #[test]
    fn escape_neutralizes_templates_refs_and_nowiki_terminators() {
        let hostile = r"See {{Infobox}} and <ref>x</ref> then </nowiki>{{evil}} after";
        let escaped = escape_verbatim(hostile);
        assert!(escaped.starts_with("<nowiki>") && escaped.ends_with("</nowiki>"));
        let inner = &escaped["<nowiki>".len()..escaped.len() - "</nowiki>".len()];
        // The terminator case: no literal `</nowiki>` may survive inside the wrapper.
        assert!(!inner.contains("</nowiki>"));
        // Angle brackets are entity-encoded so no tag (ref, nowiki) is live.
        assert!(!inner.contains('<') && !inner.contains('>'));
        // Content is preserved (entity-decoded form still names the template).
        assert!(inner.contains("{{Infobox}}"));
    }

    #[test]
    fn escape_round_trips_preexisting_entities_faithfully() {
        // `&lt;` in the source text must not collapse into a live `<`.
        assert_eq!(escape_verbatim("a &lt; b"), "<nowiki>a &amp;lt; b</nowiki>");
    }

    #[test]
    fn ref_label_derives_names_and_falls_back_to_ordinal() {
        // Named ref: cite_ref-<name>_<seq>-<use>
        assert_eq!(
            ref_label("cite_ref-Lux_history_1-0", 4),
            "ref \"Lux history\""
        );
        // Unnamed ref: cite_ref-<n> — n is internal, use the per-report ordinal.
        assert_eq!(ref_label("cite_ref-6", 4), "ref #5");
        // Unparseable / empty id: ordinal fallback, never the raw id.
        assert_eq!(ref_label("", 0), "ref #1");
    }

    #[test]
    fn utc_date_formats_from_epoch_ms() {
        assert_eq!(format_utc_date(1_783_886_599_386), "2026-07-12");
        assert_eq!(format_utc_date(0), "1970-01-01");
    }

    #[test]
    fn truncate_claim_char_boundary_safe() {
        use super::truncate_claim;

        // Simple ASCII under limit
        assert_eq!(truncate_claim("hello", 10), "hello");

        // Simple ASCII over limit
        assert_eq!(truncate_claim("hello world", 5), "hello…");

        // Multibyte chars (accents) under limit — should not add ellipsis
        assert_eq!(truncate_claim("café", 10), "café");

        // Multibyte chars over limit
        let result = truncate_claim("café café café", 5);
        assert!(result.ends_with('…'));
        assert_eq!(result.chars().count(), 6); // 5 chars + '…'
    }

    #[test]
    fn sanitize_reason_char_boundary_safe() {
        use super::sanitize_reason;

        // Multibyte text with cite_ref should not panic
        let reason = "cite_ref-64café and more text";
        let result = sanitize_reason(reason, 0);
        assert!(result.contains("and more text"));

        // Multiple cite_refs with multibyte
        let reason = "cite_ref-1 and cite_ref-2café near cite_ref-3";
        let result = sanitize_reason(reason, 2);
        assert!(result.contains("and"));
        assert!(result.contains("near"));
        // Each cite_ref should be rewritten
        assert!(result.matches("ref").count() >= 3);
    }

    #[test]
    fn sanitize_reason_unnamed_refs_are_block_anchored() {
        use super::sanitize_reason;

        // Unnamed ref (all digits) becomes block-anchored (Codex round 3, PR 154).
        let reason = "cite_ref-64 has no resolvable claim text";
        let result = sanitize_reason(reason, 5);
        assert!(result.contains("block 5"));
        assert!(!result.contains("ref #"));

        // Named ref stays on the derived-name path
        let reason = "cite_ref-author_1-0 has no resolvable claim text";
        let result = sanitize_reason(reason, 5);
        assert!(result.contains("author"));
        assert!(!result.contains("block"));
    }

    #[test]
    fn distinct_unnamed_refs_in_same_output_render_distinct_labels() {
        use super::sanitize_reason;

        // Two unnamed refs in extraction failures should NOT both be "ref #1"
        // (Codex round 3, PR 154).
        let failure1 = "cite_ref-64 has no resolvable claim text";
        let failure2 = "cite_ref-65 has a broken parse";
        let result1 = sanitize_reason(failure1, 1);
        let result2 = sanitize_reason(failure2, 3);

        assert!(
            result1.contains("block 1"),
            "failure 1 should anchor to block 1"
        );
        assert!(
            result2.contains("block 3"),
            "failure 2 should anchor to block 3"
        );
        assert_ne!(
            result1, result2,
            "distinct failures should render distinct labels"
        );
    }
}

#[cfg(test)]
mod bucket_tests {
    use super::{Bucket, bucket_for};
    use sp42_citation::{
        CitationFinding, CitationFindingKind, CitationVerdict, GroundingAssertion, GroundingStatus,
        PanelAgreement, SourceProvenance, SourceUnavailableReason, SupportLevel,
    };

    // Fixture helper: house style is programmatic construction with defaults
    // (cf. citation_page_report.rs:212). Reuse via a shared `fn finding()` in
    // this module; fields not under test take neutral values.
    fn finding(
        verdict: CitationVerdict,
        grounding: GroundingStatus,
        archived: bool,
        unavailable: Option<SourceUnavailableReason>,
    ) -> CitationFinding {
        CitationFinding {
            kind: CitationFindingKind::default(),
            verdict,
            grounding_status: grounding,
            source_unavailable_reason: unavailable,
            unusable_reason: None,
            agreement: PanelAgreement::new(3, 3),
            passage: None,
            provenance: SourceProvenance {
                url: if archived {
                    url::Url::parse("https://web.archive.org/x").unwrap()
                } else {
                    url::Url::parse("https://example.org/a").unwrap()
                },
                content_hash: String::new(),
                fetched_at: 0,
                http_status: Some(200),
            },
            source_excerpt: None,
            metadata: None,
            grounding: GroundingAssertion::SourceFetched {
                source_hash: String::new(),
            },
            use_site_ordinal: 0,
            ref_id: String::new(),
            claim: String::new(),
            preceding_context: Vec::new(),
            archive_of: if archived {
                Some(url::Url::parse("https://example.org/dead-live").unwrap())
            } else {
                None
            },
            is_bare_url_ref: false,
            book_scan: None,
            schema_version: 1,
        }
    }

    #[test]
    fn verdict_partitions_and_grounding_annotates() {
        use CitationVerdict as V;
        use GroundingStatus as G;
        use SupportLevel as L;
        let cases = [
            // (verdict, grounding, archived, unavailable_reason) -> bucket
            (
                V::Judged(L::NotSupported),
                G::NotApplicable,
                false,
                None,
                Bucket::Disagreement,
            ),
            (
                V::Judged(L::Partial),
                G::Located,
                false,
                None,
                Bucket::Disagreement,
            ),
            // Non-exact grounding on Partial stays a disagreement (annotated).
            (
                V::Judged(L::Partial),
                G::Unlocated,
                false,
                None,
                Bucket::Disagreement,
            ),
            // Archive-backed disagreement stays a disagreement (with handle).
            (
                V::Judged(L::NotSupported),
                G::NotApplicable,
                true,
                None,
                Bucket::Disagreement,
            ),
            // Supported + exact + archive -> recovered.
            (
                V::Judged(L::Supported),
                G::Located,
                true,
                None,
                Bucket::Recovered,
            ),
            // Supported + exact, no archive -> spot-check record.
            (
                V::Judged(L::Supported),
                G::Located,
                false,
                None,
                Bucket::Supported,
            ),
            // Grounding caveat wins the bucket, archive annotates.
            (
                V::Judged(L::Supported),
                G::LocatedFuzzy,
                true,
                None,
                Bucket::Unconfirmed,
            ),
            (
                V::Judged(L::Supported),
                G::Unlocated,
                false,
                None,
                Bucket::Unconfirmed,
            ),
            (
                V::Judged(L::Supported),
                G::NotApplicable,
                false,
                None,
                Bucket::Unconfirmed,
            ),
            (
                V::SourceUnavailable,
                G::NotApplicable,
                false,
                Some(SourceUnavailableReason::Unreachable),
                Bucket::DeadLink,
            ),
            (
                V::SourceUnavailable,
                G::NotApplicable,
                false,
                Some(SourceUnavailableReason::Unusable),
                Bucket::Unreadable,
            ),
            // Legacy record with no reason: dead link (the conservative read).
            (
                V::SourceUnavailable,
                G::NotApplicable,
                false,
                None,
                Bucket::DeadLink,
            ),
        ];
        for (verdict, grounding, archived, unavailable, expected) in cases {
            let f = finding(verdict, grounding, archived, unavailable);
            assert_eq!(
                bucket_for(&f),
                expected,
                "{verdict:?}/{grounding:?}/archived={archived}"
            );
        }
    }
}

#[cfg(test)]
mod renderer_tests {
    use super::*;

    #[test]
    fn appendix_renders_the_full_criterion_2_structure() {
        let report = fixtures::full_report();
        let out = render_ga_appendix(&report, 1_783_886_599_386, "0.1.0");
        // Section + heading structure, consequence order.
        let idx = |needle: &str| {
            out.find(needle)
                .unwrap_or_else(|| panic!("missing: {needle}"))
        };
        assert!(idx(crate::copy::BUCKET_DISAGREEMENTS) < idx(crate::copy::BUCKET_RECOVERED));
        assert!(idx(crate::copy::BUCKET_RECOVERED) < idx(crate::copy::BUCKET_DEAD_LINKS));
        assert!(idx(crate::copy::BUCKET_DEAD_LINKS) < idx(crate::copy::BUCKET_UNREADABLE));
        assert!(idx(crate::copy::BUCKET_UNREADABLE) < idx(crate::copy::BUCKET_UNCONFIRMED));
        assert!(idx(crate::copy::BUCKET_UNCONFIRMED) < idx(crate::copy::BUCKET_SUPPORTED));
        assert!(idx(crate::copy::BUCKET_SUPPORTED) < idx(crate::copy::BUCKET_SKIPPED));
        // Honesty arms.
        assert!(out.contains(crate::copy::ASSESSED_LINE));
        assert!(
            out.contains(crate::copy::FRAMING_LINE) && out.contains(crate::copy::EXPLAINER_URL)
        );
        assert!(out.contains("2026-07-12"), "footer render date");
        assert!(out.contains("rev 12345"), "footer rev_id");
        // Stats line states the grounded/unconfirmed split within supported.
        assert!(out.contains("of them unconfirmed"));
    }

    #[test]
    fn no_raw_contract_identifiers_anywhere() {
        let out = render_ga_appendix(&fixtures::full_report(), 0, "0.1.0");
        for token in [
            "NotSupported",
            "SourceUnavailable",
            "Unlocated",
            "LocatedFuzzy",
            "NotApplicable",
            "cite_ref-",
            "ShortBody",
            "PdfBody",
            "snake_case",
            "not_supported",
            "source_unavailable",
        ] {
            assert!(!out.contains(token), "raw identifier leaked: {token}");
        }
    }

    #[test]
    fn no_pass_fail_wording_in_the_assembled_appendix() {
        let out = render_ga_appendix(&fixtures::full_report(), 0, "0.1.0").to_lowercase();
        let banned = [
            "pass", "passed", "passes", "fail", "failed", "fails", "failure",
        ];
        for word in out.split(|c: char| !c.is_ascii_alphabetic()) {
            assert!(!banned.contains(&word), "pass/fail wording leaked: {word}");
        }
    }

    #[test]
    fn unusable_reasons_wire_to_their_own_findings() {
        let out = render_ga_appendix(&fixtures::full_report(), 0, "0.1.0");
        assert!(out.contains(crate::copy::unusable_reason(
            sp42_citation::BodyUsabilityReason::PdfBody
        )));
        assert!(out.contains(crate::copy::unusable_reason(
            sp42_citation::BodyUsabilityReason::ViewerShell
        )));
    }

    #[test]
    fn rendering_is_deterministic() {
        let report = fixtures::full_report();
        let a = render_ga_appendix(&report, 1_700_000_000_000, "0.1.0");
        let b = render_ga_appendix(&report, 1_700_000_000_000, "0.1.0");
        assert_eq!(a, b);
    }

    #[test]
    fn hostile_verbatim_fields_render_inert() {
        let report = fixtures::hostile_report();
        let out = render_ga_appendix(&report, 0, "0.1.0");
        // Count opens and closes: every <nowiki> the renderer opens, it closes,
        // and no embedded terminator adds an extra close.
        assert_eq!(
            out.matches("<nowiki>").count(),
            out.matches("</nowiki>").count()
        );
        // The embedded terminator survives only entity-encoded.
        assert!(out.contains("&lt;/nowiki&gt;"));
        // The ref tag never appears live.
        assert!(!out.contains("<ref>"));
    }

    #[test]
    fn no_quote_disagreement_states_no_passage_and_excerpt_is_labeled_context() {
        let report = fixtures::full_report();
        let out = render_ga_appendix(&report, 0, "0.1.0");
        assert!(out.contains(crate::copy::NO_PASSAGE_LINE));
        assert!(out.contains(crate::copy::EXCERPT_CONTEXT_LABEL));
    }

    #[test]
    fn archive_handle_renders_in_every_bucket_that_carries_it() {
        let out = render_ga_appendix(&fixtures::full_report(), 0, "0.1.0");
        // Disagreement and Unconfirmed buckets carry archive handles;
        // Recovered embeds the URL directly in the description.
        assert_eq!(out.matches(crate::copy::ARCHIVE_HANDLE_PREFIX).count(), 2);
    }

    #[test]
    fn skips_and_extraction_failures_render_with_rewritten_ids() {
        let out = render_ga_appendix(&fixtures::full_report(), 0, "0.1.0");
        assert!(out.contains(crate::copy::SKIPPED_NON_URL));
        // BlockFailure.reason "ref cite_ref-64 has no resolvable claim text"
        // renders with the raw id rewritten (covered by the identifier scan too;
        // assert the line survived the rewrite rather than being dropped).
        assert!(out.contains("has no resolvable claim text"));
    }

    #[test]
    fn panel_split_annotation_renders_on_minority_verdicts_only() {
        let out = render_ga_appendix(&fixtures::full_report(), 0, "0.1.0");
        assert_eq!(out.matches(crate::copy::PANEL_SPLIT_LINE).count(), 1);
    }

    #[test]
    fn bundled_ref_supported_lines_are_distinguishable() {
        let out = render_ga_appendix(&fixtures::bundled_ref_report(), 0, "0.1.0");
        assert!(out.contains("https://example.org/one") && out.contains("https://example.org/two"));
    }

    #[test]
    fn truncate_claim_handles_multibyte_chars_correctly() {
        // Multibyte claim under the cap should not get ellipsis
        let claim_with_accents = "café résumé naïve"; // 17 chars
        let truncated = truncate_claim(claim_with_accents, 20);
        assert_eq!(truncated, claim_with_accents);
        assert!(!truncated.ends_with('…'));

        // Multibyte claim over the cap should truncate at char boundary + ellipsis
        let long_claim = "café café café café café café café café café"; // 43 chars with spaces
        let truncated = truncate_claim(long_claim, 20);
        assert_eq!(truncated.chars().count(), 21); // 20 chars + '…'
        assert!(truncated.ends_with('…'));
        // Verify no panic and proper char boundary (not in middle of é)
        assert!(truncated.chars().all(|c| !c.is_control()));
    }

    #[test]
    fn sanitize_reason_handles_multibyte_cite_refs() {
        // Regression: "cite_ref-64café" should not panic on byte slicing
        let reason = "Error in cite_ref-64café during extraction";
        let result = sanitize_reason(reason, 0);
        // Should rewrite cite_ref-64café part and not panic
        assert!(result.contains("Error in"));
        assert!(result.contains("during extraction"));
        // The cite_ref should be rewritten (64café is not all digits so it falls back to ordinal)
        assert!(result.contains("ref"));
    }

    #[test]
    fn extraction_failure_reason_with_hostile_content_is_escaped() {
        let mut report = fixtures::full_report();
        // Replace the extraction failure with a hostile one
        report.extraction_failures = vec![sp42_citation::BlockFailure {
            block_ordinal: 1,
            reason: "cite_ref-64 has {{Template}} and </nowiki> injected".to_string(),
        }];

        let out = render_ga_appendix(&report, 0, "0.1.0");

        // The reason should be escaped and thus inert
        // Templates should be preserved but wrapped in <nowiki>
        assert!(out.contains("{{Template}}"));
        // Angle brackets should be entity-encoded
        assert!(out.contains("&lt;/nowiki&gt;"));
        // The outer <nowiki> wrappers should balance
        assert_eq!(
            out.matches("<nowiki>").count(),
            out.matches("</nowiki>").count()
        );
    }

    #[test]
    fn recovered_bucket_line_contains_archive_url() {
        let report = fixtures::full_report();
        let out = render_ga_appendix(&report, 0, "0.1.0");

        // The Recovered finding (index 3) has archive_of: Some(url::Url::parse("https://web.archive.org/x").unwrap())
        // Find the Recovered section and verify it contains the archive URL
        let recovered_idx = out
            .find(crate::copy::BUCKET_RECOVERED)
            .expect("recovered bucket present");
        let recovered_section = &out[recovered_idx..];
        // Look for the archive URL in the Recovered section (up to next bucket)
        let recovered_end = recovered_section
            .find(crate::copy::BUCKET_DEAD_LINKS)
            .unwrap_or(recovered_section.len());
        let recovered_text = &recovered_section[..recovered_end];
        assert!(
            recovered_text.contains("https://web.archive.org/x"),
            "Recovered bucket should contain the archive URL"
        );
    }

    #[test]
    fn recovered_line_updates_to_the_archive_and_names_the_dead_link() {
        // Codex P1 (PR 154): provenance.url is the archive actually read;
        // archive_of is the dead live URL. The repair instruction must point
        // at the archive, never the dead link.
        let out = render_ga_appendix(&fixtures::full_report(), 0, "0.1.0");
        assert!(
            out.contains("update the citation to [https://web.archive.org/x]"),
            "repair handle must target the archive"
        );
        assert!(
            out.contains("replacing the dead link: [https://example.org/dead-live]"),
            "the dead live URL is named as dead"
        );
        assert!(
            !out.contains("update the citation to [https://example.org/dead-live]"),
            "never instruct citing the dead URL"
        );
    }

    #[test]
    fn no_model_findings_are_never_labeled_panel_split() {
        // Codex round 2 (PR 154): PanelAgreement(0, 0) means no panel voted
        // (deterministic book outcomes); the low-confidence annotation is
        // only for real panels lacking a majority.
        let mut report = fixtures::full_report();
        for finding in &mut report.findings {
            finding.agreement = sp42_citation::PanelAgreement::new(0, 0);
        }
        let out = render_ga_appendix(&report, 0, "0.1.0");
        assert_eq!(out.matches(crate::copy::PANEL_SPLIT_LINE).count(), 0);
    }

    #[test]
    fn failed_book_lookups_render_as_tool_failures_not_catalog_misses() {
        // Codex round 2 (PR 154): a BookSource skip whose resolution says
        // LookupFailed is a transport failure, not a catalog miss.
        let mut report = fixtures::full_report();
        report.skipped.push(sp42_citation::SkippedRef {
            ref_id: "cite_ref-flaky_20-0".to_string(),
            reason: sp42_citation::SkippedReason::BookSource,
            block_ordinal: 3,
            book_sources: Vec::new(),
        });
        report.book_resolutions.push(sp42_citation::BookResolution {
            ref_id: "cite_ref-flaky_20-0".to_string(),
            block_ordinal: 3,
            identifiers: Vec::new(),
            cited_page: None,
            outcome: sp42_citation::BookResolutionOutcome::LookupFailed {
                message: "connect timeout".to_string(),
            },
            enrichment_candidates: Vec::new(),
        });
        let out = render_ga_appendix(&report, 0, "0.1.0");
        assert!(out.contains(crate::copy::SKIPPED_BOOK_LOOKUP_FAILED));
        assert!(
            !out.contains("no catalog record the tool could use\n* ref \"flaky"),
            "the flaky ref must not read as a catalog miss"
        );
    }

    #[test]
    fn book_skip_lines_name_their_identifiers() {
        // Codex round 4 (PR 154): two missed books under one ref stay
        // distinguishable by their identifiers.
        let mut report = fixtures::full_report();
        for isbn in ["9780306406157", "9780140328721"] {
            report.skipped.push(sp42_citation::SkippedRef {
                ref_id: "cite_ref-twobooks_30-0".to_string(),
                reason: sp42_citation::SkippedReason::BookSource,
                block_ordinal: 5,
                book_sources: vec![sp42_citation::BookSource {
                    identifiers: vec![sp42_citation::BookIdentifier::isbn(isbn).expect("valid")],
                    cited_page: Some("9".to_string()),
                }],
            });
        }
        let out = render_ga_appendix(&report, 0, "0.1.0");
        assert!(out.contains("ISBN 9780306406157"));
        assert!(out.contains("ISBN 9780140328721"));
    }

    #[test]
    fn round5_regressions_labels_punctuation_and_copy() {
        // Codex round 5 (PR 154), three regressions in one maximal render:
        let mut report = fixtures::full_report();
        // (1) unnamed skipped ref: block-anchored, never a fabricated number.
        report.skipped.push(sp42_citation::SkippedRef {
            ref_id: "cite_ref-9".to_string(),
            reason: sp42_citation::SkippedReason::NonUrlSource,
            block_ordinal: 4,
            book_sources: Vec::new(),
        });
        // (2) punctuated unnamed id in a failure reason stays block-anchored.
        report
            .extraction_failures
            .push(sp42_citation::BlockFailure {
                block_ordinal: 7,
                reason: "verify went wrong for cite_ref-64: connect timeout".to_string(),
            });
        // (3) lookup-not-completed path exercised so the pass/fail word scan
        // covers ALL copy branches (the word "failure" slipped through when
        // this path was untested by the scan).
        report.skipped.push(sp42_citation::SkippedRef {
            ref_id: "cite_ref-flaky_21-0".to_string(),
            reason: sp42_citation::SkippedReason::BookSource,
            block_ordinal: 5,
            book_sources: vec![sp42_citation::BookSource {
                identifiers: vec![
                    sp42_citation::BookIdentifier::isbn("9780306406157").expect("valid"),
                ],
                cited_page: None,
            }],
        });
        report.book_resolutions.push(sp42_citation::BookResolution {
            ref_id: "cite_ref-flaky_21-0".to_string(),
            block_ordinal: 5,
            identifiers: vec![sp42_citation::BookIdentifier::isbn("9780306406157").expect("valid")],
            cited_page: None,
            outcome: sp42_citation::BookResolutionOutcome::LookupFailed {
                message: "connect timeout".to_string(),
            },
            enrichment_candidates: Vec::new(),
        });

        let out = render_ga_appendix(&report, 0, "0.1.0");
        assert!(out.contains("an unnamed ref (block 4)"), "skip label");
        assert!(
            out.contains("an unnamed ref (block 7): connect timeout"),
            "punctuated token"
        );
        assert!(!out.contains("ref #1: connect"), "no fabricated numbering");
        // The full-output word scan, over every copy branch this render hits.
        let lower = out.to_lowercase();
        let banned = [
            "pass", "passed", "passes", "fail", "failed", "fails", "failure",
        ];
        for word in lower.split(|c: char| !c.is_ascii_alphabetic()) {
            assert!(!banned.contains(&word), "pass/fail wording leaked: {word}");
        }
    }

    #[test]
    fn book_scan_provenance_renders_page_mismatch_and_notes() {
        // Codex round 6 (PR 154): a supported book finding found on a
        // different scanned page than cited must disclose both pages; a
        // book_scan note (e.g. whole-book fallback) renders wherever carried.
        let mut report = fixtures::full_report();
        let supported_idx = report
            .findings
            .iter()
            .position(|f| {
                matches!(
                    f.verdict,
                    sp42_citation::CitationVerdict::Judged(sp42_citation::SupportLevel::Supported)
                ) && f.book_scan.is_none()
                    && f.archive_of.is_none()
                    && f.grounding_status == sp42_citation::GroundingStatus::Located
            })
            .expect("a plain supported finding");
        report.findings[supported_idx].book_scan = Some(sp42_citation::BookScanProvenance {
            ocaid: "item0001".to_string(),
            scanned_page: Some(32),
            cited_page: Some("42".to_string()),
            note: Some("cited page had no match; whole-book search".to_string()),
        });
        let out = render_ga_appendix(&report, 0, "0.1.0");
        assert!(out.contains("located on scanned page 32; the citation names p. 42"));
        assert!(out.contains("whole-book search"));
    }

    #[test]
    fn books_section_never_fabricates_ref_numbers() {
        // Codex round 6 (PR 154): unnamed book refs are block-anchored in
        // the Books consulted section, like everywhere else.
        let mut report = fixtures::full_report();
        report.book_resolutions.push(sp42_citation::BookResolution {
            ref_id: "cite_ref-9".to_string(),
            block_ordinal: 6,
            identifiers: Vec::new(),
            cited_page: None,
            outcome: sp42_citation::BookResolutionOutcome::Resolved {
                identifier: sp42_citation::BookIdentifier::isbn("9780306406157").expect("valid"),
                edition: Box::default(),
                scan: None,
            },
            enrichment_candidates: Vec::new(),
        });
        let out = render_ga_appendix(&report, 0, "0.1.0");
        let books_idx = out
            .find(crate::copy::BUCKET_BOOKS_CONSULTED)
            .expect("books section");
        assert!(out[books_idx..].contains("an unnamed ref (block 6)"));
    }

    #[test]
    fn demo_fixture_renders_the_full_appendix_shape() {
        let raw = include_str!("../../../fixtures/page_report_ga_demo.json");
        let report: sp42_citation::PageVerificationReport =
            serde_json::from_str(raw).expect("fixture parses as a saved report");
        let out = super::render_ga_appendix(&report, 1_752_300_000_000, "0.1.0");
        for heading in [
            crate::copy::BUCKET_DISAGREEMENTS,
            crate::copy::BUCKET_RECOVERED,
            crate::copy::BUCKET_DEAD_LINKS,
            crate::copy::BUCKET_UNREADABLE,
            crate::copy::BUCKET_UNCONFIRMED,
            crate::copy::BUCKET_SUPPORTED,
            crate::copy::BUCKET_SKIPPED,
            crate::copy::BUCKET_EXTRACTION_FAILURES,
        ] {
            assert!(out.contains(heading), "missing bucket: {heading}");
        }
        assert!(!out.contains("cite_ref-"), "raw id leaked from fixture");
    }

    #[test]
    fn per_source_lookup_join_distinguishes_multiple_books_in_same_ref() {
        // Codex round 3 (PR 154): a URL-less ref citing two books should
        // render each with its matching outcome, not both as failures.
        let mut report = fixtures::full_report();

        // Add two skips for the same ref, each with different book_sources.
        let isbn_1 = sp42_citation::BookIdentifier::Isbn("978-0-123-45678-9".to_string());
        let isbn_2 = sp42_citation::BookIdentifier::Isbn("978-0-987-65432-1".to_string());

        report.skipped.push(sp42_citation::SkippedRef {
            ref_id: "cite_ref-multi_books".to_string(),
            reason: sp42_citation::SkippedReason::BookSource,
            block_ordinal: 2,
            book_sources: vec![sp42_citation::BookSource {
                identifiers: vec![isbn_1.clone()],
                cited_page: None,
            }],
        });

        report.skipped.push(sp42_citation::SkippedRef {
            ref_id: "cite_ref-multi_books".to_string(),
            reason: sp42_citation::SkippedReason::BookSource,
            block_ordinal: 2,
            book_sources: vec![sp42_citation::BookSource {
                identifiers: vec![isbn_2.clone()],
                cited_page: None,
            }],
        });

        // First resolution: LookupFailed for ISBN 1
        report.book_resolutions.push(sp42_citation::BookResolution {
            ref_id: "cite_ref-multi_books".to_string(),
            block_ordinal: 2,
            identifiers: vec![isbn_1],
            cited_page: None,
            outcome: sp42_citation::BookResolutionOutcome::LookupFailed {
                message: "timeout".to_string(),
            },
            enrichment_candidates: Vec::new(),
        });

        // Second resolution: NotFound for ISBN 2
        report.book_resolutions.push(sp42_citation::BookResolution {
            ref_id: "cite_ref-multi_books".to_string(),
            block_ordinal: 2,
            identifiers: vec![isbn_2],
            cited_page: None,
            outcome: sp42_citation::BookResolutionOutcome::NotFound,
            enrichment_candidates: Vec::new(),
        });

        let out = render_ga_appendix(&report, 0, "0.1.0");

        // Both should render in the skipped section.
        assert_eq!(
            out.matches(crate::copy::SKIPPED_BOOK_LOOKUP_FAILED).count(),
            1,
            "one failure should be a tool failure"
        );
        assert_eq!(
            out.matches(crate::copy::SKIPPED_BOOK_UNRESOLVED).count(),
            1,
            "one failure should be a catalog miss"
        );
    }

    #[test]
    fn books_consulted_section_renders_resolved_editions() {
        // Codex round 3 (PR 154): book resolutions with Resolved outcomes
        // should render in a "Books consulted" section showing title and scan state.
        let mut report = fixtures::full_report();

        // Add a resolved book with an exact scan.
        let isbn = sp42_citation::BookIdentifier::Isbn("978-0-123-45678-9".to_string());
        report.book_resolutions.push(sp42_citation::BookResolution {
            ref_id: "cite_ref-book_with_scan".to_string(),
            block_ordinal: 0,
            identifiers: vec![isbn],
            cited_page: None,
            outcome: sp42_citation::BookResolutionOutcome::Resolved {
                identifier: sp42_citation::BookIdentifier::Isbn("978-0-123-45678-9".to_string()),
                edition: Box::new(sp42_citation::OpenLibraryEdition {
                    title: Some("The Great Gatsby".to_string()),
                    ..Default::default()
                }),
                scan: Some(sp42_citation::ScanAvailability {
                    exact: vec![sp42_citation::ScanItem {
                        status: "full access".to_string(),
                        item_url: "https://archive.org/details/xyz".to_string(),
                        ol_edition_id: None,
                        ocaid: Some("xyz".to_string()),
                    }],
                    similar: Vec::new(),
                }),
            },
            enrichment_candidates: Vec::new(),
        });

        let out = render_ga_appendix(&report, 0, "0.1.0");

        // Books section should render.
        assert!(out.contains(crate::copy::BUCKET_BOOKS_CONSULTED));
        // Title should be present (and escaped).
        assert!(out.contains("Great Gatsby"));
        // Scan state for exact match should be present.
        assert!(out.contains(crate::copy::BOOK_SCAN_EXACT));
    }
}
