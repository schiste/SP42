//! Helpers for building ranked patrol queues from edit events.

use crate::errors::ScoringError;
use crate::priority_queue::PriorityQueue;
use crate::scoring_engine::{score_edit, score_edit_with_context};
use crate::types::{EditEvent, QueueHeuristicPolicy, QueuedEdit, ScoringConfig, ScoringContext};
use std::collections::{BTreeMap, BTreeSet};

/// Score a batch of edit events and return them in patrol priority order.
///
/// # Errors
///
/// Returns [`ScoringError`] when any event cannot be scored.
pub fn build_ranked_queue<I>(
    events: I,
    scoring_config: &ScoringConfig,
) -> Result<Vec<QueuedEdit>, ScoringError>
where
    I: IntoIterator<Item = EditEvent>,
{
    build_ranked_queue_with_policy(events, scoring_config, &QueueHeuristicPolicy::default())
}

/// Score a batch of edit events with contextual inputs and return them in
/// patrol priority order.
///
/// # Errors
///
/// Returns [`ScoringError`] when any event cannot be scored.
pub fn build_ranked_queue_with_contexts<I>(
    events: I,
    scoring_config: &ScoringConfig,
) -> Result<Vec<QueuedEdit>, ScoringError>
where
    I: IntoIterator<Item = (EditEvent, ScoringContext)>,
{
    let mut queue = PriorityQueue::new();

    for (event, context) in events {
        let score = score_edit_with_context(&event, scoring_config, &context)?;
        queue.push(score.total, QueuedEdit { event, score });
    }

    let mut ranked = Vec::with_capacity(queue.len());
    while let Some(item) = queue.pop() {
        ranked.push(item);
    }

    Ok(ranked)
}

/// Score a batch of edit events with queue-level heuristics and return them in
/// patrol priority order.
///
/// # Errors
///
/// Returns [`ScoringError`] when any event cannot be scored.
pub fn build_ranked_queue_with_policy<I>(
    events: I,
    scoring_config: &ScoringConfig,
    policy: &QueueHeuristicPolicy,
) -> Result<Vec<QueuedEdit>, ScoringError>
where
    I: IntoIterator<Item = EditEvent>,
{
    let events: Vec<EditEvent> = events.into_iter().collect();
    let duplicate_clusters = duplicate_cluster_sizes(&events);
    let trusted_usernames = normalized_trusted_usernames(policy);
    let mut queue = PriorityQueue::new();

    for event in events {
        let trust_override = event.performer.is_registered()
            && trusted_usernames.contains(&normalize_identifier(event.performer.stable_label()));
        let duplicate_cluster_size = if policy.duplicate_cluster_boost.is_enabled() {
            duplicate_fingerprint(&event)
                .and_then(|fingerprint| duplicate_clusters.get(&fingerprint).copied())
        } else {
            None
        };
        let context = ScoringContext {
            trust_override: trust_override.into(),
            duplicate_cluster_size,
            ..ScoringContext::default()
        };
        let score =
            if context.trust_override.is_enabled() || context.duplicate_cluster_size.is_some() {
                score_edit_with_context(&event, scoring_config, &context)?
            } else {
                score_edit(&event, scoring_config)?
            };
        queue.push(score.total, QueuedEdit { event, score });
    }

    let mut ranked = Vec::with_capacity(queue.len());
    while let Some(item) = queue.pop() {
        ranked.push(item);
    }

    Ok(ranked)
}

fn normalized_trusted_usernames(policy: &QueueHeuristicPolicy) -> BTreeSet<String> {
    policy
        .trusted_usernames
        .iter()
        .map(|value| normalize_identifier(value))
        .collect()
}

fn duplicate_cluster_sizes(events: &[EditEvent]) -> BTreeMap<String, u32> {
    let mut counts = BTreeMap::new();
    for event in events {
        let Some(fingerprint) = duplicate_fingerprint(event) else {
            continue;
        };
        let count = counts.entry(fingerprint).or_insert(0u32);
        *count = count.saturating_add(1);
    }
    counts
}

fn duplicate_fingerprint(event: &EditEvent) -> Option<String> {
    if !event.performer.is_newcomer_like() {
        return None;
    }

    let comment = normalize_identifier(event.comment.as_deref().unwrap_or("no-comment"));
    let title = normalize_identifier(&event.title);
    Some(format!(
        "{}|{}|{}|{}",
        title,
        comment,
        event.namespace,
        event.byte_delta.signum()
    ))
}

