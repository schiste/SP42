//! Composite edit scoring lives here.

use crate::errors::ScoringError;
use crate::types::{
    CompositeScore, EditEvent, EditorIdentity, ScoreWeights, ScoringConfig, ScoringContext,
    ScoringSignal, SignalContribution,
};

const LARGE_REMOVAL_THRESHOLD: i32 = -500;
const PROFANITY_MARKERS: [&str; 4] = ["fuck", "merde", "shit", "putain"];
const LINK_MARKERS: [&str; 2] = ["http://", "https://"];
const TRUSTED_TAGS: [&str; 3] = ["mw-manual-revert", "mw-rollback", "trusted"];
const REVERT_TAGS: [&str; 2] = ["mw-reverted", "mw-undo"];

/// Score an edit event from deterministic local signals.
///
/// # Errors
///
/// Returns [`ScoringError`] when the scoring configuration is internally
/// inconsistent.
pub fn score_edit(
    event: &EditEvent,
    config: &ScoringConfig,
) -> Result<CompositeScore, ScoringError> {
    score_edit_with_context(event, config, &ScoringContext::default())
}

/// Score an edit event using local signals and optional user/ML context.
///
/// # Errors
///
/// Returns [`ScoringError`] when the scoring configuration is internally
/// inconsistent.
pub fn score_edit_with_context(
    event: &EditEvent,
    config: &ScoringConfig,
    context: &ScoringContext,
) -> Result<CompositeScore, ScoringError> {
    if config.max_score < config.base_score {
        return Err(ScoringError::Computation {
            message: "max_score must be greater than or equal to base_score".to_string(),
        });
    }

    let mut total = config.base_score;
    let mut contributions = Vec::new();

    apply_editor_signal(event, &config.weights, &mut total, &mut contributions);

    if event.is_new_page {
        push_signal(
            ScoringSignal::NewPage,
            config.weights.new_page,
            None,
            &mut total,
            &mut contributions,
        );
    }

    if event.byte_delta <= LARGE_REMOVAL_THRESHOLD {
        push_signal(
            ScoringSignal::LargeContentRemoval,
            config.weights.large_content_removal,
            Some(format!("byte delta {}", event.byte_delta)),
            &mut total,
            &mut contributions,
        );
    }

    if event.is_bot {
        push_signal(
            ScoringSignal::BotLikeEdit,
            config.weights.bot_like_edit,
            None,
            &mut total,
            &mut contributions,
        );
    }

    if contains_any(event.comment.as_deref(), &PROFANITY_MARKERS) {
        push_signal(
            ScoringSignal::Profanity,
            config.weights.profanity,
            event.comment.clone(),
            &mut total,
            &mut contributions,
        );
    }

    if contains_any(event.comment.as_deref(), &LINK_MARKERS) {
        push_signal(
            ScoringSignal::LinkSpam,
            config.weights.link_spam,
            event.comment.clone(),
            &mut total,
            &mut contributions,
        );
    }

    if event
        .tags
        .iter()
        .any(|tag| contains_tag(tag, &TRUSTED_TAGS))
    {
        push_signal(
            ScoringSignal::TrustedUser,
            config.weights.trusted_user,
            Some("trusted tag detected".to_string()),
            &mut total,
            &mut contributions,
        );
    }

    if event.tags.iter().any(|tag| contains_tag(tag, &REVERT_TAGS)) {
        push_signal(
            ScoringSignal::RevertedBefore,
            config.weights.reverted_before,
            Some("revert-related tag detected".to_string()),
            &mut total,
            &mut contributions,
        );
    }

    apply_context_signals(context, &config.weights, &mut total, &mut contributions);

    total = total.clamp(0, config.max_score);

    Ok(CompositeScore {
        total,
        contributions,
    })
}

