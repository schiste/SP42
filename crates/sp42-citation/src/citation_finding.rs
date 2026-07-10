//! Presentation helpers for a single [`crate::CitationFinding`].
//!
//! These are pure functions over the finding data — severity classification for
//! problem-first ordering, and human-readable labels — so any reviewer surface
//! (the app's finding cards today, potentially the CLI report) shares one
//! definition of "which citations need attention" and how each axis reads. The
//! wasm view layer stays a thin mapping from these to badges/markup, and the
//! logic is unit-tested here on the host (the app crate's tests are wasm-gated).

use crate::{
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

/// The triage bucket a finding belongs to, used to group the report into labelled
/// sections ordered by how actionable each is for an editor. Ordering, most to
/// least: a refuted claim, support whose quote could not be located, a partial,
/// a dead link, a source the tool could not read (a human still can), and finally
/// a confirmed `Supported`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindingGroup {
    /// The source does not back the claim.
    NotSupported,
    /// The panel judged support, but the quote did not locate *exactly* in the
    /// source — either it was not found, or only a guarded fuzzy match hit. Core
    /// treats fuzzy support as human-weighable but never groundable, so it needs
    /// review and must not sit in the clean `Supported` bucket.
    Unverified,
    /// The source partially backs the claim.
    Partial,
    /// The live source could not be fetched (link rot).
    DeadLink,
    /// The source was fetched but the tool could not read it (PDF, paywall, …);
    /// a human still can.
    Unreadable,
    /// The claim is backed, but only via an archive because the live URL is dead —
    /// the citation works yet the live link has rotted and wants repair, so it must
    /// not hide in the clean `Supported` bucket.
    VerifiedViaArchive,
    /// The source confirms the claim (quote located).
    Supported,
}

impl FindingGroup {
    /// All groups in display order (most actionable first).
    pub const ALL: [FindingGroup; 7] = [
        FindingGroup::NotSupported,
        FindingGroup::Unverified,
        FindingGroup::Partial,
        FindingGroup::DeadLink,
        FindingGroup::Unreadable,
        FindingGroup::VerifiedViaArchive,
        FindingGroup::Supported,
    ];

    /// Which group a finding belongs to.
    #[must_use]
    pub fn of(finding: &CitationFinding) -> Self {
        match finding.verdict {
            CitationVerdict::Judged(SupportLevel::NotSupported) => FindingGroup::NotSupported,
            CitationVerdict::Judged(SupportLevel::Supported | SupportLevel::Partial)
                if matches!(
                    finding.grounding_status,
                    GroundingStatus::Unlocated | GroundingStatus::LocatedFuzzy
                ) =>
            {
                FindingGroup::Unverified
            }
            CitationVerdict::Judged(SupportLevel::Partial) => FindingGroup::Partial,
            // A fully-supported claim that was only confirmed against an archive (the
            // live URL is dead) is not clean — the dead link wants repair, so pull it
            // out of the collapsed Supported bucket. (Partial-via-archive stays Partial,
            // already surfaced; its dead link is still shown on the card.)
            CitationVerdict::Judged(SupportLevel::Supported) if finding.archive_of.is_some() => {
                FindingGroup::VerifiedViaArchive
            }
            CitationVerdict::Judged(SupportLevel::Supported) => FindingGroup::Supported,
            CitationVerdict::SourceUnavailable => match finding.source_unavailable_reason {
                Some(SourceUnavailableReason::Unreachable) => FindingGroup::DeadLink,
                _ => FindingGroup::Unreadable,
            },
        }
    }

    /// Display order (0 = most actionable, sorts first).
    #[must_use]
    pub const fn order(self) -> u8 {
        match self {
            FindingGroup::NotSupported => 0,
            FindingGroup::Unverified => 1,
            FindingGroup::Partial => 2,
            FindingGroup::DeadLink => 3,
            FindingGroup::Unreadable => 4,
            FindingGroup::VerifiedViaArchive => 5,
            FindingGroup::Supported => 6,
        }
    }

    /// Section heading.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            FindingGroup::NotSupported => "Not supported",
            FindingGroup::Unverified => "Unverified",
            FindingGroup::Partial => "Partial",
            FindingGroup::DeadLink => "Dead link",
            FindingGroup::Unreadable => "Couldn't read source",
            FindingGroup::VerifiedViaArchive => "Verified via archive",
            FindingGroup::Supported => "Supported",
        }
    }

    /// A one-line editor hint on what the group means / what to do, when one helps.
    #[must_use]
    pub const fn hint(self) -> Option<&'static str> {
        match self {
            FindingGroup::Unverified => Some(
                "the panel judged support, but the quote was not located exactly (fuzzy match or not found) — weigh it by hand",
            ),
            FindingGroup::DeadLink => {
                Some("source could not be fetched — replace the link or add an archive")
            }
            FindingGroup::Unreadable => Some(
                "the tool could not extract this source (PDF, paywall, …); open it and check by hand",
            ),
            FindingGroup::VerifiedViaArchive => Some(
                "the live link is dead but an archive backs the claim — point the citation at the archive",
            ),
            _ => None,
        }
    }

    /// Whether the section starts collapsed. Only confirmed `Supported` findings
    /// need no editor attention, so everything else opens by default.
    #[must_use]
    pub const fn collapsed_by_default(self) -> bool {
        matches!(self, FindingGroup::Supported)
    }
}

