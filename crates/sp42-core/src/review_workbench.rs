//! Shared patrol workbench helpers for prepared actions and training exports.

use serde::{Deserialize, Serialize};
use url::Url;

use crate::action_executor::{
    PatrolRequest, RollbackRequest, SessionActionExecutionRequest, SessionActionKind, UndoRequest,
    build_patrol_request, build_rollback_request, build_undo_request,
};
use crate::errors::ReviewWorkbenchError;
use crate::training_data::{TrainingLabel, encode_csv, encode_json_line};
use crate::types::{Action, EditorIdentity, HttpMethod, QueuedEdit, WikiConfig};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreparedRequestPreview {
    pub label: String,
    pub method: HttpMethod,
    pub url: Url,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewWorkbench {
    pub rev_id: u64,
    pub title: String,
    pub requests: Vec<PreparedRequestPreview>,
    pub training_jsonl: String,
    pub training_csv: String,
}

/// Build a prepared patrol workbench snapshot for an already-ranked edit.
///
/// # Errors
///
/// Returns [`ReviewWorkbenchError`] when action previews or training exports
/// cannot be constructed from the provided inputs.
pub fn build_review_workbench(
    config: &WikiConfig,
    item: &QueuedEdit,
    token: &str,
    actor: &str,
    note: Option<&str>,
) -> Result<ReviewWorkbench, ReviewWorkbenchError> {
    if token.trim().is_empty() {
        return Err(ReviewWorkbenchError::Incomplete {
            message: "token is required".to_string(),
        });
    }

    if actor.trim().is_empty() {
        return Err(ReviewWorkbenchError::Incomplete {
            message: "actor is required".to_string(),
        });
    }

    let rollback_request = build_rollback_request(
        config,
        &RollbackRequest {
            title: item.event.title.clone(),
            user: performer_label(&item.event.performer),
            token: token.to_string(),
            summary: note.map(ToString::to_string),
        },
    )?;
    let patrol_request = build_patrol_request(
        config,
        &PatrolRequest {
            rev_id: item.event.rev_id,
            token: token.to_string(),
        },
    )?;

    let undo_after_rev_id = item
        .event
        .old_rev_id
        .or_else(|| item.event.rev_id.checked_sub(1))
        .ok_or_else(|| ReviewWorkbenchError::Incomplete {
            message: "undo preview requires old_rev_id or rev_id > 0".to_string(),
        })?;
    let undo_request = build_undo_request(
        config,
        &UndoRequest {
            title: item.event.title.clone(),
            undo_rev_id: item.event.rev_id,
            undo_after_rev_id,
            token: token.to_string(),
            summary: note.map(ToString::to_string),
        },
    )?;

    let labels = vec![
        build_label(item, actor, Action::Rollback, note),
        build_label(item, actor, Action::MarkPatrolled, note),
        build_label(item, actor, Action::Revert, note),
    ];

    let mut training_jsonl = String::new();
    for label in &labels {
        training_jsonl.push_str(&encode_json_line(label)?);
    }

    Ok(ReviewWorkbench {
        rev_id: item.event.rev_id,
        title: item.event.title.clone(),
        requests: vec![
            to_preview("rollback", &rollback_request),
            to_preview("patrol", &patrol_request),
            to_preview("undo", &undo_request),
        ],
        training_jsonl,
        training_csv: encode_csv(&labels),
    })
}

/// Build tokenless session action execution requests for a ranked edit.
///
/// # Errors
///
/// Returns [`ReviewWorkbenchError`] when the edit cannot support an undo
/// request because there is no prior revision to target.
pub fn build_session_action_execution_requests(
    item: &QueuedEdit,
    note: Option<&str>,
) -> Result<Vec<SessionActionExecutionRequest>, ReviewWorkbenchError> {
    let undo_after_rev_id = item
        .event
        .old_rev_id
        .or_else(|| item.event.rev_id.checked_sub(1))
        .ok_or_else(|| ReviewWorkbenchError::Incomplete {
            message: "undo request requires old_rev_id or rev_id > 0".to_string(),
        })?;

    Ok(vec![
        SessionActionExecutionRequest {
            wiki_id: item.event.wiki_id.clone(),
            kind: SessionActionKind::Rollback,
            rev_id: item.event.rev_id,
            title: Some(item.event.title.clone()),
            target_user: Some(performer_label(&item.event.performer)),
            undo_after_rev_id: None,
            summary: note.map(ToString::to_string),
        },
        SessionActionExecutionRequest {
            wiki_id: item.event.wiki_id.clone(),
            kind: SessionActionKind::Patrol,
            rev_id: item.event.rev_id,
            title: None,
            target_user: None,
            undo_after_rev_id: None,
            summary: note.map(ToString::to_string),
        },
        SessionActionExecutionRequest {
            wiki_id: item.event.wiki_id.clone(),
            kind: SessionActionKind::Undo,
            rev_id: item.event.rev_id,
            title: Some(item.event.title.clone()),
            target_user: None,
            undo_after_rev_id: Some(undo_after_rev_id),
            summary: note.map(ToString::to_string),
        },
    ])
}

fn build_label(
    item: &QueuedEdit,
    actor: &str,
    action: Action,
    note: Option<&str>,
) -> TrainingLabel {
    TrainingLabel {
        wiki_id: item.event.wiki_id.clone(),
        rev_id: item.event.rev_id,
        actor: actor.to_string(),
        action,
        captured_at_ms: item.event.timestamp_ms,
        note: note.map(ToString::to_string),
    }
}

fn to_preview(label: &str, request: &crate::types::HttpRequest) -> PreparedRequestPreview {
    PreparedRequestPreview {
        label: label.to_string(),
        method: request.method.clone(),
        url: request.url.clone(),
        body: String::from_utf8_lossy(&request.body).into_owned(),
    }
}

fn performer_label(performer: &EditorIdentity) -> String {
    match performer {
        EditorIdentity::Registered { username } => username.clone(),
        EditorIdentity::Anonymous { label } | EditorIdentity::Temporary { label } => label.clone(),
    }
}

#[cfg(test)]
mod tests {
    use crate::config_parser::parse_wiki_config;
    use crate::scoring_engine::score_edit;
    use crate::types::{EditEvent, EditorIdentity, QueuedEdit, ScoringConfig};

    use super::{build_review_workbench, build_session_action_execution_requests};

    #[test]
    fn builds_request_and_training_previews() {
        let config = parse_wiki_config(include_str!("../../../configs/frwiki.yaml"))
            .expect("config should parse");
        let event = EditEvent {
            wiki_id: "frwiki".to_string(),
            title: "Exemple".to_string(),
            namespace: 0,
            rev_id: 123_456,
            old_rev_id: Some(123_455),
            performer: EditorIdentity::Registered {
                username: "ExampleUser".to_string(),
            },
            timestamp_ms: 1_710_000_000_000,
            is_bot: false,
            is_minor: false,
            is_new_page: false,
            tags: vec![],
            comment: Some("cleanup".to_string()),
            byte_delta: 12,
            is_patrolled: false,
        };
        let item = QueuedEdit {
            score: score_edit(&event, &ScoringConfig::default()).expect("score should compute"),
            event,
        };

        let workbench =
            build_review_workbench(&config, &item, "token-123", "Reviewer", Some("test note"))
                .expect("workbench should build");

        assert_eq!(workbench.requests.len(), 3);
        assert!(workbench.training_jsonl.contains("\"action\":\"Rollback\""));
        assert!(workbench.training_csv.contains("MarkPatrolled"));
    }

    #[test]
    fn builds_tokenless_session_action_requests() {
        let config = parse_wiki_config(include_str!("../../../configs/frwiki.yaml"))
            .expect("config should parse");
        let event = EditEvent {
            wiki_id: "frwiki".to_string(),
            title: "Exemple".to_string(),
            namespace: 0,
            rev_id: 123_456,
            old_rev_id: Some(123_455),
            performer: EditorIdentity::Registered {
                username: "ExampleUser".to_string(),
            },
            timestamp_ms: 1_710_000_000_000,
            is_bot: false,
            is_minor: false,
            is_new_page: false,
            tags: vec![],
            comment: Some("cleanup".to_string()),
            byte_delta: 12,
            is_patrolled: false,
        };
        let item = QueuedEdit {
            score: score_edit(&event, &ScoringConfig::default()).expect("score should compute"),
            event,
        };

        let requests = build_session_action_execution_requests(&item, Some("test note"))
            .expect("session action requests should build");

        assert_eq!(requests.len(), 3);
        assert!(requests.iter().all(|request| request.summary.is_some()));
        assert!(
            requests
                .iter()
                .all(|request| request.wiki_id == config.wiki_id)
        );
    }
}