fn apply_context_signals(
    context: &ScoringContext,
    weights: &ScoreWeights,
    total: &mut i32,
    contributions: &mut Vec<SignalContribution>,
) {
    if let Some(profile) = &context.user_risk {
        let severity = profile.warning_level.severity();
        if severity > 0 {
            let scaled_weight = scale_weight(weights.warning_history, severity, 4);
            let note = Some(format!(
                "warning level {:?}, {} warning templates",
                profile.warning_level, profile.warning_count
            ));
            push_signal(
                ScoringSignal::WarningHistory,
                scaled_weight,
                note,
                total,
                contributions,
            );
        }
    }

    if let Some(liftwing_risk) = context.liftwing_risk {
        let Some(bounded_risk) = normalize_probability(liftwing_risk) else {
            return;
        };
        if bounded_risk > 0.0 {
            let percentage = format!("{:.0}", f64::from(bounded_risk) * 100.0)
                .parse::<i32>()
                .unwrap_or(0);
            let scaled_weight = scale_weight(weights.liftwing_risk, percentage, 100);
            push_signal(
                ScoringSignal::LiftWingRisk,
                scaled_weight,
                Some(format!("probability {bounded_risk:.2}")),
                total,
                contributions,
            );
        }
    }
}

fn scale_weight(weight: i32, numerator: i32, denominator: i32) -> i32 {
    if denominator <= 0 {
        return 0;
    }

    let numerator = i64::from(weight).saturating_mul(i64::from(numerator));
    let adjustment = i64::from(denominator / 2);
    let rounded = if numerator.is_negative() {
        numerator.saturating_sub(adjustment)
    } else {
        numerator.saturating_add(adjustment)
    };
    let scaled = rounded
        .saturating_div(i64::from(denominator))
        .clamp(i64::from(i32::MIN), i64::from(i32::MAX));
    i32::try_from(scaled).unwrap_or(if scaled.is_negative() {
        i32::MIN
    } else {
        i32::MAX
    })
}

fn apply_editor_signal(
    event: &EditEvent,
    weights: &ScoreWeights,
    total: &mut i32,
    contributions: &mut Vec<SignalContribution>,
) {
    if matches!(
        event.performer,
        EditorIdentity::Anonymous { .. } | EditorIdentity::Temporary { .. }
    ) {
        push_signal(
            ScoringSignal::AnonymousUser,
            weights.anonymous_user,
            None,
            total,
            contributions,
        );
    }
}

fn push_signal(
    signal: ScoringSignal,
    weight: i32,
    note: Option<String>,
    total: &mut i32,
    contributions: &mut Vec<SignalContribution>,
) {
    *total = total.saturating_add(weight);
    contributions.push(SignalContribution {
        signal,
        weight,
        note,
    });
}

fn normalize_probability(probability: f32) -> Option<f32> {
    if !probability.is_finite() {
        return None;
    }

    Some(probability.clamp(0.0, 1.0))
}

fn contains_any(haystack: Option<&str>, markers: &[&str]) -> bool {
    let Some(haystack) = haystack else {
        return false;
    };
    let lowercase = haystack.to_ascii_lowercase();
    markers.iter().any(|marker| lowercase.contains(marker))
}

