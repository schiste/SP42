use leptos::prelude::*;
use sp42_patrol::{
    PatrolScenarioFinding, PatrolScenarioReadiness, PatrolScenarioReport, PatrolScenarioSection,
    ReportSeverity, render_patrol_scenario_text,
};
use sp42_ui::{
    BadgeHeader, BadgeHeaderProps, Card, CardHeader, CardHeaderProps, CardProps, CodeBlock,
    CodeBlockProps, Density, Disclosure, DisclosureProps, Gap, Grid, GridColumns, GridProps,
    Inline, InlineProps, Panel, PanelProps, Text, TextList, TextListItem, TextListItemProps,
    TextListProps, TextProps, TextTone, TextWeight,
};

use super::{InspectorFeed, StatusBadge, StatusTone, inspector_entries_from_lines, ui_children};

#[component]
pub fn PatrolScenarioPanel(report: PatrolScenarioReport) -> impl IntoView {
    let badges = scenario_badges(&report);
    let narrative_lines = scenario_storyboard_lines(&report);
    let section_cards = ordered_sections(&report);
    let findings = report.findings.clone();
    let report_text = render_patrol_scenario_text(&report);
    let finding_count = findings.len();
    let finding_tone = finding_summary_tone(&findings);
    let section_count = report.sections.len();

    Panel(PanelProps::new(ui_children(move || {
        view! {
            {BadgeHeader(BadgeHeaderProps::new(
                "Typed patrol scenario summary derived from the live queue, context, diff, action, stream, backlog, and coordination inputs.",
                ui_children(move || view! {
                    <StatusBadge label="Scenario".to_string() tone=StatusTone::Accent />
                    {badges
                        .into_iter()
                        .map(|(label, tone)| view! { <StatusBadge label=label tone=tone /> })
                        .collect_view()}
                }.into_any()),
            ))}

            {Grid(
                GridProps::new(ui_children(move || view! {
                    {Card(CardProps::new(ui_children(move || view! {
                        {CardHeader(CardHeaderProps::new("Queue to Action Narrative").with_actions(ui_children(move || view! {
                        <StatusBadge
                            label=format!("{finding_count} findings")
                            tone=finding_tone
                        />
                        }.into_any())))}
                        <InspectorFeed entries=inspector_entries_from_lines(&narrative_lines) />
                    }.into_any())))}

                    {Card(CardProps::new(ui_children(move || view! {
                        {CardHeader(CardHeaderProps::new("Findings").with_actions(ui_children(move || view! {
                        <StatusBadge
                            label=format!("{section_count} section(s)")
                            tone=StatusTone::Info
                        />
                        }.into_any())))}
                    {Grid(GridProps::new(ui_children(move || view! {
                        {findings
                            .into_iter()
                            .map(|finding| view! { <FindingCard finding=finding /> })
                            .collect_view()}
                    }.into_any())).with_gap(Gap::Small))}
                    }.into_any())))}
                }.into_any()))
                .with_columns(GridColumns::AutoFit)
            )}

            {Grid(
                GridProps::new(ui_children(move || view! {
                {section_cards
                    .into_iter()
                    .map(|section| view! { <ScenarioSectionCard section=section /> })
                    .collect_view()}
                }.into_any()))
                .with_columns(GridColumns::AutoFit)
            )}

            {Disclosure(DisclosureProps::new(
                "Technical snapshot",
                ui_children(move || view! {
                    {CodeBlock(CodeBlockProps::new(report_text))}
                }.into_any()),
            ))}
        }
        .into_any()
    })))
}

#[component]
fn FindingCard(finding: PatrolScenarioFinding) -> impl IntoView {
    let (tone, label) = finding_meta(finding.severity);
    let code = finding.code;
    let message = finding.message;

    Card(
        CardProps::new(ui_children(move || {
            view! {
                {Inline(
                    InlineProps::new(ui_children(move || view! {
                        <StatusBadge label=label.to_string() tone=tone />
                        {Text(
                            TextProps::new(ui_children(move || view! { {code} }.into_any()))
                                .with_weight(TextWeight::Medium)
                        )}
                    }.into_any()))
                    .with_gap(Gap::Small)
                )}
                {Text(
                    TextProps::new(ui_children(move || view! { {message} }.into_any()))
                        .with_tone(TextTone::Accent)
                )}
            }
            .into_any()
        }))
        .with_density(Density::Compact),
    )
}

#[component]
fn ScenarioSectionCard(section: PatrolScenarioSection) -> impl IntoView {
    let tone = if section.available {
        StatusTone::Success
    } else {
        StatusTone::Warning
    };
    let section_name = section.name;
    let summary_lines = section.summary_lines;

    let availability_label = if section.available {
        "available"
    } else {
        "missing"
    };
    let availability_tone = if section.available {
        StatusTone::Success
    } else {
        StatusTone::Warning
    };

    Card(
        CardProps::new(ui_children(move || {
            view! {
                {Inline(
                    InlineProps::new(ui_children(move || view! {
                        <StatusBadge label=section_name tone=tone />
                        <StatusBadge
                            label=availability_label.to_string()
                            tone=availability_tone
                        />
                    }.into_any()))
                    .with_justify(sp42_ui::Justify::Between)
                )}
                {TextList(TextListProps::new(ui_children(move || view! {
                {summary_lines
                    .into_iter()
                    .map(|line| {
                        TextListItem(TextListItemProps::new(ui_children(move || {
                            view! { {line} }.into_any()
                        })))
                    })
                    .collect_view()}
                }.into_any())))}
            }
            .into_any()
        }))
        .with_density(Density::Compact),
    )
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
        "queue depth={} wiki={} readiness={}",
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
    use sp42_patrol::{
        PatrolScenarioFinding, PatrolScenarioReadiness, PatrolScenarioReport,
        PatrolScenarioSection, ReportSeverity,
    };

    fn sample_report() -> PatrolScenarioReport {
        PatrolScenarioReport {
            wiki_id: "frwiki".to_string(),
            queue_depth: 2,
            readiness: PatrolScenarioReadiness::Ready,
            selected: Some(sp42_patrol::PatrolScenarioSelectedEdit {
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
            debug_snapshot: sp42_reporting::build_debug_snapshot(
                &sp42_reporting::DebugSnapshotInputs {
                    queue: &[],
                    selected: None,
                    scoring_context: None,
                    diff: None,
                    review_workbench: None,
                    stream_status: None,
                    backlog_status: None,
                    coordination: None,
                },
            ),
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
