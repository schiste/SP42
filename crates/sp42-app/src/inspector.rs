use std::collections::BTreeMap;

use futures::executor::block_on;
use sp42_core::traits::{MemoryStorage, ReplayEventSource, StubHttpClient};
use sp42_core::{
    BacklogRuntime, BacklogRuntimeConfig, CoordinationSnapshot, CoordinationStateSummary,
    HttpResponse, ReviewWorkbench, StreamRuntime, WikiConfig,
};

const SAMPLE_STREAM_EVENTS: &str =
    include_str!("../../../fixtures/frwiki_recentchanges_batch.jsonl");
const SAMPLE_BACKLOG_RESPONSE: &str = r#"{
  "continue": {
    "rccontinue": "20260324010202|456"
  },
  "query": {
    "recentchanges": [
      {
        "type": "edit",
        "title": "Vandalisme",
        "ns": 0,
        "revid": 123459,
        "old_revid": 123458,
        "user": "192.0.2.10",
        "timestamp": "2026-03-24T01:02:03Z",
        "bot": false,
        "minor": false,
        "new": false,
        "oldlen": 120,
        "newlen": 80,
        "comment": "demo backlog item",
        "tags": ["mw-reverted"]
      },
      {
        "type": "new",
        "title": "Nouvelle page",
        "ns": 0,
        "revid": 123460,
        "old_revid": 123459,
        "user": "Example",
        "timestamp": "2026-03-24T01:03:03Z",
        "bot": false,
        "minor": false,
        "new": true,
        "oldlen": 0,
        "newlen": 44,
        "comment": "demo backlog item",
        "tags": []
      }
    ]
  }
}"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BacklogPreviewRequest {
    pub limit: u16,
    pub include_bots: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamPreviewRequest {
    pub steps: usize,
}

#[must_use]
pub fn render_review_workbench_lines(workbench: &ReviewWorkbench) -> Vec<String> {
    let mut lines = vec![
        format!(
            "review rev={} title=\"{}\"",
            workbench.rev_id, workbench.title
        ),
        format!("requests={}", workbench.requests.len()),
        format!(
            "training_jsonl_rows={}",
            workbench.training_jsonl.lines().count()
        ),
        format!(
            "training_csv_rows={}",
            workbench.training_csv.lines().skip(1).count()
        ),
    ];

    lines.extend(workbench.requests.iter().map(|request| {
        format!(
            "{} {:?} {} {}",
            request.label, request.method, request.url, request.body
        )
    }));

    lines
}

#[must_use]
pub fn render_coordination_snapshot_lines(snapshot: &CoordinationSnapshot) -> Vec<String> {
    if snapshot.rooms.is_empty() {
        return vec!["No active coordination rooms.".to_string()];
    }

    snapshot
        .rooms
        .iter()
        .map(|room| {
            format!(
                "{}: clients={} messages={}",
                room.wiki_id, room.connected_clients, room.published_messages
            )
        })
        .collect()
}

#[must_use]
pub fn render_coordination_state_lines(summary: &CoordinationStateSummary) -> Vec<String> {
    let mut lines = vec![
        format!("room={}", summary.wiki_id),
        format!("claims={}", summary.claims.len()),
        format!("presence={}", summary.presence.len()),
        format!("flags={}", summary.flagged_edits.len()),
        format!("score_deltas={}", summary.score_deltas.len()),
        format!("recent_actions={}", summary.recent_actions.len()),
    ];

    lines.extend(summary.claims.iter().map(|claim| {
        format!(
            "claim rev={} actor={} wiki={}",
            claim.rev_id, claim.actor, claim.wiki_id
        )
    }));

    lines
}

