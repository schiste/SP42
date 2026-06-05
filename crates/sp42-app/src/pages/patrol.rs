use leptos::prelude::*;
use sp42_core::SessionActionKind;

use crate::components::filter_bar::{FilterBar, PatrolFilterParams};
use crate::platform::config::configured_default_wiki_id;
use crate::platform::console;

mod action_controller;
mod eventstream_controller;
mod load_controller;
mod queue_controller;
mod revision_artifacts;
mod view_components;

use action_controller::{PatrolActionControllerInput, create_patrol_action_controller};
use eventstream_controller::install_patrol_eventstream;
use load_controller::{PatrolLoadControllerInput, create_patrol_load_controller};
use queue_controller::create_patrol_queue_controller;
use revision_artifacts::{
    RevisionArtifactEffectsInput, create_revision_artifact_controller,
    install_revision_artifact_effects,
};
use view_components::{
    ActionFooter, AuthRequiredView, BackofficeModal, DiffPane, HelpModal, QueuePane, SessionBar,
};

#[component]
pub fn PatrolSurface() -> impl IntoView {
    let active_wiki_id = configured_default_wiki_id();
    let (selected_rev_id, set_selected_rev_id) = signal(None::<u64>);
    let (review_note, set_review_note) = signal(String::new());
    let (show_help, set_show_help) = signal(false);
    let (show_backoffice, set_show_backoffice) = signal(false);
    let revision_artifacts = create_revision_artifact_controller();
    let diff_loading = revision_artifacts.diff_loading;
    let current_diff = revision_artifacts.current_diff;
    let media_diff_loading = revision_artifacts.media_diff_loading;
    let current_media_diff = revision_artifacts.current_media_diff;
    let set_tag_action = revision_artifacts.set_tag_action;
    let set_edit_action = revision_artifacts.set_edit_action;
    let queue_controller = create_patrol_queue_controller(selected_rev_id);
    let filters = queue_controller.filters;
    let set_filters = queue_controller.set_filters;
    let all_edits = queue_controller.all_edits;
    let set_all_edits = queue_controller.set_all_edits;
    let group_rev_ids = queue_controller.group_rev_ids;
    let queue_signal = queue_controller.queue;
    let selected_index = queue_controller.selected_index;

    // Single authoritative source for the selected edit.
    // Only updated by explicit human actions — never by EventStream inserts.
    let (selected_edit, set_selected_edit) = signal(None::<sp42_core::QueuedEdit>);

    let load_controller = create_patrol_load_controller(PatrolLoadControllerInput {
        active_wiki_id: active_wiki_id.clone(),
        filters,
        selected_index,
        set_selected_rev_id,
        set_all_edits,
        artifacts: revision_artifacts,
    });
    let view_data = load_controller.view_data;
    let load_error = load_controller.load_error;
    let next_continue = load_controller.next_continue;
    let bootstrap_attempted = load_controller.bootstrap_attempted;
    let bootstrap_error = load_controller.bootstrap_error;
    let set_bootstrap_attempted = load_controller.set_bootstrap_attempted;
    let set_selection_only_refetch = load_controller.set_selection_only_refetch;
    let load_action = load_controller.load_action;

    let action_controller = create_patrol_action_controller(PatrolActionControllerInput {
        view_data,
        selected_edit,
        review_note,
        set_review_note,
        group_rev_ids,
        queue: queue_signal,
        all_edits,
        set_all_edits,
        selected_index,
        set_selected_rev_id,
        set_selection_only_refetch,
        reload_after_action: load_action,
    });
    let set_action_trigger = action_controller.set_action_trigger;
    let set_skip_trigger = action_controller.set_skip_trigger;
    let action_pending = action_controller.action_pending;
    let action_status = action_controller.action_status;
    let set_action_status = action_controller.set_action_status;

    let queue_len = Memo::new(move |_| queue_signal.get().len());
    let has_selection = Memo::new(move |_| selected_index.get() < queue_len.get());

    // Filter changes reset selection to the first visible item.
    // Uses get_untracked for the queue so EventStream inserts don't
    // trigger this — only explicit filter toggles do.
    Effect::new(move |prev_filters: Option<PatrolFilterParams>| {
        let current = filters.get();
        if prev_filters.as_ref() != Some(&current) {
            let queue = queue_signal.get_untracked();
            let first_rev = queue.first().map(|e| e.event.rev_id);
            let message = format!(
                "[SP42] filters changed → {} visible edits{}",
                queue.len(),
                first_rev.map_or_else(String::new, |rev_id| format!(", selecting rev {rev_id}"))
            );
            if queue.is_empty() {
                console::debug(&message);
            } else {
                console::info(&message);
            }
            set_selected_rev_id.set(first_rev);
        }
        current
    });

    install_revision_artifact_effects(RevisionArtifactEffectsInput {
        selected_rev_id,
        queue: queue_signal,
        selected_edit,
        set_selected_edit,
        all_edits,
        set_all_edits,
        set_action_status,
        artifacts: revision_artifacts,
    });

    install_patrol_eventstream(active_wiki_id.clone(), all_edits, set_all_edits);

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
                let queue = queue_signal.get();
                if idx > 0 {
                    if let Some(prev) = queue.get(idx - 1) {
                        set_selected_rev_id.set(Some(prev.event.rev_id));
                    }
                }
            }
            "ArrowDown" => {
                event.prevent_default();
                let idx = selected_index.get();
                let queue = queue_signal.get();
                if let Some(next) = queue.get(idx + 1) {
                    set_selected_rev_id.set(Some(next.event.rev_id));
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
                    return view! {
                        <AuthRequiredView
                            bridge_mode=view.auth.bridge_mode.clone()
                            has_token=view.auth.local_token_available
                            bootstrap_attempted=bootstrap_attempted
                            bootstrap_error=bootstrap_error
                            set_bootstrap_attempted=set_bootstrap_attempted
                            load_action=load_action
                        />
                    }.into_any();
                }
            }

            view! {
                <div
                    tabindex="0"
                    on:keydown=on_keydown
                    class="patrol-grid"
                >

                    <HelpModal show_help=show_help set_show_help=set_show_help />

                    <BackofficeModal
                        show_backoffice=show_backoffice
                        set_show_backoffice=set_show_backoffice
                        view_data=view_data
                    />

                    <SessionBar
                        view_data=view_data
                        load_error=load_error
                        action_status=action_status
                        set_show_help=set_show_help
                    />

                    <FilterBar
                        filters=filters
                        set_filters=set_filters
                        next_continue=next_continue
                    />

                    <QueuePane
                        queue=queue_signal
                        selected_rev_id=selected_rev_id
                        set_selected_rev_id=set_selected_rev_id
                        group_rev_ids=group_rev_ids
                        load_error=load_error
                        load_action=load_action
                    />

                    <DiffPane
                        selected_edit=selected_edit
                        diff_loading=diff_loading
                        current_diff=current_diff
                        current_media_diff=current_media_diff
                        media_diff_loading=media_diff_loading
                        set_tag_action=set_tag_action
                        set_edit_action=set_edit_action
                    />

                    <ActionFooter
                        view_data=view_data
                        queue=queue_signal
                        review_note=review_note
                        set_review_note=set_review_note
                        has_selection=has_selection
                        action_pending=action_pending
                        set_action_trigger=set_action_trigger
                        set_skip_trigger=set_skip_trigger
                    />
                </div>
            }.into_any()
        }}
    }
}
