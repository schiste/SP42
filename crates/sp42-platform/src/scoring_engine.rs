//! Composite edit scoring lives here.

use crate::errors::ScoringError;
use crate::types::{
    CompositeScore, EditEvent, ScoreWeights, ScoringCombinationRule, ScoringConfig, ScoringContext,
    ScoringSignal, SignalContribution,
};

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

    // Entity content: the wikitext heuristics and revertrisk context do not
    // apply (ADR-0016 Decisions 5/7) — a uniform base score with no signal
    // contributions keeps entity queues chronological over the bot-filtered
    // stream (PRD-0011 Q3) instead of ranking on signals that misread
    // entity JSON.
    if crate::wikibase::derive_content_model_capabilities(event.content_model.as_deref())
        .entity_diff
    {
        return Ok(CompositeScore {
            total: config.base_score.clamp(0, config.max_score),
            contributions: Vec::new(),
        });
    }

    let mut total = config.base_score;
    let mut contributions = Vec::new();
    apply_primary_edit_signals(event, config, context, &mut total, &mut contributions);
    apply_context_signals(context, &config.weights, &mut total, &mut contributions);
    apply_combination_rules(&config.combination_rules, &mut total, &mut contributions);

    total = total.clamp(0, config.max_score);

    Ok(CompositeScore {
        total,
        contributions,
    })
}

fn apply_primary_edit_signals(
    event: &EditEvent,
    config: &ScoringConfig,
    context: &ScoringContext,
    total: &mut i32,
    contributions: &mut Vec<SignalContribution>,
) {
    let mut identity_total = 0;

    apply_editor_signal(
        event,
        &config.weights,
        total,
        contributions,
        &mut identity_total,
        config,
    );
    apply_edit_shape_signals(event, config, &config.weights, total, contributions);
    apply_trusted_and_revert_signals(
        event,
        context,
        config,
        &config.weights,
        total,
        contributions,
        &mut identity_total,
    );
    apply_identity_contribution_cap(
        config.identity.contribution_cap,
        total,
        contributions,
        identity_total,
    );
}

fn apply_edit_shape_signals(
    event: &EditEvent,
    config: &ScoringConfig,
    weights: &ScoreWeights,
    total: &mut i32,
    contributions: &mut Vec<SignalContribution>,
) {
    if event.is_new_page.is_enabled() {
        push_signal(
            ScoringSignal::NewPage,
            weights.new_page,
            None,
            total,
            contributions,
        );
    }

    if event.byte_delta <= config.signal_parameters.large_content_removal_threshold {
        push_signal(
            ScoringSignal::LargeContentRemoval,
            weights.large_content_removal,
            Some(format!("byte delta {}", event.byte_delta)),
            total,
            contributions,
        );
    }

    if event.is_bot.is_enabled() {
        push_signal(
            ScoringSignal::BotLikeEdit,
            weights.bot_like_edit,
            None,
            total,
            contributions,
        );
    }

    if contains_any(
        event.comment.as_deref(),
        &config.signal_parameters.profanity_markers,
    ) {
        push_signal(
            ScoringSignal::Profanity,
            weights.profanity,
            event.comment.clone(),
            total,
            contributions,
        );
    }

    if contains_any(
        event.comment.as_deref(),
        &config.signal_parameters.link_markers,
    ) {
        push_signal(
            ScoringSignal::LinkSpam,
            weights.link_spam,
            event.comment.clone(),
            total,
            contributions,
        );
    }

    if let Some(reason) = detect_obvious_vandalism(event, config) {
        push_signal(
            ScoringSignal::ObviousVandalism,
            weights.obvious_vandalism,
            Some(reason),
            total,
            contributions,
        );
    }
}

