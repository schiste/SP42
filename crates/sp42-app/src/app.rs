use leptos::prelude::*;

use crate::platform::auth::{
    AuthSession, begin_login, bootstrap_dev_auth_session, fetch_auth_session, logout,
    save_local_credentials,
};
use crate::platform::config::{is_local_deployment, request_wiki_switch, selected_wiki_id};

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
        let was_loading = matches!(auth.get_untracked(), AuthState::Loading);
        let was_authed =
            matches!(auth.get_untracked(), AuthState::Ready(ref session) if session.authenticated);
        match fetch_auth_session().await {
            Ok(session) => {
                // Only swap state when the authenticated status actually changes
                // (or on the very first probe). A periodic re-check that always
                // set the signal would remount — and reset — the workspace on
                // every tab focus. Codex review #90.
                if was_loading || session.authenticated != was_authed {
                    set_auth.set(AuthState::Ready(session));
                }
            }
            // On the first probe a failed request is treated as anonymous so the
            // gate is shown rather than trapping the user on a spinner. On a later
            // re-check a transient network error is ignored: don't log an
            // authenticated user out over a blip; only a definitive
            // `authenticated == false` response above re-gates.
            Err(_) => {
                if was_loading {
                    set_auth.set(AuthState::Ready(AuthSession::default()));
                }
            }
        }
    });
    let refresh = Callback::new(move |()| {
        refresh.dispatch_local(());
    });

    Effect::new(move |ran: Option<bool>| {
        if ran.is_none() {
            refresh.run(());
            // Re-check the session when the tab regains visibility. Sessions
            // expire after a server-side idle timeout; without this the workspace
            // keeps rendering on a dead session and the next API call 401s,
            // stranding the user in a stale UI until a manual reload. Codex
            // review #90.
            register_visibility_recheck(refresh);
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

/// Re-run `refresh` whenever the tab becomes visible again, so an idle-expired
/// session returns the user to the login gate instead of a stale workspace.
fn register_visibility_recheck(refresh: Callback<()>) {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;

    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return;
    };
    let callback = Closure::<dyn FnMut(web_sys::Event)>::new(move |_event: web_sys::Event| {
        // Only re-check when the tab is shown, not when it is hidden.
        let visible = web_sys::window()
            .and_then(|window| window.document())
            .is_none_or(|document| !document.hidden());
        if visible {
            refresh.run(());
        }
    });
    let _ = document
        .add_event_listener_with_callback("visibilitychange", callback.as_ref().unchecked_ref());
    // Leak the closure intentionally — it must live for the page lifetime (the
    // same pattern as the PWA install-prompt listener in `platform/pwa.rs`).
    callback.forget();
}

/// The unauthenticated login screen (required before the workspace renders).
#[component]
fn LoginGate(session: AuthSession, refresh: Callback<()>) -> impl IntoView {
    // Local dev with nothing configured yet: show the first-run setup window so
    // a developer can paste credentials straight into .env.wikimedia.local
    // (ADR-0014) instead of editing files by hand.
    if is_local_deployment() && !session.oauth_client_ready && !session.local_token_available {
        return view! {
            <div class="auth-gate"><LocalSetupPanel /></div>
        }
        .into_any();
    }

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
    .into_any()
}

/// First-run local setup: capture Wikimedia credentials and write them into
/// `.env.wikimedia.local` via the local-only endpoint (ADR-0014).
#[component]
fn LocalSetupPanel() -> impl IntoView {
    let (token, set_token) = signal(String::new());
    let (key, set_key) = signal(String::new());
    let (secret, set_secret) = signal(String::new());
    let (result, set_result) = signal(None::<Result<String, String>>);

    let save = Action::new_local(move |(): &()| async move {
        let outcome = save_local_credentials(
            &token.get_untracked(),
            &key.get_untracked(),
            &secret.get_untracked(),
        )
        .await;
        set_result.set(Some(outcome));
    });

    view! {
        <div class="auth-gate-card">
            <div class="workspace-brand">"SP42"</div>
            <h1 class="auth-gate-title">"Set up local access"</h1>
            <p class="auth-gate-lead">
                "No Wikimedia credentials found. Paste a personal OAuth2 access token to start \
                 right away, or your OAuth consumer key + secret for the full login flow. "
                <a
                    href="https://meta.wikimedia.org/wiki/Special:OAuthConsumerRegistration/propose/oauth2"
                    target="_blank"
                    rel="noreferrer"
                >
                    "Where do I get these?"
                </a>
            </p>
            <form
                class="auth-setup-form"
                on:submit=move |ev| {
                    ev.prevent_default();
                    save.dispatch_local(());
                }
            >
                <label class="auth-setup-field">
                    <span>"Access token (quickest)"</span>
                    <input
                        type="password"
                        autocomplete="off"
                        prop:value=move || token.get()
                        on:input=move |ev| set_token.set(event_target_value(&ev))
                    />
                </label>
                <label class="auth-setup-field">
                    <span>"OAuth consumer key (optional)"</span>
                    <input
                        type="text"
                        autocomplete="off"
                        prop:value=move || key.get()
                        on:input=move |ev| set_key.set(event_target_value(&ev))
                    />
                </label>
                <label class="auth-setup-field">
                    <span>"OAuth consumer secret (optional)"</span>
                    <input
                        type="password"
                        autocomplete="off"
                        prop:value=move || secret.get()
                        on:input=move |ev| set_secret.set(event_target_value(&ev))
                    />
                </label>
                <button class="btn btn-success" type="submit">
                    "Save to .env.wikimedia.local"
                </button>
            </form>
            {move || match result.get() {
                Some(Ok(file)) => view! {
                    <p class="auth-gate-status">
                        "Saved to " {file}
                        ". Restart the dev server (Ctrl-C, then ./scripts/dev-local.sh) to apply."
                    </p>
                }
                .into_any(),
                Some(Err(error)) => view! {
                    <p class="auth-gate-error">{error}</p>
                }
                .into_any(),
                None => ().into_any(),
            }}
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
    // resolve (ADR-0014). Submitting sets the ?wiki= override and reloads. The
    // datalist is the full embedded site list, fetched once, so the input is a
    // type-to-filter dropdown over every Wikimedia project.
    let (wiki_input, set_wiki_input) = signal(selected_wiki_id());
    let (wiki_options, set_wiki_options) = signal(Vec::<String>::new());
    Effect::new(move |ran: Option<bool>| {
        if ran.is_none() {
            wasm_bindgen_futures::spawn_local(async move {
                set_wiki_options.set(fetch_known_wikis().await);
            });
        }
        true
    });

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
                        {move || {
                            wiki_options
                                .get()
                                .into_iter()
                                .map(|id| view! { <option value=id></option> })
                                .collect_view()
                        }}
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

/// Fetch the full list of resolvable Wikimedia `wiki_id`s for the picker
/// dropdown (`GET /wikis`, the embedded `SiteMatrix` snapshot). Empty on failure.
async fn fetch_known_wikis() -> Vec<String> {
    let url = crate::platform::config::api_url(sp42_core::routes::WIKIS_PATH);
    let Ok(bytes) = crate::platform::http::get_bytes(&url, "fetch wiki list").await else {
        return Vec::new();
    };
    serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()
        .and_then(|value| {
            value
                .get("wiki_ids")
                .and_then(|ids| ids.as_array())
                .map(|ids| {
                    ids.iter()
                        .filter_map(|id| id.as_str().map(str::to_string))
                        .collect()
                })
        })
        .unwrap_or_default()
}

fn current_login_next() -> String {
    // Preserve the full location (path + ?wiki=… + #view=…) so OAuth login
    // returns to the requested project/view, not the default. Codex review #90.
    crate::platform::globals::browser_window()
        .and_then(|window| {
            let location = window.location();
            let path = location.pathname().ok()?;
            let search = location.search().unwrap_or_default();
            let hash = location.hash().unwrap_or_default();
            Some(format!("{path}{search}{hash}"))
        })
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