/// Render a sample recentchanges backlog preview using the shared backlog runtime.
///
/// # Errors
///
/// Returns `Err(String)` when request construction, polling, or runtime state
/// updates fail.
pub fn render_backlog_preview_lines(
    config: &WikiConfig,
    request: &BacklogPreviewRequest,
) -> Result<Vec<String>, String> {
    let storage = MemoryStorage::default();
    let client = StubHttpClient::new([Ok(HttpResponse {
        status: 200,
        headers: BTreeMap::new(),
        body: SAMPLE_BACKLOG_RESPONSE.as_bytes().to_vec(),
    })]);
    let mut runtime = BacklogRuntime::from_config(
        config,
        storage,
        BacklogRuntimeConfig {
            limit: request.limit,
            include_bots: request.include_bots,
        },
    );

    block_on(async {
        runtime
            .initialize()
            .await
            .map_err(|error| error.to_string())?;
        let http_request = runtime
            .build_next_request()
            .map_err(|error| error.to_string())?;
        let batch = runtime
            .poll(&client)
            .await
            .map_err(|error| error.to_string())?;
        let status = runtime.status();

        let mut lines = vec![
            format!(
                "request {:?} {}",
                http_request.method,
                http_request.url.as_str()
            ),
            format!(
                "limit={} include_bots={}",
                request.limit, request.include_bots
            ),
            format!(
                "batch_size={} next_continue={}",
                batch.events.len(),
                batch.next_continue.as_deref().unwrap_or("none")
            ),
            format!(
                "checkpoint_key={} polls={} total_events={}",
                status.checkpoint_key, status.poll_count, status.total_events
            ),
        ];

        lines.extend(batch.events.iter().map(|event| {
            format!(
                "rev={} title=\"{}\" score_hint={}",
                event.rev_id, event.title, event.byte_delta
            )
        }));

        Ok::<_, String>(lines)
    })
}

/// Render a sample backlog polling preview using the shared recentchanges runtime.
///
/// # Errors
///
/// Returns `Err(String)` when request construction, polling, or runtime state
/// updates fail.
pub fn render_stream_preview_lines(
    config: &WikiConfig,
    request: &StreamPreviewRequest,
) -> Result<Vec<String>, String> {
    let source = ReplayEventSource::new(SAMPLE_STREAM_EVENTS.lines().enumerate().filter_map(
        |(index, line)| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }

            Some(sp42_core::ServerSentEvent {
                event_type: Some("message".to_string()),
                id: Some(format!("stream-fixture-{}", index + 1)),
                data: trimmed.to_string(),
                retry_ms: None,
            })
        },
    ));
    let storage = MemoryStorage::default();
    let mut runtime = StreamRuntime::from_config(config, source, storage);

    block_on(async {
        runtime
            .initialize()
            .await
            .map_err(|error| error.to_string())?;
        let mut seen = Vec::new();

        for _ in 0..request.steps {
            match runtime
                .next_actionable_event()
                .await
                .map_err(|error| error.to_string())?
            {
                Some(event) => {
                    seen.push(format!(
                        "rev={} title=\"{}\" namespace={} score_hint={}",
                        event.rev_id, event.title, event.namespace, event.byte_delta
                    ));
                }
                None => break,
            }
        }

        let status = runtime.status();
        let mut lines = vec![
            format!("steps_requested={}", request.steps),
            format!(
                "checkpoint_key={} last_event_id={}",
                status.checkpoint_key,
                status.last_event_id.as_deref().unwrap_or("none")
            ),
            format!(
                "delivered={} filtered={} reconnects={}",
                status.delivered_events, status.filtered_events, status.reconnect_attempts
            ),
        ];
        lines.extend(seen);
        Ok::<_, String>(lines)
    })
}

#[must_use]
pub fn backlog_request_from_limit(limit: &str, include_bots: bool) -> BacklogPreviewRequest {
    let parsed_limit = limit.trim().parse::<u16>().unwrap_or(25);
    BacklogPreviewRequest {
        limit: parsed_limit,
        include_bots,
    }
}

#[must_use]
pub fn stream_request_from_steps(steps: &str) -> StreamPreviewRequest {
    let parsed_steps = steps.trim().parse::<usize>().unwrap_or(3);
    StreamPreviewRequest {
        steps: parsed_steps,
    }
}

