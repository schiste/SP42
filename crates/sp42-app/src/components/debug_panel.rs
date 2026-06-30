use leptos::prelude::*;
use sp42_ui::{
    BadgeHeader, BadgeHeaderProps, Gap, Inline, InlineProps, Panel, PanelProps, Surface,
};

use super::{
    inspector_feed::{
        InspectorFeed, classify_inspector_line, inspector_entries_from_lines, kind_meta,
    },
    status_badge::{StatusBadge, Tone},
    ui_children,
};

#[component]
pub fn DebugPanel(lines: Vec<String>) -> impl IntoView {
    let entries = inspector_entries_from_lines(&lines);
    let queue_depth = lines
        .iter()
        .find(|line| line.starts_with("queue_depth="))
        .and_then(|line| {
            line.split_once('=')
                .and_then(|(_, value)| value.parse::<usize>().ok())
        })
        .unwrap_or(entries.len());
    let active_kinds = summarize_kinds(&lines);

    Panel(PanelProps::new(ui_children(move || {
        view! {
            {BadgeHeader(
                BadgeHeaderProps::new(
                    "Structured state snapshot for the current dashboard view.",
                    ui_children(move || view! {
                        <StatusBadge label="Debug Panel".to_string() tone=Tone::Accent />
                        <StatusBadge label=format!("{} lines", queue_depth) tone=Tone::Info />
                    }.into_any()),
                )
            )}
            {Inline(InlineProps::new(ui_children(move || view! {
                {active_kinds
                    .into_iter()
                    .map(|(label, count, tone)| {
                        view! {
                            <StatusBadge label=format!("{label} {count}") tone=tone />
                        }
                    })
                    .collect_view()}
            }.into_any())).with_gap(Gap::Small))}
            <InspectorFeed entries=entries />
        }
        .into_any()
    }))
    .with_surface(Surface::Default))
}

fn summarize_kinds(lines: &[String]) -> Vec<(String, usize, Tone)> {
    use std::collections::BTreeMap;

    let mut counts = BTreeMap::<String, (usize, Tone)>::new();
    for line in lines {
        let kind = classify_inspector_line(line);
        let (tone, label) = kind_meta(kind);
        let entry = counts.entry(label.to_string()).or_insert((0, tone));
        entry.0 = entry.0.saturating_add(1);
    }

    counts
        .into_iter()
        .map(|(label, (count, tone))| (label, count, tone))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::summarize_kinds;

    #[test]
    fn summarizes_kinds_by_label() {
        let counts = summarize_kinds(&[
            "stream_delivered=3".to_string(),
            "stream_filtered=1".to_string(),
            "queue_depth=2".to_string(),
        ]);

        assert!(
            counts
                .iter()
                .any(|(label, count, _)| label == "Stream" && *count == 2)
        );
        assert!(
            counts
                .iter()
                .any(|(label, count, _)| label == "Queue" && *count == 1)
        );
    }
}
