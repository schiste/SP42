//! Measured ensemble agreement — the honest, observed replacement for a
//! model-emitted confidence number (ADR-0006 §2/§3).
//!
//! A panel of independent models each produces a [`Verdict`]; a pure vote selects the
//! most-voted value, with ties at the maximum broken by a **skeptical** tiebreaker that
//! never resolves *up* to `Supported` (ADR-0006 §2). The agreement carried alongside is
//! [`PanelAgreement`] — measured vote counts, never a model-reported number, and the
//! derived fraction is computed at display, never stored (ADR-0006 §3, ADR-0008 §6).
//!
//! `n_class_vote` / `binary_vote` return `None` for an empty panel — a vote needs
//! voters. (Rust's idiomatic "no value"; the wikiharness original throws.)

use serde::{Deserialize, Serialize};

use super::verdict::Verdict;

/// Measured agreement among a panel's independent votes (ADR-0006 §3, ADR-0008 §2).
///
/// Carries only the observed counts; the fraction `winner_votes / panel_size` is
/// derived at the display layer and never serialized (no numeric-confidence field on
/// the wire, ADR-0008 §6). Meaningful only for `panel_size >= 2`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelAgreement {
    /// The number of models that voted.
    pub panel_size: u8,
    /// How many of them backed the winning value (the measured agreement count).
    pub winner_votes: u8,
}

impl PanelAgreement {
    /// Construct measured agreement from raw counts.
    #[must_use]
    pub const fn new(panel_size: u8, winner_votes: u8) -> Self {
        Self {
            panel_size,
            winner_votes,
        }
    }

    /// The derived agreement fraction `winner_votes / panel_size` (0.0 for an empty
    /// panel). For display only — never persisted (ADR-0006 §3).
    #[must_use]
    pub fn fraction(self) -> f64 {
        if self.panel_size == 0 {
            0.0
        } else {
            f64::from(self.winner_votes) / f64::from(self.panel_size)
        }
    }

    /// `true` when the agreement signal is meaningful — a panel of at least two
    /// models (ADR-0006 §3).
    #[must_use]
    pub const fn is_meaningful(self) -> bool {
        self.panel_size >= 2
    }
}

/// The result of an n-class vote over a panel (ADR-0006 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NClassVote {
    /// The elected verdict (plurality, skeptical tiebreaker on a tie at the maximum).
    pub winner: Verdict,
    /// Measured agreement backing the winner.
    pub agreement: PanelAgreement,
    /// Per-verdict tally, indexed in [`Verdict::ALL`] order.
    pub counts: [usize; 4],
}

impl NClassVote {
    /// The number of votes cast for `verdict`.
    #[must_use]
    pub fn count(&self, verdict: Verdict) -> usize {
        self.counts[verdict_index(verdict)]
    }
}

/// The result of a binary support/no-support vote (ADR-0006 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BinaryVote {
    /// `true` iff a strict majority of the panel landed in the support class
    /// (`Supported` / `Partial`). A tie is **not** a majority (the skeptical default).
    pub positive: bool,
    /// Measured agreement backing the chosen side.
    pub agreement: PanelAgreement,
}

/// Index of a verdict within the fixed [`Verdict::ALL`] tally order.
const fn verdict_index(verdict: Verdict) -> usize {
    match verdict {
        Verdict::Supported => 0,
        Verdict::Partial => 1,
        Verdict::NotSupported => 2,
        Verdict::SourceUnavailable => 3,
    }
}

/// The skeptical tiebreaker rank — higher wins a tie at the maximum count, and the
/// ordering never lets a tie resolve *up* to `Supported` (ADR-0006 §2).
///
/// `Partial > NotSupported > SourceUnavailable > Supported`.
const fn tiebreaker_rank(verdict: Verdict) -> u8 {
    match verdict {
        Verdict::Partial => 4,
        Verdict::NotSupported => 3,
        Verdict::SourceUnavailable => 2,
        Verdict::Supported => 1,
    }
}