#[cfg(test)]
mod tests {
    use sp42_core::parse_wiki_config;

    use super::{
        backlog_request_from_limit, render_backlog_preview_lines,
        render_coordination_snapshot_lines, render_coordination_state_lines,
        render_review_workbench_lines, render_stream_preview_lines, stream_request_from_steps,
    };
    use sp42_core::{
        Action, ActionBroadcast, CoordinationRoomSummary, CoordinationSnapshot,
        CoordinationStateSummary, EditClaim, FlaggedEdit, PresenceHeartbeat, RaceResolution,
        ReviewWorkbench,
    };

    const CONFIG: &str = include_str!("../../../configs/frwiki.yaml");

    #[test]
    fn parses_request_controls() {
        let backlog = backlog_request_from_limit("19", true);
        let stream = stream_request_from_steps("5");

        assert_eq!(backlog.limit, 19);
        assert!(backlog.include_bots);
        assert_eq!(stream.steps, 5);
    }

    #[test]
    fn renders_review_workbench_lines() {
        let workbench = ReviewWorkbench {
            rev_id: 123,
            title: "Example".to_string(),
            requests: Vec::new(),
            training_jsonl: r#"{"rev_id":123}"#.to_string(),
            training_csv: "rev_id,label\n123,rollback\n".to_string(),
        };

        let lines = render_review_workbench_lines(&workbench);

        assert!(lines.iter().any(|line| line.contains("review rev=123")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("training_csv_rows=1"))
        );
    }

    #[test]
    fn renders_coordination_state_and_snapshot_lines() {
        let snapshot = CoordinationSnapshot {
            rooms: vec![CoordinationRoomSummary {
                wiki_id: "frwiki".to_string(),
                connected_clients: 2,
                published_messages: 4,
                claim_count: 1,
                presence_count: 1,
                flagged_edit_count: 1,
                score_delta_count: 0,
                race_resolution_count: 1,
                recent_action_count: 1,
            }],
        };
        let state = CoordinationStateSummary {
            wiki_id: "frwiki".to_string(),
            claims: vec![EditClaim {
                wiki_id: "frwiki".to_string(),
                rev_id: 1,
                actor: "Alice".to_string(),
            }],
            presence: vec![PresenceHeartbeat {
                wiki_id: "frwiki".to_string(),
                actor: "Alice".to_string(),
                active_edit_count: 1,
            }],
            flagged_edits: vec![FlaggedEdit {
                wiki_id: "frwiki".to_string(),
                rev_id: 1,
                score: 95,
                reason: "demo".to_string(),
            }],
            score_deltas: vec![],
            race_resolutions: vec![RaceResolution {
                wiki_id: "frwiki".to_string(),
                rev_id: 1,
                winning_actor: "Alice".to_string(),
            }],
            recent_actions: vec![ActionBroadcast {
                wiki_id: "frwiki".to_string(),
                rev_id: 1,
                action: Action::Rollback,
                actor: "Alice".to_string(),
            }],
        };

        let snapshot_lines = render_coordination_snapshot_lines(&snapshot);
        let state_lines = render_coordination_state_lines(&state);

        assert!(snapshot_lines[0].contains("frwiki"));
        assert!(state_lines.iter().any(|line| line.contains("claims=1")));
    }

    #[test]
    fn renders_stream_and_backlog_preview_lines() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let backlog_lines = render_backlog_preview_lines(
            &config,
            &super::BacklogPreviewRequest {
                limit: 25,
                include_bots: false,
            },
        )
        .expect("backlog preview should render");
        let stream_lines =
            render_stream_preview_lines(&config, &super::StreamPreviewRequest { steps: 2 })
                .expect("stream preview should render");

        assert!(
            backlog_lines
                .iter()
                .any(|line| line.contains("batch_size=2"))
        );
        assert!(stream_lines.iter().any(|line| line.contains("delivered=")));
    }
}
