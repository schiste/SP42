use leptos::prelude::*;
use sp42_core::{PatrolScenarioReadiness, PatrolScenarioReport, ReportSeverity};

use super::{InspectorFeed, StatusBadge, StatusTone, inspector_entries_from_lines};

#[component]
pub fn PatrolSessionDigestPanel(report: PatrolScenarioReport) -> impl IntoView {
    let badges = session_digest_badges(&report);
    let digest_lines = session_digest_lines(&report);
    let recommendation = recommended_next_step(&report);
    let findings = report.findings.clone();

    view! {
        <section
            style="display:grid;gap:10px;padding:17px;border-radius:6px;border:1px solid rgba(148,163,184,.18);background:rgba(11,18,33,.96);"
        >
            <header style="display:grid;gap:7px;">
                <div style="display:flex;align-items:center;gap:7px;flex-wrap:wrap;">
                    <StatusBadge label="Session Digest".to_string() tone=StatusTone::Accent />
                    {badges
                        .into_iter()
                        .map(|(label, tone)| view! { <StatusBadge label=label tone=tone /> })
                        .collect_view()}
                </div>
                <p style="margin:0;color:#8b9fc0;">
                    "A patrol-first summary that turns the live queue into a quick review decision before the action rail and diff."
                </p>
            </header>

            <div style="display:grid;grid-template-columns:repeat(auto-fit,minmax(280px,1fr));gap:10px;">
                <article
                    style="display:grid;gap:7px;padding:10px 17px;border-radius:4px;border:1px solid rgba(148,163,184,.18);background:rgba(15,23,42,.58);"
                >
                    <div style="display:flex;align-items:center;justify-content:space-between;gap:7px;flex-wrap:wrap;">
                        <h3 style="margin:0;font-size:1rem;">"Session Flow"</h3>
                        <StatusBadge label=recommendation tone=recommendation_tone(&report) />
                    </div>
                    <InspectorFeed entries=inspector_entries_from_lines(&digest_lines) />
                </article>

                <article
                    style="display:grid;gap:7px;padding:10px 17px;border-radius:4px;border:1px solid rgba(148,163,184,.18);background:rgba(15,23,42,.58);"
                >
                    <div style="display:flex;align-items:center;justify-content:space-between;gap:7px;flex-wrap:wrap;">
                        <h3 style="margin:0;font-size:1rem;">"Decision Signals"</h3>
                        <StatusBadge
                            label=format!("{} finding(s)", findings.len())
                            tone={finding_summary_tone(&findings)}
                        />
                    </div>
                    <ul style="margin:0;padding-inline-start:17px;color:#eff4ff;display:grid;gap:4px;">
                        {findings
                            .into_iter()
                            .map(|finding| view! { <li>{finding_summary_line(&finding)}</li> })
                            .collect_view()}
                    </ul>
                </article>
            </div>
        </section>
    }
}

#[must_use]
pub fn session_digest_badges(report: &PatrolScenarioReport) -> Vec<(String, StatusTone)> {
    let selected_badge = report
        .selected
        .as_ref()
        .map(|selected| {
            (
                format!("rev {}", selected.rev_id),
                if selected.score >= 80 {
                    StatusTone::Warning
                } else {
                    StatusTone::Success
                },
            )
        })
        .unwrap_or_else(|| ("no selection".to_string(), StatusTone::Warning));

    vec![
        (
            format!("{} queue", report.queue_depth),
            if report.queue_depth == 0 {
                StatusTone::Warning
            } else {
                StatusTone::Success
            },
        ),
        (
            readiness_label(report.readiness).to_string(),
            readiness_tone(report.readiness),
        ),
        selected_badge,
        (
            format!("{} finding(s)", report.findings.len()),
            finding_summary_tone(&report.findings),
        ),
    ]
}

#[must_use]
pub fn session_digest_lines(report: &PatrolScenarioReport) -> Vec<String> {
    let mut lines = vec![format!(
        "queue depth={} readiness={:?} wiki={}",
        report.queue_depth, report.readiness, report.wiki_id
    )];

    if let Some(selected) = &report.selected {
        lines.push(format!(
            "selected rev={} title=\"{}\" score={} signals={}",
            selected.rev_id, selected.title, selected.score, selected.signals
        ));
    } else {
        lines.push("selected unavailable".to_string());
    }

    lines.extend([
        format!("context {}", section_summary(report, "Context")),
        format!("diff {}", section_summary(report, "Diff")),
        format!("action rail {}", section_summary(report, "Workbench")),
        format!("coordination {}", section_summary(report, "Coordination")),
        format!("next_step={}", recommended_next_step(report)),
    ]);

    lines
}

#[must_use]
pub fn recommended_next_step(report: &PatrolScenarioReport) -> String {
    if report.queue_depth == 0 || report.readiness == PatrolScenarioReadiness::Blocked {
        "inspect the live signal before acting".to_string()
    } else if report
        .findings
        .iter()
        .any(|finding| finding.severity != ReportSeverity::Info)
    {
        "inspect diff before action".to_string()
    } else {
        "prepare patrol action".to_string()
    }
}

