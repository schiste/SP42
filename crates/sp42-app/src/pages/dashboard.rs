use leptos::prelude::*;

use crate::components::status_badge::{StatusBadge, StatusTone};

#[component]
pub fn DashboardPage(children: Children) -> impl IntoView {
    let highlights = dashboard_highlights();

    view! {
        <main
            style="min-height:100vh;padding:17px;background:#08111f;color:#eff4ff;"
        >
            <div style="width:min(100%, 960px);margin:0 auto;display:grid;gap:10px;">
                <header
                    style="display:grid;gap:10px;padding:17px;border-radius:6px;border:1px solid rgba(148,163,184,.14);background:#0b1324;"
                >
                    <div style="display:flex;align-items:center;gap:7px;flex-wrap:wrap;">
                        {highlights
                            .into_iter()
                            .map(|(label, tone)| view! { <StatusBadge label=label tone=tone /> })
                            .collect_view()}
                    </div>
                    <div style="display:grid;gap:4px;max-width:72rem;">
                        <p style="margin:0;color:#8fb7ff;font-size:.82rem;letter-spacing:.16em;text-transform:uppercase;font-weight:700;">
                            {sp42_core::branding::PROJECT_NAME}
                        </p>
                        <h1 style="margin:0;font-size:clamp(1.8rem,3vw,3rem);line-height:1.05;letter-spacing:-.04em;">
                            "Live patrol surface"
                        </h1>
                        <p style="margin:0;color:#8b9fc0;font-size:13px;line-height:1.6;max-width:46rem;">
                            "Scan the live queue, inspect the selected edit, and take action without leaving the page."
                        </p>
                    </div>
                </header>

                <section
                    style="padding:17px;border-radius:6px;border:1px solid rgba(148,163,184,.14);background:rgba(15,23,42,.5);"
                >
                    {children()}
                </section>
            </div>
        </main>
    }
}

#[must_use]
pub fn dashboard_highlights() -> Vec<(String, StatusTone)> {
    vec![
        ("Live Queue".to_string(), StatusTone::Accent),
        ("Action Ready".to_string(), StatusTone::Success),
        ("Room Sync".to_string(), StatusTone::Info),
    ]
}

#[cfg(test)]
mod tests {
    use super::{StatusTone, dashboard_highlights};

    #[test]
    fn dashboard_highlights_are_stable() {
        let highlights = dashboard_highlights();

        assert_eq!(highlights.len(), 3);
        assert_eq!(highlights[0].0, "Live Queue");
        assert_eq!(highlights[1].1, StatusTone::Success);
    }
}
