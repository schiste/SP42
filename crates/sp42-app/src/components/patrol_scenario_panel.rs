use leptos::prelude::*;
use sp42_core::{
    PatrolScenarioFinding, PatrolScenarioReadiness, PatrolScenarioReport, PatrolScenarioSection,
    ReportSeverity, render_patrol_scenario_text,
};

use super::{InspectorFeed, StatusBadge, StatusTone, inspector_entries_from_lines};

#[component]
pub fn PatrolScenarioPanel(report: PatrolScenarioReport) -> impl IntoView {
    let badges = scenario_badges(&report);
    let narrative_lines = scenario_storyboard_lines(&report);
    let section_cards = ordered_sections(&report);
    let findings = report.findings.clone();
    let report_text = render_patrol_scenario_text(&report);

    view! {
        <section
            style="display:grid;gap:10px;padding:17px;border-radius:6px;border:1px solid rgba(148,163,184,.2);background:rgba(10,18,33,.94);"
        >
            <header style="display:grid;gap:7px;">
                <div style="display:flex;align-items:center;gap:7px;flex-wrap:wrap;">
                    <StatusBadge label="Scenario".to_string() tone=StatusTone::Accent />
                    {badges
                        .into_iter()
                        .map(|(label, tone)| view! { <StatusBadge label=label tone=tone /> })
                        .collect_view()}
                </div>
                <p style="margin:0;color:#8b9fc0;">
                    "Typed patrol scenario summary derived from the live queue, context, diff, action, stream, backlog, and coordination inputs."
                </p>
            </header>

            <div style="display:grid;grid-template-columns:repeat(auto-fit,minmax(280px,1fr));gap:10px;">
                <article
                    style="display:grid;gap:7px;padding:10px 17px;border-radius:4px;border:1px solid rgba(148,163,184,.16);background:rgba(15,23,42,.58);"
                >
                    <div style="display:flex;align-items:center;justify-content:space-between;gap:7px;flex-wrap:wrap;">
                        <h3 style="margin:0;font-size:1rem;">"Queue to Action Narrative"</h3>
                        <StatusBadge
                            label=format!("{} findings", findings.len())
                            tone={finding_summary_tone(&findings)}
                        />
                    </div>
                    <InspectorFeed entries=inspector_entries_from_lines(&narrative_lines) />
                </article>

                <article
                    style="display:grid;gap:7px;padding:10px 17px;border-radius:4px;border:1px solid rgba(148,163,184,.16);background:rgba(15,23,42,.58);"
                >
                    <div style="display:flex;align-items:center;justify-content:space-between;gap:7px;flex-wrap:wrap;">
                        <h3 style="margin:0;font-size:1rem;">"Findings"</h3>
                        <StatusBadge
                            label=format!("{} section(s)", report.sections.len())
                            tone=StatusTone::Info
                        />
                    </div>
                    <div style="display:grid;gap:7px;">
                        {findings
                            .into_iter()
                            .map(|finding| view! { <FindingCard finding=finding /> })
                            .collect_view()}
                    </div>
                </article>
            </div>

            <div style="display:grid;grid-template-columns:repeat(auto-fit,minmax(220px,1fr));gap:10px;">
                {section_cards
                    .into_iter()
                    .map(|section| view! { <ScenarioSectionCard section=section /> })
                    .collect_view()}
            </div>

            <details
                style="padding:10px 17px;border-radius:4px;border:1px solid rgba(148,163,184,.14);background:rgba(8,15,29,.58);"
            >
                <summary style="cursor:pointer;font-weight:700;">"Technical snapshot"</summary>
                <pre
                    style="margin:.75rem 0 0;overflow:auto;white-space:pre-wrap;word-break:break-word;font-family:ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;font-size:.9rem;line-height:1.55;color:#eff4ff;"
                >{report_text}</pre>
            </details>
        </section>
    }
}

#[component]
fn FindingCard(finding: PatrolScenarioFinding) -> impl IntoView {
    let (tone, label) = finding_meta(finding.severity);

    view! {
        <article
            style="display:grid;gap:4px;padding:10px;border-radius:4px;border:1px solid rgba(148,163,184,.16);background:rgba(8,15,29,.44);"
        >
            <div style="display:flex;align-items:center;gap:7px;flex-wrap:wrap;">
                <StatusBadge label=label.to_string() tone=tone />
                <span style="color:#dce4f2;font-size:.82rem;font-weight:600;">{finding.code}</span>
            </div>
            <p style="margin:0;color:#d6e4ff;line-height:1.5;">{finding.message}</p>
        </article>
    }
}

