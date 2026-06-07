use leptos::prelude::*;
use sp42_core::QueuedEdit;
use sp42_reporting::LiveOperatorView;

use super::revision_artifacts::{
    RevisionArtifactController, cache_initial_artifacts, prefetch_queue_diffs, rev_id_from_hash,
};
use crate::components::filter_bar::PatrolFilterParams;
use crate::platform::auth::bootstrap_dev_auth_session;
use crate::platform::console;
use crate::platform::live::fetch_live_operator_view;

pub(super) struct PatrolLoadController {
    pub(super) view_data: ReadSignal<Option<LiveOperatorView>>,
    pub(super) load_error: ReadSignal<Option<String>>,
    pub(super) next_continue: ReadSignal<Option<String>>,
    pub(super) bootstrap_attempted: ReadSignal<bool>,
    pub(super) bootstrap_error: ReadSignal<Option<String>>,
    pub(super) set_bootstrap_attempted: WriteSignal<bool>,
    pub(super) set_selection_only_refetch: WriteSignal<bool>,
    pub(super) load_action: Action<(), ()>,
}

pub(super) struct PatrolLoadControllerInput {
    pub(super) active_wiki_id: String,
    pub(super) filters: ReadSignal<PatrolFilterParams>,
    pub(super) selected_index: Memo<usize>,
    pub(super) set_selected_rev_id: WriteSignal<Option<u64>>,
    pub(super) set_all_edits: WriteSignal<Vec<QueuedEdit>>,
    pub(super) artifacts: RevisionArtifactController,
}

#[derive(Clone, Copy)]
struct ApplyLoadedViewContext {
    set_view_data: WriteSignal<Option<LiveOperatorView>>,
    set_load_error: WriteSignal<Option<String>>,
    set_next_continue: WriteSignal<Option<String>>,
    selection_only_refetch: ReadSignal<bool>,
    set_selection_only_refetch: WriteSignal<bool>,
    set_selected_rev_id: WriteSignal<Option<u64>>,
    set_all_edits: WriteSignal<Vec<QueuedEdit>>,
    artifacts: RevisionArtifactController,
}

pub(super) fn create_patrol_load_controller(
    input: PatrolLoadControllerInput,
) -> PatrolLoadController {
    let (view_data, set_view_data) = signal(None::<LiveOperatorView>);
    let (load_error, set_load_error) = signal(None::<String>);
    let (next_continue, set_next_continue) = signal(None::<String>);
    let (selection_only_refetch, set_selection_only_refetch) = signal(false);
    let (bootstrap_attempted, set_bootstrap_attempted) = signal(false);
    let (bootstrap_error, set_bootstrap_error) = signal(None::<String>);

    let apply_context = ApplyLoadedViewContext {
        set_view_data,
        set_load_error,
        set_next_continue,
        selection_only_refetch,
        set_selection_only_refetch,
        set_selected_rev_id: input.set_selected_rev_id,
        set_all_edits: input.set_all_edits,
        artifacts: input.artifacts,
    };

    let load_wiki_id = input.active_wiki_id.clone();
    let load_action = Action::new_local(move |_: &()| {
        let wiki_id = load_wiki_id.clone();
        async move {
            let mut current_filters = input.filters.get();
            current_filters.selected_index = Some(input.selected_index.get_untracked());
            match fetch_live_operator_view(&wiki_id, &current_filters).await {
                Ok(view) => {
                    if view.auth.username.is_none() && !bootstrap_attempted.get_untracked() {
                        set_bootstrap_attempted.set(true);
                        let request = sp42_core::DevAuthBootstrapRequest {
                            username: String::new(),
                            scopes: Vec::new(),
                            expires_at_ms: None,
                        };
                        match bootstrap_dev_auth_session(&request).await {
                            Ok(status) if status.authenticated => {
                                set_bootstrap_error.set(None);
                                match fetch_live_operator_view(&wiki_id, &current_filters).await {
                                    Ok(view) => {
                                        apply_loaded_view(view, apply_context);
                                    }
                                    Err(error) => {
                                        apply_context.set_load_error.set(Some(error));
                                        apply_context.artifacts.set_diff_loading.set(false);
                                    }
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
                    apply_loaded_view(view, apply_context);
                }
                Err(error) => {
                    apply_context.set_load_error.set(Some(error));
                    apply_context.artifacts.set_diff_loading.set(false);
                }
            }
        }
    });

    Effect::new(move |ran: Option<bool>| {
        if ran.is_none() {
            load_action.dispatch_local(());
        }
        true
    });

    PatrolLoadController {
        view_data,
        load_error,
        next_continue,
        bootstrap_attempted,
        bootstrap_error,
        set_bootstrap_attempted,
        set_selection_only_refetch,
        load_action,
    }
}

fn apply_loaded_view(view: LiveOperatorView, context: ApplyLoadedViewContext) {
    context.set_load_error.set(None);
    context.set_next_continue.set(view.next_continue.clone());
    cache_initial_artifacts(&view, context.artifacts);
    if !context.selection_only_refetch.get_untracked() {
        console::info(&format!(
            "[SP42] server load: {} edits, diff={}",
            view.queue.len(),
            view.diff.is_some()
        ));
        if let Some(target_rev) = rev_id_from_hash() {
            console::debug(&format!("[SP42] selecting rev from hash: {target_rev}"));
            context.set_selected_rev_id.set(Some(target_rev));
        } else if let Some(first) = view.queue.first() {
            console::debug(&format!(
                "[SP42] selecting first: rev {}",
                first.event.rev_id
            ));
            context.set_selected_rev_id.set(Some(first.event.rev_id));
        }
        context.set_all_edits.set(view.queue.clone());
    }
    context.set_selection_only_refetch.set(false);
    prefetch_queue_diffs(&view, context.artifacts);
    context.set_view_data.set(Some(view));
    context.artifacts.set_diff_loading.set(false);
}