/// Ordering weight for problem-first sorting — lower sorts earlier. Delegates to
/// [`FindingGroup`] so the sort and the section grouping never drift apart.
#[must_use]
pub fn finding_severity_rank(finding: &CitationFinding) -> u8 {
    FindingGroup::of(finding).order()
}

/// A finding an editor likely needs to act on: anything that is not a confirmed
/// `Supported` — including an unusable source, which a human can still read.
#[must_use]
pub fn finding_is_problem(finding: &CitationFinding) -> bool {
    !matches!(FindingGroup::of(finding), FindingGroup::Supported)
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
        FindingGroup, GroundingCaveat, body_usability_label, finding_is_problem,
        finding_severity_rank, grounding_caveat, panel_agreement_label, source_unavailable_detail,
    };
    use crate::{
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
            source_excerpt: None,
            metadata: None,
            grounding: GroundingAssertion::SourceFetched {
                source_hash: "hash".to_string(),
            },
            use_site_ordinal: 0,
            ref_id: "ref".to_string(),
            claim: "claim".to_string(),
            preceding_context: Vec::new(),
            archive_of: None,
            is_bare_url_ref: false,
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
    fn finding_group_maps_each_case() {
        use FindingGroup::{
            DeadLink, NotSupported, Partial, Supported, Unreadable, Unverified, VerifiedViaArchive,
        };
        let case = |f: &CitationFinding| FindingGroup::of(f);
        assert_eq!(
            case(&finding(
                CitationVerdict::Judged(SupportLevel::NotSupported),
                GroundingStatus::NotApplicable
            )),
            NotSupported
        );
        assert_eq!(
            case(&finding(
                CitationVerdict::Judged(SupportLevel::Supported),
                GroundingStatus::Unlocated
            )),
            Unverified
        );
        // A partial whose quote DID locate exactly stays Partial, not Unverified.
        assert_eq!(
            case(&finding(
                CitationVerdict::Judged(SupportLevel::Partial),
                GroundingStatus::Located
            )),
            Partial
        );
        // A support whose quote located only fuzzily is NOT clean: it must leave the
        // Supported bucket and surface as Unverified (fuzzy is never groundable).
        assert_eq!(
            case(&finding(
                CitationVerdict::Judged(SupportLevel::Supported),
                GroundingStatus::LocatedFuzzy
            )),
            Unverified
        );
        assert_eq!(case(&unreachable(Some(404))), DeadLink);
        assert_eq!(
            case(&unusable(Some(BodyUsabilityReason::PdfBody))),
            Unreadable
        );
        // A fully-supported claim confirmed via an archive (live URL dead) is pulled
        // out of clean Supported into its own group.
        let mut archived = finding(
            CitationVerdict::Judged(SupportLevel::Supported),
            GroundingStatus::Located,
        );
        archived.archive_of = Some(url::Url::parse("https://dead.example/live").expect("url"));
        assert_eq!(case(&archived), VerifiedViaArchive);
        assert_eq!(
            case(&finding(
                CitationVerdict::Judged(SupportLevel::Supported),
                GroundingStatus::Located
            )),
            Supported
        );
    }

    #[test]
    fn severity_orders_by_actionability() {
        let rank = |verdict, grounding| finding_severity_rank(&finding(verdict, grounding));
        // not supported < unverified < partial < dead link < unusable < supported.
        let not_supported = rank(
            CitationVerdict::Judged(SupportLevel::NotSupported),
            GroundingStatus::NotApplicable,
        );
        let unverified = rank(
            CitationVerdict::Judged(SupportLevel::Supported),
            GroundingStatus::Unlocated,
        );
        let partial = rank(
            CitationVerdict::Judged(SupportLevel::Partial),
            GroundingStatus::Located,
        );
        let supported = rank(
            CitationVerdict::Judged(SupportLevel::Supported),
            GroundingStatus::Located,
        );
        assert!(not_supported < unverified);
        assert!(unverified < partial);
        assert!(partial < finding_severity_rank(&unreachable(None)));
        assert!(finding_severity_rank(&unreachable(None)) < finding_severity_rank(&unusable(None)));
        assert!(finding_severity_rank(&unusable(None)) < supported);
    }

    #[test]
    fn only_supported_is_not_a_problem() {
        // Everything an editor might act on is a problem — including an unusable
        // source (a human can still read the PDF).
        assert!(finding_is_problem(&finding(
            CitationVerdict::Judged(SupportLevel::NotSupported),
            GroundingStatus::NotApplicable
        )));
        assert!(finding_is_problem(&unreachable(None)));
        assert!(finding_is_problem(&unusable(Some(
            BodyUsabilityReason::PdfBody
        ))));
        // A fuzzy-only support needs review too (it is not exactly grounded).
        assert!(finding_is_problem(&finding(
            CitationVerdict::Judged(SupportLevel::Supported),
            GroundingStatus::LocatedFuzzy
        )));

        // Only an exactly-grounded "supported" is a non-problem.
        assert!(!finding_is_problem(&finding(
            CitationVerdict::Judged(SupportLevel::Supported),
            GroundingStatus::Located
        )));
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
