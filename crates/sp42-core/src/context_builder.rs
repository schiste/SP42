//! Helpers for assembling scoring context from optional external sources.

use serde::{Deserialize, Serialize};

use crate::types::ScoringContext;
use crate::user_analyzer::build_user_risk_profile;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ContextInputs {
    pub talk_page_wikitext: Option<String>,
    pub liftwing_probability: Option<f32>,
}

#[must_use]
pub fn build_scoring_context(inputs: &ContextInputs) -> ScoringContext {
    ScoringContext {
        user_risk: inputs
            .talk_page_wikitext
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(build_user_risk_profile),
        liftwing_risk: normalize_liftwing_probability(inputs.liftwing_probability),
        trust_override: Default::default(),
        duplicate_cluster_size: None,
    }
}

#[must_use]
pub fn normalize_liftwing_probability(probability: Option<f32>) -> Option<f32> {
    probability.and_then(|value| value.is_finite().then(|| value.clamp(0.0, 1.0)))
}

#[cfg(test)]
mod tests {
    use super::{ContextInputs, build_scoring_context, normalize_liftwing_probability};
    use crate::types::WarningLevel;
    use proptest::prelude::*;

    #[test]
    fn builds_scoring_context_from_available_inputs() {
        let context = build_scoring_context(&ContextInputs {
            talk_page_wikitext: Some("{{Avertissement niveau 2 pour vandalisme}}".to_string()),
            liftwing_probability: Some(0.75),
        });

        assert_eq!(
            context
                .user_risk
                .expect("user risk should exist")
                .warning_level,
            WarningLevel::Level2
        );
        assert_eq!(context.liftwing_risk, Some(0.75));
    }

    #[test]
    fn treats_blank_talk_page_input_as_missing_context() {
        let context = build_scoring_context(&ContextInputs {
            talk_page_wikitext: Some("   \n\t".to_string()),
            liftwing_probability: None,
        });

        assert!(context.user_risk.is_none());
        assert!(context.liftwing_risk.is_none());
    }

    #[test]
    fn clamps_liftwing_probability_into_unit_interval() {
        assert_eq!(normalize_liftwing_probability(Some(-0.4)), Some(0.0));
        assert_eq!(normalize_liftwing_probability(Some(1.4)), Some(1.0));
    }

    #[test]
    fn rejects_non_finite_liftwing_probabilities() {
        assert_eq!(normalize_liftwing_probability(Some(f32::NAN)), None);
        assert_eq!(normalize_liftwing_probability(Some(f32::INFINITY)), None);
        assert_eq!(
            normalize_liftwing_probability(Some(f32::NEG_INFINITY)),
            None
        );
    }

    proptest! {
        #[test]
        fn property_normalized_liftwing_probability_is_idempotent(value in -10_000.0f32..10_000.0f32) {
            let normalized = normalize_liftwing_probability(Some(value)).expect("finite values should normalize");

            prop_assert!((0.0..=1.0).contains(&normalized));
            prop_assert_eq!(
                normalize_liftwing_probability(Some(normalized)),
                Some(normalized)
            );
        }
    }
}
