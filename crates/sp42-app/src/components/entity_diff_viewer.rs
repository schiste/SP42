//! Structured entity diff rendering (ADR-0016 / PRD-0011): the review surface
//! for Wikibase entity revisions. Renders an [`EntityDiff`] as classified,
//! human-readable change rows — "added statement *educated at* → …", "changed
//! the English label" — never raw entity JSON. Labels arrive resolved on the
//! wire report (best-effort); unresolved ids render raw rather than blocking.

use std::collections::BTreeMap;

use leptos::prelude::*;
use sp42_core::{
    AliasChange, ContentDiffReport, EntityDiff, SitelinkChange, StatementChange,
    StatementChangeParts, TermChange, WikibaseStatement, render_snak_value,
};
use sp42_ui::{
    DiffState, DiffStateProps, DiffTone, DiffViewerShell, DiffViewerShellProps, EntityChangeRow,
    EntityChangeRowProps, EntityDiffPanel, EntityDiffPanelProps, EntityDiffSection,
    EntityDiffSectionProps,
};

use super::ui_children;

fn label_for<'ids>(labels: &'ids BTreeMap<String, String>, id: &'ids str) -> &'ids str {
    labels.get(id).map_or(id, String::as_str)
}

fn statement_value_text(
    labels: &BTreeMap<String, String>,
    statement: &WikibaseStatement,
) -> String {
    let display = render_snak_value(&statement.value);
    match display.item {
        Some(item) => label_for(labels, &item).to_owned(),
        None => display.text,
    }
}

fn statement_summary(labels: &BTreeMap<String, String>, statement: &WikibaseStatement) -> String {
    format!(
        "{}: {}",
        label_for(labels, &statement.property),
        statement_value_text(labels, statement)
    )
}

fn changed_part_names(parts: StatementChangeParts) -> Vec<&'static str> {
    let mut names = Vec::new();
    if parts.value {
        names.push("value");
    }
    if parts.qualifiers {
        names.push("qualifiers");
    }
    if parts.rank {
        names.push("rank");
    }
    if parts.references {
        names.push("references");
    }
    if parts.other {
        names.push("other");
    }
    names
}

fn optional_term(value: Option<&String>) -> String {
    value.map_or_else(
        || "(none)".to_owned(),
        |value| format!("\u{201c}{value}\u{201d}"),
    )
}

fn section(title: &'static str, rows: Vec<AnyView>) -> Option<AnyView> {
    if rows.is_empty() {
        return None;
    }
    Some(
        EntityDiffSection(EntityDiffSectionProps::new(
            title,
            ui_children(move || rows.into_any()),
        ))
        .into_any(),
    )
}

fn change_row(tone: DiffTone, badge: &'static str, text: String) -> AnyView {
    EntityChangeRow(EntityChangeRowProps::new(tone, badge, text)).into_any()
}

fn term_rows(changes: &[TermChange]) -> Vec<AnyView> {
    changes
        .iter()
        .map(|change| {
            let (tone, badge) = match (&change.before, &change.after) {
                (None, Some(_)) => (DiffTone::Insert, "+"),
                (Some(_), None) => (DiffTone::Delete, "\u{2212}"),
                _ => (DiffTone::Equal, "~"),
            };
            change_row(
                tone,
                badge,
                format!(
                    "{}: {} \u{2192} {}",
                    change.language,
                    optional_term(change.before.as_ref()),
                    optional_term(change.after.as_ref())
                ),
            )
        })
        .collect()
}

fn alias_rows(changes: &[AliasChange]) -> Vec<AnyView> {
    changes
        .iter()
        .map(|change| {
            change_row(
                DiffTone::Equal,
                "~",
                format!(
                    "{}: [{}] \u{2192} [{}]",
                    change.language,
                    change.before.join(", "),
                    change.after.join(", ")
                ),
            )
        })
        .collect()
}

fn sitelink_rows(changes: &[SitelinkChange]) -> Vec<AnyView> {
    changes
        .iter()
        .map(|change| {
            let (tone, badge) = match (&change.before, &change.after) {
                (None, Some(_)) => (DiffTone::Insert, "+"),
                (Some(_), None) => (DiffTone::Delete, "\u{2212}"),
                _ => (DiffTone::Equal, "~"),
            };
            change_row(
                tone,
                badge,
                format!(
                    "{}: {} \u{2192} {}",
                    change.site,
                    optional_term(change.before.as_ref()),
                    optional_term(change.after.as_ref())
                ),
            )
        })
        .collect()
}

fn statement_rows(labels: &BTreeMap<String, String>, changes: &[StatementChange]) -> Vec<AnyView> {
    changes
        .iter()
        .map(|change| match change {
            StatementChange::Added { statement } => {
                change_row(DiffTone::Insert, "+", statement_summary(labels, statement))
            }
            StatementChange::Removed { statement } => change_row(
                DiffTone::Delete,
                "\u{2212}",
                statement_summary(labels, statement),
            ),
            StatementChange::Changed {
                before,
                after,
                parts,
            } => {
                let parts = changed_part_names(*parts).join(", ");
                change_row(
                    DiffTone::Equal,
                    "~",
                    format!(
                        "{}: {} \u{2192} {} ({parts} changed)",
                        label_for(labels, &after.property),
                        statement_value_text(labels, before),
                        statement_value_text(labels, after),
                    ),
                )
            }
        })
        .collect()
}

fn entity_diff_sections(report: &ContentDiffReport, diff: &EntityDiff) -> Vec<AnyView> {
    let labels = &report.labels;
    [
        section("Labels", term_rows(&diff.labels)),
        section("Descriptions", term_rows(&diff.descriptions)),
        section("Aliases", alias_rows(&diff.aliases)),
        section("Sitelinks", sitelink_rows(&diff.sitelinks)),
        section("Statements", statement_rows(labels, &diff.statements)),
        section(
            "Other fields",
            diff.other
                .iter()
                .map(|change| change_row(DiffTone::Equal, "~", format!("{} changed", change.key)))
                .collect(),
        ),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Render a routed entity diff report as classified change sections.
#[component]
pub fn EntityDiffViewer(report: ContentDiffReport) -> impl IntoView {
    let body = match &report.diff {
        sp42_core::ContentDiff::Entity { diff } if diff.has_changes() => {
            entity_diff_sections(&report, diff)
        }
        sp42_core::ContentDiff::Entity { .. } => {
            return DiffState(DiffStateProps::new(
                "Entity diff viewer",
                "No entity changes detected.",
            ))
            .into_any();
        }
        // A text report reaching this component is a routing bug upstream;
        // say so instead of rendering nothing.
        sp42_core::ContentDiff::Text { .. } => {
            return DiffState(DiffStateProps::new(
                "Entity diff viewer",
                "This revision is not an entity revision.",
            ))
            .into_any();
        }
    };
    DiffViewerShell(DiffViewerShellProps::new(
        "Entity diff viewer",
        ui_children(move || {
            EntityDiffPanel(EntityDiffPanelProps::new(ui_children(move || {
                body.into_any()
            })))
            .into_any()
        }),
    ))
    .into_any()
}
