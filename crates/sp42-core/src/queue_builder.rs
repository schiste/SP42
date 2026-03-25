//! Helpers for building ranked patrol queues from edit events.

use crate::errors::ScoringError;
use crate::priority_queue::PriorityQueue;
use crate::scoring_engine::{score_edit, score_edit_with_context};
use crate::types::{EditEvent, QueuedEdit, ScoringConfig, ScoringContext};

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
    let mut queue = PriorityQueue::new();

    for event in events {
        let score = score_edit(&event, scoring_config)?;
        queue.push(score.total, QueuedEdit { event, score });
    }

    let mut ranked = Vec::with_capacity(queue.len());
    while let Some(item) = queue.pop() {
        ranked.push(item);
    }

    Ok(ranked)
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

#[cfg(test)]
mod tests {
    use super::{build_ranked_queue, build_ranked_queue_with_contexts};
    use crate::types::{
        EditEvent, EditorIdentity, ScoringConfig, ScoringContext, UserRiskProfile, WarningLevel,
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
                },
            ),
        ];

        let ranked = build_ranked_queue_with_contexts(events, &ScoringConfig::default())
            .expect("contextual queue should build");

        assert_eq!(ranked[0].event.rev_id, 2);
        assert!(ranked[0].score.total > ranked[1].score.total);
    }
}
