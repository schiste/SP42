//! Structured entity diff rendering (ADR-0016 / PRD-0011): the review surface
//! for Wikibase entity revisions. The rows arrive **pre-rendered** on the wire
//! (`EntityDiffReport`, produced by the platform's shared
//! `render_entity_diff_report`, labels already resolved server-side), so this
//! component only maps report rows onto sp42-ui primitives — keeping the
//! browser's deserialize surface, and the wasm bundle, small.

use leptos::prelude::*;
use sp42_core::{EntityChangeKind, EntityDiffReport};
use sp42_ui::{
    DiffState, DiffStateProps, DiffTone, DiffViewerShell, DiffViewerShellProps, EntityChangeRow,
    EntityChangeRowProps, EntityDiffPanel, EntityDiffPanelProps, EntityDiffSection,
    EntityDiffSectionProps,
};

use super::ui_children;

const fn row_style(kind: EntityChangeKind) -> (DiffTone, &'static str) {
    match kind {
        EntityChangeKind::Added => (DiffTone::Insert, "+"),
        EntityChangeKind::Removed => (DiffTone::Delete, "\u{2212}"),
        EntityChangeKind::Changed => (DiffTone::Equal, "~"),
    }
}

/// Render a pre-rendered entity diff report as classified change sections.
#[component]
pub fn EntityDiffViewer(report: EntityDiffReport) -> impl IntoView {
    if !report.has_changes() {
        return DiffState(DiffStateProps::new(
            "Entity diff viewer",
            "No entity changes detected.",
        ))
        .into_any();
    }
    let sections: Vec<AnyView> = report
        .sections
        .into_iter()
        .map(|section| {
            let rows: Vec<AnyView> = section
                .rows
                .into_iter()
                .map(|row| {
                    let (tone, badge) = row_style(row.kind);
                    EntityChangeRow(EntityChangeRowProps::new(tone, badge, row.text)).into_any()
                })
                .collect();
            EntityDiffSection(EntityDiffSectionProps::new(
                section.title,
                ui_children(move || rows.into_any()),
            ))
            .into_any()
        })
        .collect();
    DiffViewerShell(DiffViewerShellProps::new(
        "Entity diff viewer",
        ui_children(move || {
            EntityDiffPanel(EntityDiffPanelProps::new(ui_children(move || {
                sections.into_any()
            })))
            .into_any()
        }),
    ))
    .into_any()
}
