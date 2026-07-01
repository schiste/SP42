use leptos::prelude::*;
use sp42_ui::{WorkspaceGrid, WorkspaceGridProps};

use crate::components::filter_bar::FilterBar;
use crate::components::ui_children;
use crate::platform::config::selected_wiki_id;

mod action_controller;
mod eventstream_controller;
mod keyboard_controller;
mod load_controller;
mod queue_controller;
mod revision_artifacts;
mod view_components;

use action_controller::{PatrolActionControllerInput, create_patrol_action_controller};
use eventstream_controller::install_patrol_eventstream;
use keyboard_controller::handle_patrol_keydown;
use load_controller::{PatrolLoadControllerInput, create_patrol_load_controller};
use queue_controller::{create_patrol_queue_controller, install_filter_selection_reset};
use revision_artifacts::{
    RevisionArtifactEffectsInput, create_revision_artifact_controller,
    install_revision_artifact_effects,
};
use view_components::{
    ActionFooter, AuthRequiredView, BackofficeModal, DiffPane, HelpModal, QueuePane, SessionBar,
};

#[component]
pub fn PatrolSurface() -> impl IntoView {
    let active_wiki_id = selected_wiki_id();
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

    // The active wiki's resolved default namespaces, so the filter checkboxes show
    // what the server uses for an unfiltered query (configured wikis like enwiki
    // differ from the universal default). Seeded with the shared default, then
    // refined by a one-shot fetch on mount. Codex review #90.
    let (default_namespaces, set_default_namespaces) =
        signal(sp42_core::DEFAULT_PATROL_NAMESPACES.to_vec());
    {
        let wiki = active_wiki_id.clone();
        let fetch = Action::new_local(move |(): &()| {
            let wiki = wiki.clone();
            async move {
                if let Ok(namespaces) =
                    crate::platform::live::fetch_wiki_namespace_defaults(&wiki).await
                {
                    if !namespaces.is_empty() {
                        set_default_namespaces.set(namespaces);
                    }
                }
            }
        });
        Effect::new(move |ran: Option<bool>| {
            if ran.is_none() {
                fetch.dispatch_local(());
            }
            true
        });
    }
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

    install_filter_selection_reset(filters, queue_signal, set_selected_rev_id);

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

            WorkspaceGrid(
                WorkspaceGridProps::new(ui_children(move || view! {

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
                        default_namespaces=default_namespaces
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
                }.into_any()))
                .on_keydown(move |event| {
                        handle_patrol_keydown(
                            event,
                            set_action_trigger,
                            set_skip_trigger,
                            selected_index,
                            queue_signal,
                            set_selected_rev_id,
                            set_show_backoffice,
                            set_show_help,
                        );
                })
            )
            .into_any()
        }}
    }
}
