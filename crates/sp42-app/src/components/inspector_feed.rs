use leptos::prelude::*;
use sp42_ui::{
    Card, CardProps, Gap, Inline, InlineProps, Size, Stack, StackProps, Text, TextFamily,
    TextOverflow, TextProps,
};

use super::status_badge::{StatusBadge, Tone};
use super::ui_children;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InspectorLineKind {
    Queue,
    Stream,
    Backlog,
    Coordination,
    Auth,
    Diff,
    Review,
    Runtime,
    Server,
    General,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectorEntry {
    pub kind: InspectorLineKind,
    pub text: String,
}

#[component]
pub fn InspectorFeed(entries: Vec<InspectorEntry>) -> impl IntoView {
    Stack(
        StackProps::new(ui_children(move || {
            view! {
            {entries
                .into_iter()
                .map(|entry| view! { <InspectorEntryRow entry=entry /> })
                .collect_view()}
            }
            .into_any()
        }))
        .with_gap(Gap::Medium),
    )
}

#[component]
fn InspectorEntryRow(entry: InspectorEntry) -> impl IntoView {
    let (tone, label) = kind_meta(entry.kind);

    Card(
        CardProps::new(ui_children(move || {
            view! {
                {Inline(
                    InlineProps::new(ui_children(move || {
                        view! { <StatusBadge label=label.to_string() tone=tone /> }.into_any()
                    }))
                    .with_gap(Gap::Small),
                )}
                {Text(
                    TextProps::new(ui_children(move || view! { {entry.text} }.into_any()))
                        .with_size(Size::Large)
                        .with_family(TextFamily::Mono)
                        .with_overflow(TextOverflow::PreserveLines),
                )}
            }
            .into_any()
        }))
        .with_density(sp42_ui::Density::Compact),
    )
}

#[must_use]
pub fn inspector_entries_from_lines(lines: &[String]) -> Vec<InspectorEntry> {
    lines
        .iter()
        .map(|line| InspectorEntry {
            kind: classify_inspector_line(line),
            text: line.clone(),
        })
        .collect()
}

#[must_use]
pub fn classify_inspector_line(line: &str) -> InspectorLineKind {
    let prefix = line.split_once('=').map(|(left, _)| left).unwrap_or(line);
    let prefix = prefix.trim();

    match prefix {
        value if value.starts_with("queue") => InspectorLineKind::Queue,
        value
            if value.starts_with("selected")
                || value.starts_with("workbench")
                || value.starts_with("training") =>
        {
            InspectorLineKind::Review
        }
        value if value.starts_with("stream") => InspectorLineKind::Stream,
        value if value.starts_with("backlog") || value.starts_with("request") => {
            InspectorLineKind::Backlog
        }
        value if value.starts_with("coordination") || value.starts_with("room") => {
            InspectorLineKind::Coordination
        }
        value
            if value.starts_with("auth")
                || value.starts_with("oauth")
                || value.starts_with("bridge") =>
        {
            InspectorLineKind::Auth
        }
        value if value.starts_with("diff") => InspectorLineKind::Diff,
        value if value.starts_with("context") || value.starts_with("runtime") => {
            InspectorLineKind::Runtime
        }
        value if value.starts_with("server") || value.starts_with("project") => {
            InspectorLineKind::Server
        }
        value if value.starts_with("checkpoint") || value.starts_with("poll") => {
            InspectorLineKind::Backlog
        }
        _ => InspectorLineKind::General,
    }
}

#[must_use]
pub fn kind_meta(kind: InspectorLineKind) -> (Tone, &'static str) {
    match kind {
        InspectorLineKind::Queue => (Tone::Accent, "Queue"),
        InspectorLineKind::Stream => (Tone::Info, "Stream"),
        InspectorLineKind::Backlog => (Tone::Warning, "Backlog"),
        InspectorLineKind::Coordination => (Tone::Info, "Coordination"),
        InspectorLineKind::Auth => (Tone::Accent, "Auth"),
        InspectorLineKind::Diff => (Tone::Default, "Diff"),
        InspectorLineKind::Review => (Tone::Success, "Review"),
        InspectorLineKind::Runtime => (Tone::Default, "Runtime"),
        InspectorLineKind::Server => (Tone::Default, "Server"),
        InspectorLineKind::General => (Tone::Default, "General"),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        InspectorLineKind, classify_inspector_line, inspector_entries_from_lines, kind_meta,
    };

    #[test]
    fn classifies_known_prefixes() {
        assert_eq!(
            classify_inspector_line("stream_delivered=3"),
            InspectorLineKind::Stream
        );
        assert_eq!(
            classify_inspector_line("backlog_polls=2"),
            InspectorLineKind::Backlog
        );
        assert_eq!(
            classify_inspector_line("coordination_claims=1"),
            InspectorLineKind::Coordination
        );
        assert_eq!(
            classify_inspector_line("auth_mode=oauth"),
            InspectorLineKind::Auth
        );
        assert_eq!(
            classify_inspector_line("diff changed=true"),
            InspectorLineKind::Diff
        );
        assert_eq!(
            classify_inspector_line("queue_depth=4"),
            InspectorLineKind::Queue
        );
    }

    #[test]
    fn preserves_entry_text() {
        let entries = inspector_entries_from_lines(&["queue_depth=4".to_string()]);

        assert_eq!(entries[0].text, "queue_depth=4");
        assert_eq!(entries[0].kind, InspectorLineKind::Queue);
    }

    #[test]
    fn kind_meta_returns_nonempty_labels() {
        for kind in [
            InspectorLineKind::Queue,
            InspectorLineKind::Stream,
            InspectorLineKind::Backlog,
            InspectorLineKind::Coordination,
            InspectorLineKind::Auth,
            InspectorLineKind::Diff,
            InspectorLineKind::Review,
            InspectorLineKind::Runtime,
            InspectorLineKind::Server,
            InspectorLineKind::General,
        ] {
            let (_, label) = kind_meta(kind);
            assert!(!label.is_empty());
        }
    }
}
