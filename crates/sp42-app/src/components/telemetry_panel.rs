use leptos::prelude::*;

use super::{InspectorFeed, StatusBadge, StatusTone, inspector_entries_from_lines};

#[component]
pub fn TelemetryPanel(
    title: String,
    description: String,
    tone: StatusTone,
    badges: Vec<(String, StatusTone)>,
    lines: Vec<String>,
) -> impl IntoView {
    let entries = inspector_entries_from_lines(&lines);

    view! {
        <section
            style="display:grid;gap:10px;padding:17px;border-radius:6px;border:1px solid rgba(148,163,184,.18);background:rgba(15,23,42,.88);"
        >
            <header style="display:grid;gap:7px;">
                <div style="display:flex;align-items:center;gap:7px;flex-wrap:wrap;">
                    <StatusBadge label=title tone=tone />
                    {badges
                        .into_iter()
                        .map(|(label, tone)| view! { <StatusBadge label=label tone=tone /> })
                        .collect_view()}
                </div>
                <p style="margin:0;color:#8b9fc0;">
                    {description}
                </p>
            </header>
            <InspectorFeed entries=entries />
        </section>
    }
}
