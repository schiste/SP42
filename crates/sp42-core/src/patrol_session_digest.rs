//! Pure digest/explanation layer over patrol scenario findings.

use serde::{Deserialize, Serialize};

use crate::operator_summary::{
    PatrolOperatorSummary, PatrolOperatorSummaryInputs, build_patrol_operator_summary,
};
use crate::patrol_scenario_report::{
    PatrolScenarioFinding, PatrolScenarioReadiness, PatrolScenarioReport, ReportSeverity,
};
use crate::report_document::{
    ReportDocument, ReportSection, render_report_document_markdown, render_report_document_text,
};
use crate::review_workbench::ReviewWorkbench;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatrolSessionSeverityCount {
    pub severity: ReportSeverity,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatrolSessionSelectedSummary {
    pub rev_id: u64,
    pub title: String,
    pub score: i32,
    pub signals: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatrolSessionWorkbenchSummary {
    pub rev_id: u64,
    pub title: String,
    pub request_labels: Vec<String>,
    pub training_rows: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatrolSessionSectionSummary {
    pub name: String,
    pub available: bool,
    pub summary_lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatrolSessionDigest {
    pub wiki_id: String,
    pub queue_depth: usize,
    pub readiness: PatrolScenarioReadiness,
    pub selected: Option<PatrolSessionSelectedSummary>,
    pub findings: Vec<PatrolScenarioFinding>,
    pub severity_counts: Vec<PatrolSessionSeverityCount>,
    pub operator_summary: PatrolOperatorSummary,
    pub sections: Vec<PatrolSessionSectionSummary>,
    pub workbench: Option<PatrolSessionWorkbenchSummary>,
    pub explanation_lines: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PatrolSessionDigestInputs<'a> {
    pub report: &'a PatrolScenarioReport,
    pub review_workbench: Option<&'a ReviewWorkbench>,
}

/// Build a compact session digest that explains why the current patrol view
/// is ready, limited, or blocked.
#[must_use]
pub fn build_patrol_session_digest(inputs: &PatrolSessionDigestInputs<'_>) -> PatrolSessionDigest {
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

    let severity_counts = operator_summary.severity_counts.clone();
    let sections = report
        .sections
        .iter()
        .map(|section| PatrolSessionSectionSummary {
            name: section.name.clone(),
            available: section.available,
            summary_lines: section.summary_lines.clone(),
        })
        .collect::<Vec<_>>();

    PatrolSessionDigest {
        wiki_id: report.wiki_id.clone(),
        queue_depth: report.queue_depth,
        readiness: report.readiness,
        selected,
        findings: report.findings.clone(),
        severity_counts,
        operator_summary,
        sections,
        workbench,
        explanation_lines: build_explanation_lines(report, inputs.review_workbench),
    }
}

/// Render the digest as a plain-text summary.
#[must_use]
pub fn render_patrol_session_digest_text(digest: &PatrolSessionDigest) -> String {
    render_report_document_text(&digest.to_report_document())
}

/// Render the digest as markdown.
#[must_use]
pub fn render_patrol_session_digest_markdown(digest: &PatrolSessionDigest) -> String {
    render_report_document_markdown(&digest.to_report_document())
}

impl PatrolSessionDigest {
    #[must_use]
    pub fn to_report_document(&self) -> ReportDocument {
        let mut lead_lines = vec![
            format!("wiki={}", self.wiki_id),
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

        let mut sections = vec![ReportSection {
            name: "Operator summary".to_string(),
            available: !self.operator_summary.notes.is_empty(),
            summary_lines: self.operator_summary.notes.clone(),
        }];

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
                summary_lines: self.findings.iter().map(format_finding_text).collect(),
            });
        }

        sections.push(ReportSection {
            name: "Sections".to_string(),
            available: !self.sections.is_empty(),
            summary_lines: self
                .sections
                .iter()
                .map(|section| {
                    format!(
                        "[{}] available={} lines={}",
                        section.name,
                        section.available,
                        section.summary_lines.len()
                    )
                })
                .collect(),
        });

        if !self.explanation_lines.is_empty() {
            sections.push(ReportSection {
                name: "Explanation".to_string(),
                available: true,
                summary_lines: self.explanation_lines.clone(),
            });
        }

        ReportDocument::new("Patrol session digest")
            .with_lead_lines(lead_lines)
            .with_sections(sections)
    }
}

fn build_explanation_lines(
    report: &PatrolScenarioReport,
    review_workbench: Option<&ReviewWorkbench>,
) -> Vec<String> {
    let mut lines = vec![
        format!("wiki_id={}", report.wiki_id),
        format!("readiness={:?}", report.readiness),
        format!("queue_depth={}", report.queue_depth),
    ];

    if let Some(selected) = &report.selected {
        lines.push(format!(
            "selected rev={} title=\"{}\" score={} signals={}",
            selected.rev_id, selected.title, selected.score, selected.signals
        ));
    } else {
        lines.push("selected=none".to_string());
    }

    lines.extend(report.findings.iter().map(format_finding_text));

    for section in &report.sections {
        lines.push(format!(
            "section {} available={}",
            section.name, section.available
        ));
        lines.extend(section.summary_lines.iter().map(|line| format!("  {line}")));
    }

    if let Some(workbench) = review_workbench {
        lines.push(format!(
            "workbench rev={} requests={} training_rows={}",
            workbench.rev_id,
            workbench.requests.len(),
            workbench.training_csv.lines().skip(1).count()
        ));
        lines.push(format!(
            "workbench request_labels={}",
            workbench
                .requests
                .iter()
                .map(|request| request.label.as_str())
                .collect::<Vec<_>>()
                .join(",")
        ));
    }

    lines
}

fn severity_counts_line(counts: &[PatrolSessionSeverityCount]) -> String {
    let mut parts = Vec::new();

    for count in counts {
        parts.push(format!("{:?}={}", count.severity, count.count));
    }

    format!("severity_counts {}", parts.join(" "))
}

fn format_finding_text(finding: &PatrolScenarioFinding) -> String {
    format!(
        "{:?} {}: {}",
        finding.severity, finding.code, finding.message
    )
}

#[cfg(test)]
mod tests {
    use crate::config_parser::parse_wiki_config;
    use crate::context_builder::{ContextInputs, build_scoring_context};
    use crate::diff_engine::diff_lines;
    use crate::review_workbench::build_review_workbench;
    use crate::scoring_engine::score_edit;
    use crate::types::{
        EditEvent, EditorIdentity, QueuedEdit, ScoringConfig, ScoringContext, UserRiskProfile,
        WarningLevel,
    };

    use super::{
        PatrolSessionDigestInputs, build_patrol_session_digest,
        render_patrol_session_digest_markdown, render_patrol_session_digest_text,
    };
    use crate::patrol_scenario_report::{
        PatrolScenarioReadiness, PatrolScenarioReportInputs, ReportSeverity,
        build_patrol_scenario_report,
    };

    #[test]
    fn builds_digest_from_scenario_report() {
        let config = parse_wiki_config(include_str!("../../../configs/frwiki.yaml"))
            .expect("config should parse");
        let event = EditEvent {
            wiki_id: "frwiki".to_string(),
            title: "Example".to_string(),
            namespace: 0,
            rev_id: 123_456,
            old_rev_id: Some(123_455),
            performer: EditorIdentity::Anonymous {
                label: "192.0.2.8".to_string(),
            },
            timestamp_ms: 1_710_000_000_000,
            is_bot: false.into(),
            is_minor: false.into(),
            is_new_page: true.into(),
            tags: vec!["mw-new-redirect".to_string()],
            comment: Some("cleanup".to_string()),
            byte_delta: -28,
            is_patrolled: false.into(),
        };
        let queue_item = QueuedEdit {
            score: score_edit(&event, &ScoringConfig::default()).expect("score should compute"),
            event,
        };
        let queue = vec![queue_item];
        let context = ScoringContext {
            user_risk: Some(UserRiskProfile {
                warning_level: WarningLevel::Level3,
                warning_count: 3,
                has_recent_vandalism_templates: true,
            }),
            liftwing_risk: Some(0.81),
        };
        let diff = diff_lines("A", "B");
        let workbench = build_review_workbench(
            &config,
            queue.first().expect("queue item"),
            "token-123",
            "Reviewer",
            Some("digest"),
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

        let digest = build_patrol_session_digest(&PatrolSessionDigestInputs {
            report: &report,
            review_workbench: Some(&workbench),
        });

        assert_eq!(digest.wiki_id, "frwiki");
        assert_eq!(digest.readiness, PatrolScenarioReadiness::Limited);
        assert!(
            digest
                .severity_counts
                .iter()
                .any(|entry| entry.severity == ReportSeverity::Warning && entry.count >= 3)
        );
        assert!(
            digest
                .explanation_lines
                .iter()
                .any(|line| line.contains("selected rev=123456"))
        );
        assert!(
            digest
                .explanation_lines
                .iter()
                .any(|line| line.contains("workbench request_labels=rollback,patrol,undo"))
        );
    }

    #[test]
    fn renders_digest_in_text_and_markdown() {
        let config = parse_wiki_config(include_str!("../../../configs/frwiki.yaml"))
            .expect("config should parse");
        let event = EditEvent {
            wiki_id: "frwiki".to_string(),
            title: "Example".to_string(),
            namespace: 0,
            rev_id: 123_456,
            old_rev_id: Some(123_455),
            performer: EditorIdentity::Registered {
                username: "Reviewer".to_string(),
            },
            timestamp_ms: 1_710_000_000_000,
            is_bot: false.into(),
            is_minor: true.into(),
            is_new_page: false.into(),
            tags: vec![],
            comment: Some("cleanup".to_string()),
            byte_delta: 12,
            is_patrolled: false.into(),
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
            Some("digest"),
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
        let digest = build_patrol_session_digest(&PatrolSessionDigestInputs {
            report: &report,
            review_workbench: Some(&workbench),
        });

        let text = render_patrol_session_digest_text(&digest);
        let markdown = render_patrol_session_digest_markdown(&digest);

        assert!(text.contains("Patrol session digest"));
        assert!(text.contains("wiki=frwiki"));
        assert!(text.contains("severity_counts"));
        assert!(text.contains("findings"));
        assert!(markdown.contains("# Patrol session digest"));
        assert!(markdown.contains("## Findings"));
        assert!(markdown.contains("## Explanation"));
    }
}
