//! Shared shell-state model tying together patrol reports, operator summaries,
//! and the interactive action timeline.

use serde::{Deserialize, Serialize};

use crate::operator_summary::{
    PatrolOperatorSummary, PatrolOperatorSummaryInputs, build_patrol_operator_summary,
};
use crate::patrol_scenario_report::{PatrolScenarioReadiness, PatrolScenarioReport};
use crate::patrol_session_digest::PatrolSessionSelectedSummary;
use crate::report_document::{
    ReportDocument, ReportSection, render_report_document_markdown, render_report_document_text,
};
use crate::review_workbench::ReviewWorkbench;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShellTimelineStage {
    Queue,
    Selected,
    Context,
    Diff,
    Workbench,
    Stream,
    Backlog,
    Coordination,
    OperatorSummary,
    Readiness,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShellTimelineEntry {
    pub stage: ShellTimelineStage,
    pub available: bool,
    pub headline: String,
    pub detail_lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShellPanelSummary {
    pub name: String,
    pub available: bool,
    pub headline: String,
    pub detail_lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShellStateModel {
    pub wiki_id: String,
    pub readiness: PatrolScenarioReadiness,
    pub queue_depth: usize,
    pub selected: Option<PatrolSessionSelectedSummary>,
    pub operator_summary: PatrolOperatorSummary,
    pub timeline: Vec<ShellTimelineEntry>,
    pub panels: Vec<ShellPanelSummary>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellStateInputs<'a> {
    pub report: &'a PatrolScenarioReport,
    pub review_workbench: Option<&'a ReviewWorkbench>,
}

#[must_use]
pub fn build_shell_state_model(inputs: &ShellStateInputs<'_>) -> ShellStateModel {
    let report = inputs.report;
    let operator_summary = build_patrol_operator_summary(&PatrolOperatorSummaryInputs {
        report,
        review_workbench: inputs.review_workbench,
    });
    let selected = report
        .selected
        .as_ref()
        .map(|selected| PatrolSessionSelectedSummary {
            rev_id: selected.rev_id,
            title: selected.title.clone(),
            score: selected.score,
            signals: selected.signals,
        });

    let panels = report
        .sections
        .iter()
        .map(|section| ShellPanelSummary {
            name: section.name.clone(),
            available: section.available,
            headline: section
                .summary_lines
                .first()
                .cloned()
                .unwrap_or_else(|| "no details available".to_string()),
            detail_lines: section.summary_lines.clone(),
        })
        .collect::<Vec<_>>();

    let timeline = build_timeline_entries(
        report,
        selected.as_ref(),
        inputs.review_workbench,
        &operator_summary,
    );
    let notes = build_shell_notes(report, inputs.review_workbench, &timeline);

    ShellStateModel {
        wiki_id: report.wiki_id.clone(),
        readiness: report.readiness,
        queue_depth: report.queue_depth,
        selected,
        operator_summary,
        timeline,
        panels,
        notes,
    }
}

#[must_use]
pub fn render_shell_state_text(model: &ShellStateModel) -> String {
    render_report_document_text(&model.to_report_document())
}

#[must_use]
pub fn render_shell_state_markdown(model: &ShellStateModel) -> String {
    render_report_document_markdown(&model.to_report_document())
}

impl ShellStateModel {
    #[must_use]
    pub fn to_report_document(&self) -> ReportDocument {
        let mut lead_lines = vec![
            format!("wiki_id={}", self.wiki_id),
            format!("readiness={:?}", self.readiness),
            format!("queue_depth={}", self.queue_depth),
            format!("timeline_steps={}", self.timeline.len()),
            format!("panel_count={}", self.panels.len()),
        ];

        if let Some(selected) = &self.selected {
            lead_lines.push(format!(
                "selected rev={} title=\"{}\" score={} signals={}",
                selected.rev_id, selected.title, selected.score, selected.signals
            ));
        } else {
            lead_lines.push("selected=none".to_string());
        }

        let sections = vec![
            ReportSection {
                name: "Timeline".to_string(),
                available: !self.timeline.is_empty(),
                summary_lines: self
                    .timeline
                    .iter()
                    .flat_map(|entry| {
                        let mut lines = vec![timeline_line(entry)];
                        lines.extend(entry.detail_lines.iter().map(|line| format!("  {line}")));
                        lines
                    })
                    .collect(),
            },
            ReportSection {
                name: "Panels".to_string(),
                available: !self.panels.is_empty(),
                summary_lines: self
                    .panels
                    .iter()
                    .flat_map(|panel| {
                        let mut lines = vec![panel_line(panel)];
                        lines.extend(panel.detail_lines.iter().map(|line| format!("  {line}")));
                        lines
                    })
                    .collect(),
            },
            ReportSection {
                name: "Operator summary".to_string(),
                available: !self.operator_summary.notes.is_empty(),
                summary_lines: self.operator_summary.notes.clone(),
            },
            ReportSection {
                name: "Notes".to_string(),
                available: !self.notes.is_empty(),
                summary_lines: self.notes.clone(),
            },
        ];

        ReportDocument::new("Shell state")
            .with_lead_lines(lead_lines)
            .with_sections(sections)
    }
}

fn build_timeline_entries(
    report: &PatrolScenarioReport,
    selected: Option<&PatrolSessionSelectedSummary>,
    review_workbench: Option<&ReviewWorkbench>,
    operator_summary: &PatrolOperatorSummary,
) -> Vec<ShellTimelineEntry> {
    vec![
        queue_timeline_entry(report),
        selected_timeline_entry(selected),
        section_timeline_entry(
            report,
            ShellTimelineStage::Context,
            "Context",
            review_workbench,
        ),
        section_timeline_entry(report, ShellTimelineStage::Diff, "Diff", review_workbench),
        section_timeline_entry(
            report,
            ShellTimelineStage::Workbench,
            "Workbench",
            review_workbench,
        ),
        section_timeline_entry(
            report,
            ShellTimelineStage::Stream,
            "Stream",
            review_workbench,
        ),
        section_timeline_entry(
            report,
            ShellTimelineStage::Backlog,
            "Backlog",
            review_workbench,
        ),
        section_timeline_entry(
            report,
            ShellTimelineStage::Coordination,
            "Coordination",
            review_workbench,
        ),
        operator_summary_timeline_entry(operator_summary),
        readiness_timeline_entry(report),
    ]
}

fn queue_timeline_entry(report: &PatrolScenarioReport) -> ShellTimelineEntry {
    ShellTimelineEntry {
        stage: ShellTimelineStage::Queue,
        available: report.queue_depth > 0,
        headline: format!("queue_depth={}", report.queue_depth),
        detail_lines: vec![format!("readiness={:?}", report.readiness)],
    }
}

fn selected_timeline_entry(selected: Option<&PatrolSessionSelectedSummary>) -> ShellTimelineEntry {
    match selected {
        Some(selected) => ShellTimelineEntry {
            stage: ShellTimelineStage::Selected,
            available: true,
            headline: format!(
                "rev={} score={} signals={}",
                selected.rev_id, selected.score, selected.signals
            ),
            detail_lines: vec![format!("title=\"{}\"", selected.title)],
        },
        None => ShellTimelineEntry {
            stage: ShellTimelineStage::Selected,
            available: false,
            headline: "selected edit unavailable".to_string(),
            detail_lines: vec!["no selected edit".to_string()],
        },
    }
}

fn section_timeline_entry(
    report: &PatrolScenarioReport,
    stage: ShellTimelineStage,
    name: &str,
    review_workbench: Option<&ReviewWorkbench>,
) -> ShellTimelineEntry {
    match section_by_name(report, name) {
        Some(section) => section_found_timeline_entry(stage, section, review_workbench),
        None if matches!(stage, ShellTimelineStage::Workbench) => {
            workbench_fallback_timeline_entry(stage, review_workbench, name)
        }
        None => missing_timeline_entry(stage, name),
    }
}

fn section_found_timeline_entry(
    stage: ShellTimelineStage,
    section: &crate::patrol_scenario_report::PatrolScenarioSection,
    review_workbench: Option<&ReviewWorkbench>,
) -> ShellTimelineEntry {
    let mut detail_lines: Vec<String> = section.summary_lines.iter().skip(1).cloned().collect();

    if matches!(stage, ShellTimelineStage::Workbench)
        && let Some(workbench) = review_workbench
    {
        detail_lines.push(format!("request_labels={}", request_labels(workbench)));
        detail_lines.push(format!(
            "training_rows={}",
            workbench.training_csv.lines().skip(1).count()
        ));
    }

    ShellTimelineEntry {
        stage,
        available: section.available,
        headline: section
            .summary_lines
            .first()
            .cloned()
            .unwrap_or_else(|| "no details available".to_string()),
        detail_lines,
    }
}

fn workbench_fallback_timeline_entry(
    stage: ShellTimelineStage,
    review_workbench: Option<&ReviewWorkbench>,
    name: &str,
) -> ShellTimelineEntry {
    match review_workbench {
        Some(workbench) => ShellTimelineEntry {
            stage,
            available: true,
            headline: format!("rev={} title=\"{}\"", workbench.rev_id, workbench.title),
            detail_lines: vec![
                format!("request_labels={}", request_labels(workbench)),
                format!(
                    "training_rows={}",
                    workbench.training_csv.lines().skip(1).count()
                ),
            ],
        },
        None => missing_timeline_entry(stage, name),
    }
}

fn missing_timeline_entry(stage: ShellTimelineStage, name: &str) -> ShellTimelineEntry {
    ShellTimelineEntry {
        stage,
        available: false,
        headline: format!("{name} unavailable"),
        detail_lines: vec![format!("{name} data missing")],
    }
}

fn operator_summary_timeline_entry(operator_summary: &PatrolOperatorSummary) -> ShellTimelineEntry {
    ShellTimelineEntry {
        stage: ShellTimelineStage::OperatorSummary,
        available: !operator_summary.notes.is_empty(),
        headline: format!(
            "findings={}",
            operator_summary
                .severity_counts
                .iter()
                .map(|entry| entry.count)
                .sum::<usize>()
        ),
        detail_lines: operator_summary.notes.clone(),
    }
}

fn readiness_timeline_entry(report: &PatrolScenarioReport) -> ShellTimelineEntry {
    ShellTimelineEntry {
        stage: ShellTimelineStage::Readiness,
        available: true,
        headline: format!("{:?}", report.readiness),
        detail_lines: vec![format!("queue_depth={}", report.queue_depth)],
    }
}

fn build_shell_notes(
    report: &PatrolScenarioReport,
    review_workbench: Option<&ReviewWorkbench>,
    timeline: &[ShellTimelineEntry],
) -> Vec<String> {
    let mut notes = vec![
        format!(
            "available_panels={}/{}",
            report
                .sections
                .iter()
                .filter(|section| section.available)
                .count(),
            report.sections.len()
        ),
        format!(
            "available_timeline_steps={}/{}",
            timeline.iter().filter(|entry| entry.available).count(),
            timeline.len()
        ),
    ];

    if let Some(workbench) = review_workbench {
        notes.push(format!(
            "workbench request_labels={}",
            workbench
                .requests
                .iter()
                .map(|request| request.label.as_str())
                .collect::<Vec<_>>()
                .join(",")
        ));
    }

    notes.extend(
        report
            .sections
            .iter()
            .filter(|section| !section.available)
            .map(|section| format!("blocked_panel={}", section.name)),
    );

    notes
}

fn section_by_name<'a>(
    report: &'a PatrolScenarioReport,
    name: &str,
) -> Option<&'a crate::patrol_scenario_report::PatrolScenarioSection> {
    report.sections.iter().find(|section| section.name == name)
}

fn request_labels(workbench: &ReviewWorkbench) -> String {
    workbench
        .requests
        .iter()
        .map(|request| request.label.as_str())
        .collect::<Vec<_>>()
        .join(",")
}

fn timeline_line(entry: &ShellTimelineEntry) -> String {
    format!(
        "{:?} available={} headline=\"{}\"",
        entry.stage, entry.available, entry.headline
    )
}

fn panel_line(panel: &ShellPanelSummary) -> String {
    format!(
        "[{}] available={} headline=\"{}\"",
        panel.name, panel.available, panel.headline
    )
}

#[cfg(test)]
mod tests {
    use crate::config_parser::parse_wiki_config;
    use crate::context_builder::{ContextInputs, build_scoring_context};
    use crate::diff_engine::diff_lines;
    use crate::review_workbench::build_review_workbench;
    use crate::scoring_engine::score_edit;
    use crate::types::{EditEvent, EditorIdentity, QueuedEdit, ScoringConfig};

    use super::{
        ShellStateInputs, build_shell_state_model, render_shell_state_markdown,
        render_shell_state_text,
    };
    use crate::patrol_scenario_report::{PatrolScenarioReportInputs, build_patrol_scenario_report};

    #[test]
    fn builds_shell_state_from_report() {
        let config = parse_wiki_config(include_str!("../../../configs/frwiki.yaml"))
            .expect("config should parse");
        let event = EditEvent {
            wiki_id: "frwiki".to_string(),
            title: "Exemple".to_string(),
            namespace: 0,
            rev_id: 123_456,
            old_rev_id: Some(123_455),
            performer: EditorIdentity::Registered {
                username: "Reviewer".to_string(),
            },
            timestamp_ms: 1_710_000_000_000,
            is_bot: false,
            is_minor: true,
            is_new_page: false,
            tags: vec![],
            comment: Some("cleanup".to_string()),
            byte_delta: 12,
            is_patrolled: false,
        };
        let queue_item = QueuedEdit {
            score: score_edit(&event, &ScoringConfig::default()).expect("score should compute"),
            event,
        };
        let queue = vec![queue_item];
        let context = build_scoring_context(&ContextInputs {
            talk_page_wikitext: Some("{{Avertissement niveau 2 pour vandalisme}}".to_string()),
            liftwing_probability: Some(0.64),
        });
        let diff = diff_lines("Old", "New");
        let workbench = build_review_workbench(
            &config,
            queue.first().expect("queue item"),
            "token-123",
            "Reviewer",
            Some("summary"),
        )
        .expect("workbench should build");
        let report = build_patrol_scenario_report(&PatrolScenarioReportInputs {
            queue: &queue,
            selected: queue.first(),
            scoring_context: Some(&context),
            diff: Some(&diff),
            review_workbench: Some(&workbench),
            stream_status: None,
            backlog_status: None,
            coordination: None,
            wiki_id_hint: None,
        });

        let model = build_shell_state_model(&ShellStateInputs {
            report: &report,
            review_workbench: Some(&workbench),
        });

        assert_eq!(model.wiki_id, "frwiki");
        assert_eq!(model.timeline.len(), 10);
        assert!(
            model
                .timeline
                .iter()
                .any(|entry| matches!(entry.stage, super::ShellTimelineStage::Workbench))
        );
        assert!(
            model
                .notes
                .iter()
                .any(|line| line.contains("workbench request_labels"))
        );
        assert!(
            model
                .to_report_document()
                .lead_lines
                .iter()
                .any(|line| line.contains("timeline_steps=10"))
        );
    }

    #[test]
    fn renders_shell_state_in_text_and_markdown() {
        let event = EditEvent {
            wiki_id: "frwiki".to_string(),
            title: "Exemple".to_string(),
            namespace: 0,
            rev_id: 123_456,
            old_rev_id: Some(123_455),
            performer: EditorIdentity::Registered {
                username: "Reviewer".to_string(),
            },
            timestamp_ms: 1_710_000_000_000,
            is_bot: false,
            is_minor: true,
            is_new_page: false,
            tags: vec![],
            comment: Some("cleanup".to_string()),
            byte_delta: 12,
            is_patrolled: false,
        };
        let queue_item = QueuedEdit {
            score: score_edit(&event, &ScoringConfig::default()).expect("score should compute"),
            event,
        };
        let queue = vec![queue_item];
        let report = build_patrol_scenario_report(&PatrolScenarioReportInputs {
            queue: &queue,
            selected: queue.first(),
            scoring_context: None,
            diff: None,
            review_workbench: None,
            stream_status: None,
            backlog_status: None,
            coordination: None,
            wiki_id_hint: Some("frwiki"),
        });
        let model = build_shell_state_model(&ShellStateInputs {
            report: &report,
            review_workbench: None,
        });

        let text = render_shell_state_text(&model);
        let markdown = render_shell_state_markdown(&model);

        assert!(text.contains("Shell state"));
        assert!(text.contains("Timeline"));
        assert!(markdown.contains("# Shell state"));
        assert!(markdown.contains("## Panels"));
    }
}
