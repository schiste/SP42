//! Verdict reasons — ADR-0031.
//!
//! A reason is a **static, machine-legible code** plus **data** parameters. The
//! human-readable text is rendered from a per-project, per-language catalog
//! keyed by that same code, and is deliberately *not* stored on the verdict.
//!
//! Why the split is load-bearing rather than stylistic: ADR-0028 §5 and
//! ADR-0029 §5 both make a reason a fail-closed policy gate — a reason matching
//! `disallowed_reasons` is rejected, not logged. For that check to be
//! enforceable it has to be decidable mechanically, and it has to mean the same
//! thing on every wiki. Matching rendered prose would make the blocklist hold
//! only on the wiki whose language it was written in, and defeated by rewording.
//!
//! So: [`ReasonCode`] carries the policy meaning, and it cannot be built by
//! runtime string concatenation. That is what lets the blocklist be checked at
//! config-lint time against a statically declared per-ruleset vocabulary.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::errors::ReasonError;

/// A policy-legible reason identifier, declared statically by a ruleset.
///
/// Opaque on purpose. It is a newtype rather than a `String` so a code cannot be
/// assembled at runtime from a format string or a concatenation — that
/// possibility is exactly what would make the blocklist undecidable at lint
/// time, and ADR-0031 §2 rules it out by requiring codes to be declared on the
/// outcome arm rather than computed.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ReasonCode(String);

impl ReasonCode {
    /// Declare a code. Intended for config parsing and ruleset compilation, not
    /// for building a code from runtime data.
    pub fn declared(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// The code's stable spelling, as it appears in configs and catalogs.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether `name` is a syntactically usable code.
    ///
    /// Deliberately permissive about shape — the real constraint is membership
    /// of a ruleset's declared vocabulary, which only the linter and the catalog
    /// loader can know.
    ///
    /// # Errors
    ///
    /// Returns [`ReasonError::EmptyCode`] for an empty name, or
    /// [`ReasonError::MalformedCode`] when it is not lower-case `snake_case`
    /// (`a-z`, `0-9`, `_`).
    pub fn validate_name(name: &str) -> Result<(), ReasonError> {
        if name.is_empty() {
            return Err(ReasonError::EmptyCode);
        }
        if !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            return Err(ReasonError::MalformedCode {
                code: name.to_string(),
            });
        }
        Ok(())
    }
}

impl std::fmt::Display for ReasonCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// One substituted value in a reason's rendered text.
///
/// ADR-0031 §5 requires these to be *data* — a page id, a count, a namespace,
/// a threshold — and never a fragment of prose. Keeping the variants typed is
/// what lets a template own all grammar and word order, so a language with
/// different syntax needs a translated template and nothing else.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasonParam {
    /// A wiki-relative name: a page title, a namespace label, a category.
    Text(String),
    /// A count, rendered with the catalog's own numeral conventions.
    Count(i64),
    /// A page id, rendered as a number.
    PageId(i64),
    /// Epoch milliseconds. Rendered by the catalog template, not here, so the
    /// date format stays a localization concern rather than a code concern.
    TimestampMs(i64),
}

impl ReasonParam {
    /// The template placeholder this value fills, for a bare name.
    #[must_use]
    pub fn placeholder(name: impl Into<String>) -> String {
        format!("{{{}}}", name.into())
    }

    /// A stable, locale-independent rendering for diagnostics and for the
    /// `String` case only. Numbers and timestamps deliberately render bare here;
    /// a template supplies the localized formatting.
    #[must_use]
    pub fn diagnostic_value(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Count(value) | Self::PageId(value) | Self::TimestampMs(value) => {
                value.to_string()
            }
        }
    }
}

/// The parameters bound to a reason's template placeholders.
///
/// `BTreeMap` rather than `HashMap` so iteration order is deterministic —
/// CONSTITUTION §1.4 asks for explicit ordering wherever order can reach an
/// output, and a rendered reason is an output.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ReasonParams(BTreeMap<String, ReasonParam>);

impl ReasonParams {
    /// An empty parameter set, for a reason whose text needs no substitution.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Bind one placeholder. Chains, so a set reads as one expression.
    #[must_use]
    pub fn with(mut self, name: impl Into<String>, value: ReasonParam) -> Self {
        self.0.insert(name.into(), value);
        self
    }

    /// Look up a bound value.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&ReasonParam> {
        self.0.get(name)
    }

    /// Every bound placeholder, in deterministic order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &ReasonParam)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Whether nothing is bound.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// A reason attached to a verdict.