#[component]
fn ScenarioSectionCard(section: PatrolScenarioSection) -> impl IntoView {
    let tone = if section.available {
        StatusTone::Success
    } else {
        StatusTone::Warning
    };

    view! {
        <article
            style="display:grid;gap:7px;padding:10px;border-radius:4px;border:1px solid rgba(148,163,184,.16);background:rgba(8,15,29,.42);"
        >
            <div style="display:flex;align-items:center;justify-content:space-between;gap:7px;flex-wrap:wrap;">
                <StatusBadge label=section.name.clone() tone=tone />
                <StatusBadge
                    label=if section.available { "available".to_string() } else { "missing".to_string() }
                    tone=if section.available { StatusTone::Success } else { StatusTone::Warning }
                />
            </div>
            <ul style="margin:0;padding-inline-start:17px;color:#eff4ff;display:grid;gap:4px;">
                {section
                    .summary_lines
                    .into_iter()
                    .map(|line| view! { <li>{line}</li> })
                    .collect_view()}
            </ul>
        </article>
    }
}

#[must_use]
pub fn scenario_badges(report: &PatrolScenarioReport) -> Vec<(String, StatusTone)> {
    let (readiness_tone, readiness_label) = readiness_meta(report.readiness);
    let available_sections = report
        .sections
        .iter()
        .filter(|section| section.available)
        .count();
    let finding_tone = finding_summary_tone(&report.findings);

    vec![
        (
            format!("{} queue", report.queue_depth),
            if report.queue_depth == 0 {
                StatusTone::Warning
            } else {
                StatusTone::Success
            },
        ),
        (readiness_label.to_string(), readiness_tone),
        (
            format!("{} available", available_sections),
            if available_sections == report.sections.len() {
                StatusTone::Success
            } else {
                StatusTone::Info
            },
        ),
        (
            format!("{} finding(s)", report.findings.len()),
            finding_tone,
        ),
    ]
}

#[must_use]
pub fn scenario_storyboard_lines(report: &PatrolScenarioReport) -> Vec<String> {
    let mut lines = vec![format!(
        "queue depth={} wiki={} readiness={:?}",
        report.queue_depth, report.wiki_id, report.readiness
    )];

    if let Some(selected) = &report.selected {
        lines.push(format!(
            "selected rev={} title=\"{}\" score={} signals={}",
            selected.rev_id, selected.title, selected.score, selected.signals
        ));
    } else {
        lines.push("selected unavailable".to_string());
    }

    for section_name in [
        "Context",
        "Diff",
        "Workbench",
        "Stream",
        "Backlog",
        "Coordination",
    ] {
        if let Some(section) = report
            .sections
            .iter()
            .find(|section| section.name == section_name)
        {
            let display_name = if section_name == "Workbench" {
                "action rail".to_string()
            } else {
                section.name.to_lowercase()
            };
            lines.push(format!(
                "{} available={} {}",
                display_name,
                section.available,
                section.summary_lines.join(" | ")
            ));
        }
    }

    lines.push(format!(
        "findings blockers={} warnings={} info={}",
        report
            .findings
            .iter()
            .filter(|finding| finding.severity == ReportSeverity::Blocker)
            .count(),
        report
            .findings
            .iter()
            .filter(|finding| finding.severity == ReportSeverity::Warning)
            .count(),
        report
            .findings
            .iter()
            .filter(|finding| finding.severity == ReportSeverity::Info)
            .count(),
    ));

    lines
}

#[must_use]
pub fn readiness_meta(readiness: PatrolScenarioReadiness) -> (StatusTone, &'static str) {
    match readiness {
        PatrolScenarioReadiness::Blocked => (StatusTone::Warning, "Blocked"),
        PatrolScenarioReadiness::Limited => (StatusTone::Info, "Limited"),
        PatrolScenarioReadiness::Ready => (StatusTone::Success, "Ready"),
    }
}

#[must_use]
pub fn finding_meta(severity: ReportSeverity) -> (StatusTone, &'static str) {
    match severity {
        ReportSeverity::Info => (StatusTone::Info, "Info"),
        ReportSeverity::Warning => (StatusTone::Warning, "Warning"),
        ReportSeverity::Blocker => (StatusTone::Accent, "Blocker"),
    }
}

