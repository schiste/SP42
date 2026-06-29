use leptos::prelude::*;
use sp42_patrol::{PatrolScenarioReadiness, ShellStateModel, ShellTimelineStage};
use sp42_ui::{
    BadgeHeader, BadgeHeaderProps, Card, CardHeader, CardHeaderProps, CardProps, Grid, GridColumns,
    GridProps, Panel, PanelProps, TextList, TextListItem, TextListItemProps, TextListProps,
};

use super::{InspectorFeed, StatusBadge, StatusTone, inspector_entries_from_lines, ui_children};

#[component]
pub fn ShellStatePanel(model: ShellStateModel) -> impl IntoView {
    let badges = shell_state_badges(&model);
    let timeline_lines = shell_state_timeline_lines(&model);
    let panel_lines = shell_state_panel_lines(&model);
    let notes = model.notes.clone();
    let timeline_count = model.timeline.len();
    let panel_count = model.panels.len();
    let note_count = notes.len();
    let note_tone = if notes.is_empty() {
        StatusTone::Neutral
    } else {
        StatusTone::Success
    };

    Panel(PanelProps::new(ui_children(move || {
        view! {
            {BadgeHeader(BadgeHeaderProps::new(
                "One shared shell-state view across browser, CLI, and desktop so the patrol workbench tells the same story on every target.",
                ui_children(move || view! {
                    <StatusBadge label="Workbench State".to_string() tone=StatusTone::Accent />
                    {badges
                        .into_iter()
                        .map(|(label, tone)| view! { <StatusBadge label=label tone=tone /> })
                        .collect_view()}
                }.into_any()),
            ))}

            {Grid(
                GridProps::new(ui_children(move || view! {
                    {Card(CardProps::new(ui_children(move || view! {
                        {CardHeader(CardHeaderProps::new("Timeline").with_actions(ui_children(move || view! {
                        <StatusBadge
                            label=format!("{timeline_count} step(s)")
                            tone=StatusTone::Info
                        />
                        }.into_any())))}
                        <InspectorFeed entries=inspector_entries_from_lines(&timeline_lines) />
                    }.into_any())))}

                    {Card(CardProps::new(ui_children(move || view! {
                        {CardHeader(CardHeaderProps::new("Surface Coverage").with_actions(ui_children(move || view! {
                        <StatusBadge
                            label=format!("{panel_count} panel(s)")
                            tone=StatusTone::Success
                        />
                        }.into_any())))}
                        <InspectorFeed entries=inspector_entries_from_lines(&panel_lines) />
                    }.into_any())))}
                }.into_any()))
                .with_columns(GridColumns::AutoFit)
            )}

            {Card(CardProps::new(ui_children(move || view! {
                {CardHeader(CardHeaderProps::new("Operator Notes").with_actions(ui_children(move || view! {
                    <StatusBadge label=format!("{note_count} note(s)") tone=note_tone />
                }.into_any())))}
                {TextList(TextListProps::new(ui_children(move || view! {
                    {notes
                        .into_iter()
                        .map(|line| {
                            TextListItem(TextListItemProps::new(ui_children(move || {
                                view! { {line} }.into_any()
                            })))
                        })
                        .collect_view()}
                }.into_any())))}
            }.into_any())))}
        }
        .into_any()
    })))
}

#[must_use]
pub fn shell_state_badges(model: &ShellStateModel) -> Vec<(String, StatusTone)> {
    vec![
        (
            format!("{} queue", model.queue_depth),
            if model.queue_depth == 0 {
                StatusTone::Warning
            } else {
                StatusTone::Success
            },
        ),
        (
            readiness_label(model.readiness).to_string(),
            readiness_tone(model.readiness),
        ),
        (
            format!(
                "{} timeline",
                model
                    .timeline
                    .iter()
                    .filter(|entry| entry.available)
                    .count()
            ),
            StatusTone::Info,
        ),
        (
            model.selected.as_ref().map_or_else(
                || "no selection".to_string(),
                |selected| format!("rev {}", selected.rev_id),
            ),
            if model.selected.is_some() {
                StatusTone::Accent
            } else {
                StatusTone::Warning
            },
        ),
    ]
}

#[must_use]
pub fn shell_state_timeline_lines(model: &ShellStateModel) -> Vec<String> {
    model
        .timeline
        .iter()
        .flat_map(|entry| {
            let stage = match entry.stage {
                ShellTimelineStage::Queue => "queue",
                ShellTimelineStage::Selected => "selected",
                ShellTimelineStage::Context => "context",
                ShellTimelineStage::Diff => "diff",
                ShellTimelineStage::Workbench => "workbench",
                ShellTimelineStage::Stream => "stream",
                ShellTimelineStage::Backlog => "backlog",
                ShellTimelineStage::Coordination => "coordination",
                ShellTimelineStage::OperatorSummary => "operator",
                ShellTimelineStage::Readiness => "readiness",
            };
            let mut lines = vec![format!(
                "{stage} available={} {}",
                entry.available, entry.headline
            )];
            lines.extend(
                entry
                    .detail_lines
                    .iter()
                    .map(|line| format!("{stage}: {line}")),
            );
            lines
        })
        .collect()
}

#[must_use]
pub fn shell_state_panel_lines(model: &ShellStateModel) -> Vec<String> {
    model
        .panels
        .iter()
        .flat_map(|panel| {
            let mut lines = vec![format!(
                "{} available={} {}",
                panel.name, panel.available, panel.headline
            )];
            lines.extend(
                panel
                    .detail_lines
                    .iter()
                    .take(3)
                    .map(|line| format!("{}: {line}", panel.name)),
            );
            lines
        })
        .collect()
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
