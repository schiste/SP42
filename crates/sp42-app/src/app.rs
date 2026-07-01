use leptos::prelude::*;
use sp42_ui::{
    Button, ButtonProps, ButtonSurface, ButtonType, Density, Field, FieldProps, Gap, GateCard,
    GateCardProps, GateShell, GateShellProps, Heading, HeadingLevel, HeadingProps, Link, LinkProps,
    Stack, StackProps, Text, TextElement, TextInput, TextInputProps, TextInputType, TextProps,
    Tone, Width, WorkspaceBody, WorkspaceBodyProps, WorkspaceBrand, WorkspaceBrandProps,
    WorkspaceInlineForm, WorkspaceInlineFormProps, WorkspaceNav, WorkspaceNavProps,
    WorkspaceSession, WorkspaceSessionProps, WorkspaceShell, WorkspaceShellProps, WorkspaceTab,
    WorkspaceTabProps, WorkspaceTabs, WorkspaceTabsProps,
};

use crate::components::ui_children;
use crate::platform::auth::{
    AuthSession, begin_login, bootstrap_dev_auth_session, fetch_auth_session, logout,
    save_local_credentials,
};
use crate::platform::config::{
    is_local_deployment, is_split_origin_deployment, request_wiki_switch, selected_wiki_id,
};

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
        let was_authed =
            matches!(auth.get_untracked(), AuthState::Ready(ref session) if session.authenticated);
        match fetch_auth_session().await {
            Ok(session) => {
                // Re-render the gate whenever we're unauthenticated — the
                // login-gate metadata (oauth_client_ready / local_token_available /
                // route paths) can change after local setup or a server restart —
                // and on the first transition into authenticated. Skip redundant
                // updates only while staying authenticated, so the workspace isn't
                // remounted (and reset) on every periodic probe. Codex review #90.
                if !session.authenticated || !was_authed {
                    set_auth.set(AuthState::Ready(session));
                }
            }
            // A failed probe re-gates only from the initial spinner; a transient
            // network blip on a later re-check is ignored rather than logging an
            // authenticated user out. /auth/session itself returns 200 with
            // `authenticated:false` for an expired session, handled above.
            Err(_) => {
                if matches!(auth.get_untracked(), AuthState::Loading) {
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
            // Single re-gate authority: any API call that returns 401 re-fetches
            // the real /auth/session and re-renders the gate, regardless of which
            // call detected the expired session. Re-fetching (rather than
            // synthesizing a default) preserves the server's gate metadata, so the
            // user sees the correct Wikimedia-login / setup state, not a stale
            // "not configured". The per-layer cookie/session/token timers are now
            // belt-and-suspenders, not the load-bearing trigger. Codex review #90.
            crate::platform::http::set_unauthorized_handler(move || {
                refresh.run(());
            });
        }
        true
    });

    view! {
        {move || match auth.get() {
            AuthState::Loading => GateShell(GateShellProps::new(ui_children(|| {
                view! {
                    {Text(
                        TextProps::new(ui_children(|| {
                            view! { "Checking your Wikimedia session..." }.into_any()
                        }))
                        .with_tone(Tone::Muted)
                        .with_element(TextElement::Paragraph)
                    )}
                }
                .into_any()
            })))
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
        return GateShell(GateShellProps::new(ui_children(|| {
            view! { <LocalSetupPanel /> }.into_any()
        })))
        .into_any();
    }

    let login_path = session.login_path.clone();
    let start_login = move |_| begin_login(&login_path, &current_login_next());

    let primary = if session.oauth_client_ready {
        Button(
            ButtonProps::new("Log in with your Wikimedia account")
                .with_tone(Tone::Success)
                .on_click(start_login),
        )
        .into_any()
    } else {
        Text(
            TextProps::new(ui_children(|| {
                view! {
                    "Wikimedia OAuth is not configured on this server (set WIKIMEDIA_CLIENT_APPLICATION_KEY and WIKIMEDIA_CLIENT_APPLICATION_SECRET)."
                }
                .into_any()
            }))
            .with_tone(Tone::Danger)
            .with_size(sp42_ui::Size::Small)
            .with_element(TextElement::Paragraph),
        )
        .into_any()
    };

    // Local-dev convenience: install a session from .env.wikimedia.local. Only
    // offered in local mode with a token available — the bootstrap route is
    // hard-gated to local mode server-side, so showing it in vps/desktop would
    // be a dead button that posts to a rejected endpoint. Codex review #90.
    let dev_bootstrap = Action::new_local(move |(): &()| async move {
        let request = sp42_core::DevAuthBootstrapRequest {
            username: String::new(),
            scopes: Vec::new(),
            expires_at_ms: None,
        };
        let _ = bootstrap_dev_auth_session(&request).await;
        refresh.run(());
    });
    let secondary = if is_local_deployment() && session.local_token_available {
        Button(
            ButtonProps::new("Use local developer token")
                .with_surface(ButtonSurface::Ghost)
                .on_click(move |_| {
                    dev_bootstrap.dispatch_local(());
                }),
        )
        .into_any()
    } else {
        ().into_any()
    };

    GateShell(GateShellProps::new(ui_children(move || {
        view! {
            {GateCard(GateCardProps::new(ui_children(move || view! {
                {WorkspaceBrand(WorkspaceBrandProps::new("SP42"))}
                {Heading(
                    HeadingProps::new(ui_children(|| view! { "Sign in to continue" }.into_any()))
                        .with_level(HeadingLevel::One)
                        .with_size(sp42_ui::Size::Large)
                )}
                {Text(
                    TextProps::new(ui_children(|| {
                        view! {
                            "SP42 patrols and edits Wikimedia projects using your own account. Log in with Wikimedia to use the workspace."
                        }
                        .into_any()
                    }))
                    .with_element(TextElement::Paragraph)
                )}
                {primary}
                {secondary}
            }.into_any())))}
        }
        .into_any()
    })))
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

    GateCard(GateCardProps::new(ui_children(move || {
        view! {
            {WorkspaceBrand(WorkspaceBrandProps::new("SP42"))}
            {Heading(
                HeadingProps::new(ui_children(|| view! { "Set up local access" }.into_any()))
                    .with_level(HeadingLevel::One)
                    .with_size(sp42_ui::Size::Large)
            )}
            {Text(
                TextProps::new(ui_children(|| {
                    view! {
                        "No Wikimedia credentials found. Paste a personal OAuth2 access token to start right away, or your OAuth consumer key + secret for the full login flow. "
                        {Link(
                            LinkProps::new(
                                "Where do I get these?",
                                "https://meta.wikimedia.org/wiki/Special:OAuthConsumerRegistration/propose/oauth2",
                            )
                            .external()
                        )}
                    }
                    .into_any()
                }))
                .with_element(TextElement::Paragraph)
            )}
            <form
                on:submit=move |ev| {
                    ev.prevent_default();
                    save.dispatch_local(());
                }
            >
                {Stack(StackProps::new(ui_children(move || view! {
                    {Field(FieldProps::new(
                        "Access token (quickest)",
                        ui_children(move || view! {
                            {TextInput(
                                TextInputProps::new("local-access-token")
                                    .with_type(TextInputType::Password)
                                    .with_autocomplete("off")
                                    .with_value(Signal::derive(move || token.get()))
                                    .with_width(Width::Full)
                                    .on_input(move |ev| set_token.set(event_target_value(&ev)))
                            )}
                        }.into_any()),
                    ))}
                    {Field(FieldProps::new(
                        "OAuth consumer key (optional)",
                        ui_children(move || view! {
                            {TextInput(
                                TextInputProps::new("local-oauth-key")
                                    .with_autocomplete("off")
                                    .with_value(Signal::derive(move || key.get()))
                                    .with_width(Width::Full)
                                    .on_input(move |ev| set_key.set(event_target_value(&ev)))
                            )}
                        }.into_any()),
                    ))}
                    {Field(FieldProps::new(
                        "OAuth consumer secret (optional)",
                        ui_children(move || view! {
                            {TextInput(
                                TextInputProps::new("local-oauth-secret")
                                    .with_type(TextInputType::Password)
                                    .with_autocomplete("off")
                                    .with_value(Signal::derive(move || secret.get()))
                                    .with_width(Width::Full)
                                    .on_input(move |ev| set_secret.set(event_target_value(&ev)))
                            )}
                        }.into_any()),
                    ))}
                    {Button(
                        ButtonProps::new("Save to .env.wikimedia.local")
                            .with_type(ButtonType::Submit)
                            .with_tone(Tone::Success)
                    )}
                }.into_any())).with_gap(Gap::Small))}
            </form>
            {move || match result.get() {
                Some(Ok(file)) => Text(
                    TextProps::new(ui_children(move || {
                        view! {
                            "Saved to " {file}
                            ". Restart the dev server (Ctrl-C, then ./scripts/dev-local.sh) to apply."
                        }
                        .into_any()
                    }))
                    .with_tone(Tone::Muted)
                    .with_size(sp42_ui::Size::Small)
                    .with_element(TextElement::Paragraph),
                )
                .into_any(),
                Some(Err(error)) => Text(
                    TextProps::new(ui_children(move || view! { {error} }.into_any()))
                        .with_tone(Tone::Danger)
                        .with_size(sp42_ui::Size::Small)
                        .with_element(TextElement::Paragraph),
                )
                .into_any(),
                None => ().into_any(),
            }}
        }
        .into_any()
    })))
}

/// The authenticated workspace (the former `App` body) plus a session header.
#[component]
fn Workspace(session: AuthSession, refresh: Callback<()>) -> impl IntoView {
    use sp42_ui::theme::{ThemeToggle, restore_theme};
    let (active_view, set_active_view) = signal(initial_workspace_view());
    let theme = restore_theme();

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

    WorkspaceShell(WorkspaceShellProps::new(ui_children(move || {
        view! {
            {WorkspaceNav(WorkspaceNavProps::new(
                "SP42 workspace",
                ui_children(move || view! {
                    {WorkspaceBrand(WorkspaceBrandProps::new("SP42"))}
                    {WorkspaceTabs(
                        WorkspaceTabsProps::new(
                            "Workspace views",
                            ui_children(move || view! {
                                {WorkspaceTab(
                                    WorkspaceTabProps::new("Patrol")
                                        .with_selected(Signal::derive(move || {
                                            active_view.get() == WorkspaceView::Patrol
                                        }))
                                        .on_click(show_patrol)
                                )}
                                {WorkspaceTab(
                                    WorkspaceTabProps::new("Article")
                                        .with_selected(Signal::derive(move || {
                                            active_view.get() == WorkspaceView::Article
                                        }))
                                        .on_click(show_article)
                                )}
                                {WorkspaceTab(
                                    WorkspaceTabProps::new("Citations")
                                        .with_selected(Signal::derive(move || {
                                            active_view.get() == WorkspaceView::Citation
                                        }))
                                        .on_click(show_citation)
                                )}
                            }.into_any()),
                        )
                    )}
                    {WorkspaceInlineForm(
                        WorkspaceInlineFormProps::new(ui_children(move || view! {
                            {Field(FieldProps::new(
                                "Wiki",
                                ui_children(move || view! {
                                    {TextInput(
                                        TextInputProps::new("wiki-picker")
                                            .with_value(Signal::derive(move || wiki_input.get()))
                                            .with_list("sp42-wiki-suggestions")
                                            .with_aria_label("Wikimedia project (dbname, e.g. enwiki, commonswiki)")
                                            .with_width(Width::Medium)
                                            .with_density(Density::Compact)
                                            .on_input(move |ev| {
                                                set_wiki_input.set(event_target_value(&ev));
                                            })
                                    )}
                                }.into_any()),
                            ))}
                            <datalist id="sp42-wiki-suggestions">
                                {move || {
                                    wiki_options
                                        .get()
                                        .into_iter()
                                        .map(|id| view! { <option value=id></option> })
                                        .collect_view()
                                }}
                            </datalist>
                            {Button(
                                ButtonProps::new("Switch")
                                    .with_type(ButtonType::Submit)
                                    .with_surface(ButtonSurface::Ghost)
                                    .with_density(Density::Compact)
                            )}
                        }.into_any()))
                        .on_submit(move |ev| {
                            ev.prevent_default();
                            request_wiki_switch(&wiki_input.get_untracked());
                        })
                    )}
                    {WorkspaceSession(WorkspaceSessionProps::new(ui_children(move || view! {
                        <span>{username.clone()}</span>
                        {Button(
                            ButtonProps::new("Log out")
                                .with_surface(ButtonSurface::Ghost)
                                .with_density(Density::Compact)
                                .on_click(move |_| {
                                    do_logout.dispatch_local(());
                                })
                        )}
                    }.into_any())))}
                    {ThemeToggle(theme)}
                }.into_any()),
            ))}
            {WorkspaceBody(WorkspaceBodyProps::new(ui_children(move || view! {
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
            }.into_any())))}
        }
        .into_any()
    })))
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
            let relative = format!("{path}{search}{hash}");
            // In split frontend/API deployments the callback redirect runs on the
            // API origin, so a relative target would strand the user on the
            // backend. Return an absolute frontend-origin URL (the server
            // validates it against its allowed origins). Same-origin deployments
            // keep the relative path. Codex review #90.
            if is_split_origin_deployment() {
                let origin = location.origin().ok()?;
                Some(format!("{origin}{relative}"))
            } else {
                Some(relative)
            }
        })
        .unwrap_or_else(|| "/".to_string())
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
