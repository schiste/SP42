use std::collections::HashMap;

use leptos::prelude::*;
use sp42_core::{LiveOperatorView, SessionActionExecutionRequest, SessionActionKind, StructuredDiff};

use crate::components::action_bar::ActionBar;
use crate::components::context_header::ContextHeader;
use crate::components::diff_viewer::DiffViewer;
use crate::components::filter_bar::{FilterBar, PatrolFilterParams};
use crate::components::queue_column::QueueColumn;
use crate::components::{PatrolScenarioPanel, PatrolSessionDigestPanel, ShellStatePanel};
use crate::platform::auth::{bootstrap_dev_auth_session, execute_dev_auth_action};
use crate::platform::eventstream::{StreamEvent, start_eventstream};
use crate::platform::live::{fetch_diff, fetch_live_operator_view};

/// Read `rev=N` from the URL hash fragment.
fn rev_id_from_hash() -> Option<u64> {
    #[cfg(target_arch = "wasm32")]
    {
        let hash = web_sys::window()?.location().hash().ok()?;
        let hash = hash.trim_start_matches('#');
        for part in hash.split('&') {
            if let Some(val) = part.strip_prefix("rev=") {
                return val.parse().ok();
            }
        }
        None
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        None
    }
}

/// Update the URL hash to reflect the selected revision.
fn set_hash_rev(rev_id: u64) {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(window) = web_sys::window() {
            let _ = window.location().set_hash(&format!("rev={rev_id}"));
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = rev_id;
    }
}

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
    let (diff_loading, set_diff_loading) = signal(false);
    let (current_diff, set_current_diff) = signal(None::<StructuredDiff>);
    let (diff_cache, set_diff_cache) = signal(HashMap::<u64, StructuredDiff>::new());
    let (queue_signal, set_queue_signal) = signal(Vec::<sp42_core::QueuedEdit>::new());
    let (selection_only_refetch, set_selection_only_refetch) = signal(false);
    let (bootstrap_attempted, set_bootstrap_attempted) = signal(false);
    let (bootstrap_error, set_bootstrap_error) = signal(None::<String>);

    // If the response shows no auth and we haven't tried yet, auto-bootstrap.
    let load_action = Action::new_local(move |_: &()| {
        let set_view_data = set_view_data;
        let set_load_error = set_load_error;
        let set_next_continue = set_next_continue;
        async move {
            let mut current_filters = filters.get();
            current_filters.selected_index = Some(selected_index.get_untracked());
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
                                match fetch_live_operator_view(DEFAULT_WIKI_ID, &current_filters)
                                    .await
                                {
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
                    // Cache the initial diff from the server response
                    if let (Some(diff), Some(sel_idx)) = (&view.diff, view.selected_index) {
                        if let Some(edit) = view.queue.get(sel_idx) {
                            let mut c = diff_cache.get_untracked();
                            c.insert(edit.event.rev_id, diff.clone());
                            set_diff_cache.set(c);
                        }
                    }
                    if let Some(ref diff) = view.diff {
                        set_current_diff.set(Some(diff.clone()));
                    }
                    // On initial load, select the edit from the URL hash if present
                    if !selection_only_refetch.get_untracked() {
                        if let Some(target_rev) = rev_id_from_hash() {
                            if let Some(idx) = view.queue.iter().position(|q| q.event.rev_id == target_rev) {
                                set_selected_index.set(idx);
                            }
                        }
                        set_queue_signal.set(view.queue.clone());
                    }
                    set_selection_only_refetch.set(false);
                    // Prefetch diffs for all queue items in the background
                    let prefetch_queue = view.queue.clone();
                    let prefetch_wiki = view.wiki_id.clone();
                    wasm_bindgen_futures::spawn_local(async move {
                        for item in &prefetch_queue {
                            let rev_id = item.event.rev_id;
                            if diff_cache.get_untracked().contains_key(&rev_id) {
                                continue;
                            }
                            let old_rev_id = item.event.old_rev_id.unwrap_or(0);
                            if old_rev_id == 0 {
                                continue;
                            }
                            if let Ok(Some(diff)) = fetch_diff(&prefetch_wiki, rev_id, old_rev_id).await {
                                let mut c = diff_cache.get_untracked();
                                c.insert(rev_id, diff);
                                set_diff_cache.set(c);
                            }
                        }
                    });
                    set_view_data.set(Some(view));
                    set_diff_loading.set(false);
                }
                Err(error) => {
                    set_load_error.set(Some(error));
                    set_diff_loading.set(false);
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
                        set_action_status.set(format!(
                            "{} accepted for rev {}",
                            kind.label(),
                            edit.event.rev_id
                        ));
                        // Remove the acted-on edit from the queue
                        let mut q = queue_signal.get_untracked();
                        if idx < q.len() {
                            q.remove(idx);
                        }
                        let new_idx = if idx >= q.len() && !q.is_empty() {
                            q.len() - 1
                        } else {
                            idx
                        };
                        set_queue_signal.set(q);
                        set_selected_index.set(new_idx);
                        set_review_note.set(String::new());
                        // Re-fetch to get fresh diff for the new selection
                        set_selection_only_refetch.set(true);
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

    let queue_len = Memo::new(move |_| view_data.get().map_or(0, |v| v.queue.len()));
    let has_selection = Memo::new(move |_| selected_index.get() < queue_len.get());

    Effect::new(move |_| {
        let _ = filters.get();
        set_selected_index.set(0);
        load_action.dispatch_local(());
    });

    // When selection changes, look up diff from cache or fetch it.
    Effect::new(move |prev: Option<usize>| {
        let idx = selected_index.get();
        let queue = queue_signal.get_untracked();
        if let Some(edit) = queue.get(idx) {
            set_hash_rev(edit.event.rev_id);
            let rev_id = edit.event.rev_id;
            let cache = diff_cache.get_untracked();
            if let Some(diff) = cache.get(&rev_id) {
                set_current_diff.set(Some(diff.clone()));
            } else if let Some(prev_idx) = prev {
                if prev_idx != idx {
                    set_diff_loading.set(true);
                    set_current_diff.set(None);
                    let old_rev_id = edit.event.old_rev_id.unwrap_or(0);
                    let wiki_id = edit.event.wiki_id.clone();
                    wasm_bindgen_futures::spawn_local(async move {
                        match fetch_diff(&wiki_id, rev_id, old_rev_id).await {
                            Ok(diff) => {
                                if let Some(ref d) = diff {
                                    let mut c = diff_cache.get_untracked();
                                    c.insert(rev_id, d.clone());
                                    set_diff_cache.set(c);
                                }
                                set_current_diff.set(diff);
                            }
                            Err(_) => {
                                set_current_diff.set(None);
                            }
                        }
                        set_diff_loading.set(false);
                    });
                }
            }
        }
        idx
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

    // Start live EventStreams SSE — insert new edits into the queue in real-time
    start_eventstream(DEFAULT_WIKI_ID, move |event: StreamEvent| {
        let filters = filters.get_untracked();

        // Apply client-side filters
        if event.patrolled && filters.unpatrolled_only {
            return;
        }
        if event.bot && !filters.include_bots {
            return;
        }
        if !filters.include_anonymous && event.user.chars().all(|c| c.is_ascii_digit() || c == '.') {
            return;
        }
        if !filters.include_new_pages && event.new_page {
            return;
        }

        let queued = stream_event_to_queued_edit(&event);

        // Only insert into the queue signal — don't touch view_data
        // so the diff/context don't re-render
        let mut q = queue_signal.get_untracked();
        if q.iter().any(|existing| existing.event.rev_id == queued.event.rev_id) {
            return;
        }
        q.insert(0, queued);
        if q.len() > 50 {
            q.truncate(50);
        }
        // Shift selected index so the current item stays selected
        let idx = selected_index.get_untracked();
        set_selected_index.set(idx + 1);
        set_queue_signal.set(q);
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
                                        class="btn"
                                        style="border-color:rgba(59,130,246,.5);background:rgba(59,130,246,.15);"
                                        on:click=move |_| { bootstrap_btn_action.dispatch_local(()); }
                                    >
                                        "Bootstrap session"
                                    </button>
                                    <button
                                        class="btn"
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
                    class="patrol-grid"
                >

                    {move || {
                        if !show_help.get() {
                            return view! { <span></span> }.into_any();
                        }
                        view! {
                            <div
                                class="modal-backdrop"
                                on:click=move |_| set_show_help.set(false)
                            >
                                <div
                                    class="modal"
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
                                        class="btn"
                                        style="margin-top:17px;width:100%;"
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
                            <div class="modal-backdrop-opaque">
                                <div style="max-width:1200px;margin:0 auto;padding:27px;display:grid;gap:17px;">
                                    <div style="display:flex;align-items:center;justify-content:space-between;">
                                        <h2 style="margin:0;font-size:17px;">"Back-office"</h2>
                                        <button
                                            class="btn"
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
                                                    <section class="panel">
                                                        <h3 style="margin:0;font-size:13px;font-weight:700;">
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

                    <div class="session-bar">
                        <span style="font-weight:700;color:var(--accent);">
                            {sp42_core::branding::PROJECT_NAME}
                        </span>
                        {move || {
                            view_data
                                .get()
                                .map(|v| {
                                    view! {
                                        <span>{v.wiki_id.clone()}</span>
                                        <span>{v.auth.username.clone().unwrap_or_else(|| "—".to_string())}</span>
                                        <span>{format!("{} edits", v.queue.len())}</span>
                                    }
                                        .into_any()
                                })
                                .unwrap_or_else(|| view! { <span>"loading..."</span> }.into_any())
                        }}
                        <div class="flex-spacer"></div>
                        <span style="width:8px;height:8px;border-radius:50%;display:inline-block;"
                            style:background=move || {
                                if load_error.get().is_some() { "var(--danger)" }
                                else if view_data.get().is_some() { "var(--success)" }
                                else { "var(--warning)" }
                            }
                        ></span>
                        {move || {
                            let status = action_status.get();
                            if !status.is_empty() {
                                view! { <span style="font-size:11px;">{status}</span> }.into_any()
                            } else {
                                view! { <span></span> }.into_any()
                            }
                        }}
                        <button
                            class="btn btn-ghost btn-compact"
                            on:click=move |_| set_show_help.set(true)
                        >
                            "?"
                        </button>
                    </div>

                    <FilterBar
                        filters=filters
                        set_filters=set_filters
                        next_continue=next_continue
                    />

                    {move || {
                        let queue = queue_signal.get();
                        if !queue.is_empty() {
                            view! {
                                <QueueColumn
                                    queue=queue
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
                                        class="btn"
                                        style="margin-top:10px;"
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

                    <div style="grid-area:main;min-width:0;min-height:0;display:grid;grid-template-rows:auto 1fr;overflow:hidden;">
                        {move || {
                            let queue = queue_signal.get();
                            let idx = selected_index.get();
                            let edit = queue.get(idx).cloned();
                            view! { <ContextHeader edit=edit /> }.into_any()
                        }}
                        <div style="overflow-y:auto;overflow-x:hidden;">
                            {move || {
                                if diff_loading.get() {
                                    view! {
                                        <div class="grid-center" style="height:100%;">
                                            <div style="text-align:center;">
                                                <div class="spinner" style="margin:0 auto;"></div>
                                                <p class="text-muted" style="margin-top:10px;font-size:12px;">"Loading diff..."</p>
                                            </div>
                                        </div>
                                    }.into_any()
                                } else {
                                    view! { <DiffViewer diff=current_diff.get() /> }.into_any()
                                }
                            }}
                        </div>
                    </div>

                    {move || {
                        if let Some(view) = view_data.get() {
                            view! {
                                <div class="action-bar">
                                    <input
                                        type="text"
                                        placeholder="Review note (optional)"
                                        aria-label="Review note"
                                        class="review-note-input"
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
                                    <ActionBar
                                        preflight=view.action_preflight.clone()
                                        capabilities=view.capabilities.clone()
                                        has_selection=Signal::derive(move || has_selection.get())
                                        action_pending=Signal::derive(move || action_pending.get())
                                        on_action=set_action_trigger
                                        on_skip=set_skip_trigger
                                    />
                                </div>
                            }
                                .into_any()
                        } else {
                            view! {
                                <div class="action-bar text-muted" style="font-size:12px;">
                                    "Actions available after queue loads."
                                </div>
                            }
                                .into_any()
                        }
                    }}
                </div>
            }.into_any()
        }}
    }
}

/// Convert a live SSE event into a QueuedEdit with a basic client-side score.
fn stream_event_to_queued_edit(event: &StreamEvent) -> sp42_core::QueuedEdit {
    use sp42_core::{
        CompositeScore, EditEvent, EditorIdentity, FlagState, QueuedEdit, SignalContribution,
        ScoringSignal,
    };

    let is_anon = event.user.chars().all(|c| c.is_ascii_digit() || c == '.' || c == ':');
    let performer = if is_anon {
        EditorIdentity::Anonymous {
            label: event.user.clone(),
        }
    } else if event.user.starts_with('~') {
        EditorIdentity::Temporary {
            label: event.user.clone(),
        }
    } else {
        EditorIdentity::Registered {
            username: event.user.clone(),
        }
    };

    // Basic client-side scoring
    let mut score = 0i32;
    let mut contributions = Vec::new();
    if is_anon {
        score += 20;
        contributions.push(SignalContribution {
            signal: ScoringSignal::AnonymousUser,
            weight: 20,
            note: None,
        });
    }
    if matches!(performer, EditorIdentity::Temporary { .. }) {
        score += 20;
        contributions.push(SignalContribution {
            signal: ScoringSignal::AnonymousUser,
            weight: 20,
            note: Some("temporary account".to_string()),
        });
    }
    if event.byte_delta().abs() > 500 {
        score += 15;
        contributions.push(SignalContribution {
            signal: ScoringSignal::LargeContentRemoval,
            weight: 15,
            note: None,
        });
    }

    QueuedEdit {
        event: EditEvent {
            wiki_id: event.wiki.clone(),
            title: event.title.clone(),
            namespace: event.namespace,
            rev_id: event.rev_id,
            old_rev_id: event.old_rev_id,
            performer,
            timestamp_ms: event.timestamp_ms,
            is_bot: FlagState::from(event.bot),
            is_minor: FlagState::from(event.minor),
            is_new_page: FlagState::from(event.new_page),
            tags: Vec::new(),
            comment: event.comment.clone(),
            byte_delta: event.byte_delta(),
            is_patrolled: FlagState::from(event.patrolled),
        },
        score: CompositeScore {
            total: score,
            contributions,
        },
    }
}
