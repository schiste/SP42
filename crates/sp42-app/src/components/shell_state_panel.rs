use leptos::prelude::*;
use sp42_core::{PatrolScenarioReadiness, ShellStateModel, ShellTimelineStage};

use super::{InspectorFeed, StatusBadge, StatusTone, inspector_entries_from_lines};

#[component]
pub fn ShellStatePanel(model: ShellStateModel) -> impl IntoView {
    let badges = shell_state_badges(&model);
    let timeline_lines = shell_state_timeline_lines(&model);
    let panel_lines = shell_state_panel_lines(&model);
    let notes = model.notes.clone();

    view! {
        <section
            style="display:grid;gap:10px;padding:17px;border-radius:6px;border:1px solid rgba(148,163,184,.18);background:rgba(8,16,30,.96);"
        >
            <header style="display:grid;gap:7px;">
                <div style="display:flex;align-items:center;gap:7px;flex-wrap:wrap;">
                    <StatusBadge label="Workbench State".to_string() tone=StatusTone::Accent />
                    {badges
                        .into_iter()
                        .map(|(label, tone)| view! { <StatusBadge label=label tone=tone /> })
                        .collect_view()}
                </div>
                <p style="margin:0;color:#8b9fc0;">
                    "One shared shell-state view across browser, CLI, and desktop so the patrol workbench tells the same story on every target."
                </p>
            </header>

            <div style="display:grid;grid-template-columns:repeat(auto-fit,minmax(280px,1fr));gap:10px;">
                <article
                    style="display:grid;gap:7px;padding:10px 17px;border-radius:4px;border:1px solid rgba(148,163,184,.18);background:rgba(15,23,42,.58);"
                >
                    <div style="display:flex;align-items:center;justify-content:space-between;gap:7px;flex-wrap:wrap;">
                        <h3 style="margin:0;font-size:1rem;">"Timeline"</h3>
                        <StatusBadge
                            label=format!("{} step(s)", model.timeline.len())
                            tone=StatusTone::Info
                        />
                    </div>
                    <InspectorFeed entries=inspector_entries_from_lines(&timeline_lines) />
                </article>

                <article
                    style="display:grid;gap:7px;padding:10px 17px;border-radius:4px;border:1px solid rgba(148,163,184,.18);background:rgba(15,23,42,.58);"
                >
                    <div style="display:flex;align-items:center;justify-content:space-between;gap:7px;flex-wrap:wrap;">
                        <h3 style="margin:0;font-size:1rem;">"Surface Coverage"</h3>
                        <StatusBadge
                            label=format!("{} panel(s)", model.panels.len())
                            tone=StatusTone::Success
                        />
                    </div>
                    <InspectorFeed entries=inspector_entries_from_lines(&panel_lines) />
                </article>
            </div>

            <article
                style="display:grid;gap:7px;padding:10px 17px;border-radius:4px;border:1px solid rgba(148,163,184,.18);background:rgba(15,23,42,.46);"
            >
                <div style="display:flex;align-items:center;justify-content:space-between;gap:7px;flex-wrap:wrap;">
                    <h3 style="margin:0;font-size:1rem;">"Operator Notes"</h3>
                    <StatusBadge
                        label=format!("{} note(s)", notes.len())
                        tone=if notes.is_empty() { StatusTone::Neutral } else { StatusTone::Success }
                    />
                </div>
                <ul style="margin:0;padding-inline-start:17px;color:#eff4ff;display:grid;gap:4px;">
                    {notes
                        .into_iter()
                        .map(|line| view! { <li>{line}</li> })
                        .collect_view()}
                </ul>
            </article>
        </section>
    }
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
