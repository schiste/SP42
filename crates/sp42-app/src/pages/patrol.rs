use leptos::prelude::*;
use sp42_core::{LiveOperatorView, SessionActionExecutionRequest, SessionActionKind};

use crate::components::action_bar::ActionBar;
use crate::components::context_sidebar::ContextSidebar;
use crate::components::diff_viewer::DiffViewer;
use crate::components::filter_bar::{FilterBar, PatrolFilterParams};
use crate::components::queue_column::QueueColumn;
use crate::components::{PatrolScenarioPanel, PatrolSessionDigestPanel, ShellStatePanel};
use crate::platform::auth::{bootstrap_dev_auth_session, execute_dev_auth_action};
use crate::platform::live::fetch_live_operator_view;

const DEFAULT_WIKI_ID: &str = "frwiki";

#[component]
pub fn PatrolSurface() -> impl IntoView {
    let (view_data, set_view_data) = signal(None::<LiveOperatorView>);
    let (load_error, set_load_error) = signal(None::<String>);
    let (selected_index, set_selected_index) = signal(0usize);
    let (action_trigger, set_action_trigger) = signal(None::<SessionActionKind>);
    let (skip_trigger, set_skip_trigger) = signal(false);
    let (action_pending, set_action_pending) = signal(false);
    let (action_status, set_action_status) = signal(String::new());
    let (filters, set_filters) = signal(PatrolFilterParams::default());
    let (next_continue, set_next_continue) = signal(None::<String>);
    let (review_note, set_review_note) = signal(String::new());
    let (show_help, set_show_help) = signal(false);
    let (show_backoffice, set_show_backoffice) = signal(false);
    let (bootstrap_attempted, set_bootstrap_attempted) = signal(false);
    let (bootstrap_error, set_bootstrap_error) = signal(None::<String>);

    // If the response shows no auth and we haven't tried yet, auto-bootstrap.
    let load_action = Action::new_local(move |_: &()| {
        let set_view_data = set_view_data;
        let set_load_error = set_load_error;
        let set_next_continue = set_next_continue;
        async move {
            let current_filters = filters.get();
            match fetch_live_operator_view(DEFAULT_WIKI_ID, &current_filters).await {
                Ok(view) => {
                    if view.auth.username.is_none() && !bootstrap_attempted.get_untracked() {
                        // Auto-bootstrap: try the local token bridge
                        set_bootstrap_attempted.set(true);
                        let request = sp42_core::DevAuthBootstrapRequest {
                            username: String::new(),
                            scopes: Vec::new(),
                            expires_at_ms: None,
                        };
                        match bootstrap_dev_auth_session(&request).await {
                            Ok(status) if status.authenticated => {
                                set_bootstrap_error.set(None);
                                // Re-fetch now that we have a session
                                match fetch_live_operator_view(DEFAULT_WIKI_ID, &current_filters).await {
                                    Ok(view2) => {
                                        set_load_error.set(None);
                                        set_next_continue.set(view2.next_continue.clone());
                                        set_view_data.set(Some(view2));
                                    }
                                    Err(error) => set_load_error.set(Some(error)),
                                }
                                return;
                            }
                            Ok(_) => {
                                set_bootstrap_error.set(Some(
                                    "Bootstrap succeeded but session not authenticated. Check .env.wikimedia.local token.".to_string(),
                                ));
                            }
                            Err(error) => {
                                set_bootstrap_error.set(Some(format!("Bootstrap failed: {error}")));
                            }
                        }
                    }
                    set_load_error.set(None);
                    set_next_continue.set(view.next_continue.clone());
                    set_view_data.set(Some(view));
                }
                Err(error) => {
                    set_load_error.set(Some(error));
                }
            }
        }
    });

    let execute_action = Action::new_local(move |kind: &SessionActionKind| {
        let kind = kind.clone();
        let set_action_pending = set_action_pending;
        let set_action_status = set_action_status;
        let set_action_trigger = set_action_trigger;
        async move {
            set_action_pending.set(true);

            let Some(view) = view_data.get() else {
                set_action_pending.set(false);
                return;
            };
            let idx = selected_index.get();
            let Some(edit) = view.queue.get(idx) else {
                set_action_pending.set(false);
                return;
            };

            let request = SessionActionExecutionRequest {
                wiki_id: view.wiki_id.clone(),
                kind: kind.clone(),
                rev_id: edit.event.rev_id,
                title: Some(edit.event.title.clone()),
                target_user: match &edit.event.performer {
                    sp42_core::EditorIdentity::Anonymous { label } => Some(label.clone()),
                    sp42_core::EditorIdentity::Registered { username } => Some(username.clone()),
                    sp42_core::EditorIdentity::Temporary { label } => Some(label.clone()),
                },
                undo_after_rev_id: edit.event.old_rev_id,
                summary: {
                    let note = review_note.get();
                    if note.is_empty() { None } else { Some(note) }
                },
            };

            match execute_dev_auth_action(&request).await {
                Ok(response) => {
                    if response.accepted {
                        set_action_status
                            .set(format!("{} accepted for rev {}", kind.label(), edit.event.rev_id));
                        let queue_len = view.queue.len();
                        if idx + 1 < queue_len {
                            set_selected_index.set(idx + 1);
                        }
                        set_review_note.set(String::new());
                        // Re-fetch fresh queue
                        load_action.dispatch_local(());
                    } else {
                        set_action_status.set(format!(
                            "{} rejected: {}",
                            kind.label(),
                            response.message.unwrap_or_default()
                        ));
                    }
                }
                Err(error) => {
                    set_action_status.set(format!("Action error: {error}"));
                }
            }

            set_action_pending.set(false);
            set_action_trigger.set(None);
        }
    });

    let queue_len = Memo::new(move |_| {
        view_data.get().map_or(0, |v| v.queue.len())
    });
    let has_selection = Memo::new(move |_| selected_index.get() < queue_len.get());

    Effect::new(move |_| {
        let _ = filters.get(); // Track filter signal
        set_selected_index.set(0); // Reset selection on filter change
        load_action.dispatch_local(());
    });

    Effect::new(move |_| {
        if let Some(kind) = action_trigger.get() {
            execute_action.dispatch_local(kind);
        }
    });

    Effect::new(move |_| {
        if skip_trigger.get() {
            set_skip_trigger.set(false);
            let idx = selected_index.get();
            if idx + 1 < queue_len.get() {
                set_selected_index.set(idx + 1);
            }
        }
    });

    let on_keydown = move |event: leptos::ev::KeyboardEvent| {
        // Don't intercept when typing in an input
        let tag = event
            .target()
            .and_then(|t| {
                use wasm_bindgen::JsCast;
                t.dyn_into::<web_sys::Element>().ok()
            })
            .map(|el| el.tag_name());
        if matches!(tag.as_deref(), Some("INPUT") | Some("TEXTAREA")) {
            return;
        }

        match event.key().as_str() {
            "r" | "R" => set_action_trigger.set(Some(SessionActionKind::Rollback)),
            "u" | "U" => set_action_trigger.set(Some(SessionActionKind::Undo)),
            "p" | "P" => set_action_trigger.set(Some(SessionActionKind::Patrol)),
            "s" | "S" => set_skip_trigger.set(true),
            "ArrowUp" => {
                event.prevent_default();
                let idx = selected_index.get();
                if idx > 0 {
                    set_selected_index.set(idx - 1);
                }
            }
            "ArrowDown" => {
                event.prevent_default();
                let idx = selected_index.get();
                if idx + 1 < queue_len.get() {
                    set_selected_index.set(idx + 1);
                }
            }
            "D" if event.ctrl_key() && event.shift_key() => {
                event.prevent_default();
                set_show_backoffice.update(|v| *v = !*v);
            }
            "?" => set_show_help.set(true),
            "Escape" => {
                set_show_help.set(false);
                set_show_backoffice.set(false);
            }
            _ => {}
        }
    };

    view! {
        {move || {
            // If the fetch succeeded but no user is authenticated, show the
            // auth bootstrap prompt instead of the patrol layout.
            if let Some(ref view) = view_data.get() {
                if view.auth.username.is_none() {
                    let bootstrap_btn_action = Action::new_local(move |_: &()| {
                        async move {
                            set_bootstrap_attempted.set(false); // Allow re-attempt
                            load_action.dispatch_local(());
                        }
                    });
                    let bridge_mode = view.auth.bridge_mode.clone();
                    let has_token = view.auth.local_token_available;
                    return view! {
                        <div style="display:grid;place-items:center;height:100vh;\
                                    background:#08111f;color:#eff4ff;padding:27px;">
                            <div style="max-width:440px;text-align:center;">
                                <h1 style="font-size:21px;margin:0 0 10px;">
                                    "Authentication required"
                                </h1>
                                {if bootstrap_attempted.get() {
                                    view! {
                                        <p style="color:#f59e0b;font-size:13px;margin:0 0 10px;">
                                            {bootstrap_error.get().unwrap_or_else(|| "Auto-bootstrap did not produce an authenticated session.".to_string())}
                                        </p>
                                    }.into_any()
                                } else {
                                    view! {
                                        <p style="color:#8b9fc0;margin:0 0 10px;">
                                            "Bootstrapping session from local token bridge..."
                                        </p>
                                    }.into_any()
                                }}
                                <div style="font-size:12px;color:#8b9fc0;margin:0 0 17px;display:grid;gap:4px;">
                                    <div>{format!("Bridge mode: {bridge_mode}")}</div>
                                    <div>{format!("Local token: {}", if has_token { "present" } else { "missing" })}</div>
                                </div>
                                {if !has_token {
                                    view! {
                                        <p style="color:#ef4444;font-size:12px;margin:0 0 17px;">
                                            "No WIKIMEDIA_ACCESS_TOKEN found. Create a .env.wikimedia.local file with your token and restart the server."
                                        </p>
                                    }.into_any()
                                } else {
                                    view! { <span></span> }.into_any()
                                }}
                                <div style="display:flex;gap:10px;justify-content:center;">
                                    <button
                                        style="min-height:44px;padding:4px 17px;\
                                               border:1px solid rgba(59,130,246,.5);border-radius:4px;\
                                               background:rgba(59,130,246,.15);color:#eff4ff;font:inherit;\
                                               cursor:pointer;"
                                        on:click=move |_| { bootstrap_btn_action.dispatch_local(()); }
                                    >
                                        "Bootstrap session"
                                    </button>
                                    <button
                                        style="min-height:44px;padding:4px 17px;\
                                               border:1px solid rgba(148,163,184,.18);border-radius:4px;\
                                               background:transparent;color:#eff4ff;font:inherit;\
                                               cursor:pointer;"
                                        on:click=move |_| { load_action.dispatch_local(()); }
                                    >
                                        "Retry"
                                    </button>
                                </div>
                            </div>
                        </div>
                    }.into_any();
                }
            }

            view! {
                <div
                    tabindex="0"
                    on:keydown=on_keydown
                    class="sp42-patrol-grid"
                    style="display:grid;grid-template-columns:min(220px,25vw) 1fr min(220px,25vw);\
                           grid-template-rows:auto auto 1fr auto;\
                           height:100vh;background:#08111f;color:#eff4ff;\
                           font-family:system-ui,-apple-system,BlinkMacSystemFont,'Segoe UI',\
                           Roboto,'Noto Sans','Liberation Sans',sans-serif;"
                >
                    <style>
                        {"@media (max-width: 768px) { \
                           .sp42-patrol-grid { \
                             grid-template-columns: 1fr !important; \
                             grid-template-rows: auto auto auto 1fr auto auto !important; \
                           } \
                           .sp42-patrol-grid nav { max-height: 30vh; } \
                           .sp42-patrol-grid aside { max-height: 25vh; } \
                         }"}
                    </style>

                    {move || {
                        if !show_help.get() {
                            return view! { <span></span> }.into_any();
                        }
                        view! {
                            <div
                                style="position:fixed;inset:0;z-index:100;\
                                       display:grid;place-items:center;\
                                       background:rgba(8,17,31,.85);"
                                on:click=move |_| set_show_help.set(false)
                            >
                                <div
                                    style="max-width:400px;width:90%;padding:27px;\
                                           background:#0b1324;border:1px solid rgba(148,163,184,.18);\
                                           border-radius:6px;"
                                    on:click=move |ev| ev.stop_propagation()
                                >
                                    <h2 style="margin:0 0 17px;font-size:17px;">"Keyboard Shortcuts"</h2>
                                    <div style="display:grid;gap:7px;font-size:13px;">
                                        <div style="display:flex;justify-content:space-between;">
                                            <span style="color:#8b9fc0;">"Rollback"</span>
                                            <kbd style="color:#eff4ff;font-weight:700;">"R"</kbd>
                                        </div>
                                        <div style="display:flex;justify-content:space-between;">
                                            <span style="color:#8b9fc0;">"Undo"</span>
                                            <kbd style="color:#eff4ff;font-weight:700;">"U"</kbd>
                                        </div>
                                        <div style="display:flex;justify-content:space-between;">
                                            <span style="color:#8b9fc0;">"Patrol"</span>
                                            <kbd style="color:#eff4ff;font-weight:700;">"P"</kbd>
                                        </div>
                                        <div style="display:flex;justify-content:space-between;">
                                            <span style="color:#8b9fc0;">"Skip"</span>
                                            <kbd style="color:#eff4ff;font-weight:700;">"S"</kbd>
                                        </div>
                                        <div style="display:flex;justify-content:space-between;">
                                            <span style="color:#8b9fc0;">"Previous edit"</span>
                                            <kbd style="color:#eff4ff;font-weight:700;">"\u{2191}"</kbd>
                                        </div>
                                        <div style="display:flex;justify-content:space-between;">
                                            <span style="color:#8b9fc0;">"Next edit"</span>
                                            <kbd style="color:#eff4ff;font-weight:700;">"\u{2193}"</kbd>
                                        </div>
                                        <div style="display:flex;justify-content:space-between;">
                                            <span style="color:#8b9fc0;">"This help"</span>
                                            <kbd style="color:#eff4ff;font-weight:700;">"?"</kbd>
                                        </div>
                                        <div style="display:flex;justify-content:space-between;">
                                            <span style="color:#8b9fc0;">"Back-office"</span>
                                            <kbd style="color:#eff4ff;font-weight:700;">"Ctrl+Shift+D"</kbd>
                                        </div>
                                    </div>
                                    <button
                                        style="margin-top:17px;min-height:44px;width:100%;padding:4px 17px;\
                                               border:1px solid rgba(148,163,184,.18);border-radius:4px;\
                                               background:transparent;color:#eff4ff;font:inherit;\
                                               font-size:13px;cursor:pointer;"
                                        on:click=move |_| set_show_help.set(false)
                                    >
                                        "Close"
                                    </button>
                                </div>
                            </div>
                        }.into_any()
                    }}

                    {move || {
                        if !show_backoffice.get() {
                            return view! { <span></span> }.into_any();
                        }
                        let view = view_data.get();
                        view! {
                            <div
                                style="position:fixed;inset:0;z-index:90;\
                                       overflow-y:auto;\
                                       background:rgba(8,17,31,.95);"
                            >
                                <div style="max-width:1200px;margin:0 auto;padding:27px;\
                                            display:grid;gap:17px;">
                                    <div style="display:flex;align-items:center;justify-content:space-between;">
                                        <h2 style="margin:0;font-size:17px;color:#eff4ff;">
                                            "Back-office"
                                        </h2>
                                        <button
                                            style="min-height:44px;padding:4px 17px;\
                                                   border:1px solid rgba(148,163,184,.18);border-radius:4px;\
                                                   background:transparent;color:#eff4ff;font:inherit;\
                                                   font-size:13px;cursor:pointer;"
                                            on:click=move |_| set_show_backoffice.set(false)
                                        >
                                            "Close (Esc)"
                                        </button>
                                    </div>
                                    {if let Some(ref v) = view {
                                        let history_entries = v.action_history.entries.clone();
                                        view! {
                                            <PatrolScenarioPanel report=v.scenario_report.clone() />
                                            <PatrolSessionDigestPanel report=v.scenario_report.clone() />
                                            <ShellStatePanel model=v.shell_state.clone() />
                                            {if history_entries.is_empty() {
                                                view! { <span></span> }.into_any()
                                            } else {
                                                view! {
                                                    <section
                                                        style="display:grid;gap:10px;padding:17px;border-radius:6px;\
                                                               border:1px solid rgba(148,163,184,.18);\
                                                               background:rgba(15,23,42,.88);"
                                                    >
                                                        <h3 style="margin:0;font-size:13px;font-weight:700;color:#eff4ff;">
                                                            "Action History"
                                                        </h3>
                                                        <div style="display:grid;gap:4px;">
                                                            {history_entries.into_iter().map(|entry| {
                                                                let label = entry.kind.label().to_string();
                                                                let status_color = if entry.accepted { "#22c55e" } else { "#ef4444" };
                                                                let status_text = if entry.accepted { "OK" } else { "Failed" };
                                                                let detail = entry.error.or(entry.api_code).unwrap_or_default();
                                                                view! {
                                                                    <div style="display:flex;align-items:center;gap:7px;\
                                                                                font-size:12px;padding:4px 0;\
                                                                                border-block-end:1px solid rgba(148,163,184,.12);">
                                                                        <span style="font-weight:700;color:#eff4ff;text-transform:capitalize;">
                                                                            {label}
                                                                        </span>
                                                                        <span style="color:#8b9fc0;">
                                                                            {format!("r{}", entry.rev_id)}
                                                                        </span>
                                                                        <span style=format!("color:{status_color};font-weight:700;")>
                                                                            {status_text}
                                                                        </span>
                                                                        {if !detail.is_empty() {
                                                                            view! {
                                                                                <span style="color:#f59e0b;font-size:11px;">
                                                                                    {detail}
                                                                                </span>
                                                                            }.into_any()
                                                                        } else {
                                                                            view! { <span></span> }.into_any()
                                                                        }}
                                                                    </div>
                                                                }
                                                            }).collect_view()}
                                                        </div>
                                                    </section>
                                                }.into_any()
                                            }}
                                        }.into_any()
                                    } else {
                                        view! {
                                            <p style="color:#8b9fc0;">"Load the patrol queue first."</p>
                                        }.into_any()
                                    }}
                                </div>
                            </div>
                        }.into_any()
                    }}

                    <div style="grid-column:1/-1;display:flex;align-items:center;gap:10px;\
                                padding:4px 10px;height:28px;\
                                background:#0b1324;border-block-end:1px solid rgba(148,163,184,.18);\
                                font-size:12px;color:#8b9fc0;">
                        <span style="font-weight:700;color:#8fb7ff;">
                            {sp42_core::branding::PROJECT_NAME}
                        </span>
                        {move || {
                            view_data
                                .get()
                                .map(|v| {
                                    view! {
                                        <span>{v.wiki_id.clone()}</span>
                                        <span>
                                            {v.auth.username.clone().unwrap_or_else(|| "not authenticated".to_string())}
                                        </span>
                                        <span>
                                            {format!("{} edits in queue", v.queue.len())}
                                        </span>
                                    }
                                        .into_any()
                                })
                                .unwrap_or_else(|| view! { <span>"loading..."</span> }.into_any())
                        }}
                        <div style="flex:1;"></div>
                        // Connection indicator
                        <span style="width:10px;height:10px;border-radius:4px;\
                                     background:{};display:inline-block;"
                            style:background=move || {
                                if load_error.get().is_some() {
                                    "#ef4444"
                                } else if view_data.get().is_some() {
                                    "#22c55e"
                                } else {
                                    "#f59e0b"
                                }
                            }
                        ></span>
                        {move || {
                            if let Some(ref view) = view_data.get() {
                                if !view.notes.is_empty() {
                                    return view! {
                                        <span style="font-size:11px;color:#f59e0b;">
                                            {view.notes.join(" | ")}
                                        </span>
                                    }.into_any();
                                }
                            }
                            view! { <span></span> }.into_any()
                        }}
                        {move || {
                            if let Some(ref view) = view_data.get() {
                                if let Some(ref room) = view.coordination_room {
                                    return view! {
                                        <span style="font-size:11px;color:#8b9fc0;">
                                            {format!("{} online", room.connected_clients)}
                                        </span>
                                    }.into_any();
                                }
                            }
                            view! { <span></span> }.into_any()
                        }}
                        {move || {
                            let status = action_status.get();
                            if !status.is_empty() {
                                view! {
                                    <span style="font-size:11px;">{status}</span>
                                }
                                    .into_any()
                            } else {
                                view! { <span></span> }.into_any()
                            }
                        }}
                        // Telemetry: server response time
                        {move || {
                            if let Some(ref view) = view_data.get() {
                                if view.telemetry.total_duration_ms > 0 {
                                    return view! {
                                        <span style="font-size:11px;color:#8b9fc0;">
                                            {format!("{}ms", view.telemetry.total_duration_ms)}
                                        </span>
                                    }.into_any();
                                }
                            }
                            view! { <span></span> }.into_any()
                        }}
                        {move || {
                            if let Some(ref view) = view_data.get() {
                                let status = &view.action_status;
                                if status.total_actions > 0 {
                                    let label = format!(
                                        "{} actions ({} OK)",
                                        status.total_actions, status.successful_actions,
                                    );
                                    let has_failure = status.last_execution.as_ref().is_some_and(|e| !e.accepted);
                                    let color = if has_failure { "#f59e0b" } else { "#8b9fc0" };
                                    return view! {
                                        <span style=format!("font-size:11px;color:{color};")>
                                            {label}
                                        </span>
                                    }.into_any();
                                }
                            }
                            view! { <span style="font-size:11px;color:#8b9fc0;">"0 actions"</span> }.into_any()
                        }}
                        <button
                            style="min-height:32px;padding:2px 8px;\
                                   border:1px solid rgba(148,163,184,.18);border-radius:4px;\
                                   background:transparent;color:#8b9fc0;font:inherit;\
                                   font-size:12px;cursor:pointer;"
                            on:click=move |_| set_show_help.set(true)
                        >
                            "?"
                        </button>
                    </div>

                    <div style="grid-column:1/-1;">
                        <FilterBar
                            filters=filters
                            set_filters=set_filters
                            next_continue=next_continue
                        />
                    </div>

                    {move || {
                        if let Some(view) = view_data.get() {
                            view! {
                                <QueueColumn
                                    queue=view.queue.clone()
                                    selected_index=selected_index
                                    set_selected_index=set_selected_index
                                />
                            }
                                .into_any()
                        } else if let Some(error) = load_error.get() {
                            view! {
                                <div style="padding:17px;color:#ef4444;">
                                    <p style="font-weight:700;">"Queue unavailable"</p>
                                    <p style="font-size:12px;">{error}</p>
                                    <button
                                        style="margin-top:10px;min-height:44px;padding:4px 17px;\
                                               border:1px solid rgba(148,163,184,.18);border-radius:4px;\
                                               background:transparent;color:#eff4ff;font:inherit;\
                                               cursor:pointer;"
                                        on:click=move |_| { load_action.dispatch_local(()); }
                                    >
                                        "Retry"
                                    </button>
                                </div>
                            }
                                .into_any()
                        } else {
                            view! {
                                <div style="padding:17px;color:#8b9fc0;">
                                    "Loading queue..."
                                </div>
                            }
                                .into_any()
                        }
                    }}

                    <div style="min-width:0;min-height:0;overflow-y:auto;overflow-x:hidden;">
                        {move || {
                            if let Some(view) = view_data.get() {
                                let idx = selected_index.get();
                                let server_idx = view.selected_index.unwrap_or(0);
                                // Diff is only valid for the server-selected edit
                                if idx == server_idx {
                                    view! { <DiffViewer diff=view.diff.clone() /> }.into_any()
                                } else {
                                    // Server only provides the diff for the top-ranked edit
                                    view! {
                                        <div style="display:grid;place-items:center;height:100%;color:#8b9fc0;font-size:12px;">
                                            <p>"Diff available for the top-ranked edit only."</p>
                                        </div>
                                    }.into_any()
                                }
                            } else {
                                view! {
                                    <div
                                        role="main"
                                        aria-label="Diff viewer"
                                        style="display:grid;place-items:center;color:#8b9fc0;"
                                    >
                                        {if load_error.get().is_some() {
                                            "Diff unavailable."
                                        } else {
                                            "Loading diff..."
                                        }}
                                    </div>
                                }
                                    .into_any()
                            }
                        }}
                    </div>

                    {move || {
                        if let Some(view) = view_data.get() {
                            let idx = selected_index.get();
                            let edit = view.queue.get(idx).cloned();
                            view! {
                                <ContextSidebar
                                    view=view.clone()
                                    edit=edit
                                />
                            }
                                .into_any()
                        } else {
                            view! {
                                <aside
                                    role="complementary"
                                    aria-label="Edit context"
                                    style="padding:10px;color:#8b9fc0;\
                                           border-inline-start:1px solid rgba(148,163,184,.18);"
                                >
                                    "Loading..."
                                </aside>
                            }
                                .into_any()
                        }
                    }}

                    <div style="grid-column:1/-1;">
                        <div style="display:flex;align-items:center;gap:7px;\
                                    padding:4px 10px;background:#0b1324;\
                                    border-block-start:1px solid rgba(148,163,184,.18);">
                            <input
                                type="text"
                                placeholder="Review note (optional)"
                                aria-label="Review note"
                                style="flex:1;min-height:32px;padding:4px 10px;\
                                       background:#111b2e;color:#eff4ff;\
                                       border:1px solid rgba(148,163,184,.18);\
                                       border-radius:4px;font:inherit;font-size:12px;"
                                prop:value=move || review_note.get()
                                on:input=move |ev| {
                                    use wasm_bindgen::JsCast;
                                    let value = ev.target()
                                        .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                                        .map(|el| el.value())
                                        .unwrap_or_default();
                                    set_review_note.set(value);
                                }
                            />
                        </div>
                        {move || {
                            if let Some(view) = view_data.get() {
                                view! {
                                    <ActionBar
                                        preflight=view.action_preflight.clone()
                                        capabilities=view.capabilities.clone()
                                        has_selection=Signal::derive(move || has_selection.get())
                                        action_pending=Signal::derive(move || action_pending.get())
                                        on_action=set_action_trigger
                                        on_skip=set_skip_trigger
                                    />
                                }
                                    .into_any()
                            } else {
                                view! {
                                    <div style="display:flex;align-items:center;padding:0 10px;\
                                                height:44px;background:#0b1324;\
                                                border-block-start:1px solid rgba(148,163,184,.18);\
                                                font-size:12px;color:#8b9fc0;">
                                        "Actions available after queue loads."
                                    </div>
                                }
                                    .into_any()
                            }
                        }}
                    </div>
                </div>
            }.into_any()
        }}
    }
}
