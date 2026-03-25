use leptos::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellSectionSpec {
    pub id: &'static str,
    pub title: &'static str,
    pub description: &'static str,
}

#[must_use]
pub fn shell_surface_sections() -> Vec<ShellSectionSpec> {
    vec![
        ShellSectionSpec {
            id: "live-review",
            title: "Live Review",
            description: "The selected edit, its diff, and the live decision context.",
        },
        ShellSectionSpec {
            id: "patrol-actions",
            title: "Patrol Actions",
            description: "The action rail and operator controls for the current candidate.",
        },
        ShellSectionSpec {
            id: "advanced-live-details",
            title: "Advanced Details",
            description: "Scenario digest, shell state, coordination, debug, PWA, and auth support surfaces.",
        },
    ]
}

#[must_use]
pub fn shell_section_anchor(id: &str) -> String {
    format!("#{id}")
}

#[component]
pub fn ShellSection(
    id: String,
    title: String,
    description: String,
    children: Children,
) -> impl IntoView {
    view! {
        <section
            id=id.clone()
            style="display:grid;gap:10px;padding:17px;border-radius:6px;border:1px solid rgba(148,163,184,.2);background:rgba(15,23,42,.88);"
        >
            <header style="display:grid;gap:7px;">
                <div style="display:flex;align-items:center;gap:7px;flex-wrap:wrap;">
                    <span
                        style="display:inline-flex;align-items:center;gap:4px;padding:4px 10px;border-radius:4px;border:1px solid rgba(148,163,184,.22);background:rgba(255,255,255,.05);color:#eff4ff;font-size:.78rem;line-height:1.1;font-weight:700;letter-spacing:.02em;text-transform:uppercase;"
                    >
                        {title.clone()}
                    </span>
                    <a
                        href=shell_section_anchor(&id)
                        style="font-size:.8rem;color:#8fb7ff;text-decoration:none;"
                    >
                        "Permalink"
                    </a>
                </div>
                <p style="margin:0;color:#8b9fc0;">
                    {description}
                </p>
            </header>
            <div>
                {children()}
            </div>
        </section>
    }
}

#[component]
pub fn ShellSurfaceIndex() -> impl IntoView {
    let sections = shell_surface_sections();

    view! {
        <nav
            aria-label="SP42 shell sections"
            style="display:grid;gap:10px;padding:17px;border-radius:6px;border:1px solid rgba(148,163,184,.18);background:rgba(8,15,29,.7);"
        >
            <div style="display:flex;align-items:center;justify-content:space-between;gap:10px;flex-wrap:wrap;">
                <div>
                    <h2 style="margin:0;font-size:1rem;">"Shell index"</h2>
                    <p style="margin:.25rem 0 0;color:#8b9fc0;">"Jump to the live browser shell slices."</p>
                </div>
            </div>
            <div style="display:flex;gap:7px;flex-wrap:wrap;">
                {sections
                    .into_iter()
                    .map(|section| {
                        view! {
                            <a
                                href=shell_section_anchor(section.id)
                                style="display:inline-flex;align-items:center;gap:4px;padding:4px 10px;border-radius:4px;border:1px solid rgba(148,163,184,.22);background:rgba(255,255,255,.05);color:#eff4ff;text-decoration:none;font-size:.8rem;font-weight:700;"
                            >
                                {section.title}
                            </a>
                        }
                    })
                    .collect_view()}
            </div>
        </nav>
    }
}

#[cfg(test)]
mod tests {
    use super::{shell_section_anchor, shell_surface_sections};

    #[test]
    fn shell_surface_sections_are_stable_and_unique() {
        let sections = shell_surface_sections();
        let ids: Vec<_> = sections.iter().map(|section| section.id).collect();

        assert_eq!(
            ids,
            vec![
                "live-review",
                "patrol-actions",
                "advanced-live-details",
            ]
        );
        assert!(sections.iter().all(|section| !section.title.is_empty()));
        assert!(
            sections
                .iter()
                .all(|section| !section.description.is_empty())
        );
    }

    #[test]
    fn shell_section_anchor_prefixes_hash() {
        assert_eq!(shell_section_anchor("live-review"), "#live-review");
    }
}
