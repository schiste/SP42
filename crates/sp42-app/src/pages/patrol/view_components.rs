use std::collections::HashMap;

use leptos::prelude::*;
use sp42_core::{MediaDiffReport, QueuedEdit, SessionActionKind, StructuredDiff};
use sp42_patrol::LiveOperatorView;

use crate::components::action_bar::ActionBar;
use crate::components::context_header::ContextHeader;
use crate::components::diff_viewer::{DiffViewer, EditAction, TagAction};
use crate::components::media_diff_gallery::MediaDiffGallery;
use crate::components::queue_column::QueueColumn;
use crate::components::{PatrolScenarioPanel, PatrolSessionDigestPanel, ShellStatePanel};

#[component]
pub(super) fn AuthRequiredView(
    bridge_mode: String,
    has_token: bool,
    bootstrap_attempted: ReadSignal<bool>,
    bootstrap_error: ReadSignal<Option<String>>,
    set_bootstrap_attempted: WriteSignal<bool>,
    load_action: Action<(), ()>,
) -> impl IntoView {
    view! {
        <div style="display:grid;place-items:center;height:100vh;\
                    background:#08111f;color:#eff4ff;padding:27px;">
            <div style="max-width:440px;text-align:center;">
                <h1 style="font-size:21px;margin:0 0 10px;">
                    "Authentication required"
                </h1>
                {move || {
                    if bootstrap_attempted.get() {
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
                    }
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
                        on:click=move |_| {
                            set_bootstrap_attempted.set(false);
                            load_action.dispatch_local(());
                        }
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
    }
}

#[component]
pub(super) fn HelpModal(
    show_help: ReadSignal<bool>,
    set_show_help: WriteSignal<bool>,
) -> impl IntoView {
    view! {
        {move || {
            if !show_help.get() {
                return view! { <span></span> }.into_any();
            }
            view! {
                <div class="modal-backdrop" on:click=move |_| set_show_help.set(false)>
                    <div class="modal" on:click=move |ev| ev.stop_propagation()>
                        <h2 style="margin:0 0 17px;font-size:17px;">"Keyboard Shortcuts"</h2>
                        <div style="display:grid;gap:7px;font-size:13px;">
                            {shortcut_row("Rollback", "R")}
                            {shortcut_row("Undo", "U")}
                            {shortcut_row("Patrol", "P")}
                            {shortcut_row("Skip", "S")}
                            {shortcut_row("Previous edit", "\u{2191}")}
                            {shortcut_row("Next edit", "\u{2193}")}
                            {shortcut_row("This help", "?")}
                            {shortcut_row("Back-office", "Ctrl+Shift+D")}
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
    }
}

#[component]
pub(super) fn BackofficeModal(
    show_backoffice: ReadSignal<bool>,
    set_show_backoffice: WriteSignal<bool>,
    view_data: ReadSignal<Option<LiveOperatorView>>,
) -> impl IntoView {
    view! {
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
                            <button class="btn" on:click=move |_| set_show_backoffice.set(false)>
                                "Close (Esc)"
                            </button>
                        </div>
                        {if let Some(ref view) = view {
                            let history_entries = view.action_history.entries.clone();
                            view! {
                                <PatrolScenarioPanel report=view.scenario_report.clone() />
                                <PatrolSessionDigestPanel report=view.scenario_report.clone() />
                                <ShellStatePanel model=view.shell_state.clone() />
                                <ActionHistoryPanel history_entries=history_entries />
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
    }
}

#[component]
pub(super) fn ActionHistoryPanel(
    history_entries: Vec<sp42_core::ActionExecutionLogEntry>,
) -> impl IntoView {
    if history_entries.is_empty() {
        return view! { <span></span> }.into_any();
    }

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
    }
    .into_any()
}

#[component]
pub(super) fn SessionBar(
    view_data: ReadSignal<Option<LiveOperatorView>>,
    load_error: ReadSignal<Option<String>>,
    action_status: ReadSignal<String>,
    set_show_help: WriteSignal<bool>,
) -> impl IntoView {
    view! {
        <div class="session-bar">
            <span style="font-weight:700;color:var(--accent);">
                {sp42_core::branding::PROJECT_NAME}
            </span>
            {move || {
                view_data
                    .get()
                    .map(|view| {
                        view! {
                            <span>{view.wiki_id.clone()}</span>
                            <span>{view.auth.username.clone().unwrap_or_else(|| "—".to_string())}</span>
                            <span>{format!("{} edits", view.queue.len())}</span>
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
    }
}

#[component]
pub(super) fn QueuePane(
    queue: Memo<Vec<QueuedEdit>>,
    selected_rev_id: ReadSignal<Option<u64>>,
    set_selected_rev_id: WriteSignal<Option<u64>>,
    group_rev_ids: ReadSignal<HashMap<u64, Vec<u64>>>,
    load_error: ReadSignal<Option<String>>,
    load_action: Action<(), ()>,
) -> impl IntoView {
    view! {
        {move || {
            let visible_queue = queue.get();
            if !visible_queue.is_empty() {
                view! {
                    <QueueColumn
                        queue=visible_queue
                        selected_rev_id=Signal::derive(move || selected_rev_id.get())
                        set_selected_rev_id=set_selected_rev_id
                        group_counts=group_rev_ids
                            .get_untracked()
                            .iter()
                            .map(|(k, v)| (*k, v.len()))
                            .collect()
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
    }
}

#[component]
pub(super) fn DiffPane(
    selected_edit: ReadSignal<Option<QueuedEdit>>,
    diff_loading: ReadSignal<bool>,
    current_diff: ReadSignal<Option<StructuredDiff>>,
    current_media_diff: ReadSignal<Option<MediaDiffReport>>,
    media_diff_loading: ReadSignal<bool>,
    set_tag_action: WriteSignal<Option<TagAction>>,
    set_edit_action: WriteSignal<Option<EditAction>>,
) -> impl IntoView {
    view! {
        <div style="grid-area:main;min-width:0;min-height:0;display:grid;grid-template-rows:auto 1fr;overflow:hidden;">
            {move || {
                view! { <ContextHeader edit=selected_edit.get() /> }.into_any()
            }}
            {move || {
                let report = current_media_diff.get();
                let show_media_diff = report.as_ref().is_some_and(MediaDiffReport::has_changes);
                let layout_style = if show_media_diff {
                    "display:grid;grid-template-columns:minmax(0,1fr) minmax(260px,320px);\
                     gap:10px;overflow:hidden;padding-top:10px;"
                } else {
                    "display:grid;grid-template-columns:minmax(0,1fr);\
                     gap:10px;overflow:hidden;padding-top:10px;"
                };

                view! {
                    <div style=layout_style>
                        <div style="min-width:0;overflow-y:auto;overflow-x:hidden;">
                            {move || {
                                let selected = selected_edit.get();
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
                                    view! {
                                        <DiffViewer
                                            diff=current_diff.get()
                                            wiki_id=selected.as_ref().map(|edit| edit.event.wiki_id.clone()).unwrap_or_default()
                                            rev_id=selected.as_ref().map(|edit| edit.event.rev_id).unwrap_or(0)
                                            old_rev_id=selected.as_ref().and_then(|edit| edit.event.old_rev_id).unwrap_or(0)
                                            on_tag=set_tag_action
                                            on_edit=set_edit_action
                                        />
                                    }.into_any()
                                }
                            }}
                        </div>
                        {if show_media_diff {
                            view! {
                                <div style="min-width:0;overflow:hidden;">
                                    <MediaDiffGallery
                                        report=report
                                        loading=Signal::derive(move || media_diff_loading.get())
                                    />
                                </div>
                            }.into_any()
                        } else {
                            view! { <span></span> }.into_any()
                        }}
                    </div>
                }.into_any()
            }}
        </div>
    }
}

#[component]
pub(super) fn ActionFooter(
    view_data: ReadSignal<Option<LiveOperatorView>>,
    queue: Memo<Vec<QueuedEdit>>,
    review_note: ReadSignal<String>,
    set_review_note: WriteSignal<String>,
    has_selection: Memo<bool>,
    action_pending: ReadSignal<bool>,
    set_action_trigger: WriteSignal<Option<SessionActionKind>>,
    set_skip_trigger: WriteSignal<bool>,
) -> impl IntoView {
    view! {
        {move || {
            let has_view = view_data.get().is_some();
            let has_queue = !queue.get().is_empty();
            if has_view || has_queue {
                let preflight = view_data
                    .get()
                    .map(|view| view.action_preflight.clone())
                    .unwrap_or_default();
                let capabilities = view_data
                    .get()
                    .map(|view| view.capabilities.clone())
                    .unwrap_or_default();
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
                                    .and_then(|target| target.dyn_into::<web_sys::HtmlInputElement>().ok())
                                    .map(|element| element.value())
                                    .unwrap_or_default();
                                set_review_note.set(value);
                            }
                        />
                        <ActionBar
                            preflight=preflight
                            capabilities=capabilities
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
    }
}

fn shortcut_row(label: &'static str, key: &'static str) -> impl IntoView {
    view! {
        <div style="display:flex;justify-content:space-between;">
            <span style="color:#8b9fc0;">{label}</span>
            <kbd style="color:#eff4ff;font-weight:700;">{key}</kbd>
        </div>
    }
}