#[must_use]
pub fn recommendation_tone(report: &PatrolScenarioReport) -> StatusTone {
    if report.queue_depth == 0 || report.readiness == PatrolScenarioReadiness::Blocked {
        StatusTone::Warning
    } else if report
        .findings
        .iter()
        .any(|finding| finding.severity != ReportSeverity::Info)
    {
        StatusTone::Accent
    } else {
        StatusTone::Success
    }
}

#[must_use]
pub fn finding_summary_tone(findings: &[sp42_core::PatrolScenarioFinding]) -> StatusTone {
    if findings
        .iter()
        .any(|finding| finding.severity == ReportSeverity::Blocker)
    {
        StatusTone::Warning
    } else if findings
        .iter()
        .any(|finding| finding.severity == ReportSeverity::Warning)
    {
        StatusTone::Accent
    } else {
        StatusTone::Success
    }
}

#[must_use]
pub fn finding_summary_line(finding: &sp42_core::PatrolScenarioFinding) -> String {
    format!(
        "{:?} {}: {}",
        finding.severity, finding.code, finding.message
    )
}

fn readiness_label(readiness: PatrolScenarioReadiness) -> &'static str {
    match readiness {
        PatrolScenarioReadiness::Blocked => "Blocked",
        PatrolScenarioReadiness::Limited => "Limited",
        PatrolScenarioReadiness::Ready => "Ready",
    }
}

fn readiness_tone(readiness: PatrolScenarioReadiness) -> StatusTone {
    match readiness {
        PatrolScenarioReadiness::Blocked => StatusTone::Warning,
        PatrolScenarioReadiness::Limited => StatusTone::Info,
        PatrolScenarioReadiness::Ready => StatusTone::Success,
    }
}

fn section_summary(report: &PatrolScenarioReport, name: &str) -> String {
    report
        .sections
        .iter()
        .find(|section| section.name == name)
        .map(|section| {
            if section.summary_lines.is_empty() {
                format!("available={}", section.available)
            } else {
                format!(
                    "available={} {}",
                    section.available,
                    section.summary_lines.join(" | ")
                )
            }
        })
        .unwrap_or_else(|| "missing".to_string())
}

#[cfg(test)]
mod tests {
    use super::StatusTone;
    use super::{
        finding_summary_line, finding_summary_tone, recommended_next_step, session_digest_badges,
        session_digest_lines,
    };
    use sp42_core::{
        PatrolScenarioFinding, PatrolScenarioReadiness, PatrolScenarioReport,
        PatrolScenarioSection, ReportSeverity,
    };

    fn sample_report(readiness: PatrolScenarioReadiness) -> PatrolScenarioReport {
        PatrolScenarioReport {
            wiki_id: "frwiki".to_string(),
            queue_depth: 2,
            readiness,
            selected: Some(sp42_core::PatrolScenarioSelectedEdit {
                wiki_id: "frwiki".to_string(),
                rev_id: 123_456,
                title: "Example".to_string(),
                score: 42,
                signals: 3,
            }),
            sections: vec![PatrolScenarioSection {
                name: "Context".to_string(),
                available: true,
                summary_lines: vec!["user_risk=true".to_string()],
            }],
            findings: vec![PatrolScenarioFinding {
                severity: ReportSeverity::Info,
                code: "diff_changes".to_string(),
                message: "diff segments=2".to_string(),
            }],
            debug_snapshot: sp42_core::build_debug_snapshot(&sp42_core::DebugSnapshotInputs {
                queue: &[],
                selected: None,
                scoring_context: None,
                diff: None,
                review_workbench: None,
                stream_status: None,
                backlog_status: None,
                coordination: None,
            }),
        }
    }

    #[test]
    fn digest_badges_include_readiness_and_selection() {
        let badges = session_digest_badges(&sample_report(PatrolScenarioReadiness::Ready));

        assert!(badges.iter().any(|(label, _)| label == "2 queue"));
        assert!(badges.iter().any(|(label, _)| label == "Ready"));
        assert!(badges.iter().any(|(label, _)| label == "rev 123456"));
    }

    #[test]
    fn digest_lines_include_next_step() {
        let lines = session_digest_lines(&sample_report(PatrolScenarioReadiness::Ready));

        assert!(lines.iter().any(|line| line.starts_with("queue depth=2")));
        assert!(
            lines
                .iter()
                .any(|line| line.starts_with("selected rev=123456"))
        );
        assert!(lines.iter().any(|line| line.starts_with("next_step=")));
    }

    #[test]
    fn next_step_reflects_readiness_and_findings() {
        assert_eq!(
            recommended_next_step(&sample_report(PatrolScenarioReadiness::Blocked)),
            "collect telemetry before acting"
        );
        assert_eq!(
            recommended_next_step(&sample_report(PatrolScenarioReadiness::Ready)),
            "prepare patrol action"
        );
    }

    #[test]
    fn finding_helpers_render_labels() {
        let finding = PatrolScenarioFinding {
            severity: ReportSeverity::Warning,
            code: "high_score".to_string(),
            message: "selected edit is high risk".to_string(),
        };

        assert_eq!(finding_summary_tone(&[finding.clone()]), StatusTone::Accent);
        assert!(finding_summary_line(&finding).contains("high_score"));
    }
}