///
/// One sentence of justification, as a code plus its data. Never prose: the
/// text lives in the catalog so a verdict recorded on one wiki's language stays
/// meaningful to a reader of another (ADR-0031 §1, §4).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Reason {
    /// The policy-legible identity, and what `disallowed_reasons` matches on.
    pub code: ReasonCode,
    /// The data the catalog's template substitutes.
    #[serde(default)]
    pub params: ReasonParams,
}

impl Reason {
    /// A reason with no substituted values.
    #[must_use]
    pub fn new(code: ReasonCode) -> Self {
        Self {
            code,
            params: ReasonParams::empty(),
        }
    }

    /// A reason with substituted values.
    #[must_use]
    pub fn with_params(code: ReasonCode, params: ReasonParams) -> Self {
        Self { code, params }
    }

    /// Whether this reason's code is in a blocklist.
    ///
    /// The whole of ADR-0031 §2-§3 in one function: because `code` is static and
    /// declared per ruleset arm, the blocklist check is decidable from config
    /// alone and needs no evaluation-time string handling. Rendered text is
    /// never consulted, which is what makes the check language-independent.
    #[must_use]
    pub fn is_disallowed(&self, disallowed: &std::collections::BTreeSet<ReasonCode>) -> bool {
        disallowed.contains(&self.code)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{Reason, ReasonCode, ReasonError, ReasonParam, ReasonParams};

    fn code(name: &str) -> ReasonCode {
        ReasonCode::declared(name)
    }

    #[test]
    fn code_names_are_validated() {
        assert!(ReasonCode::validate_name("insufficient_sources").is_ok());
        assert!(ReasonCode::validate_name("csd_g4_recreated").is_ok());
        assert!(matches!(
            ReasonCode::validate_name(""),
            Err(ReasonError::EmptyCode)
        ));
        assert!(matches!(
            ReasonCode::validate_name("Not Snake"),
            Err(ReasonError::MalformedCode { .. })
        ));
    }

    #[test]
    fn params_iterate_in_deterministic_order() {
        // BTreeMap, not HashMap: CONSTITUTION §1.4 wants ordering to be a
        // property of the type rather than of the run.
        let params = ReasonParams::empty()
            .with("zebra", ReasonParam::Count(1))
            .with("alpha", ReasonParam::Text("x".into()));
        let names: Vec<&str> = params.iter().map(|(k, _)| k).collect();
        assert_eq!(names, vec!["alpha", "zebra"]);
    }

    #[test]
    fn placeholder_rendering_is_braced() {
        assert_eq!(ReasonParam::placeholder("page"), "{page}");
    }

    #[test]
    fn diagnostic_value_is_locale_independent() {
        assert_eq!(ReasonParam::Text("Ada".into()).diagnostic_value(), "Ada");
        assert_eq!(ReasonParam::Count(-3).diagnostic_value(), "-3");
        assert_eq!(ReasonParam::PageId(4242).diagnostic_value(), "4242");
        // Timestamps render bare here on purpose: date formatting belongs to the
        // catalog template, so it stays a localization decision.
        assert_eq!(
            ReasonParam::TimestampMs(1_700_000_000_000).diagnostic_value(),
            "1700000000000"
        );
    }

    #[test]
    fn disallowed_matches_on_code_not_text() {
        let blocked: BTreeSet<ReasonCode> = BTreeSet::from([code("citation_formatting_complaint")]);
        let reason = Reason::with_params(
            code("citation_formatting_complaint"),
            ReasonParams::empty().with("detail", ReasonParam::Text("whatever".into())),
        );
        assert!(
            reason.is_disallowed(&blocked),
            "a blocked code is blocked regardless of its parameters"
        );

        let other = Reason::new(code("insufficient_sources"));
        assert!(!other.is_disallowed(&blocked));
    }

    #[test]
    fn reason_round_trips_through_serde_with_code_as_a_bare_string() {
        let reason = Reason::with_params(
            code("notability_failed"),
            ReasonParams::empty().with("threshold", ReasonParam::Count(3)),
        );
        let json = serde_json::to_string(&reason).expect("serializes");
        assert!(
            json.contains("\"notability_failed\""),
            "the code stays a readable string on the wire: {json}"
        );
        let back: Reason = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(back, reason);
    }

    #[test]
    fn params_default_to_empty_for_an_absent_field() {
        // Additive and back-compatible, per the house rule in sp42-platform.
        let reason: Reason = serde_json::from_str(r#"{"code":"g4_recreated"}"#).expect("decodes");
        assert!(reason.params.is_empty());
    }
}