fn normalize_identifier(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::{
        build_ranked_queue, build_ranked_queue_with_contexts, build_ranked_queue_with_policy,
    };
    use crate::types::{
        EditEvent, EditorIdentity, QueueHeuristicPolicy, ScoringConfig, ScoringContext,
        ScoringSignal, UserRiskProfile, WarningLevel,
    };

    #[test]
    fn ranks_highest_score_first() {
        let events = vec![
            EditEvent {
                wiki_id: "frwiki".to_string(),
                title: "Trusted".to_string(),
                namespace: 0,
                rev_id: 1,
                old_rev_id: Some(0),
                performer: EditorIdentity::Registered {
                    username: "TrustedUser".to_string(),
                },
                timestamp_ms: 1_710_000_000_000,
                is_bot: false.into(),
                is_minor: true.into(),
                is_new_page: false.into(),
                tags: vec!["trusted".to_string()],
                comment: Some("cleanup".to_string()),
                byte_delta: 5,
                is_patrolled: false.into(),
            },
            EditEvent {
                wiki_id: "frwiki".to_string(),
                title: "Risky".to_string(),
                namespace: 0,
                rev_id: 2,
                old_rev_id: Some(1),
                performer: EditorIdentity::Anonymous {
                    label: "192.0.2.1".to_string(),
                },
                timestamp_ms: 1_710_000_000_001,
                is_bot: false.into(),
                is_minor: false.into(),
                is_new_page: true.into(),
                tags: vec![],
                comment: Some("http://spam.example".to_string()),
                byte_delta: 250,
                is_patrolled: false.into(),
            },
        ];

        let ranked = build_ranked_queue(events, &ScoringConfig::default())
            .expect("ranked queue should build");

        assert_eq!(ranked[0].event.rev_id, 2);
        assert_eq!(ranked[1].event.rev_id, 1);
    }

    #[test]
    fn applies_contextual_scores_when_ranking() {
        let events = vec![
            (
                EditEvent {
                    wiki_id: "frwiki".to_string(),
                    title: "Without context".to_string(),
                    namespace: 0,
                    rev_id: 1,
                    old_rev_id: Some(0),
                    performer: EditorIdentity::Registered {
                        username: "TrustedUser".to_string(),
                    },
                    timestamp_ms: 1_710_000_000_000,
                    is_bot: false.into(),
                    is_minor: true.into(),
                    is_new_page: false.into(),
                    tags: vec![],
                    comment: Some("cleanup".to_string()),
                    byte_delta: 5,
                    is_patrolled: false.into(),
                },
                ScoringContext::default(),
            ),
            (
                EditEvent {
                    wiki_id: "frwiki".to_string(),
                    title: "With context".to_string(),
                    namespace: 0,
                    rev_id: 2,
                    old_rev_id: Some(1),
                    performer: EditorIdentity::Registered {
                        username: "RiskyUser".to_string(),
                    },
                    timestamp_ms: 1_710_000_000_100,
                    is_bot: false.into(),
                    is_minor: false.into(),
                    is_new_page: false.into(),
                    tags: vec![],
                    comment: Some("blanking".to_string()),
                    byte_delta: -20,
                    is_patrolled: false.into(),
                },
                ScoringContext {
                    user_risk: Some(UserRiskProfile {
                        warning_level: WarningLevel::Level4,
                        warning_count: 4,
                        has_recent_vandalism_templates: true,
                    }),
                    liftwing_risk: Some(0.7),
                    ..ScoringContext::default()
                },
            ),
        ];

        let ranked = build_ranked_queue_with_contexts(events, &ScoringConfig::default())
            .expect("contextual queue should build");

        assert_eq!(ranked[0].event.rev_id, 2);
        assert!(ranked[0].score.total > ranked[1].score.total);
    }

    #[test]
    fn trusted_user_policy_suppresses_registered_editor() {
        let events = vec![
            EditEvent {
                wiki_id: "frwiki".to_string(),
                title: "Trusted".to_string(),
                namespace: 0,
                rev_id: 10,
                old_rev_id: Some(9),
                performer: EditorIdentity::Registered {
                    username: "TrustedUser".to_string(),
                },
                timestamp_ms: 1_710_000_000_000,
                is_bot: false.into(),
                is_minor: false.into(),
                is_new_page: true.into(),
                tags: vec![],
                comment: Some("sincere cleanup".to_string()),
                byte_delta: 50,
                is_patrolled: false.into(),
            },
            EditEvent {
                wiki_id: "frwiki".to_string(),
                title: "Risky".to_string(),
                namespace: 0,
                rev_id: 11,
                old_rev_id: Some(10),
                performer: EditorIdentity::Anonymous {
                    label: "192.0.2.10".to_string(),
                },
                timestamp_ms: 1_710_000_000_100,
                is_bot: false.into(),
                is_minor: false.into(),
                is_new_page: false.into(),
                tags: vec![],
                comment: Some("rvv vandalisme".to_string()),
                byte_delta: -1_200,
                is_patrolled: false.into(),
            },
        ];

        let ranked = build_ranked_queue_with_policy(
            events,
            &ScoringConfig::default(),
            &QueueHeuristicPolicy {
                trusted_usernames: vec!["TrustedUser".to_string()],
                ..QueueHeuristicPolicy::default()
            },
        )
        .expect("queue should build");

        assert_eq!(ranked[0].event.rev_id, 11);
        assert!(
            ranked[1]
                .score
                .contributions
                .iter()
                .any(|entry| entry.signal == ScoringSignal::TrustedUser)
        );
    }

    #[test]
    fn duplicate_cluster_boost_applies_to_newcomer_patterns() {
        let events = vec![
            EditEvent {
                wiki_id: "frwiki".to_string(),
                title: "Same".to_string(),
                namespace: 0,
                rev_id: 20,
                old_rev_id: Some(19),
                performer: EditorIdentity::Temporary {
                    label: "~2026-123".to_string(),
                },
                timestamp_ms: 1_710_000_000_000,
                is_bot: false.into(),
                is_minor: false.into(),
                is_new_page: false.into(),
                tags: vec![],
                comment: Some("spam link".to_string()),
                byte_delta: 200,
                is_patrolled: false.into(),
            },
            EditEvent {
                wiki_id: "frwiki".to_string(),
                title: "Same".to_string(),
                namespace: 0,
                rev_id: 21,
                old_rev_id: Some(20),
                performer: EditorIdentity::Temporary {
                    label: "~2026-456".to_string(),
                },
                timestamp_ms: 1_710_000_000_001,
                is_bot: false.into(),
                is_minor: false.into(),
                is_new_page: false.into(),
                tags: vec![],
                comment: Some("spam link".to_string()),
                byte_delta: 220,
                is_patrolled: false.into(),
            },
        ];

        let ranked =
            build_ranked_queue(events, &ScoringConfig::default()).expect("queue should build");

        assert!(
            ranked[0]
                .score
                .contributions
                .iter()
                .any(|entry| entry.signal == ScoringSignal::DuplicatePattern)
        );
    }
}
