use leptos::prelude::*;
use sp42_core::{DevAuthCapabilityReport, SessionActionKind};

#[component]
pub fn ActionBar(
    capabilities: DevAuthCapabilityReport,
    has_selection: Signal<bool>,
    action_pending: Signal<bool>,
    on_action: WriteSignal<Option<SessionActionKind>>,
    on_skip: WriteSignal<bool>,
) -> impl IntoView {
    let can_rollback = capabilities.capabilities.moderation.can_rollback;
    let can_patrol = capabilities.capabilities.moderation.can_patrol;
    let can_undo = capabilities.capabilities.editing.can_undo;

    let btn_base = "min-height:44px;padding:4px 17px;border:1px solid rgba(148,163,184,.14);\
                    border-radius:4px;font:inherit;font-size:13px;font-weight:700;\
                    cursor:pointer;transition:opacity 120ms;";

    view! {
        <div
            role="toolbar"
            aria-label="Patrol actions"
            style="display:flex;align-items:center;gap:7px;padding:0 10px;\
                   background:#0b1324;border-block-start:1px solid rgba(148,163,184,.14);"
        >
            // Rollback — destructive, filled red-tinted
            <button
                style=format!(
                    "{btn_base}background:rgba(239,68,68,.18);color:#fecaca;border-color:rgba(239,68,68,.3);",
                )
                aria-keyshortcuts="r"
                disabled=move || !can_rollback || !has_selection.get() || action_pending.get()
                on:click=move |_| on_action.set(Some(SessionActionKind::Rollback))
            >
                "R Rollback"
            </button>

            // Undo — neutral outlined
            <button
                style=format!(
                    "{btn_base}background:transparent;color:#eff4ff;",
                )
                aria-keyshortcuts="u"
                disabled=move || !can_undo || !has_selection.get() || action_pending.get()
                on:click=move |_| on_action.set(Some(SessionActionKind::Undo))
            >
                "U Undo"
            </button>

            // Patrol — green-tinted
            <button
                style=format!(
                    "{btn_base}background:rgba(34,197,94,.14);color:#bbf7d0;border-color:rgba(34,197,94,.3);",
                )
                aria-keyshortcuts="p"
                disabled=move || !can_patrol || !has_selection.get() || action_pending.get()
                on:click=move |_| on_action.set(Some(SessionActionKind::Patrol))
            >
                "P Patrol"
            </button>

            // Skip — muted outlined
            <button
                style=format!(
                    "{btn_base}background:transparent;color:#8b9fc0;",
                )
                aria-keyshortcuts="s"
                disabled=move || !has_selection.get()
                on:click=move |_| on_skip.set(true)
            >
                "S Skip"
            </button>

            // Spacer
            <div style="flex:1;"></div>

            // Status area (right side)
            <div style="font-size:11px;color:#8b9fc0;">
                {move || {
                    if action_pending.get() {
                        "Executing...".to_string()
                    } else {
                        String::new()
                    }
                }}
            </div>
        </div>
    }
}
