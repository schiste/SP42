use std::collections::HashMap;

use leptos::prelude::*;
use sp42_core::{EditorIdentity, QueuedEdit, SessionActionExecutionRequest, SessionActionKind};
use sp42_patrol::LiveOperatorView;

use crate::platform::auth::{bootstrap_dev_auth_session, execute_dev_auth_action};
use crate::platform::console;

pub(super) struct PatrolActionController {
    pub(super) set_action_trigger: WriteSignal<Option<SessionActionKind>>,
    pub(super) set_skip_trigger: WriteSignal<bool>,
    pub(super) action_pending: ReadSignal<bool>,
    pub(super) action_status: ReadSignal<String>,
    pub(super) set_action_status: WriteSignal<String>,
}

pub(super) struct PatrolActionControllerInput {
    pub(super) view_data: ReadSignal<Option<LiveOperatorView>>,
    pub(super) selected_edit: ReadSignal<Option<QueuedEdit>>,
    pub(super) review_note: ReadSignal<String>,
    pub(super) set_review_note: WriteSignal<String>,
    pub(super) group_rev_ids: ReadSignal<HashMap<u64, Vec<u64>>>,
    pub(super) queue: Memo<Vec<QueuedEdit>>,
    pub(super) all_edits: ReadSignal<Vec<QueuedEdit>>,
    pub(super) set_all_edits: WriteSignal<Vec<QueuedEdit>>,
    pub(super) selected_index: Memo<usize>,
    pub(super) set_selected_rev_id: WriteSignal<Option<u64>>,
    pub(super) set_selection_only_refetch: WriteSignal<bool>,
    pub(super) reload_after_action: Action<(), ()>,
}

pub(super) fn create_patrol_action_controller(
    input: PatrolActionControllerInput,
) -> PatrolActionController {
    let (action_trigger, set_action_trigger) = signal(None::<SessionActionKind>);
    let (skip_trigger, set_skip_trigger) = signal(false);
    let (action_pending, set_action_pending) = signal(false);
    let (action_status, set_action_status) = signal(String::new());

    install_action_effect(
        input.view_data,
        input.selected_edit,
        input.review_note,
        input.set_review_note,
        input.group_rev_ids,
        input.queue,
        input.all_edits,
        input.set_all_edits,
        input.set_selected_rev_id,
        input.set_selection_only_refetch,
        input.reload_after_action,
        action_trigger,
        set_action_trigger,
        set_action_pending,
        set_action_status,
    );
    install_skip_effect(
        skip_trigger,
        set_skip_trigger,
        input.selected_index,
        input.queue,
        input.set_selected_rev_id,
    );

    PatrolActionController {
        set_action_trigger,
        set_skip_trigger,
        action_pending,
        action_status,
        set_action_status,
    }
}

#[allow(clippy::too_many_arguments)]
fn install_action_effect(
    view_data: ReadSignal<Option<LiveOperatorView>>,
    selected_edit: ReadSignal<Option<QueuedEdit>>,
    review_note: ReadSignal<String>,
    set_review_note: WriteSignal<String>,
    group_rev_ids: ReadSignal<HashMap<u64, Vec<u64>>>,
    queue: Memo<Vec<QueuedEdit>>,
    all_edits: ReadSignal<Vec<QueuedEdit>>,
    set_all_edits: WriteSignal<Vec<QueuedEdit>>,
    set_selected_rev_id: WriteSignal<Option<u64>>,
    set_selection_only_refetch: WriteSignal<bool>,
    reload_after_action: Action<(), ()>,
    action_trigger: ReadSignal<Option<SessionActionKind>>,
    set_action_trigger: WriteSignal<Option<SessionActionKind>>,
    set_action_pending: WriteSignal<bool>,
    set_action_status: WriteSignal<String>,
) {
    let execute_action = Action::new_local(move |kind: &SessionActionKind| {
        let kind = *kind;
        async move {
            set_action_pending.set(true);

            let Some(view) = view_data.get() else {
                set_action_pending.set(false);
                set_action_trigger.set(None);
                return;
            };
            let Some(edit) = selected_edit.get_untracked() else {
                set_action_pending.set(false);
                set_action_trigger.set(None);
                return;
            };

            let request = build_action_request(&view, &edit, kind, &review_note, &group_rev_ids);
            log_action_request(&request, &edit);

            match execute_dev_auth_action(&request).await {
                Ok(response) if response.accepted => {
                    set_action_status.set(format!(
                        "{} accepted for rev {}",
                        kind.label(),
                        edit.event.rev_id
                    ));
                    remove_accepted_edit(
                        &edit,
                        &queue,
                        &group_rev_ids,
                        &all_edits,
                        set_all_edits,
                        set_selected_rev_id,
                    );
                    set_review_note.set(String::new());
                    set_selection_only_refetch.set(true);
                    reload_after_action.dispatch_local(());
                }
                Ok(response) => {
                    set_action_status.set(format!(
                        "{} rejected: {}",
                        kind.label(),
                        response.message.unwrap_or_default()
                    ));
                }
                Err(error) if is_auth_error(&error) => {
                    set_action_status.set("Session expired, re-authenticating...".to_string());
                    retry_after_reauthentication(
                        &request,
                        &edit,
                        kind,
                        &all_edits,
                        set_all_edits,
                        set_review_note,
                        set_action_status,
                    )
                    .await;
                }
                Err(error) => {
                    set_action_status.set(format!("Action error: {error}"));
                }
            }

            set_action_pending.set(false);
            set_action_trigger.set(None);
        }
    });

    Effect::new(move |_| {
        if let Some(kind) = action_trigger.get() {
            execute_action.dispatch_local(kind);
        }
    });
}

