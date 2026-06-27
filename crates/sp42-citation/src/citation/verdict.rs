//! Citation verdict value set (ADR-0007 §1) and its canonical wire form (ADR-0008 §6).
//!
//! Two representations are kept deliberately:
//! - [`CitationVerdict`] / [`SupportLevel`] — the **two-axis contract type**. Modeling
//!   availability and support as separate axes makes "a can't-judge outcome is never a
//!   support judgment" a property of the type, not a convention (ADR-0007 §1, Alt (g)).
//! - [`Verdict`] — the flat four-value scale the voting and parsing algorithms operate
//!   over (the internal currency). Lossless conversion goes both ways.
//!
//! Both flatten to exactly one of four `snake_case` wire strings —
//! `supported` / `partial` / `not_supported` / `source_unavailable` — defined once in
//! [`Verdict::as_wire`] / [`Verdict::from_wire`]. There is **no numeric confidence field**
//! anywhere on these types, enforced structurally (ADR-0007 §1).

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The graded support level for a citation whose source body was usable (ADR-0007 §1).
///
/// `Supported` / `Partial` are gated by the anti-fabrication invariant (ADR-0007 §5):
/// they may only be surfaced with a verbatim, in-session-locatable quote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupportLevel {
    /// The source contains all of the claim's assertions.
    Supported,
    /// The source contains only some assertions, or supports them only with hedged language.
    Partial,
    /// The source addresses the topic but contradicts the claim, or has no evidence — and
    /// also the home of "support could not be established" (there is no separate "unclear").
    NotSupported,
}

/// The two-axis categorical citation verdict (ADR-0007 §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CitationVerdict {
    /// STEP 1 passed: a usable source body was assessed, yielding a [`SupportLevel`].
    Judged(SupportLevel),
    /// STEP 1 failed: no usable source body — no support judgment is possible (abstention).
    SourceUnavailable,
}

/// The flat four-value scale the voting/parsing algorithms operate over.
///
/// This is the internal currency; [`CitationVerdict`] is the contract type. Both
/// serialize to the same four wire strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// See [`SupportLevel::Supported`].
    Supported,
    /// See [`SupportLevel::Partial`].
    Partial,
    /// See [`SupportLevel::NotSupported`].
    NotSupported,
    /// Abstention: the source could not be used (no support judgment).
    SourceUnavailable,
}

impl Verdict {
    /// All four verdict values, in the fixed order used by the vote tally (ADR-0006).
    pub const ALL: [Verdict; 4] = [
        Verdict::Supported,
        Verdict::Partial,
        Verdict::NotSupported,
        Verdict::SourceUnavailable,
    ];

    /// The canonical `snake_case` wire string for this verdict (the one source of truth).
    #[must_use]
    pub const fn as_wire(self) -> &'static str {
        match self {
            Verdict::Supported => "supported",
            Verdict::Partial => "partial",
            Verdict::NotSupported => "not_supported",
            Verdict::SourceUnavailable => "source_unavailable",
        }
    }

    /// Parse a verdict from its canonical wire string, or `None` if unrecognized.
    #[must_use]
    pub fn from_wire(value: &str) -> Option<Verdict> {
        match value {
            "supported" => Some(Verdict::Supported),
            "partial" => Some(Verdict::Partial),
            "not_supported" => Some(Verdict::NotSupported),
            "source_unavailable" => Some(Verdict::SourceUnavailable),
            _ => None,
        }
    }

    /// `true` for the support-class verdicts (`Supported` / `Partial`) that the
    /// anti-fabrication gate requires a located quote for (ADR-0007 §5).
    #[must_use]
    pub const fn is_support_class(self) -> bool {
        matches!(self, Verdict::Supported | Verdict::Partial)
    }
}

impl From<CitationVerdict> for Verdict {
    fn from(value: CitationVerdict) -> Self {
        match value {
            CitationVerdict::Judged(SupportLevel::Supported) => Verdict::Supported,
            CitationVerdict::Judged(SupportLevel::Partial) => Verdict::Partial,
            CitationVerdict::Judged(SupportLevel::NotSupported) => Verdict::NotSupported,
            CitationVerdict::SourceUnavailable => Verdict::SourceUnavailable,
        }
    }
}

impl From<Verdict> for CitationVerdict {
    fn from(value: Verdict) -> Self {
        match value {
            Verdict::Supported => CitationVerdict::Judged(SupportLevel::Supported),
            Verdict::Partial => CitationVerdict::Judged(SupportLevel::Partial),
            Verdict::NotSupported => CitationVerdict::Judged(SupportLevel::NotSupported),
            Verdict::SourceUnavailable => CitationVerdict::SourceUnavailable,
        }
    }
}

