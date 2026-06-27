use leptos::prelude::*;

use crate::platform::auth::{
    AuthSession, begin_login, bootstrap_dev_auth_session, fetch_auth_session, logout,
};
use crate::platform::config::{request_wiki_switch, selected_wiki_id};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkspaceView {
    Patrol,
    Article,
    Citation,
}

#[derive(Clone)]
enum AuthState {
    Loading,
    Ready(AuthSession),
}

/// Root gate: SP42 requires a Wikimedia login (ADR-0014). Fetch the session on
/// mount; render the workspace when authenticated, otherwise the login screen.
#[component]
pub fn App() -> impl IntoView {
    let (auth, set_auth) = signal(AuthState::Loading);

    let refresh = Action::new_local(move |(): &()| async move {
        let state = match fetch_auth_session().await {
            Ok(session) => AuthState::Ready(session),
            // A failed probe is treated as anonymous so the gate is shown rather
            // than trapping the user on a spinner.
            Err(_) => AuthState::Ready(AuthSession::default()),
        };
        set_auth.set(state);
    });
    let refresh = Callback::new(move |()| {
        refresh.dispatch_local(());
    });

    Effect::new(move |ran: Option<bool>| {
        if ran.is_none() {
            refresh.run(());
        }
        true
    });

    view! {
        {move || match auth.get() {
            AuthState::Loading => view! {
                <div class="auth-gate">
                    <p class="auth-gate-status">"Checking your Wikimedia session…"</p>
                </div>
            }
            .into_any(),
            AuthState::Ready(session) if session.authenticated => {
                view! { <Workspace session=session refresh=refresh /> }.into_any()
            }
            AuthState::Ready(session) => {
                view! { <LoginGate session=session refresh=refresh /> }.into_any()
            }
        }}
    }
}

/// The unauthenticated login screen (required before the workspace renders).
#[component]
fn LoginGate(session: AuthSession, refresh: Callback<()>) -> impl IntoView {
    let login_path = session.login_path.clone();
    let start_login = move |_| begin_login(&login_path, &current_login_next());

    let primary = if session.oauth_client_ready {
        view! {
            <button class="btn btn-success" type="button" on:click=start_login>
                "Log in with your Wikimedia account"
            </button>
        }
        .into_any()
    } else {
        view! {
            <p class="auth-gate-error">
                "Wikimedia OAuth is not configured on this server "
                "(set WIKIMEDIA_CLIENT_APPLICATION_KEY and WIKIMEDIA_CLIENT_APPLICATION_SECRET)."
            </p>
        }
        .into_any()
    };

    // Local-dev convenience: install a session from .env.wikimedia.local. Only
    // offered when the server reports a local token is available.
    let dev_bootstrap = Action::new_local(move |(): &()| async move {
        let request = sp42_core::DevAuthBootstrapRequest {
            username: String::new(),
            scopes: Vec::new(),
            expires_at_ms: None,
        };
        let _ = bootstrap_dev_auth_session(&request).await;
        refresh.run(());
    });
    let secondary = if session.local_token_available {
        view! {
            <button
                class="btn btn-ghost"
                type="button"
                on:click=move |_| {
                    dev_bootstrap.dispatch_local(());
                }
            >
                "Use local developer token"
            </button>
        }
        .into_any()
    } else {
        ().into_any()
    };

    view! {
        <div class="auth-gate">
            <div class="auth-gate-card">
                <div class="workspace-brand">"SP42"</div>
                <h1 class="auth-gate-title">"Sign in to continue"</h1>
                <p class="auth-gate-lead">
                    "SP42 patrols and edits Wikimedia projects using your own account. \
                     Log in with Wikimedia to use the workspace."
                </p>
                {primary}
                {secondary}
            </div>
        </div>
    }
}

/// The authenticated workspace (the former `App` body) plus a session header.
#[component]
fn Workspace(session: AuthSession, refresh: Callback<()>) -> impl IntoView {
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

    let username = session
        .username
        .clone()
        .unwrap_or_else(|| "Wikimedia user".to_string());
    let do_logout = Action::new_local(move |(): &()| async move {
        let _ = logout().await;
        refresh.run(());
    });

    // Wiki picker: point the workspace at any Wikimedia project the server can
    // resolve (ADR-0014). Submitting sets the ?wiki= override and reloads.
    let (wiki_input, set_wiki_input) = signal(selected_wiki_id());

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
                <form
                    class="workspace-wiki-picker"
                    on:submit=move |ev| {
                        ev.prevent_default();
                        request_wiki_switch(&wiki_input.get_untracked());
                    }
                >
                    <label class="workspace-wiki-label" for="wiki-picker">"Wiki"</label>
                    <input
                        id="wiki-picker"
                        class="workspace-wiki-input"
                        type="text"
                        list="sp42-wiki-suggestions"
                        prop:value=move || wiki_input.get()
                        on:input=move |ev| set_wiki_input.set(event_target_value(&ev))
                        aria-label="Wikimedia project (dbname, e.g. enwiki, commonswiki)"
                    />
                    <datalist id="sp42-wiki-suggestions">
                        <option value="enwiki"></option>
                        <option value="frwiki"></option>
                        <option value="dewiki"></option>
                        <option value="eswiki"></option>
                        <option value="commonswiki"></option>
                        <option value="wikidatawiki"></option>
                    </datalist>
                    <button class="btn btn-ghost btn-compact" type="submit">
                        "Switch"
                    </button>
                </form>
                <div class="workspace-session">
                    <span class="workspace-session-user">{username}</span>
                    <button
                        class="btn btn-ghost btn-compact"
                        type="button"
                        on:click=move |_| {
                            do_logout.dispatch_local(());
                        }
                    >
                        "Log out"
                    </button>
                </div>
            </nav>
            <main class="workspace-body">
                {move || match active_view.get() {
                    WorkspaceView::Patrol => view! {
                        <crate::pages::patrol::PatrolSurface />
                    }
                    .into_any(),
                    WorkspaceView::Article => view! {
                        <crate::pages::article::ArticleSurface />
                    }
                    .into_any(),
                    WorkspaceView::Citation => view! {
                        <crate::pages::citation::CitationSurface />
                    }
                    .into_any(),
                }}
            </main>
        </div>
    }
}

fn current_login_next() -> String {
    crate::platform::globals::browser_window()
        .and_then(|window| window.location().pathname().ok())
        .unwrap_or_else(|| "/".to_string())
}

fn workspace_tab_class(active: bool) -> &'static str {
    if active {
        "workspace-tab workspace-tab-active"
    } else {
        "workspace-tab"
    }
}

fn initial_workspace_view() -> WorkspaceView {
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

fn set_workspace_hash(view: WorkspaceView) {
    if let Some(window) = web_sys::window() {
        let hash = match view {
            WorkspaceView::Patrol => "view=patrol",
            WorkspaceView::Article => "view=article",
            WorkspaceView::Citation => "view=citation",
        };
        let _ = window.location().set_hash(hash);
    }
}