fn contains_tag(tag: &str, markers: &[&str]) -> bool {
    let lowercase = tag.to_ascii_lowercase();
    markers.iter().any(|marker| lowercase.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::{score_edit, score_edit_with_context};
    use crate::types::{
        EditEvent, EditorIdentity, ScoringConfig, ScoringContext, ScoringSignal, UserRiskProfile,
        WarningLevel,
    };
    use proptest::prelude::*;

    fn sample_event() -> EditEvent {
        EditEvent {
            wiki_id: "frwiki".to_string(),
            title: "Exemple".to_string(),
            namespace: 0,
            rev_id: 42,
            old_rev_id: Some(41),
            performer: EditorIdentity::Anonymous {
                label: "192.0.2.1".to_string(),
            },
            timestamp_ms: 1_710_000_000_000,
            is_bot: false,
            is_minor: false,
            is_new_page: true,
            tags: vec!["mobile edit".to_string()],
            comment: Some("Ajout http://spam.example.test".to_string()),
            byte_delta: 200,
            is_patrolled: false,
        }
    }

    #[test]
    fn scores_multiple_positive_signals() {
        let event = sample_event();
        let score = score_edit(&event, &ScoringConfig::default()).expect("score should compute");

        assert_eq!(score.total, 65);
        assert!(
            score
                .contributions
                .iter()
                .any(|entry| entry.signal == ScoringSignal::AnonymousUser)
        );
        assert!(
            score
                .contributions
                .iter()
                .any(|entry| entry.signal == ScoringSignal::NewPage)
        );
        assert!(
            score
                .contributions
                .iter()
                .any(|entry| entry.signal == ScoringSignal::LinkSpam)
        );
    }

    #[test]
    fn bot_signal_reduces_total() {
        let mut event = sample_event();
        event.is_bot = true;

        let score = score_edit(&event, &ScoringConfig::default()).expect("score should compute");

        assert!(
            score
                .contributions
                .iter()
                .any(|entry| entry.signal == ScoringSignal::BotLikeEdit)
        );
        assert_eq!(score.total, 15);
    }

    #[test]
    fn rejects_invalid_configuration() {
        let event = sample_event();
        let config = ScoringConfig {
            base_score: 10,
            max_score: 0,
            ..ScoringConfig::default()
        };

        assert!(score_edit(&event, &config).is_err());
    }

    #[test]
    fn applies_warning_history_and_liftwing_context() {
        let event = sample_event();
        let context = ScoringContext {
            user_risk: Some(UserRiskProfile {
                warning_level: WarningLevel::Level4,
                warning_count: 3,
                has_recent_vandalism_templates: true,
            }),
            liftwing_risk: Some(0.5),
        };

        let score = score_edit_with_context(&event, &ScoringConfig::default(), &context)
            .expect("score should compute");

        assert!(
            score
                .contributions
                .iter()
                .any(|entry| entry.signal == ScoringSignal::WarningHistory)
        );
        assert!(
            score
                .contributions
                .iter()
                .any(|entry| entry.signal == ScoringSignal::LiftWingRisk)
        );
        assert_eq!(score.total, 100);
    }

    #[test]
    fn ignores_non_finite_liftwing_risk_values() {
        let event = sample_event();
        let context = ScoringContext {
            user_risk: None,
            liftwing_risk: Some(f32::NAN),
        };

        let score = score_edit_with_context(&event, &ScoringConfig::default(), &context)
            .expect("score should compute");

        assert!(
            score
                .contributions
                .iter()
                .all(|entry| entry.signal != ScoringSignal::LiftWingRisk)
        );
    }

    #[test]
    fn extreme_weights_do_not_overflow_total_or_scaling() {
        let mut event = sample_event();
        event.is_bot = true;
        event.tags = vec!["mw-reverted".to_string(), "trusted".to_string()];
        event.comment = Some("http://example.test merde".to_string());
        event.byte_delta = i32::MIN;
        let config = ScoringConfig {
            base_score: 0,
            max_score: i32::MAX,
            weights: crate::types::ScoreWeights {
                anonymous_user: i32::MAX,
                new_page: i32::MAX,
                reverted_before: i32::MAX,
                large_content_removal: i32::MAX,
                profanity: i32::MAX,
                link_spam: i32::MAX,
                trusted_user: i32::MIN,
                bot_like_edit: i32::MIN,
                liftwing_risk: i32::MAX,
                warning_history: i32::MAX,
            },
        };
        let context = ScoringContext {
            user_risk: Some(UserRiskProfile {
                warning_level: WarningLevel::Final,
                warning_count: u32::MAX,
                has_recent_vandalism_templates: true,
            }),
            liftwing_risk: Some(1.0),
        };

        let score = score_edit_with_context(&event, &config, &context)
            .expect("extreme score should compute");

        assert!(score.total >= 0);
        assert!(score.total <= config.max_score);
    }

    #[test]
    fn applies_trusted_revert_and_large_removal_signals() {
        let mut event = sample_event();
        event.is_new_page = false;
        event.comment = Some("clean edit".to_string());
        event.tags = vec!["mw-manual-revert".to_string(), "mw-reverted".to_string()];
        event.byte_delta = -900;

        let score = score_edit(&event, &ScoringConfig::default()).expect("score should compute");

        assert!(
            score
                .contributions
                .iter()
                .any(|entry| entry.signal == ScoringSignal::TrustedUser)
        );
        assert!(
            score
                .contributions
                .iter()
                .any(|entry| entry.signal == ScoringSignal::RevertedBefore)
        );
        assert!(
            score
                .contributions
                .iter()
                .any(|entry| entry.signal == ScoringSignal::LargeContentRemoval)
        );
    }

    #[test]
    fn rounds_negative_scaled_weights_symmetrically() {
        let event = sample_event();
        let context = ScoringContext {
            user_risk: Some(UserRiskProfile {
                warning_level: WarningLevel::Level1,
                warning_count: 1,
                has_recent_vandalism_templates: false,
            }),
            liftwing_risk: None,
        };
        let config = ScoringConfig {
            weights: crate::types::ScoreWeights {
                warning_history: -35,
                ..crate::types::ScoreWeights::default()
            },
            ..ScoringConfig::default()
        };

        let score =
            score_edit_with_context(&event, &config, &context).expect("score should compute");
        let warning_weight = score
            .contributions
            .iter()
            .find(|entry| entry.signal == ScoringSignal::WarningHistory)
            .map(|entry| entry.weight);

        assert_eq!(warning_weight, Some(-9));
    }

    proptest! {
        #[test]
        fn property_score_stays_within_config_bounds(
            byte_delta in -5000i32..5000,
            is_new_page in any::<bool>(),
            is_bot in any::<bool>(),
            has_link in any::<bool>(),
            has_profanity in any::<bool>(),
            max_score in 1i32..200,
        ) {
            let mut event = sample_event();
            event.byte_delta = byte_delta;
            event.is_new_page = is_new_page;
            event.is_bot = is_bot;
            event.comment = Some(match (has_link, has_profanity) {
                (true, true) => "http://example.test merde".to_string(),
                (true, false) => "http://example.test".to_string(),
                (false, true) => "merde".to_string(),
                (false, false) => "clean edit".to_string(),
            });

            let config = ScoringConfig {
                max_score,
                ..ScoringConfig::default()
            };

            let score = score_edit(&event, &config).expect("score should compute");

            prop_assert!(score.total >= config.base_score.clamp(0, config.max_score));
            prop_assert!(score.total <= config.max_score);
        }

        #[test]
        fn property_positive_signals_do_not_reduce_score(byte_delta in -5000i32..5000) {
            let mut baseline = sample_event();
            baseline.is_new_page = false;
            baseline.comment = Some("clean edit".to_string());
            baseline.byte_delta = byte_delta;

            let mut with_signal = baseline.clone();
            with_signal.comment = Some("http://example.test".to_string());

            let config = ScoringConfig::default();
            let baseline_score = score_edit(&baseline, &config).expect("baseline score should compute");
            let signal_score = score_edit(&with_signal, &config).expect("signal score should compute");

            prop_assert!(signal_score.total >= baseline_score.total);
        }

        #[test]
        fn property_negative_signals_do_not_increase_score(is_new_page in any::<bool>()) {
            let mut baseline = sample_event();
            baseline.is_bot = false;
            baseline.is_new_page = is_new_page;

            let mut with_negative_signal = baseline.clone();
            with_negative_signal.is_bot = true;

            let config = ScoringConfig::default();
            let baseline_score = score_edit(&baseline, &config).expect("baseline score should compute");
            let signal_score = score_edit(&with_negative_signal, &config).expect("signal score should compute");

            prop_assert!(signal_score.total <= baseline_score.total);
        }

        #[test]
        fn property_extreme_config_weights_never_escape_score_bounds(
            base_score in 0i32..=1_000_000,
            max_delta in 0i32..=1_000_000,
            anonymous_user in any::<i32>(),
            new_page in any::<i32>(),
            reverted_before in any::<i32>(),
            large_content_removal in any::<i32>(),
            profanity in any::<i32>(),
            link_spam in any::<i32>(),
            trusted_user in any::<i32>(),
            bot_like_edit in any::<i32>(),
            liftwing_risk in any::<i32>(),
            warning_history in any::<i32>(),
            liftwing_probability in any::<f32>(),
        ) {
            let mut event = sample_event();
            event.is_bot = true;
            event.tags = vec!["mw-reverted".to_string(), "trusted".to_string()];
            event.comment = Some("http://example.test merde".to_string());
            event.byte_delta = i32::MIN;

            let max_score = base_score.saturating_add(max_delta);
            let config = ScoringConfig {
                base_score,
                max_score,
                weights: crate::types::ScoreWeights {
                    anonymous_user,
                    new_page,
                    reverted_before,
                    large_content_removal,
                    profanity,
                    link_spam,
                    trusted_user,
                    bot_like_edit,
                    liftwing_risk,
                    warning_history,
                },
            };
            let context = ScoringContext {
                user_risk: Some(UserRiskProfile {
                    warning_level: WarningLevel::Final,
                    warning_count: u32::MAX,
                    has_recent_vandalism_templates: true,
                }),
                liftwing_risk: Some(liftwing_probability),
            };

            let score = score_edit_with_context(&event, &config, &context).expect("score should compute");

            prop_assert!(score.total >= 0);
            prop_assert!(score.total <= config.max_score);
        }

        #[test]
        fn property_total_matches_clamped_contribution_sum(
            byte_delta in -5000i32..5000,
            is_new_page in any::<bool>(),
            is_bot in any::<bool>(),
            has_link in any::<bool>(),
            has_profanity in any::<bool>(),
            trusted in any::<bool>(),
            reverted in any::<bool>(),
            max_score in 1i32..200,
        ) {
            let mut event = sample_event();
            event.byte_delta = byte_delta;
            event.is_new_page = is_new_page;
            event.is_bot = is_bot;
            event.tags = [
                trusted.then_some("trusted".to_string()),
                reverted.then_some("mw-reverted".to_string()),
            ]
            .into_iter()
            .flatten()
            .collect();
            event.comment = Some(match (has_link, has_profanity) {
                (true, true) => "http://example.test merde".to_string(),
                (true, false) => "http://example.test".to_string(),
                (false, true) => "merde".to_string(),
                (false, false) => "clean edit".to_string(),
            });

            let config = ScoringConfig {
                max_score,
                ..ScoringConfig::default()
            };

            let score = score_edit(&event, &config).expect("score should compute");
            let raw_total = config.base_score
                + score.contributions.iter().map(|entry| entry.weight).sum::<i32>();

            prop_assert_eq!(score.total, raw_total.clamp(0, config.max_score));
        }

        #[test]
        fn property_signals_are_emitted_at_most_once(
            byte_delta in -5000i32..5000,
            is_new_page in any::<bool>(),
            is_bot in any::<bool>(),
            has_link in any::<bool>(),
            has_profanity in any::<bool>(),
            trusted in any::<bool>(),
            reverted in any::<bool>(),
        ) {
            let mut event = sample_event();
            event.byte_delta = byte_delta;
            event.is_new_page = is_new_page;
            event.is_bot = is_bot;
            event.tags = [
                trusted.then_some("trusted".to_string()),
                reverted.then_some("mw-reverted".to_string()),
            ]
            .into_iter()
            .flatten()
            .collect();
            event.comment = Some(match (has_link, has_profanity) {
                (true, true) => "http://example.test merde".to_string(),
                (true, false) => "http://example.test".to_string(),
                (false, true) => "merde".to_string(),
                (false, false) => "clean edit".to_string(),
            });

            let score = score_edit(&event, &ScoringConfig::default()).expect("score should compute");
            for (index, contribution) in score.contributions.iter().enumerate() {
                prop_assert!(
                    score.contributions[index + 1..]
                        .iter()
                        .all(|candidate| candidate.signal != contribution.signal)
                );
            }
        }
    }
}