fn apply_trusted_and_revert_signals(
    event: &EditEvent,
    context: &ScoringContext,
    config: &ScoringConfig,
    weights: &ScoreWeights,
    total: &mut i32,
    contributions: &mut Vec<SignalContribution>,
    identity_total: &mut i32,
) {
    let trusted_tag_detected = event
        .tags
        .iter()
        .any(|tag| contains_tag(tag, &config.signal_parameters.trusted_tags));
    let trusted_override_detected = context.trust_override.is_enabled();
    if trusted_tag_detected || trusted_override_detected {
        let note = match (trusted_tag_detected, trusted_override_detected) {
            (true, true) => Some("trusted tag and public trusted-user rule matched".to_string()),
            (true, false) => Some("trusted tag detected".to_string()),
            (false, true) => Some("public trusted-user rule matched".to_string()),
            (false, false) => None,
        };
        push_signal(
            ScoringSignal::TrustedUser,
            weights.trusted_user,
            note,
            total,
            contributions,
        );
        *identity_total = identity_total.saturating_add(weights.trusted_user);
    }

    if event
        .tags
        .iter()
        .any(|tag| contains_tag(tag, &config.signal_parameters.revert_tags))
    {
        push_signal(
            ScoringSignal::RevertedBefore,
            weights.reverted_before,
            Some("revert-related tag detected".to_string()),
            total,
            contributions,
        );
    }
}

fn apply_context_signals(
    context: &ScoringContext,
    weights: &ScoreWeights,
    total: &mut i32,
    contributions: &mut Vec<SignalContribution>,
) {
    apply_warning_history_signal(context, weights, total, contributions);
    apply_liftwing_signal(context, weights, total, contributions);
    apply_boolean_context_signals(context, weights, total, contributions);
    apply_duplicate_pattern_signal(context, weights, total, contributions);
}