impl CitationVerdict {
    /// The canonical `snake_case` wire string (delegates to [`Verdict::as_wire`]).
    #[must_use]
    pub fn as_wire(self) -> &'static str {
        Verdict::from(self).as_wire()
    }

    /// `true` iff this verdict is a support judgment (i.e. not the abstention case).
    #[must_use]
    pub const fn is_support_judgment(self) -> bool {
        matches!(self, CitationVerdict::Judged(_))
    }
}

impl Serialize for Verdict {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_wire())
    }
}

impl<'de> Deserialize<'de> for Verdict {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Verdict::from_wire(&raw).ok_or_else(|| D::Error::custom(format!("unknown verdict {raw:?}")))
    }
}

impl Serialize for CitationVerdict {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_wire())
    }
}

impl<'de> Deserialize<'de> for CitationVerdict {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Verdict::deserialize(deserializer)?.into())
    }
}

/// The kind discriminator for the read-only Finding channel (ADR-0008 §2).
///
/// A single value today; kept an enum (not a bare marker) so the read-only Finding
/// channel can carry future informational kinds without a breaking change (Art. 9.2).
/// Serializes as `citation_verdict`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CitationFindingKind {
    /// A graded support verdict for one (claim, source) use-site.
    #[default]
    CitationVerdict,
}

#[cfg(test)]
mod tests {
    use super::{CitationFindingKind, CitationVerdict, SupportLevel, Verdict};

    #[test]
    fn verdict_wire_round_trips_all_four_values() {
        for (verdict, wire) in [
            (Verdict::Supported, "supported"),
            (Verdict::Partial, "partial"),
            (Verdict::NotSupported, "not_supported"),
            (Verdict::SourceUnavailable, "source_unavailable"),
        ] {
            let json = serde_json::to_string(&verdict).expect("serialize");
            assert_eq!(json, format!("\"{wire}\""));
            let parsed: Verdict = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(parsed, verdict);
            assert_eq!(verdict.as_wire(), wire);
            assert_eq!(Verdict::from_wire(wire), Some(verdict));
        }
    }

    #[test]
    fn citation_verdict_flattens_to_the_same_four_wire_strings() {
        for (verdict, wire) in [
            (
                CitationVerdict::Judged(SupportLevel::Supported),
                "supported",
            ),
            (CitationVerdict::Judged(SupportLevel::Partial), "partial"),
            (
                CitationVerdict::Judged(SupportLevel::NotSupported),
                "not_supported",
            ),
            (CitationVerdict::SourceUnavailable, "source_unavailable"),
        ] {
            let json = serde_json::to_string(&verdict).expect("serialize");
            assert_eq!(json, format!("\"{wire}\""));
            let parsed: CitationVerdict = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(parsed, verdict);
            assert_eq!(verdict.as_wire(), wire);
        }
    }

    #[test]
    fn abstention_never_serializes_as_a_support_level() {
        // ADR-0007 §1 / ADR-0008 contract test: a can't-judge outcome can never masquerade
        // as a support judgment on the wire.
        let json = serde_json::to_string(&CitationVerdict::SourceUnavailable).expect("serialize");
        assert_eq!(json, "\"source_unavailable\"");
        assert!(!CitationVerdict::SourceUnavailable.is_support_judgment());
        assert!(CitationVerdict::Judged(SupportLevel::NotSupported).is_support_judgment());
    }

    #[test]
    fn verdict_and_citation_verdict_convert_losslessly() {
        for verdict in Verdict::ALL {
            let round_trip: Verdict = CitationVerdict::from(verdict).into();
            assert_eq!(round_trip, verdict);
        }
    }

    #[test]
    fn unknown_wire_string_is_rejected() {
        assert!(Verdict::from_wire("banana").is_none());
        assert!(serde_json::from_str::<Verdict>("\"banana\"").is_err());
        assert!(serde_json::from_str::<CitationVerdict>("\"unclear\"").is_err());
    }

    #[test]
    fn support_class_is_exactly_supported_and_partial() {
        assert!(Verdict::Supported.is_support_class());
        assert!(Verdict::Partial.is_support_class());
        assert!(!Verdict::NotSupported.is_support_class());
        assert!(!Verdict::SourceUnavailable.is_support_class());
    }

    #[test]
    fn finding_kind_serializes_as_citation_verdict() {
        let json = serde_json::to_string(&CitationFindingKind::CitationVerdict).expect("serialize");
        assert_eq!(json, "\"citation_verdict\"");
        let parsed: CitationFindingKind = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, CitationFindingKind::CitationVerdict);
    }
}
