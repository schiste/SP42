//! Structured operator-facing summary for patrol sessions.

use serde::{Deserialize, Serialize};

use crate::patrol_scenario_report::{
    PatrolScenarioReadiness, PatrolScenarioReport, ReportSeverity,
};
use crate::patrol_session_digest::{
    PatrolSessionSelectedSummary, PatrolSessionSeverityCount, PatrolSessionWorkbenchSummary,
};
use crate::report_document::{
    ReportDocument, ReportSection, render_report_document_markdown, render_report_document_text,
};
use crate::review_workbench::ReviewWorkbench;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatrolOperatorSectionSummary {
    pub name: String,
    pub available: bool,
    pub headline: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatrolOperatorSummary {
    pub wiki_id: String,
    pub readiness: PatrolScenarioReadiness,
    pub queue_depth: usize,
    pub selected: Option<PatrolSessionSelectedSummary>,
    pub severity_counts: Vec<PatrolSessionSeverityCount>,
    pub section_overview: Vec<PatrolOperatorSectionSummary>,
    pub workbench: Option<PatrolSessionWorkbenchSummary>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PatrolOperatorSummaryInputs<'a> {
    pub report: &'a PatrolScenarioReport,
    pub review_workbench: Option<&'a ReviewWorkbench>,
}

#[must_use]
pub fn build_patrol_operator_summary(
    inputs: &PatrolOperatorSummaryInputs<'_>,
) -> PatrolOperatorSummary {
    let report = inputs.report;
    let selected = report
        .selected
        .as_ref()
        .map(|selected| PatrolSessionSelectedSummary {
            rev_id: selected.rev_id,
            title: selected.title.clone(),
            score: selected.score,
            signals: selected.signals,
        });
    let workbench = inputs
        .review_workbench
        .map(|workbench| PatrolSessionWorkbenchSummary {
            rev_id: workbench.rev_id,
            title: workbench.title.clone(),
            request_labels: workbench
                .requests
                .iter()
                .map(|request| request.label.clone())
                .collect(),
            training_rows: workbench.training_csv.lines().skip(1).count(),
        });

    let severity_counts = severity_counts(&report.findings);
    let section_overview = report
        .sections
        .iter()
        .map(|section| PatrolOperatorSectionSummary {
            name: section.name.clone(),
            available: section.available,
            headline: section
                .summary_lines
                .first()
                .cloned()
                .unwrap_or_else(|| "no details available".to_string()),
        })
        .collect::<Vec<_>>();

    PatrolOperatorSummary {
        wiki_id: report.wiki_id.clone(),
        readiness: report.readiness,
        queue_depth: report.queue_depth,
        selected,
        severity_counts,
        section_overview,
        workbench,
        notes: build_operator_notes(report, inputs.review_workbench),
    }
}

#[must_use]
pub fn render_patrol_operator_summary_text(summary: &PatrolOperatorSummary) -> String {
    render_report_document_text(&summary.to_report_document())
}

#[must_use]
pub fn render_patrol_operator_summary_markdown(summary: &PatrolOperatorSummary) -> String {
    render_report_document_markdown(&summary.to_report_document())
}

impl PatrolOperatorSummary {
    #[must_use]
    pub fn to_report_document(&self) -> ReportDocument {
        let mut lead_lines = vec![
            format!("wiki_id={}", self.wiki_id),
            format!("readiness={:?}", self.readiness),
            format!("queue_depth={}", self.queue_depth),
            severity_counts_line(&self.severity_counts),
        ];

        if let Some(selected) = &self.selected {
            lead_lines.push(format!(
                "selected rev={} title=\"{}\" score={} signals={}",
                selected.rev_id, selected.title, selected.score, selected.signals
            ));
        } else {
            lead_lines.push("selected=none".to_string());
        }

        if let Some(workbench) = &self.workbench {
            lead_lines.push(format!(
                "workbench rev={} title=\"{}\" requests={} training_rows={}",
                workbench.rev_id,
                workbench.title,
                workbench.request_labels.len(),
                workbench.training_rows
            ));
        }

        ReportDocument::new("Patrol operator summary")
            .with_lead_lines(lead_lines)
            .with_sections(vec![
                ReportSection {
                    name: "Section overview".to_string(),
                    available: !self.section_overview.is_empty(),
                    summary_lines: self
                        .section_overview
                        .iter()
                        .map(|section| {
                            format!(
                                "[{}] available={} headline=\"{}\"",
                                section.name, section.available, section.headline
                            )
                        })
                        .collect(),
                },
                ReportSection {
                    name: "Operational notes".to_string(),
                    available: !self.notes.is_empty(),
                    summary_lines: self.notes.clone(),
                },
            ])
    }
}

fn build_operator_notes(
    report: &PatrolScenarioReport,
    review_workbench: Option<&ReviewWorkbench>,
) -> Vec<String> {
    let mut notes = vec![
        format!("findings={}", report.findings.len()),
        format!(
            "available_sections={}/{}",
            report
                .sections
                .iter()
                .filter(|section| section.available)
                .count(),
            report.sections.len()
        ),
    ];

    let blocked_sections = report
        .sections
        .iter()
        .filter(|section| !section.available)
        .map(|section| section.name.as_str())
        .collect::<Vec<_>>();
    if !blocked_sections.is_empty() {
        notes.push(format!("blocked_sections={}", blocked_sections.join(",")));
    }

    let top_findings = report
        .findings
        .iter()
        .take(3)
        .map(|finding| {
            format!(
                "{:?} {}: {}",
                finding.severity, finding.code, finding.message
            )
        })
        .collect::<Vec<_>>();
    if !top_findings.is_empty() {
        notes.push("top_findings".to_string());
        notes.extend(top_findings);
    }

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

    notes
}

fn severity_counts(
    findings: &[crate::patrol_scenario_report::PatrolScenarioFinding],
) -> Vec<PatrolSessionSeverityCount> {
    let mut info = 0usize;
    let mut warning = 0usize;
    let mut blocker = 0usize;

    for finding in findings {
        match finding.severity {
            ReportSeverity::Info => info = info.saturating_add(1),
            ReportSeverity::Warning => warning = warning.saturating_add(1),
            ReportSeverity::Blocker => blocker = blocker.saturating_add(1),
        }
    }

    vec![
        PatrolSessionSeverityCount {
            severity: ReportSeverity::Info,
            count: info,
        },
        PatrolSessionSeverityCount {
            severity: ReportSeverity::Warning,
            count: warning,
        },
        PatrolSessionSeverityCount {
            severity: ReportSeverity::Blocker,
            count: blocker,
        },
    ]
}

fn severity_counts_line(counts: &[PatrolSessionSeverityCount]) -> String {
    let mut parts = Vec::new();

    for count in counts {
        parts.push(format!("{:?}={}", count.severity, count.count));
    }

    format!("severity_counts {}", parts.join(" "))
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
        PatrolOperatorSummaryInputs, build_patrol_operator_summary,
        render_patrol_operator_summary_markdown, render_patrol_operator_summary_text,
    };
    use crate::patrol_scenario_report::{PatrolScenarioReportInputs, build_patrol_scenario_report};

    #[test]
    fn builds_operator_summary_from_report() {
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

        let summary = build_patrol_operator_summary(&PatrolOperatorSummaryInputs {
            report: &report,
            review_workbench: Some(&workbench),
        });

        assert_eq!(summary.wiki_id, "frwiki");
        assert_eq!(summary.queue_depth, 1);
        assert!(
            summary
                .notes
                .iter()
                .any(|line| line.contains("blocked_sections"))
        );
        assert!(
            summary
                .to_report_document()
                .lead_lines
                .iter()
                .any(|line| line.contains("selected rev=123456"))
        );
    }

    #[test]
    fn renders_operator_summary_in_text_and_markdown() {
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
        let summary = build_patrol_operator_summary(&PatrolOperatorSummaryInputs {
            report: &report,
            review_workbench: None,
        });

        let text = render_patrol_operator_summary_text(&summary);
        let markdown = render_patrol_operator_summary_markdown(&summary);

        assert!(text.contains("Patrol operator summary"));
        assert!(markdown.contains("# Patrol operator summary"));
        assert!(markdown.contains("## Operational notes"));
    }
}
