//! Presentation helpers for a single [`sp42_core::CitationFinding`].
//!
//! These are pure functions over the finding data — severity classification for
//! problem-first ordering, and human-readable labels — so any reviewer surface
//! (the app's finding cards today, potentially the CLI report) shares one
//! definition of "which citations need attention" and how each axis reads. The
//! wasm view layer stays a thin mapping from these to badges/markup, and the
//! logic is unit-tested here on the host (the app crate's tests are wasm-gated).

use sp42_core::{
    BodyUsabilityReason, CitationFinding, CitationVerdict, GroundingStatus, PanelAgreement,
    SourceUnavailableReason, SupportLevel,
};

/// Whether the panel judged the source to back the claim (the support class:
/// `Supported` or `Partial`).
#[must_use]
pub fn is_support(verdict: CitationVerdict) -> bool {
    matches!(
        verdict,
        CitationVerdict::Judged(SupportLevel::Supported | SupportLevel::Partial)
    )
}

/// Ordering weight for problem-first sorting — lower sorts earlier. The tiers,
/// most urgent first: refuted (`NotSupported`), support whose quote could not be
/// located (unverified), a dead live link, a partial, an unusable source (a tool
/// limitation, not the article's fault), and finally a grounded `Supported`.
#[must_use]
pub fn finding_severity_rank(finding: &CitationFinding) -> u8 {
    match finding.verdict {
        CitationVerdict::Judged(SupportLevel::NotSupported) => 0,
        _ if is_support(finding.verdict)
            && finding.grounding_status == GroundingStatus::Unlocated =>
        {
            1
        }
        CitationVerdict::SourceUnavailable
            if finding.source_unavailable_reason == Some(SourceUnavailableReason::Unreachable) =>
        {
            2
        }
        CitationVerdict::Judged(SupportLevel::Partial) => 3,
        CitationVerdict::SourceUnavailable => 4,
        CitationVerdict::Judged(SupportLevel::Supported) => 5,
    }
}

/// A finding a reviewer likely needs to act on: refuted, unverified support, a
/// dead link, or a partial. Unusable sources (PDF/paywall) and grounded support
/// are not surfaced by a "problems only" filter.
#[must_use]
pub fn finding_is_problem(finding: &CitationFinding) -> bool {
    finding_severity_rank(finding) <= 3
}

/// The measured panel agreement, rendered only when it carries information — a
/// panel of at least two models (ADR-0006 §3).
#[must_use]
pub fn panel_agreement_label(agreement: PanelAgreement) -> Option<String> {
    agreement
        .is_meaningful()
        .then(|| format!("{}/{} agree", agreement.winner_votes, agreement.panel_size))
}

/// The grounding axis as a reviewer-facing caveat, orthogonal to the verdict.
/// `Unverified` is the important one: the panel claimed support but the quote was
/// not located in the source, so the support must not read as a clean pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroundingCaveat {
    /// The supporting quote located verbatim in the source.
    Located,
    /// The quote located only via the guarded fuzzy match.
    Fuzzy,
    /// The support verdict's quote did not locate — surfaced as unverified.
    Unverified,
}

impl GroundingCaveat {
    /// Reviewer-facing label for the caveat.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            GroundingCaveat::Located => "quote located in source",
            GroundingCaveat::Fuzzy => "quote located (fuzzy match)",
            GroundingCaveat::Unverified => "unverified — quote not found in source",
        }
    }

    /// `true` when the caveat warrants a caution treatment (fuzzy or unverified),
    /// as opposed to a clean located match.
    #[must_use]
    pub const fn is_caution(self) -> bool {
        matches!(self, GroundingCaveat::Fuzzy | GroundingCaveat::Unverified)
    }
}

/// The grounding caveat for a finding's status, or `None` when no supporting
/// quote is expected (`NotApplicable`) and the axis would be noise.
#[must_use]
pub fn grounding_caveat(status: GroundingStatus) -> Option<GroundingCaveat> {
    match status {
        GroundingStatus::Located => Some(GroundingCaveat::Located),
        GroundingStatus::LocatedFuzzy => Some(GroundingCaveat::Fuzzy),
        GroundingStatus::Unlocated => Some(GroundingCaveat::Unverified),
        GroundingStatus::NotApplicable => None,
    }
}