fn install_skip_effect(
    skip_trigger: ReadSignal<bool>,
    set_skip_trigger: WriteSignal<bool>,
    selected_index: Memo<usize>,
    queue: Memo<Vec<QueuedEdit>>,
    set_selected_rev_id: WriteSignal<Option<u64>>,
) {
    Effect::new(move |_| {
        if skip_trigger.get() {
            set_skip_trigger.set(false);
            let idx = selected_index.get();
            let queue = queue.get_untracked();
            if let Some(next) = queue.get(idx + 1) {
                set_selected_rev_id.set(Some(next.event.rev_id));
            }
        }
    });
}

fn build_action_request(
    view: &LiveOperatorView,
    edit: &QueuedEdit,
    kind: SessionActionKind,
    review_note: &ReadSignal<String>,
    group_rev_ids: &ReadSignal<HashMap<u64, Vec<u64>>>,
) -> SessionActionExecutionRequest {
    SessionActionExecutionRequest {
        wiki_id: view.wiki_id.clone(),
        kind,
        rev_id: edit.event.rev_id,
        title: Some(edit.event.title.clone()),
        target_user: target_user(&edit.event.performer),
        undo_after_rev_id: edit.event.old_rev_id,
        summary: Some(action_summary(review_note)),
        selected_text: None,
        batch_rev_ids: group_rev_ids
            .get_untracked()
            .get(&edit.event.rev_id)
            .cloned(),
        replacement_text: None,
        node_locator: None,
    }
}

fn target_user(performer: &EditorIdentity) -> Option<String> {
    match performer {
        EditorIdentity::Anonymous { label } => Some(label.clone()),
        EditorIdentity::Registered { username } => Some(username.clone()),
        EditorIdentity::Temporary { label } => Some(label.clone()),
    }
}

fn action_summary(review_note: &ReadSignal<String>) -> String {
    let note = review_note.get();
    if note.is_empty() {
        "SP42".to_string()
    } else {
        format!("SP42: {note}")
    }
}

fn log_action_request(request: &SessionActionExecutionRequest, edit: &QueuedEdit) {
    let batch_count = request.batch_rev_ids.as_ref().map_or(1, Vec::len);
    console::info(&format!(
        "[SP42] action {} on rev {} title={:?} (batch={})",
        request.kind.label(),
        edit.event.rev_id,
        edit.event.title,
        batch_count
    ));
}

fn remove_accepted_edit(
    edit: &QueuedEdit,
    queue: &Memo<Vec<QueuedEdit>>,
    group_rev_ids: &ReadSignal<HashMap<u64, Vec<u64>>>,
    all_edits: &ReadSignal<Vec<QueuedEdit>>,
    set_all_edits: WriteSignal<Vec<QueuedEdit>>,
    set_selected_rev_id: WriteSignal<Option<u64>>,
) {
    let current_queue = queue.get_untracked();
    let acted_rev = edit.event.rev_id;
    let next_rev = current_queue
        .iter()
        .skip_while(|candidate| candidate.event.rev_id != acted_rev)
        .nth(1)
        .or_else(|| {
            current_queue
                .iter()
                .rev()
                .skip_while(|candidate| candidate.event.rev_id != acted_rev)
                .nth(1)
        })
        .map(|candidate| candidate.event.rev_id);

    let revs_to_remove = group_rev_ids
        .get_untracked()
        .get(&acted_rev)
        .cloned()
        .unwrap_or_else(|| vec![acted_rev]);
    let mut edits = all_edits.get_untracked();
    edits.retain(|candidate| !revs_to_remove.contains(&candidate.event.rev_id));
    set_all_edits.set(edits);
    set_selected_rev_id.set(next_rev);
    console::debug(&format!(
        "[SP42] removed rev {acted_rev}, next → {next_rev:?}"
    ));
}

async fn retry_after_reauthentication(
    request: &SessionActionExecutionRequest,
    edit: &QueuedEdit,
    kind: SessionActionKind,
    all_edits: &ReadSignal<Vec<QueuedEdit>>,
    set_all_edits: WriteSignal<Vec<QueuedEdit>>,
    set_review_note: WriteSignal<String>,
    set_action_status: WriteSignal<String>,
) {
    let bootstrap_request = sp42_core::DevAuthBootstrapRequest {
        username: String::new(),
        scopes: Vec::new(),
        expires_at_ms: None,
    };
    if bootstrap_dev_auth_session(&bootstrap_request)
        .await
        .is_err()
    {
        set_action_status.set("Re-authentication failed. Reload the page.".to_string());
        return;
    }

    match execute_dev_auth_action(request).await {
        Ok(response) if response.accepted => {
            set_action_status.set(format!(
                "{} accepted for rev {} (re-authenticated)",
                kind.label(),
                edit.event.rev_id
            ));
            let mut queue = all_edits.get_untracked();
            if let Some(pos) = queue
                .iter()
                .position(|candidate| candidate.event.rev_id == edit.event.rev_id)
            {
                queue.remove(pos);
                set_all_edits.set(queue);
            }
            set_review_note.set(String::new());
        }
        Ok(response) => {
            set_action_status.set(format!(
                "{} rejected: {}",
                kind.label(),
                response.message.unwrap_or_default()
            ));
        }
        Err(retry_error) => {
            set_action_status.set(format!("Retry failed: {retry_error}"));
        }
    }
}

fn is_auth_error(error: &str) -> bool {
    error.contains("401") || error.contains("No authenticated")
}
