use leptos::prelude::*;

use super::{
    inspector_feed::{
        InspectorFeed, classify_inspector_line, inspector_entries_from_lines, kind_meta,
    },
    status_badge::{StatusBadge, StatusTone},
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

    view! {
        <section
            style="display:grid;gap:10px;padding:17px;border-radius:6px;border:1px solid rgba(148,163,184,.14);background:rgba(15,23,42,.88);"
        >
            <header style="display:grid;gap:4px;">
                <div style="display:flex;align-items:center;gap:7px;flex-wrap:wrap;">
                    <StatusBadge label="Debug Panel".to_string() tone=StatusTone::Accent />
                    <StatusBadge label=format!("{} lines", queue_depth) tone=StatusTone::Info />
                </div>
                <p style="margin:0;color:#8b9fc0;">
                    "Structured state snapshot for the current dashboard view."
                </p>
            </header>
            <div style="display:flex;gap:7px;flex-wrap:wrap;">
                {active_kinds
                    .into_iter()
                    .map(|(label, count, tone)| {
                        view! {
                            <StatusBadge label=format!("{label} {count}") tone=tone />
                        }
                    })
                    .collect_view()}
            </div>
            <InspectorFeed entries=entries />
        </section>
    }
}

fn summarize_kinds(lines: &[String]) -> Vec<(String, usize, StatusTone)> {
    use std::collections::BTreeMap;

    let mut counts = BTreeMap::<String, (usize, StatusTone)>::new();
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