/// A human-readable reason for a `SourceUnavailable` verdict: a dead link (with
/// its HTTP status when known) or a fetched-but-unreadable body (with the
/// classifier detail). `None` for any other verdict.
#[must_use]
pub fn source_unavailable_detail(finding: &CitationFinding) -> Option<String> {
    match finding.source_unavailable_reason? {
        SourceUnavailableReason::Unreachable => {
            let status = finding
                .provenance
                .http_status
                .map_or_else(String::new, |status| format!(" (HTTP {status})"));
            Some(format!("source unavailable — dead link{status}"))
        }
        SourceUnavailableReason::Unusable => Some(format!(
            "source unavailable — {}",
            finding
                .unusable_reason
                .map_or("could not read content", body_usability_label)
        )),
    }
}

/// Plain-language rendering of a body-classifier reason (the `{:?}` debug form is
/// not reviewer-facing).
#[must_use]
pub fn body_usability_label(reason: BodyUsabilityReason) -> &'static str {
    match reason {
        BodyUsabilityReason::Ok => "usable",
        BodyUsabilityReason::JsonLdLeak => "page returned metadata, not article text",
        BodyUsabilityReason::CssLeak => "page returned a stylesheet, not article text",
        BodyUsabilityReason::AntiBotChallenge => "blocked by an anti-bot challenge",
        BodyUsabilityReason::WaybackRedirectNotice => "archive returned a redirect notice",
        BodyUsabilityReason::WaybackChrome => "archive returned only toolbar chrome",
        BodyUsabilityReason::AmazonStub => "Amazon boilerplate, not article text",
        BodyUsabilityReason::ShortBody => "body too short to verify",
        BodyUsabilityReason::PdfBody => "PDF — not machine-readable here",
        BodyUsabilityReason::ViewerShell => "JavaScript viewer shell, no readable text",
        BodyUsabilityReason::NavChromePaywall => "paywall / sign-in wall",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        GroundingCaveat, body_usability_label, finding_is_problem, finding_severity_rank,
        grounding_caveat, panel_agreement_label, source_unavailable_detail,
    };
    use sp42_core::{
        BodyUsabilityReason, CitationFinding, CitationFindingKind, CitationVerdict,
        GroundingAssertion, GroundingStatus, PanelAgreement, SourceProvenance,
        SourceUnavailableReason, SupportLevel,
    };

    fn finding(verdict: CitationVerdict, grounding: GroundingStatus) -> CitationFinding {
        CitationFinding {
            kind: CitationFindingKind::CitationVerdict,
            verdict,
            grounding_status: grounding,
            source_unavailable_reason: None,
            unusable_reason: None,
            agreement: PanelAgreement::new(3, 3),
            passage: None,
            provenance: SourceProvenance {
                url: url::Url::parse("https://example.org/a").expect("test url"),
                content_hash: "hash".to_string(),
                fetched_at: 0,
                http_status: Some(200),
            },
            grounding: GroundingAssertion::SourceFetched {
                source_hash: "hash".to_string(),
            },
            use_site_ordinal: 0,
            ref_id: "ref".to_string(),
            claim: "claim".to_string(),
            preceding_context: Vec::new(),
            archive_of: None,
            schema_version: 1,
        }
    }

    fn unreachable(http_status: Option<u16>) -> CitationFinding {
        let mut f = finding(
            CitationVerdict::SourceUnavailable,
            GroundingStatus::NotApplicable,
        );
        f.source_unavailable_reason = Some(SourceUnavailableReason::Unreachable);
        f.provenance.http_status = http_status;
        f
    }

    fn unusable(reason: Option<BodyUsabilityReason>) -> CitationFinding {
        let mut f = finding(
            CitationVerdict::SourceUnavailable,
            GroundingStatus::NotApplicable,
        );
        f.source_unavailable_reason = Some(SourceUnavailableReason::Unusable);
        f.unusable_reason = reason;
        f
    }

    #[test]
    fn severity_orders_problems_before_clean_citations() {
        let not_supported = finding(
            CitationVerdict::Judged(SupportLevel::NotSupported),
            GroundingStatus::NotApplicable,
        );
        let unverified = finding(
            CitationVerdict::Judged(SupportLevel::Supported),
            GroundingStatus::Unlocated,
        );
        let supported = finding(
            CitationVerdict::Judged(SupportLevel::Supported),
            GroundingStatus::Located,
        );

        // Refuted < unverified-support < dead link < unusable < supported.
        assert!(finding_severity_rank(&not_supported) < finding_severity_rank(&unverified));
        assert!(finding_severity_rank(&unverified) < finding_severity_rank(&unreachable(None)));
        assert!(finding_severity_rank(&unreachable(None)) < finding_severity_rank(&unusable(None)));
        assert!(finding_severity_rank(&unusable(None)) < finding_severity_rank(&supported));
    }

    #[test]
    fn problem_filter_keeps_actionable_drops_clean() {
        assert!(finding_is_problem(&finding(
            CitationVerdict::Judged(SupportLevel::NotSupported),
            GroundingStatus::NotApplicable
        )));
        // A "supported" whose quote did not locate is unverified — still a problem.
        assert!(finding_is_problem(&finding(
            CitationVerdict::Judged(SupportLevel::Supported),
            GroundingStatus::Unlocated
        )));
        assert!(finding_is_problem(&unreachable(None)));
        assert!(finding_is_problem(&finding(
            CitationVerdict::Judged(SupportLevel::Partial),
            GroundingStatus::Located
        )));

        // A grounded "supported" and a tool-limited unusable source are not.
        assert!(!finding_is_problem(&finding(
            CitationVerdict::Judged(SupportLevel::Supported),
            GroundingStatus::Located
        )));
        assert!(!finding_is_problem(&unusable(Some(
            BodyUsabilityReason::PdfBody
        ))));
    }

    #[test]
    fn agreement_shows_only_for_a_real_panel() {
        assert_eq!(
            panel_agreement_label(PanelAgreement::new(3, 2)),
            Some("2/3 agree".to_string())
        );
        // A single-model "panel" carries no agreement signal.
        assert_eq!(panel_agreement_label(PanelAgreement::new(1, 1)), None);
    }

    #[test]
    fn unlocated_support_is_flagged_unverified() {
        let caveat = grounding_caveat(GroundingStatus::Unlocated).expect("caveat present");
        assert_eq!(caveat, GroundingCaveat::Unverified);
        assert!(caveat.label().contains("unverified"));
        assert!(caveat.is_caution());
        // A located match is not a caution.
        assert!(
            !grounding_caveat(GroundingStatus::Located)
                .expect("caveat")
                .is_caution()
        );
        // No quote expected → no caveat (would be noise).
        assert!(grounding_caveat(GroundingStatus::NotApplicable).is_none());
    }

    #[test]
    fn unavailable_detail_carries_http_status_and_reason() {
        assert_eq!(
            source_unavailable_detail(&unreachable(Some(404))),
            Some("source unavailable — dead link (HTTP 404)".to_string())
        );
        let pdf = source_unavailable_detail(&unusable(Some(BodyUsabilityReason::PdfBody)))
            .expect("unusable detail");
        assert!(pdf.contains("PDF"));
        // A grounded support has nothing unavailable to report.
        assert_eq!(
            source_unavailable_detail(&finding(
                CitationVerdict::Judged(SupportLevel::Supported),
                GroundingStatus::Located
            )),
            None
        );
    }

    #[test]
    fn body_usability_label_is_plain_language() {
        assert_eq!(
            body_usability_label(BodyUsabilityReason::NavChromePaywall),
            "paywall / sign-in wall"
        );
    }
}
