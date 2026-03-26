//! Shared debug-panel snapshot builders for all targets.

use serde::{Deserialize, Serialize};

use crate::backlog_runtime::BacklogRuntimeStatus;
use crate::coordination_state::CoordinationStateSummary;
use crate::diff_engine::StructuredDiff;
use crate::review_workbench::ReviewWorkbench;
use crate::stream_runtime::StreamRuntimeStatus;
use crate::types::{QueuedEdit, ScoringContext};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TraceLevel {
    Info,
    Debug,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionTrace {
    pub level: TraceLevel,
    pub category: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PerformanceMarker {
    pub operation: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DebugSnapshot {
    pub summary_lines: Vec<String>,
    pub decision_traces: Vec<DecisionTrace>,
    pub performance_markers: Vec<PerformanceMarker>,
}

#[derive(Debug, Clone, Default)]
pub struct DebugSnapshotInputs<'a> {
    pub queue: &'a [QueuedEdit],
    pub selected: Option<&'a QueuedEdit>,
    pub scoring_context: Option<&'a ScoringContext>,
    pub diff: Option<&'a StructuredDiff>,
    pub review_workbench: Option<&'a ReviewWorkbench>,
    pub stream_status: Option<&'a StreamRuntimeStatus>,
    pub backlog_status: Option<&'a BacklogRuntimeStatus>,
    pub coordination: Option<&'a CoordinationStateSummary>,
}

/// Build a portable debug snapshot from the current product state.
#[must_use]
pub fn build_debug_snapshot(inputs: &DebugSnapshotInputs<'_>) -> DebugSnapshot {
    let mut snapshot = DebugSnapshot::default();

    snapshot
        .summary_lines
        .push(format!("queue_depth={}", inputs.queue.len()));

    if let Some(selected) = inputs.selected {
        push_selected_snapshot(&mut snapshot, selected);
    }

    if let Some(scoring_context) = inputs.scoring_context {
        push_scoring_context_snapshot(&mut snapshot, scoring_context);
    }

    if let Some(diff) = inputs.diff {
        push_diff_snapshot(&mut snapshot, diff);
    }

    if let Some(workbench) = inputs.review_workbench {
        push_review_workbench_snapshot(&mut snapshot, workbench);
    }

    if let Some(stream_status) = inputs.stream_status {
        push_stream_snapshot(&mut snapshot, stream_status);
    }

    if let Some(backlog_status) = inputs.backlog_status {
        push_backlog_snapshot(&mut snapshot, backlog_status);
    }

    if let Some(coordination) = inputs.coordination {
        push_coordination_snapshot(&mut snapshot, coordination);
    }

    snapshot
}

fn push_selected_snapshot(snapshot: &mut DebugSnapshot, selected: &QueuedEdit) {
    snapshot.summary_lines.extend([
        format!("selected_rev_id={}", selected.event.rev_id),
        format!("selected_title={}", selected.event.title),
        format!("selected_score={}", selected.score.total),
        format!("selected_signals={}", selected.score.contributions.len()),
    ]);

    for contribution in &selected.score.contributions {
        snapshot.decision_traces.push(DecisionTrace {
            level: TraceLevel::Debug,
            category: "scoring".to_string(),
            message: format!(
                "{:?}: weight={:+} note={}",
                contribution.signal,
                contribution.weight,
                contribution.note.as_deref().unwrap_or("none")
            ),
        });
    }
}

fn push_scoring_context_snapshot(snapshot: &mut DebugSnapshot, scoring_context: &ScoringContext) {
    snapshot.summary_lines.push(format!(
        "context user_risk={} liftwing={}",
        scoring_context.user_risk.is_some(),
        scoring_context
            .liftwing_risk
            .map_or_else(|| "none".to_string(), |value| format!("{value:.2}"))
    ));

    if let Some(user_risk) = &scoring_context.user_risk {
        snapshot.decision_traces.push(DecisionTrace {
            level: TraceLevel::Debug,
            category: "user-risk".to_string(),
            message: format!(
                "warning_level={:?} warning_count={} vandalism_templates={}",
                user_risk.warning_level,
                user_risk.warning_count,
                user_risk.has_recent_vandalism_templates
            ),
        });
    }
}

fn push_diff_snapshot(snapshot: &mut DebugSnapshot, diff: &StructuredDiff) {
    snapshot.summary_lines.push(format!(
        "diff changed={} inserted_segments={} deleted_segments={} inserted_chars={} deleted_chars={}",
        diff.stats.has_changes(),
        diff.stats.insert_segments,
        diff.stats.delete_segments,
        diff.stats.inserted_char_count,
        diff.stats.deleted_char_count
    ));
    snapshot.performance_markers.push(PerformanceMarker {
        operation: "diff".to_string(),
        detail: format!("segments={}", diff.segments.len()),
    });
}

fn push_review_workbench_snapshot(snapshot: &mut DebugSnapshot, workbench: &ReviewWorkbench) {
    snapshot.summary_lines.extend([
        format!("workbench_requests={}", workbench.requests.len()),
        format!(
            "training_rows={}",
            workbench.training_csv.lines().skip(1).count()
        ),
    ]);
    snapshot.decision_traces.push(DecisionTrace {
        level: TraceLevel::Info,
        category: "review-workbench".to_string(),
        message: format!("prepared_actions_for_rev={}", workbench.rev_id),
    });
}

fn push_stream_snapshot(snapshot: &mut DebugSnapshot, stream_status: &StreamRuntimeStatus) {
    snapshot.summary_lines.extend([
        format!("stream_delivered={}", stream_status.delivered_events),
        format!("stream_filtered={}", stream_status.filtered_events),
        format!("stream_reconnects={}", stream_status.reconnect_attempts),
    ]);
    snapshot.performance_markers.push(PerformanceMarker {
        operation: "stream-runtime".to_string(),
        detail: format!(
            "checkpoint={} last_event_id={}",
            stream_status.checkpoint_key,
            stream_status.last_event_id.as_deref().unwrap_or("none")
        ),
    });
}

fn push_backlog_snapshot(snapshot: &mut DebugSnapshot, backlog_status: &BacklogRuntimeStatus) {
    snapshot.summary_lines.extend([
        format!("backlog_polls={}", backlog_status.poll_count),
        format!("backlog_total_events={}", backlog_status.total_events),
        format!(
            "backlog_next_continue={}",
            backlog_status.next_continue.as_deref().unwrap_or("none")
        ),
    ]);
    snapshot.performance_markers.push(PerformanceMarker {
        operation: "backlog-runtime".to_string(),
        detail: format!(
            "checkpoint={} last_batch_size={}",
            backlog_status.checkpoint_key, backlog_status.last_batch_size
        ),
    });
}

fn push_coordination_snapshot(
    snapshot: &mut DebugSnapshot,
    coordination: &CoordinationStateSummary,
) {
    snapshot.summary_lines.extend([
        format!("coordination_claims={}", coordination.claims.len()),
        format!("coordination_presence={}", coordination.presence.len()),
        format!("coordination_actions={}", coordination.recent_actions.len()),
    ]);
    snapshot.decision_traces.push(DecisionTrace {
        level: TraceLevel::Info,
        category: "coordination".to_string(),
        message: format!(
            "wiki={} claims={} flags={} deltas={}",
            coordination.wiki_id,
            coordination.claims.len(),
            coordination.flagged_edits.len(),
            coordination.score_deltas.len()
        ),
    });
}

#[cfg(test)]
mod tests {
    use crate::diff_engine::diff_lines;
    use crate::review_workbench::build_review_workbench;
    use crate::scoring_engine::{score_edit, score_edit_with_context};
    use crate::types::{
        EditEvent, EditorIdentity, QueuedEdit, ScoringConfig, ScoringContext, UserRiskProfile,
        WarningLevel,
    };

    use super::{DebugSnapshotInputs, TraceLevel, build_debug_snapshot};

    #[test]
    fn builds_snapshot_from_available_inputs() {
        let config =
            crate::config_parser::parse_wiki_config(include_str!("../../../configs/frwiki.yaml"))
                .expect("config should parse");
        let event = EditEvent {
            wiki_id: "frwiki".to_string(),
            title: "Exemple".to_string(),
            namespace: 0,
            rev_id: 123_456,
            old_rev_id: Some(123_455),
            performer: EditorIdentity::Anonymous {
                label: "192.0.2.10".to_string(),
            },
            timestamp_ms: 1_710_000_000_000,
            is_bot: false.into(),
            is_minor: false.into(),
            is_new_page: false.into(),
            tags: vec!["mw-reverted".to_string()],
            comment: Some("demo".to_string()),
            byte_delta: -30,
            is_patrolled: false.into(),
        };
        let context = ScoringContext {
            user_risk: Some(UserRiskProfile {
                warning_level: WarningLevel::Level2,
                warning_count: 2,
                has_recent_vandalism_templates: true,
            }),
            liftwing_risk: Some(0.72),
            ..ScoringContext::default()
        };
        let item = QueuedEdit {
            score: score_edit_with_context(&event, &ScoringConfig::default(), &context)
                .expect("score should compute"),
            event,
        };
        let diff = diff_lines("Avant\n", "Avant\nApres\n");
        let workbench = build_review_workbench(&config, &item, "token-123", "Reviewer", None)
            .expect("workbench should build");
        let coordination = crate::coordination_state::CoordinationStateSummary {
            wiki_id: "frwiki".to_string(),
            claims: vec![crate::types::EditClaim {
                wiki_id: "frwiki".to_string(),
                rev_id: 123_456,
                actor: "Reviewer".to_string(),
            }],
            presence: Vec::new(),
            flagged_edits: Vec::new(),
            score_deltas: Vec::new(),
            race_resolutions: Vec::new(),
            recent_actions: Vec::new(),
        };
        let stream_status = crate::stream_runtime::StreamRuntimeStatus {
            checkpoint_key: "stream.last_event_id.frwiki".to_string(),
            last_event_id: Some("event-2".to_string()),
            delivered_events: 1,
            filtered_events: 2,
            reconnect_attempts: 1,
        };
        let backlog_status = crate::backlog_runtime::BacklogRuntimeStatus {
            checkpoint_key: "recentchanges.rccontinue.frwiki".to_string(),
            next_continue: Some("20260324010202|456".to_string()),
            last_batch_size: 25,
            total_events: 80,
            poll_count: 4,
        };

        let snapshot = build_debug_snapshot(&DebugSnapshotInputs {
            queue: std::slice::from_ref(&item),
            selected: Some(&item),
            scoring_context: Some(&context),
            diff: Some(&diff),
            review_workbench: Some(&workbench),
            stream_status: Some(&stream_status),
            backlog_status: Some(&backlog_status),
            coordination: Some(&coordination),
        });

        assert!(
            snapshot
                .summary_lines
                .iter()
                .any(|line| line == "queue_depth=1")
        );
        assert!(
            snapshot
                .summary_lines
                .iter()
                .any(|line| line.contains("selected_score="))
        );
        assert!(
            snapshot
                .decision_traces
                .iter()
                .any(|trace| trace.category == "scoring")
        );
        assert!(
            snapshot
                .decision_traces
                .iter()
                .any(|trace| trace.level == TraceLevel::Info && trace.category == "coordination")
        );
        assert_eq!(snapshot.performance_markers.len(), 3);
    }

    #[test]
    fn empty_snapshot_still_reports_queue_depth() {
        let snapshot = build_debug_snapshot(&DebugSnapshotInputs::default());

        assert_eq!(snapshot.summary_lines, vec!["queue_depth=0".to_string()]);
        assert!(snapshot.decision_traces.is_empty());
        assert!(snapshot.performance_markers.is_empty());
    }

    #[test]
    fn selected_scoring_traces_include_signal_breakdown() {
        let event = EditEvent {
            wiki_id: "frwiki".to_string(),
            title: "Nouvelle page".to_string(),
            namespace: 0,
            rev_id: 1,
            old_rev_id: None,
            performer: EditorIdentity::Anonymous {
                label: "198.51.100.2".to_string(),
            },
            timestamp_ms: 1_710_000_000_000,
            is_bot: false.into(),
            is_minor: false.into(),
            is_new_page: true.into(),
            tags: Vec::new(),
            comment: None,
            byte_delta: 100,
            is_patrolled: false.into(),
        };
        let item = QueuedEdit {
            score: score_edit(&event, &ScoringConfig::default()).expect("score should compute"),
            event,
        };

        let snapshot = build_debug_snapshot(&DebugSnapshotInputs {
            queue: std::slice::from_ref(&item),
            selected: Some(&item),
            ..DebugSnapshotInputs::default()
        });

        assert!(
            snapshot
                .decision_traces
                .iter()
                .any(|trace| trace.message.contains("AnonymousUser"))
        );
        assert!(
            snapshot
                .decision_traces
                .iter()
                .any(|trace| trace.message.contains("NewPage"))
        );
    }
}
