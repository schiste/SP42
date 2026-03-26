//! Pure patrol scenario report builder and renderers.

use serde::{Deserialize, Serialize};

use crate::backlog_runtime::BacklogRuntimeStatus;
use crate::coordination_state::CoordinationStateSummary;
use crate::debug_snapshot::{DebugSnapshot, DebugSnapshotInputs, build_debug_snapshot};
use crate::diff_engine::StructuredDiff;
use crate::report_document::{
    ReportDocument, ReportSection, render_report_document_markdown, render_report_document_text,
};
use crate::review_workbench::ReviewWorkbench;
use crate::stream_runtime::StreamRuntimeStatus;
use crate::types::{QueuedEdit, ScoringContext};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReportSeverity {
    Info,
    Warning,
    Blocker,
}

impl std::fmt::Display for ReportSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Info => "Info",
            Self::Warning => "Warning",
            Self::Blocker => "Blocker",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatrolScenarioFinding {
    pub severity: ReportSeverity,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatrolScenarioSection {
    pub name: String,
    pub available: bool,
    pub summary_lines: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PatrolScenarioReadiness {
    Blocked,
    Limited,
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatrolScenarioSelectedEdit {
    pub wiki_id: String,
    pub rev_id: u64,
    pub title: String,
    pub score: i32,
    pub signals: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatrolScenarioReport {
    pub wiki_id: String,
    pub queue_depth: usize,
    pub readiness: PatrolScenarioReadiness,
    pub selected: Option<PatrolScenarioSelectedEdit>,
    pub sections: Vec<PatrolScenarioSection>,
    pub findings: Vec<PatrolScenarioFinding>,
    pub debug_snapshot: DebugSnapshot,
}

#[derive(Debug, Clone, Default)]
pub struct PatrolScenarioReportInputs<'a> {
    pub queue: &'a [QueuedEdit],
    pub selected: Option<&'a QueuedEdit>,
    pub scoring_context: Option<&'a ScoringContext>,
    pub diff: Option<&'a StructuredDiff>,
    pub review_workbench: Option<&'a ReviewWorkbench>,
    pub stream_status: Option<&'a StreamRuntimeStatus>,
    pub backlog_status: Option<&'a BacklogRuntimeStatus>,
    pub coordination: Option<&'a CoordinationStateSummary>,
    pub wiki_id_hint: Option<&'a str>,
}

#[must_use]
pub fn build_patrol_scenario_report(
    inputs: &PatrolScenarioReportInputs<'_>,
) -> PatrolScenarioReport {
    let selected = inputs.selected.or_else(|| inputs.queue.first());
    let wiki_id = resolve_wiki_id(inputs);
    let debug_snapshot = build_debug_snapshot(&DebugSnapshotInputs {
        queue: inputs.queue,
        selected,
        scoring_context: inputs.scoring_context,
        diff: inputs.diff,
        review_workbench: inputs.review_workbench,
        stream_status: inputs.stream_status,
        backlog_status: inputs.backlog_status,
        coordination: inputs.coordination,
    });

    let sections = vec![
        build_queue_section(inputs.queue, selected),
        build_context_section(inputs.scoring_context),
        build_diff_section(inputs.diff),
        build_review_workbench_section(inputs.review_workbench),
        build_stream_section(inputs.stream_status),
        build_backlog_section(inputs.backlog_status),
        build_coordination_section(inputs.coordination),
    ];

    let selected_summary = selected.map(|item| PatrolScenarioSelectedEdit {
        wiki_id: item.event.wiki_id.clone(),
        rev_id: item.event.rev_id,
        title: item.event.title.clone(),
        score: item.score.total,
        signals: item.score.contributions.len(),
    });

    let readiness = compute_readiness(inputs.queue, &sections);
    let findings = build_findings(
        &sections,
        selected_summary.as_ref(),
        inputs.diff,
        inputs.scoring_context,
    );

    PatrolScenarioReport {
        wiki_id,
        queue_depth: inputs.queue.len(),
        readiness,
        selected: selected_summary,
        sections,
        findings,
        debug_snapshot,
    }
}

#[must_use]
pub fn render_patrol_scenario_text(report: &PatrolScenarioReport) -> String {
    render_report_document_text(&report.to_report_document())
}

#[must_use]
pub fn render_patrol_scenario_markdown(report: &PatrolScenarioReport) -> String {
    render_report_document_markdown(&report.to_report_document())
}

impl PatrolScenarioReport {
    #[must_use]
    pub fn to_report_document(&self) -> ReportDocument {
        let mut lead_lines = vec![
            format!("wiki={}", self.wiki_id),
            format!("readiness={:?}", self.readiness),
            format!("queue_depth={}", self.queue_depth),
        ];

        if let Some(selected) = &self.selected {
            lead_lines.push(format!(
                "selected rev={} title=\"{}\" score={} signals={}",
                selected.rev_id, selected.title, selected.score, selected.signals
            ));
        } else {
            lead_lines.push("selected=none".to_string());
        }

        let mut sections = Vec::new();

        if self.findings.is_empty() {
            sections.push(ReportSection {
                name: "Findings".to_string(),
                available: false,
                summary_lines: vec!["no findings".to_string()],
            });
        } else {
            sections.push(ReportSection {
                name: "Findings".to_string(),
                available: true,
                summary_lines: self
                    .findings
                    .iter()
                    .map(|finding| {
                        format!(
                            "{} {}: {}",
                            format_severity(finding.severity),
                            finding.code,
                            finding.message
                        )
                    })
                    .collect(),
            });
        }

        sections.extend(self.sections.iter().map(|section| ReportSection {
            name: section.name.clone(),
            available: section.available,
            summary_lines: section.summary_lines.clone(),
        }));

        if !self.debug_snapshot.summary_lines.is_empty() {
            sections.push(ReportSection {
                name: "Debug snapshot".to_string(),
                available: true,
                summary_lines: self.debug_snapshot.summary_lines.clone(),
            });
        }

        ReportDocument::new("Patrol report")
            .with_lead_lines(lead_lines)
            .with_sections(sections)
    }
}

fn resolve_wiki_id(inputs: &PatrolScenarioReportInputs<'_>) -> String {
    inputs
        .coordination
        .map(|coordination| coordination.wiki_id.clone())
        .or_else(|| inputs.selected.map(|item| item.event.wiki_id.clone()))
        .or_else(|| inputs.queue.first().map(|item| item.event.wiki_id.clone()))
        .or_else(|| inputs.wiki_id_hint.map(ToOwned::to_owned))
        .unwrap_or_else(|| "unknown".to_string())
}

fn compute_readiness(
    queue: &[QueuedEdit],
    sections: &[PatrolScenarioSection],
) -> PatrolScenarioReadiness {
    if queue.is_empty() {
        return PatrolScenarioReadiness::Blocked;
    }

    if sections.iter().all(|section| section.available) {
        PatrolScenarioReadiness::Ready
    } else {
        PatrolScenarioReadiness::Limited
    }
}

fn build_findings(
    sections: &[PatrolScenarioSection],
    selected: Option<&PatrolScenarioSelectedEdit>,
    diff: Option<&StructuredDiff>,
    scoring_context: Option<&ScoringContext>,
) -> Vec<PatrolScenarioFinding> {
    let mut findings = Vec::new();

    for section in sections {
        if !section.available {
            findings.push(PatrolScenarioFinding {
                severity: ReportSeverity::Warning,
                code: format!("missing_{}", normalize_code(&section.name)),
                message: format!("{} data is unavailable", section.name),
            });
        }
    }

    if let Some(selected) = selected {
        if selected.score >= 80 {
            findings.push(PatrolScenarioFinding {
                severity: ReportSeverity::Warning,
                code: "high_score".to_string(),
                message: format!(
                    "selected edit {} is high risk with score {}",
                    selected.rev_id, selected.score
                ),
            });
        } else if selected.score <= 10 {
            findings.push(PatrolScenarioFinding {
                severity: ReportSeverity::Info,
                code: "low_score".to_string(),
                message: format!(
                    "selected edit {} is low risk with score {}",
                    selected.rev_id, selected.score
                ),
            });
        }
    } else {
        findings.push(PatrolScenarioFinding {
            severity: ReportSeverity::Blocker,
            code: "no_selection".to_string(),
            message: "no selected edit is available".to_string(),
        });
    }

    if let Some(diff) = diff {
        findings.push(PatrolScenarioFinding {
            severity: if diff.stats.has_changes() {
                ReportSeverity::Info
            } else {
                ReportSeverity::Warning
            },
            code: "diff_changes".to_string(),
            message: format!(
                "diff segments={}, inserted_chars={}, deleted_chars={}",
                diff.segments.len(),
                diff.stats.inserted_char_count,
                diff.stats.deleted_char_count
            ),
        });
    }

    if let Some(context) = scoring_context {
        if let Some(user_risk) = &context.user_risk {
            findings.push(PatrolScenarioFinding {
                severity: ReportSeverity::Warning,
                code: "user_risk".to_string(),
                message: format!(
                    "warning level {:?}, warnings={}, vandalism_templates={}",
                    user_risk.warning_level,
                    user_risk.warning_count,
                    user_risk.has_recent_vandalism_templates
                ),
            });
        }

        if let Some(liftwing_risk) = context.liftwing_risk {
            findings.push(PatrolScenarioFinding {
                severity: if liftwing_risk >= 0.5 {
                    ReportSeverity::Warning
                } else {
                    ReportSeverity::Info
                },
                code: "liftwing_risk".to_string(),
                message: format!("liftwing probability={liftwing_risk:.2}"),
            });
        }
    }

    findings
}

fn build_queue_section(
    queue: &[QueuedEdit],
    selected: Option<&QueuedEdit>,
) -> PatrolScenarioSection {
    match selected.or_else(|| queue.first()) {
        Some(item) => PatrolScenarioSection {
            name: "Queue".to_string(),
            available: true,
            summary_lines: vec![
                format!("depth={}", queue.len()),
                format!("top_rev_id={}", item.event.rev_id),
                format!("top_title=\"{}\"", item.event.title),
                format!("top_score={}", item.score.total),
                format!("top_signals={}", item.score.contributions.len()),
            ],
        },
        None => PatrolScenarioSection {
            name: "Queue".to_string(),
            available: false,
            summary_lines: vec!["queue is empty".to_string()],
        },
    }
}

fn build_context_section(context: Option<&ScoringContext>) -> PatrolScenarioSection {
    match context {
        Some(context) => {
            let mut summary_lines = vec![
                format!("user_risk={}", context.user_risk.is_some()),
                format!(
                    "liftwing={}",
                    context
                        .liftwing_risk
                        .map_or_else(|| "none".to_string(), |value| format!("{value:.2}"))
                ),
            ];

            if let Some(user_risk) = &context.user_risk {
                summary_lines.push(format!("warning_level={:?}", user_risk.warning_level));
                summary_lines.push(format!("warning_count={}", user_risk.warning_count));
                summary_lines.push(format!(
                    "vandalism_templates={}",
                    user_risk.has_recent_vandalism_templates
                ));
            }

            PatrolScenarioSection {
                name: "Context".to_string(),
                available: true,
                summary_lines,
            }
        }
        None => PatrolScenarioSection {
            name: "Context".to_string(),
            available: false,
            summary_lines: vec!["context unavailable".to_string()],
        },
    }
}

fn build_diff_section(diff: Option<&StructuredDiff>) -> PatrolScenarioSection {
    match diff {
        Some(diff) => PatrolScenarioSection {
            name: "Diff".to_string(),
            available: true,
            summary_lines: vec![
                format!("has_changes={}", diff.stats.has_changes()),
                format!("segments={}", diff.segments.len()),
                format!("equal_segments={}", diff.stats.equal_segments),
                format!("insert_segments={}", diff.stats.insert_segments),
                format!("delete_segments={}", diff.stats.delete_segments),
                format!("inserted_chars={}", diff.stats.inserted_char_count),
                format!("deleted_chars={}", diff.stats.deleted_char_count),
            ],
        },
        None => PatrolScenarioSection {
            name: "Diff".to_string(),
            available: false,
            summary_lines: vec!["diff unavailable".to_string()],
        },
    }
}

fn build_review_workbench_section(workbench: Option<&ReviewWorkbench>) -> PatrolScenarioSection {
    match workbench {
        Some(workbench) => PatrolScenarioSection {
            name: "Workbench".to_string(),
            available: true,
            summary_lines: vec![
                format!("rev_id={}", workbench.rev_id),
                format!("title=\"{}\"", workbench.title),
                format!("requests={}", workbench.requests.len()),
                format!(
                    "training_rows={}",
                    workbench.training_csv.lines().skip(1).count()
                ),
                format!(
                    "request_labels={}",
                    workbench
                        .requests
                        .iter()
                        .map(|request| request.label.as_str())
                        .collect::<Vec<_>>()
                        .join(",")
                ),
            ],
        },
        None => PatrolScenarioSection {
            name: "Workbench".to_string(),
            available: false,
            summary_lines: vec!["workbench unavailable".to_string()],
        },
    }
}

fn build_stream_section(stream_status: Option<&StreamRuntimeStatus>) -> PatrolScenarioSection {
    match stream_status {
        Some(stream_status) => PatrolScenarioSection {
            name: "Stream".to_string(),
            available: true,
            summary_lines: vec![
                format!("checkpoint_key={}", stream_status.checkpoint_key),
                format!(
                    "last_event_id={}",
                    stream_status.last_event_id.as_deref().unwrap_or("none")
                ),
                format!("delivered_events={}", stream_status.delivered_events),
                format!("filtered_events={}", stream_status.filtered_events),
                format!("reconnect_attempts={}", stream_status.reconnect_attempts),
            ],
        },
        None => PatrolScenarioSection {
            name: "Stream".to_string(),
            available: false,
            summary_lines: vec!["stream unavailable".to_string()],
        },
    }
}

fn build_backlog_section(backlog_status: Option<&BacklogRuntimeStatus>) -> PatrolScenarioSection {
    match backlog_status {
        Some(backlog_status) => PatrolScenarioSection {
            name: "Backlog".to_string(),
            available: true,
            summary_lines: vec![
                format!("checkpoint_key={}", backlog_status.checkpoint_key),
                format!(
                    "next_continue={}",
                    backlog_status.next_continue.as_deref().unwrap_or("none")
                ),
                format!("last_batch_size={}", backlog_status.last_batch_size),
                format!("total_events={}", backlog_status.total_events),
                format!("poll_count={}", backlog_status.poll_count),
            ],
        },
        None => PatrolScenarioSection {
            name: "Backlog".to_string(),
            available: false,
            summary_lines: vec!["backlog unavailable".to_string()],
        },
    }
}

fn build_coordination_section(
    coordination: Option<&CoordinationStateSummary>,
) -> PatrolScenarioSection {
    match coordination {
        Some(coordination) => PatrolScenarioSection {
            name: "Coordination".to_string(),
            available: true,
            summary_lines: vec![
                format!("wiki_id={}", coordination.wiki_id),
                format!("claims={}", coordination.claims.len()),
                format!("presence={}", coordination.presence.len()),
                format!("flagged_edits={}", coordination.flagged_edits.len()),
                format!("score_deltas={}", coordination.score_deltas.len()),
                format!("race_resolutions={}", coordination.race_resolutions.len()),
                format!("recent_actions={}", coordination.recent_actions.len()),
            ],
        },
        None => PatrolScenarioSection {
            name: "Coordination".to_string(),
            available: false,
            summary_lines: vec!["coordination unavailable".to_string()],
        },
    }
}

fn normalize_code(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

fn format_severity(severity: ReportSeverity) -> &'static str {
    match severity {
        ReportSeverity::Info => "INFO",
        ReportSeverity::Warning => "WARN",
        ReportSeverity::Blocker => "BLOCK",
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;

    use crate::backlog_runtime::BacklogRuntimeStatus;
    use crate::config_parser::parse_wiki_config;
    use crate::coordination_state::CoordinationStateSummary;
    use crate::diff_engine::diff_lines;
    use crate::review_workbench::build_review_workbench;
    use crate::scoring_engine::score_edit;
    use crate::stream_runtime::StreamRuntimeStatus;
    use crate::types::{
        Action, ActionBroadcast, EditEvent, EditorIdentity, FlaggedEdit, PresenceHeartbeat,
        QueuedEdit, RaceResolution, ScoreDelta, ScoringConfig, ScoringContext, UserRiskProfile,
        WarningLevel,
    };

    use super::{
        PatrolScenarioReadiness, PatrolScenarioReportInputs, ReportSeverity,
        build_patrol_scenario_report, render_patrol_scenario_markdown, render_patrol_scenario_text,
    };

    fn queue_item_strategy() -> impl Strategy<Value = QueuedEdit> {
        (
            1_u64..1_000_000_u64,
            -200_i32..200_i32,
            any::<bool>(),
            any::<bool>(),
            any::<bool>(),
        )
            .prop_map(|(rev_id, byte_delta, anonymous, new_page, bot)| {
                let performer = if anonymous {
                    EditorIdentity::Anonymous {
                        label: "192.0.2.10".to_string(),
                    }
                } else {
                    EditorIdentity::Registered {
                        username: "ExampleUser".to_string(),
                    }
                };
                let event = EditEvent {
                    wiki_id: "frwiki".to_string(),
                    title: format!("Article {rev_id}"),
                    namespace: 0,
                    rev_id,
                    old_rev_id: Some(rev_id - 1),
                    performer,
                    timestamp_ms: 1_710_000_000_000,
                    is_bot: bot.into(),
                    is_minor: false.into(),
                    is_new_page: new_page.into(),
                    tags: if bot {
                        vec!["mw-reverted".to_string()]
                    } else {
                        Vec::new()
                    },
                    comment: Some("scenario".to_string()),
                    byte_delta,
                    is_patrolled: false.into(),
                };
                let score =
                    score_edit(&event, &ScoringConfig::default()).expect("score should compute");
                QueuedEdit { event, score }
            })
    }

    fn supporting_data() -> (
        ScoringContext,
        StreamRuntimeStatus,
        BacklogRuntimeStatus,
        CoordinationStateSummary,
    ) {
        let scoring_context = ScoringContext {
            user_risk: Some(UserRiskProfile {
                warning_level: WarningLevel::Level2,
                warning_count: 2,
                has_recent_vandalism_templates: true,
            }),
            liftwing_risk: Some(0.72),
        };
        let stream_status = StreamRuntimeStatus {
            checkpoint_key: "stream.last_event_id.frwiki".to_string(),
            last_event_id: Some("event-2".to_string()),
            delivered_events: 3,
            filtered_events: 1,
            reconnect_attempts: 1,
        };
        let backlog_status = BacklogRuntimeStatus {
            checkpoint_key: "recentchanges.rccontinue.frwiki".to_string(),
            next_continue: Some("20260324010202|456".to_string()),
            last_batch_size: 25,
            total_events: 80,
            poll_count: 4,
        };
        let coordination = CoordinationStateSummary {
            wiki_id: "frwiki".to_string(),
            claims: vec![crate::types::EditClaim {
                wiki_id: "frwiki".to_string(),
                rev_id: 123_456,
                actor: "Reviewer".to_string(),
            }],
            presence: vec![PresenceHeartbeat {
                wiki_id: "frwiki".to_string(),
                actor: "Reviewer".to_string(),
                active_edit_count: 1,
            }],
            flagged_edits: vec![FlaggedEdit {
                wiki_id: "frwiki".to_string(),
                rev_id: 123_456,
                score: 91,
                reason: "possible vandalism".to_string(),
            }],
            score_deltas: vec![ScoreDelta {
                wiki_id: "frwiki".to_string(),
                rev_id: 123_456,
                delta: 8,
                reason: "liftwing".to_string(),
            }],
            race_resolutions: vec![RaceResolution {
                wiki_id: "frwiki".to_string(),
                rev_id: 123_456,
                winning_actor: "Reviewer".to_string(),
            }],
            recent_actions: vec![ActionBroadcast {
                wiki_id: "frwiki".to_string(),
                rev_id: 123_456,
                action: Action::Rollback,
                actor: "Reviewer".to_string(),
            }],
        };

        (scoring_context, stream_status, backlog_status, coordination)
    }

    #[test]
    fn empty_queue_is_blocked() {
        let report = build_patrol_scenario_report(&PatrolScenarioReportInputs::default());

        assert_eq!(report.readiness, PatrolScenarioReadiness::Blocked);
        assert_eq!(report.queue_depth, 0);
        assert!(report.selected.is_none());
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.severity == ReportSeverity::Blocker)
        );
        assert!(
            report
                .debug_snapshot
                .summary_lines
                .contains(&"queue_depth=0".to_string())
        );
    }

    #[test]
    fn builds_full_report_from_available_inputs() {
        let config = parse_wiki_config(include_str!("../../../configs/frwiki.yaml"))
            .expect("config should parse");
        let queue = vec![
            queue_item_strategy()
                .new_tree(&mut TestRunner::default())
                .expect("strategy should build")
                .current(),
        ];
        let diff = diff_lines("Avant\n", "Avant\nApres\n");
        let workbench = build_review_workbench(
            &config,
            &queue[0],
            "token-123",
            "Reviewer",
            Some("scenario"),
        )
        .expect("workbench should build");
        let (scoring_context, stream_status, backlog_status, coordination) = supporting_data();

        let report = build_patrol_scenario_report(&PatrolScenarioReportInputs {
            queue: &queue,
            selected: None,
            scoring_context: Some(&scoring_context),
            diff: Some(&diff),
            review_workbench: Some(&workbench),
            stream_status: Some(&stream_status),
            backlog_status: Some(&backlog_status),
            coordination: Some(&coordination),
            wiki_id_hint: Some("frwiki"),
        });

        assert_eq!(report.queue_depth, 1);
        assert_eq!(report.readiness, PatrolScenarioReadiness::Ready);
        assert_eq!(report.sections.len(), 7);
        assert_eq!(
            report.selected.as_ref().map(|item| item.rev_id),
            Some(queue[0].event.rev_id)
        );
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.code == "diff_changes")
        );
        assert!(
            report
                .debug_snapshot
                .summary_lines
                .iter()
                .any(|line| line == "queue_depth=1")
        );
        assert!(render_patrol_scenario_text(&report).contains("Patrol report"));
        assert!(render_patrol_scenario_text(&report).contains("wiki=frwiki"));
        assert!(render_patrol_scenario_markdown(&report).contains("# Patrol report"));
    }

    #[test]
    fn serializes_and_deserializes_report() {
        let config = parse_wiki_config(include_str!("../../../configs/frwiki.yaml"))
            .expect("config should parse");
        let queue = vec![
            queue_item_strategy()
                .new_tree(&mut TestRunner::default())
                .expect("strategy should build")
                .current(),
            queue_item_strategy()
                .new_tree(&mut TestRunner::default())
                .expect("strategy should build")
                .current(),
        ];
        let diff = diff_lines("alpha\n", "alpha\nbeta\n");
        let workbench = build_review_workbench(
            &config,
            &queue[0],
            "token-123",
            "Reviewer",
            Some("scenario"),
        )
        .expect("workbench should build");
        let (scoring_context, stream_status, backlog_status, coordination) = supporting_data();
        let report = build_patrol_scenario_report(&PatrolScenarioReportInputs {
            queue: &queue,
            selected: Some(&queue[0]),
            scoring_context: Some(&scoring_context),
            diff: Some(&diff),
            review_workbench: Some(&workbench),
            stream_status: Some(&stream_status),
            backlog_status: Some(&backlog_status),
            coordination: Some(&coordination),
            wiki_id_hint: Some("frwiki"),
        });
        let encoded = serde_json::to_string(&report).expect("report should serialize");
        let decoded: super::PatrolScenarioReport =
            serde_json::from_str(&encoded).expect("report should deserialize");

        assert_eq!(report, decoded);
    }

    proptest! {
        #[test]
        fn report_preserves_queue_depth_and_selection(queue in prop::collection::vec(queue_item_strategy(), 1..5)) {
            let diff = diff_lines("alpha\n", "alpha\nbeta\n");
            let config = parse_wiki_config(include_str!("../../../configs/frwiki.yaml"))
                .expect("config should parse");
            let workbench = build_review_workbench(
                &config,
                &queue[0],
                "token-123",
                "Reviewer",
                Some("scenario"),
            )
            .expect("workbench should build");
            let (scoring_context, stream_status, backlog_status, coordination) = supporting_data();
            let report = build_patrol_scenario_report(&PatrolScenarioReportInputs {
                queue: &queue,
                selected: None,
                scoring_context: Some(&scoring_context),
                diff: Some(&diff),
                review_workbench: Some(&workbench),
                stream_status: Some(&stream_status),
                backlog_status: Some(&backlog_status),
                coordination: Some(&coordination),
                wiki_id_hint: Some("frwiki"),
            });

            prop_assert_eq!(report.queue_depth, queue.len());
            prop_assert_eq!(
                report.selected.as_ref().map(|item| item.rev_id),
                Some(queue[0].event.rev_id)
            );
            let expected_depth = format!("queue_depth={}", queue.len());
            prop_assert_eq!(
                report.debug_snapshot.summary_lines.first().map(String::as_str),
                Some(expected_depth.as_str())
            );
            prop_assert_eq!(report.sections.len(), 7);
            prop_assert_ne!(report.readiness, PatrolScenarioReadiness::Blocked);
        }
    }

    #[test]
    fn report_severity_display_produces_human_readable_labels() {
        assert_eq!(ReportSeverity::Info.to_string(), "Info");
        assert_eq!(ReportSeverity::Warning.to_string(), "Warning");
        assert_eq!(ReportSeverity::Blocker.to_string(), "Blocker");
    }
}
