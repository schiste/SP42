use leptos::prelude::*;

use super::status_badge::{StatusBadge, StatusTone};

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
    view! {
        <div
            class="sp42-inspector-feed"
            style="display:grid;gap:10px;"
        >
            {entries
                .into_iter()
                .map(|entry| view! { <InspectorEntryRow entry=entry /> })
                .collect_view()}
        </div>
    }
}

#[component]
fn InspectorEntryRow(entry: InspectorEntry) -> impl IntoView {
    let (tone, label) = kind_meta(entry.kind);

    view! {
        <article
            class="sp42-inspector-entry"
            style="display:grid;gap:4px;padding:10px;border-radius:4px;border:1px solid rgba(148,163,184,.18);background:rgba(15,23,42,.5);"
        >
            <div style="display:flex;align-items:center;gap:7px;flex-wrap:wrap;">
                <StatusBadge label=label.to_string() tone=tone />
            </div>
            <p style="margin:0;font-family:ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;font-size:.94rem;line-height:1.5;white-space:pre-wrap;word-break:break-word;color:#eff4ff;">
                {entry.text}
            </p>
        </article>
    }
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
pub fn kind_meta(kind: InspectorLineKind) -> (StatusTone, &'static str) {
    match kind {
        InspectorLineKind::Queue => (StatusTone::Accent, "Queue"),
        InspectorLineKind::Stream => (StatusTone::Info, "Stream"),
        InspectorLineKind::Backlog => (StatusTone::Warning, "Backlog"),
        InspectorLineKind::Coordination => (StatusTone::Info, "Coordination"),
        InspectorLineKind::Auth => (StatusTone::Accent, "Auth"),
        InspectorLineKind::Diff => (StatusTone::Neutral, "Diff"),
        InspectorLineKind::Review => (StatusTone::Success, "Review"),
        InspectorLineKind::Runtime => (StatusTone::Neutral, "Runtime"),
        InspectorLineKind::Server => (StatusTone::Neutral, "Server"),
        InspectorLineKind::General => (StatusTone::Neutral, "General"),
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