fn apply_warning_history_signal(
    context: &ScoringContext,
    weights: &ScoreWeights,
    total: &mut i32,
    contributions: &mut Vec<SignalContribution>,
) {
    let Some(profile) = &context.user_risk else {
        return;
    };

    let severity = profile.warning_level.severity();
    if severity <= 0 {
        return;
    }

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

fn apply_liftwing_signal(
    context: &ScoringContext,
    weights: &ScoreWeights,
    total: &mut i32,
    contributions: &mut Vec<SignalContribution>,
) {
    let Some(liftwing_risk) = context.liftwing_risk else {
        return;
    };
    let Some(bounded_risk) = normalize_probability(liftwing_risk) else {
        return;
    };
    if bounded_risk <= 0.0 {
        return;
    }

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

fn apply_boolean_context_signals(
    context: &ScoringContext,
    weights: &ScoreWeights,
    total: &mut i32,
    contributions: &mut Vec<SignalContribution>,
) {
    apply_flagged_context_signal(
        context.link_addition_only.is_enabled(),
        ScoringSignal::LinkAddition,
        weights.link_addition,
        "only wikilink wrapper characters were added",
        total,
        contributions,
    );
    apply_flagged_context_signal(
        context.reference_addition_only.is_enabled(),
        ScoringSignal::ReferenceAddition,
        weights.reference_addition,
        "only references or citation templates were added",
        total,
        contributions,
    );
    apply_flagged_context_signal(
        context.category_addition_only.is_enabled(),
        ScoringSignal::CategoryAddition,
        weights.category_addition,
        "only category links were added",
        total,
        contributions,
    );
    apply_flagged_context_signal(
        context.interwiki_addition_only.is_enabled(),
        ScoringSignal::InterwikiAddition,
        weights.interwiki_addition,
        "only interwiki or language links were added",
        total,
        contributions,
    );
    apply_flagged_context_signal(
        context.mass_blanking_detected.is_enabled(),
        ScoringSignal::MassBlanking,
        weights.mass_blanking,
        "diff removed substantially more content than it inserted",
        total,
        contributions,
    );
    apply_flagged_context_signal(
        context.inserted_profanity_detected.is_enabled(),
        ScoringSignal::InsertedProfanity,
        weights.inserted_profanity,
        "inserted diff text contains configured profanity markers",
        total,
        contributions,
    );
    apply_flagged_context_signal(
        context.repeated_character_noise_detected.is_enabled(),
        ScoringSignal::RepeatedCharacterNoise,
        weights.repeated_character_noise,
        "inserted diff text contains repeated-character noise",
        total,
        contributions,
    );
}

fn apply_flagged_context_signal(
    enabled: bool,
    signal: ScoringSignal,
    weight: i32,
    note: &'static str,
    total: &mut i32,
    contributions: &mut Vec<SignalContribution>,
) {
    if !enabled {
        return;
    }

    push_signal(signal, weight, Some(note.to_string()), total, contributions);
}

fn apply_duplicate_pattern_signal(
    context: &ScoringContext,
    weights: &ScoreWeights,
    total: &mut i32,
    contributions: &mut Vec<SignalContribution>,
) {
    let Some(cluster_size) = context.duplicate_cluster_size else {
        return;
    };
    if cluster_size <= 1 {
        return;
    }

    let numerator = i32::try_from(cluster_size.saturating_sub(1)).unwrap_or(i32::MAX);
    let scaled_weight = scale_weight(weights.duplicate_pattern, numerator.min(4), 4);
    push_signal(
        ScoringSignal::DuplicatePattern,
        scaled_weight,
        Some(format!("duplicate cluster size {cluster_size}")),
        total,
        contributions,
    );
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
    identity_total: &mut i32,
    config: &ScoringConfig,
) {
    if config.identity.anonymous_modifier_enabled.is_enabled() && event.performer.is_anonymous() {
        push_signal(
            ScoringSignal::AnonymousUser,
            weights.anonymous_user,
            None,
            total,
            contributions,
        );
        *identity_total = identity_total.saturating_add(weights.anonymous_user);
    }

    if config.identity.temporary_modifier_enabled.is_enabled() && event.performer.is_temporary() {
        push_signal(
            ScoringSignal::TemporaryAccount,
            weights.temporary_account,
            Some("temporary account editors are newcomer-like but not raw IPs".to_string()),
            total,
            contributions,
        );
        *identity_total = identity_total.saturating_add(weights.temporary_account);
    }
}

fn apply_identity_contribution_cap(
    cap: Option<i32>,
    total: &mut i32,
    contributions: &mut Vec<SignalContribution>,
    identity_total: i32,
) {
    let Some(cap) = cap else {
        return;
    };
    let cap = cap.abs();
    let bounded = identity_total.clamp(-cap, cap);
    let adjustment = bounded.saturating_sub(identity_total);
    if adjustment == 0 {
        return;
    }

    push_signal(
        ScoringSignal::IdentityCapAdjustment,
        adjustment,
        Some(format!("identity contribution capped to +/- {cap}")),
        total,
        contributions,
    );
}

fn push_signal(
    signal: ScoringSignal,
    weight: i32,
    note: Option<String>,
    total: &mut i32,
    contributions: &mut Vec<SignalContribution>,
) {
    *total = total.saturating_add(weight);
    if let Some(existing) = contributions
        .iter_mut()
        .find(|entry| entry.signal == signal)
    {
        existing.weight = existing.weight.saturating_add(weight);
        existing.note = merge_notes(existing.note.take(), note);
        return;
    }
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

fn contains_any(haystack: Option<&str>, markers: &[String]) -> bool {
    let Some(haystack) = haystack else {
        return false;
    };
    let lowercase = haystack.to_ascii_lowercase();
    markers.iter().any(|marker| lowercase.contains(marker))
}

fn contains_tag(tag: &str, markers: &[String]) -> bool {
    let lowercase = tag.to_ascii_lowercase();
    markers.iter().any(|marker| lowercase.contains(marker))
}

fn detect_obvious_vandalism(event: &EditEvent, config: &ScoringConfig) -> Option<String> {
    let mut reasons = Vec::new();

    if event.byte_delta <= config.signal_parameters.massive_blanking_threshold {
        reasons.push(format!("massive blanking byte_delta={}", event.byte_delta));
    }

    if contains_any(
        event.comment.as_deref(),
        &config.signal_parameters.profanity_markers,
    ) {
        reasons.push("profanity marker in comment".to_string());
    }

    if contains_any(
        event.comment.as_deref(),
        &config.signal_parameters.link_markers,
    ) {
        reasons.push("external link marker in comment".to_string());
    }

    if contains_any(
        event.comment.as_deref(),
        &config.signal_parameters.suspicious_comment_markers,
    ) {
        reasons.push("suspicious moderation-style comment marker".to_string());
    }

    if has_repeated_character_run(
        event.comment.as_deref(),
        config.signal_parameters.repeated_character_run_threshold,
    ) || has_repeated_character_run(
        Some(&event.title),
        config.signal_parameters.repeated_character_run_threshold,
    ) {
        reasons.push("repeated-character noise detected".to_string());
    }

    if reasons.is_empty() {
        return None;
    }

    let severe_blanking = event.byte_delta <= config.signal_parameters.massive_blanking_threshold;
    let layered_signals = reasons.len() >= 2;
    if severe_blanking || layered_signals {
        Some(reasons.join("; "))
    } else {
        None
    }
}

fn has_repeated_character_run(value: Option<&str>, threshold: u8) -> bool {
    let Some(value) = value else {
        return false;
    };

    let mut last = '\0';
    let mut run = 0u8;
    for ch in value.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() && ch == last {
            run = run.saturating_add(1);
            if run >= threshold {
                return true;
            }
        } else {
            last = ch;
            run = 1;
        }
    }
    false
}

fn apply_combination_rules(
    rules: &[ScoringCombinationRule],
    total: &mut i32,
    contributions: &mut Vec<SignalContribution>,
) {
    for rule in rules {
        let all_present = rule
            .when_all
            .iter()
            .all(|signal| contributions.iter().any(|entry| entry.signal == *signal));
        if !all_present {
            continue;
        }
        push_signal(
            ScoringSignal::CombinationRule,
            rule.weight,
            Some(format!("combination rule `{}` matched", rule.slug)),
            total,
            contributions,
        );
    }
}

fn merge_notes(existing: Option<String>, incoming: Option<String>) -> Option<String> {
    match (existing, incoming) {
        (None, None) => None,
        (Some(note), None) | (None, Some(note)) => Some(note),
        (Some(existing), Some(incoming)) if existing == incoming => Some(existing),
        (Some(existing), Some(incoming)) => Some(format!("{existing}; {incoming}")),
    }
}

#[cfg(test)]
mod tests {
    use super::{score_edit, score_edit_with_context};
    use crate::types::{
        EditEvent, EditorIdentity, ScoringConfig, ScoringContext, ScoringSignal, UserRiskProfile,
        WarningLevel,
    };
    use proptest::prelude::*;

    #[test]
    fn entity_content_scores_uniform_base_with_no_signals() {
        // A vandalism-shaped event (anonymous editor, mass removal): scored
        // on wikitext, uniform base score on entity content (ADR-0016
        // Decisions 5/7 — heuristics and revertrisk do not apply, so entity
        // queues stay chronological, PRD-0011 Q3).
        let config = ScoringConfig::default();
        let mut event = sample_event();
        event.performer = EditorIdentity::Anonymous {
            label: "192.0.2.1".to_string(),
        };
        event.byte_delta = -12_000;

        let wikitext_score = score_edit(&event, &config).expect("wikitext event scores");
        assert!(!wikitext_score.contributions.is_empty());

        event.content_model = Some("wikibase-item".to_string());
        let entity_score = score_edit(&event, &config).expect("entity event scores");
        assert_eq!(entity_score.total, config.base_score);
        assert!(entity_score.contributions.is_empty());
    }

    fn sample_event() -> EditEvent {
        EditEvent {
            content_model: None,
            wiki_id: "frwiki".to_string(),
            title: "Exemple".to_string(),
            namespace: 0,
            rev_id: 42,
            old_rev_id: Some(41),
            performer: EditorIdentity::Anonymous {
                label: "192.0.2.1".to_string(),
            },
            timestamp_ms: 1_710_000_000_000,
            is_bot: false.into(),
            is_minor: false.into(),
            is_new_page: true.into(),
            tags: vec!["mobile edit".to_string()],
            comment: Some("Ajout http://spam.example.test".to_string()),
            byte_delta: 200,
            is_patrolled: false.into(),
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
        event.is_bot = true.into();

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
            ..ScoringContext::default()
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
            ..ScoringContext::default()
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
        event.is_bot = true.into();
        event.tags = vec!["mw-reverted".to_string(), "trusted".to_string()];
        event.comment = Some("http://example.test merde".to_string());
        event.byte_delta = i32::MIN;
        let config = ScoringConfig {
            base_score: 0,
            max_score: i32::MAX,
            identity: crate::types::ScoringIdentityConfig {
                contribution_cap: None,
                ..crate::types::ScoringIdentityConfig::default()
            },
            weights: crate::types::ScoreWeights {
                anonymous_user: i32::MAX,
                temporary_account: i32::MAX,
                new_page: i32::MAX,
                reverted_before: i32::MAX,
                large_content_removal: i32::MAX,
                link_addition: i32::MAX,
                reference_addition: i32::MAX,
                category_addition: i32::MAX,
                interwiki_addition: i32::MAX,
                mass_blanking: i32::MAX,
                inserted_profanity: i32::MAX,
                repeated_character_noise: i32::MAX,
                profanity: i32::MAX,
                link_spam: i32::MAX,
                trusted_user: i32::MIN,
                bot_like_edit: i32::MIN,
                liftwing_risk: i32::MAX,
                warning_history: i32::MAX,
                obvious_vandalism: i32::MAX,
                duplicate_pattern: i32::MAX,
            },
            ..ScoringConfig::default()
        };
        let context = ScoringContext {
            user_risk: Some(UserRiskProfile {
                warning_level: WarningLevel::Final,
                warning_count: u32::MAX,
                has_recent_vandalism_templates: true,
            }),
            liftwing_risk: Some(1.0),
            ..ScoringContext::default()
        };

        let score = score_edit_with_context(&event, &config, &context)
            .expect("extreme score should compute");

        assert!(score.total >= 0);
        assert!(score.total <= config.max_score);
    }

    #[test]
    fn applies_trusted_revert_and_large_removal_signals() {
        let mut event = sample_event();
        event.is_new_page = false.into();
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
    fn temporary_accounts_get_distinct_identity_signal() {
        let mut event = sample_event();
        event.performer = EditorIdentity::Temporary {
            label: "~2026-777".to_string(),
        };

        let score = score_edit(&event, &ScoringConfig::default()).expect("score should compute");

        assert!(
            score
                .contributions
                .iter()
                .any(|entry| entry.signal == ScoringSignal::TemporaryAccount)
        );
    }

    #[test]
    fn obvious_vandalism_fast_lane_requires_high_confidence_combo() {
        let mut event = sample_event();
        event.performer = EditorIdentity::Anonymous {
            label: "192.0.2.55".to_string(),
        };
        event.comment = Some("rvv vandalisme".to_string());
        event.byte_delta = -1_200;

        let score = score_edit(&event, &ScoringConfig::default()).expect("score should compute");

        let obvious = score
            .contributions
            .iter()
            .find(|entry| entry.signal == ScoringSignal::ObviousVandalism);

        assert!(obvious.is_some());
        assert!(obvious.and_then(|entry| entry.note.as_deref()).is_some());
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
            ..ScoringContext::default()
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

    #[test]
    fn applies_link_addition_signal_from_context() {
        let event = sample_event();
        let context = ScoringContext {
            link_addition_only: true.into(),
            ..ScoringContext::default()
        };

        let score = score_edit_with_context(&event, &ScoringConfig::default(), &context)
            .expect("score should compute");

        let link_addition = score
            .contributions
            .iter()
            .find(|entry| entry.signal == ScoringSignal::LinkAddition);

        assert_eq!(link_addition.map(|entry| entry.weight), Some(-12));
    }

    #[test]
    fn applies_new_diff_aware_signals_from_context() {
        let event = sample_event();
        let context = ScoringContext {
            reference_addition_only: true.into(),
            category_addition_only: true.into(),
            interwiki_addition_only: true.into(),
            mass_blanking_detected: true.into(),
            inserted_profanity_detected: true.into(),
            repeated_character_noise_detected: true.into(),
            ..ScoringContext::default()
        };

        let score = score_edit_with_context(&event, &ScoringConfig::default(), &context)
            .expect("score should compute");

        assert!(
            score
                .contributions
                .iter()
                .any(|entry| entry.signal == ScoringSignal::ReferenceAddition)
        );
        assert!(
            score
                .contributions
                .iter()
                .any(|entry| entry.signal == ScoringSignal::CategoryAddition)
        );
        assert!(
            score
                .contributions
                .iter()
                .any(|entry| entry.signal == ScoringSignal::InterwikiAddition)
        );
        assert!(
            score
                .contributions
                .iter()
                .any(|entry| entry.signal == ScoringSignal::MassBlanking)
        );
        assert!(
            score
                .contributions
                .iter()
                .any(|entry| entry.signal == ScoringSignal::InsertedProfanity)
        );
        assert!(
            score
                .contributions
                .iter()
                .any(|entry| entry.signal == ScoringSignal::RepeatedCharacterNoise)
        );
    }

    #[test]
    fn applies_identity_cap_adjustment() {
        let event = sample_event();
        let config = ScoringConfig {
            identity: crate::types::ScoringIdentityConfig {
                contribution_cap: Some(10),
                ..crate::types::ScoringIdentityConfig::default()
            },
            ..ScoringConfig::default()
        };

        let score = score_edit(&event, &config).expect("score should compute");

        assert_eq!(score.total, 50);
        assert!(
            score
                .contributions
                .iter()
                .any(|entry| entry.signal == ScoringSignal::IdentityCapAdjustment)
        );
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
            event.is_new_page = is_new_page.into();
            event.is_bot = is_bot.into();
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
            baseline.is_new_page = false.into();
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
            baseline.is_bot = false.into();
            baseline.is_new_page = is_new_page.into();

            let mut with_negative_signal = baseline.clone();
            with_negative_signal.is_bot = true.into();

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
            event.is_bot = true.into();
            event.tags = vec!["mw-reverted".to_string(), "trusted".to_string()];
            event.comment = Some("http://example.test merde".to_string());
            event.byte_delta = i32::MIN;

            let max_score = base_score.saturating_add(max_delta);
            let config = ScoringConfig {
                base_score,
                max_score,
                identity: crate::types::ScoringIdentityConfig {
                    contribution_cap: None,
                    ..crate::types::ScoringIdentityConfig::default()
                },
                weights: crate::types::ScoreWeights {
                    anonymous_user,
                    temporary_account: 0,
                    new_page,
                    reverted_before,
                    large_content_removal,
                    link_addition: 0,
                    reference_addition: 0,
                    category_addition: 0,
                    interwiki_addition: 0,
                    mass_blanking: 0,
                    inserted_profanity: 0,
                    repeated_character_noise: 0,
                    profanity,
                    link_spam,
                    trusted_user,
                    bot_like_edit,
                    liftwing_risk,
                    warning_history,
                    obvious_vandalism: 0,
                    duplicate_pattern: 0,
                },
                ..ScoringConfig::default()
            };
            let context = ScoringContext {
                user_risk: Some(UserRiskProfile {
                    warning_level: WarningLevel::Final,
                    warning_count: u32::MAX,
                    has_recent_vandalism_templates: true,
                }),
                liftwing_risk: Some(liftwing_probability),
                ..ScoringContext::default()
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
            event.is_new_page = is_new_page.into();
            event.is_bot = is_bot.into();
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
            event.is_new_page = is_new_page.into();
            event.is_bot = is_bot.into();
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