#[must_use]
pub fn finding_summary_tone(findings: &[PatrolScenarioFinding]) -> StatusTone {
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
pub fn ordered_sections(report: &PatrolScenarioReport) -> Vec<PatrolScenarioSection> {
    const ORDER: [&str; 7] = [
        "Queue",
        "Context",
        "Diff",
        "Workbench",
        "Stream",
        "Backlog",
        "Coordination",
    ];

    ORDER
        .iter()
        .filter_map(|name| {
            report
                .sections
                .iter()
                .find(|section| section.name == *name)
                .cloned()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::StatusTone;
    use super::{
        finding_summary_tone, ordered_sections, readiness_meta, scenario_badges,
        scenario_storyboard_lines,
    };
    use sp42_core::{
        PatrolScenarioFinding, PatrolScenarioReadiness, PatrolScenarioReport,
        PatrolScenarioSection, ReportSeverity,
    };

    fn sample_report() -> PatrolScenarioReport {
        PatrolScenarioReport {
            wiki_id: "frwiki".to_string(),
            queue_depth: 2,
            readiness: PatrolScenarioReadiness::Ready,
            selected: Some(sp42_core::PatrolScenarioSelectedEdit {
                wiki_id: "frwiki".to_string(),
                rev_id: 123_456,
                title: "Example".to_string(),
                score: 42,
                signals: 3,
            }),
            sections: vec![
                PatrolScenarioSection {
                    name: "Queue".to_string(),
                    available: true,
                    summary_lines: vec!["depth=2".to_string()],
                },
                PatrolScenarioSection {
                    name: "Context".to_string(),
                    available: true,
                    summary_lines: vec!["user_risk=true".to_string()],
                },
                PatrolScenarioSection {
                    name: "Diff".to_string(),
                    available: true,
                    summary_lines: vec!["has_changes=true".to_string()],
                },
                PatrolScenarioSection {
                    name: "Workbench".to_string(),
                    available: true,
                    summary_lines: vec!["requests=3".to_string()],
                },
                PatrolScenarioSection {
                    name: "Stream".to_string(),
                    available: true,
                    summary_lines: vec!["delivered_events=5".to_string()],
                },
                PatrolScenarioSection {
                    name: "Backlog".to_string(),
                    available: true,
                    summary_lines: vec!["poll_count=1".to_string()],
                },
                PatrolScenarioSection {
                    name: "Coordination".to_string(),
                    available: true,
                    summary_lines: vec!["claims=1".to_string()],
                },
            ],
            findings: vec![
                PatrolScenarioFinding {
                    severity: ReportSeverity::Warning,
                    code: "high_score".to_string(),
                    message: "selected edit is high risk".to_string(),
                },
                PatrolScenarioFinding {
                    severity: ReportSeverity::Info,
                    code: "diff_changes".to_string(),
                    message: "diff segments=2".to_string(),
                },
            ],
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
    fn readiness_metadata_is_stable() {
        assert_eq!(
            readiness_meta(PatrolScenarioReadiness::Blocked).1,
            "Blocked"
        );
        assert_eq!(
            readiness_meta(PatrolScenarioReadiness::Ready).0,
            StatusTone::Success
        );
    }

    #[test]
    fn scenario_badges_include_queue_and_findings() {
        let badges = scenario_badges(&sample_report());

        assert!(badges.iter().any(|(label, _)| label == "2 queue"));
        assert!(badges.iter().any(|(label, _)| label == "2 finding(s)"));
    }

    #[test]
    fn storyboard_lines_cover_queue_to_workbench_flow() {
        let lines = scenario_storyboard_lines(&sample_report());

        assert!(lines.iter().any(|line| line.starts_with("queue depth=2")));
        assert!(
            lines
                .iter()
                .any(|line| line.starts_with("selected rev=123456"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.starts_with("workbench available=true"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.starts_with("diff available=true"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.starts_with("findings blockers=0 warnings=1 info=1"))
        );
    }

    #[test]
    fn ordered_sections_preserve_flow_order() {
        let sections = ordered_sections(&sample_report());

        assert_eq!(
            sections.first().map(|section| section.name.as_str()),
            Some("Queue")
        );
        assert_eq!(
            sections.last().map(|section| section.name.as_str()),
            Some("Coordination")
        );
    }

    #[test]
    fn finding_summary_tone_reflects_severity() {
        let mut findings = sample_report().findings;
        assert_eq!(finding_summary_tone(&findings), StatusTone::Accent);
        findings.push(PatrolScenarioFinding {
            severity: ReportSeverity::Blocker,
            code: "no_selection".to_string(),
            message: "missing selection".to_string(),
        });
        assert_eq!(finding_summary_tone(&findings), StatusTone::Warning);
    }
}
