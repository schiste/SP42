use leptos::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkspaceView {
    Patrol,
    Article,
    Citation,
}

#[component]
pub fn App() -> impl IntoView {
    let (active_view, set_active_view) = signal(initial_workspace_view());

    let show_patrol = move |_| {
        set_workspace_hash(WorkspaceView::Patrol);
        set_active_view.set(WorkspaceView::Patrol);
    };
    let show_article = move |_| {
        set_workspace_hash(WorkspaceView::Article);
        set_active_view.set(WorkspaceView::Article);
    };
    let show_citation = move |_| {
        set_workspace_hash(WorkspaceView::Citation);
        set_active_view.set(WorkspaceView::Citation);
    };

    view! {
        <div class="workspace-shell">
            <nav class="workspace-nav" aria-label="SP42 workspace">
                <div class="workspace-brand">"SP42"</div>
                <div class="workspace-tabs" role="tablist" aria-label="Workspace views">
                    <button
                        type="button"
                        class=move || workspace_tab_class(active_view.get() == WorkspaceView::Patrol)
                        aria-selected=move || (active_view.get() == WorkspaceView::Patrol).to_string()
                        on:click=show_patrol
                    >
                        "Patrol"
                    </button>
                    <button
                        type="button"
                        class=move || workspace_tab_class(active_view.get() == WorkspaceView::Article)
                        aria-selected=move || (active_view.get() == WorkspaceView::Article).to_string()
                        on:click=show_article
                    >
                        "Article"
                    </button>
                    <button
                        type="button"
                        class=move || workspace_tab_class(active_view.get() == WorkspaceView::Citation)
                        aria-selected=move || (active_view.get() == WorkspaceView::Citation).to_string()
                        on:click=show_citation
                    >
                        "Citations"
                    </button>
                </div>
            </nav>
            <main class="workspace-body">
                {move || match active_view.get() {
                    WorkspaceView::Patrol => view! {
                        <crate::pages::patrol::PatrolSurface />
                    }.into_any(),
                    WorkspaceView::Article => view! {
                        <crate::pages::article::ArticleSurface />
                    }.into_any(),
                    WorkspaceView::Citation => view! {
                        <crate::pages::citation::CitationSurface />
                    }.into_any(),
                }}
            </main>
        </div>
    }
}

fn workspace_tab_class(active: bool) -> &'static str {
    if active {
        "workspace-tab workspace-tab-active"
    } else {
        "workspace-tab"
    }
}

fn initial_workspace_view() -> WorkspaceView {
    #[cfg(target_arch = "wasm32")]
    {
        let Some(window) = web_sys::window() else {
            return WorkspaceView::Patrol;
        };
        let Ok(hash) = window.location().hash() else {
            return WorkspaceView::Patrol;
        };
        if hash.contains("view=article") {
            WorkspaceView::Article
        } else if hash.contains("view=citation") {
            WorkspaceView::Citation
        } else {
            WorkspaceView::Patrol
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        WorkspaceView::Patrol
    }
}

fn set_workspace_hash(view: WorkspaceView) {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(window) = web_sys::window() {
            let hash = match view {
                WorkspaceView::Patrol => "view=patrol",
                WorkspaceView::Article => "view=article",
                WorkspaceView::Citation => "view=citation",
            };
            let _ = window.location().set_hash(hash);
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = view;
    }
}