/// Tally a panel's verdicts and elect the plurality winner, breaking ties at the
/// maximum with the skeptical tiebreaker. Returns `None` for an empty panel.
#[must_use]
pub fn n_class_vote(verdicts: &[Verdict]) -> Option<NClassVote> {
    if verdicts.is_empty() {
        return None;
    }

    let mut counts = [0usize; 4];
    for &verdict in verdicts {
        counts[verdict_index(verdict)] += 1;
    }
    let max_count = counts.iter().copied().max().unwrap_or(0);

    let mut winner: Option<Verdict> = None;
    for candidate in Verdict::ALL {
        if counts[verdict_index(candidate)] != max_count {
            continue;
        }
        match winner {
            None => winner = Some(candidate),
            Some(current) if tiebreaker_rank(candidate) > tiebreaker_rank(current) => {
                winner = Some(candidate);
            }
            Some(_) => {}
        }
    }
    let winner = winner?;

    Some(NClassVote {
        winner,
        agreement: PanelAgreement::new(
            u8::try_from(verdicts.len()).unwrap_or(u8::MAX),
            u8::try_from(max_count).unwrap_or(u8::MAX),
        ),
        counts,
    })
}

/// Collapse a panel to a binary support/no-support decision by strict majority of the
/// support class (`Supported` / `Partial`). A tie is negative (skeptical). Returns
/// `None` for an empty panel.
#[must_use]
pub fn binary_vote(verdicts: &[Verdict]) -> Option<BinaryVote> {
    if verdicts.is_empty() {
        return None;
    }

    let support_count = verdicts.iter().filter(|v| v.is_support_class()).count();
    let panel = verdicts.len();
    let positive = support_count > panel / 2;
    let backing = if positive {
        support_count
    } else {
        panel - support_count
    };

    Some(BinaryVote {
        positive,
        agreement: PanelAgreement::new(
            u8::try_from(panel).unwrap_or(u8::MAX),
            u8::try_from(backing).unwrap_or(u8::MAX),
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::{PanelAgreement, binary_vote, n_class_vote};
    use crate::citation::verdict::Verdict;

    #[test]
    fn unanimous_panel_has_full_agreement() {
        let vote = n_class_vote(&[Verdict::Supported, Verdict::Supported, Verdict::Supported])
            .expect("non-empty");
        assert_eq!(vote.winner, Verdict::Supported);
        assert_eq!(vote.agreement, PanelAgreement::new(3, 3));
        assert!((vote.agreement.fraction() - 1.0).abs() < f64::EPSILON);
        assert_eq!(vote.count(Verdict::Supported), 3);
    }

    #[test]
    fn clear_plurality_wins_with_measured_fraction() {
        let vote = n_class_vote(&[
            Verdict::Supported,
            Verdict::Supported,
            Verdict::NotSupported,
        ])
        .expect("non-empty");
        assert_eq!(vote.winner, Verdict::Supported);
        assert_eq!(vote.agreement, PanelAgreement::new(3, 2));
        assert!((vote.agreement.fraction() - 2.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn tie_never_resolves_up_to_supported() {
        let vote = n_class_vote(&[Verdict::Supported, Verdict::NotSupported]).expect("non-empty");
        assert_eq!(vote.winner, Verdict::NotSupported);
        assert_eq!(vote.agreement, PanelAgreement::new(2, 1));
    }

    #[test]
    fn tie_prefers_partial_over_not_supported() {
        let vote = n_class_vote(&[Verdict::Partial, Verdict::NotSupported]).expect("non-empty");
        assert_eq!(vote.winner, Verdict::Partial);
    }

    #[test]
    fn empty_panel_has_no_vote() {
        assert!(n_class_vote(&[]).is_none());
        assert!(binary_vote(&[]).is_none());
    }

    #[test]
    fn binary_majority_support_is_positive() {
        let vote = binary_vote(&[Verdict::Supported, Verdict::Partial, Verdict::NotSupported])
            .expect("non-empty");
        assert!(vote.positive);
        assert_eq!(vote.agreement, PanelAgreement::new(3, 2));
    }

    #[test]
    fn binary_no_support_is_negative_with_full_agreement() {
        let vote = n_class_no_support();
        assert!(!vote.positive);
        assert_eq!(vote.agreement, PanelAgreement::new(3, 3));
    }

    fn n_class_no_support() -> super::BinaryVote {
        binary_vote(&[
            Verdict::NotSupported,
            Verdict::NotSupported,
            Verdict::SourceUnavailable,
        ])
        .expect("non-empty")
    }

    #[test]
    fn binary_tie_is_negative_skeptical() {
        let vote = binary_vote(&[Verdict::Supported, Verdict::NotSupported]).expect("non-empty");
        assert!(!vote.positive);
        assert_eq!(vote.agreement, PanelAgreement::new(2, 1));
    }

    #[test]
    fn agreement_is_meaningful_only_for_two_or_more() {
        assert!(!PanelAgreement::new(1, 1).is_meaningful());
        assert!(PanelAgreement::new(2, 1).is_meaningful());
    }
}
