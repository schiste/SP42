use std::collections::HashMap;

use leptos::prelude::*;
use sp42_core::{
    ContentDiff, ContentDiffReport, MediaDiffReport, QueuedEdit, SessionActionExecutionRequest,
    SessionActionKind, StructuredDiff,
};
use sp42_patrol::LiveOperatorView;

use crate::components::diff_viewer::{EditAction, TagAction};
use crate::platform::auth::execute_dev_auth_action;
use crate::platform::console;
use crate::platform::live::{fetch_content_diff, fetch_media_diff};

#[derive(Clone, Copy)]
pub(in crate::pages::patrol) struct RevisionArtifactController {
    pub(in crate::pages::patrol) diff_loading: ReadSignal<bool>,
    pub(in crate::pages::patrol) set_diff_loading: WriteSignal<bool>,
    pub(in crate::pages::patrol) current_diff: ReadSignal<Option<StructuredDiff>>,
    set_current_diff: WriteSignal<Option<StructuredDiff>>,
    pub(in crate::pages::patrol) current_entity_diff: ReadSignal<Option<ContentDiffReport>>,
    set_current_entity_diff: WriteSignal<Option<ContentDiffReport>>,
    diff_cache: ReadSignal<HashMap<u64, StructuredDiff>>,
    set_diff_cache: WriteSignal<HashMap<u64, StructuredDiff>>,
    entity_diff_cache: ReadSignal<HashMap<u64, ContentDiffReport>>,
    set_entity_diff_cache: WriteSignal<HashMap<u64, ContentDiffReport>>,
    pub(in crate::pages::patrol) media_diff_loading: ReadSignal<bool>,
    set_media_diff_loading: WriteSignal<bool>,
    pub(in crate::pages::patrol) current_media_diff: ReadSignal<Option<MediaDiffReport>>,
    set_current_media_diff: WriteSignal<Option<MediaDiffReport>>,
    media_diff_cache: ReadSignal<HashMap<u64, MediaDiffReport>>,
    set_media_diff_cache: WriteSignal<HashMap<u64, MediaDiffReport>>,
    edit_action: ReadSignal<Option<EditAction>>,
    pub(in crate::pages::patrol) set_edit_action: WriteSignal<Option<EditAction>>,
    tag_action: ReadSignal<Option<TagAction>>,
    pub(in crate::pages::patrol) set_tag_action: WriteSignal<Option<TagAction>>,
}

pub(super) struct RevisionArtifactEffectsInput {
    pub(super) selected_rev_id: ReadSignal<Option<u64>>,
    pub(super) queue: Memo<Vec<QueuedEdit>>,
    pub(super) selected_edit: ReadSignal<Option<QueuedEdit>>,
    pub(super) set_selected_edit: WriteSignal<Option<QueuedEdit>>,
    pub(super) all_edits: ReadSignal<Vec<QueuedEdit>>,
    pub(super) set_all_edits: WriteSignal<Vec<QueuedEdit>>,
    pub(super) set_action_status: WriteSignal<String>,
    pub(super) artifacts: RevisionArtifactController,
}

pub(super) fn create_revision_artifact_controller() -> RevisionArtifactController {
    let (diff_loading, set_diff_loading) = signal(false);
    let (current_diff, set_current_diff) = signal(None::<StructuredDiff>);
    let (current_entity_diff, set_current_entity_diff) = signal(None::<ContentDiffReport>);
    let (diff_cache, set_diff_cache) = signal(HashMap::<u64, StructuredDiff>::new());
    let (entity_diff_cache, set_entity_diff_cache) =
        signal(HashMap::<u64, ContentDiffReport>::new());
    let (media_diff_loading, set_media_diff_loading) = signal(false);
    let (current_media_diff, set_current_media_diff) = signal(None::<MediaDiffReport>);
    let (media_diff_cache, set_media_diff_cache) = signal(HashMap::<u64, MediaDiffReport>::new());
    let (edit_action, set_edit_action) = signal(None::<EditAction>);
    let (tag_action, set_tag_action) = signal(None::<TagAction>);

    RevisionArtifactController {
        diff_loading,
        set_diff_loading,
        current_diff,
        set_current_diff,
        current_entity_diff,
        set_current_entity_diff,
        diff_cache,
        set_diff_cache,
        entity_diff_cache,
        set_entity_diff_cache,
        media_diff_loading,
        set_media_diff_loading,
        current_media_diff,
        set_current_media_diff,
        media_diff_cache,
        set_media_diff_cache,
        edit_action,
        set_edit_action,
        tag_action,
        set_tag_action,
    }
}

pub(in crate::pages::patrol) fn cache_initial_artifacts(
    view: &LiveOperatorView,
    artifacts: RevisionArtifactController,
) {
    if let (Some(diff), Some(selected_index)) = (&view.diff, view.selected_index) {
        if let Some(edit) = view.queue.get(selected_index) {
            let mut cache = artifacts.diff_cache.get_untracked();
            cache.insert(edit.event.rev_id, diff.clone());
            artifacts.set_diff_cache.set(cache);
        }
    }

    if let (Some(media_diff), Some(selected_index)) = (&view.media_diff, view.selected_index) {
        if let Some(edit) = view.queue.get(selected_index) {
            let mut cache = artifacts.media_diff_cache.get_untracked();
            cache.insert(edit.event.rev_id, media_diff.clone());
            artifacts.set_media_diff_cache.set(cache);
        }
    }

    if let Some(ref diff) = view.diff {
        artifacts.set_current_diff.set(Some(diff.clone()));
    }
    artifacts
        .set_current_media_diff
        .set(view.media_diff.clone());
}

/// Apply a fetched content-diff report to the controller: text reports feed
/// the existing text pipeline; entity reports feed the entity view
/// (ADR-0016 Decision 4 routing, on the client edge).
fn apply_content_diff(
    artifacts: RevisionArtifactController,
    rev_id: u64,
    report: Option<ContentDiffReport>,
) {
    match report {
        Some(report) => match report.diff {
            ContentDiff::Text { diff } => {
                let mut cache = artifacts.diff_cache.get_untracked();
                cache.insert(rev_id, diff.clone());
                artifacts.set_diff_cache.set(cache);
                artifacts.set_current_diff.set(Some(diff));
                artifacts.set_current_entity_diff.set(None);
            }
            ContentDiff::Entity { .. } => {
                let mut cache = artifacts.entity_diff_cache.get_untracked();
                cache.insert(rev_id, report.clone());
                artifacts.set_entity_diff_cache.set(cache);
                artifacts.set_current_entity_diff.set(Some(report));
                artifacts.set_current_diff.set(None);
            }
        },
        None => {
            artifacts.set_current_diff.set(None);
            artifacts.set_current_entity_diff.set(None);
        }
    }
}

pub(in crate::pages::patrol) fn prefetch_queue_diffs(
    view: &LiveOperatorView,
    artifacts: RevisionArtifactController,
) {
    let prefetch_queue = view.queue.clone();
    let prefetch_wiki = view.wiki_id.clone();
    wasm_bindgen_futures::spawn_local(async move {
        for item in &prefetch_queue {
            let rev_id = item.event.rev_id;
            if artifacts.diff_cache.get_untracked().contains_key(&rev_id) {
                continue;
            }
            if artifacts
                .entity_diff_cache
                .get_untracked()
                .contains_key(&rev_id)
            {
                continue;
            }
            let old_rev_id = item.event.old_rev_id.unwrap_or(0);
            if old_rev_id == 0 {
                continue;
            }
            if let Ok(Some(report)) = fetch_content_diff(&prefetch_wiki, rev_id, old_rev_id).await {
                match report.diff {
                    ContentDiff::Text { diff } => {
                        let mut cache = artifacts.diff_cache.get_untracked();
                        cache.insert(rev_id, diff);
                        artifacts.set_diff_cache.set(cache);
                    }
                    ContentDiff::Entity { .. } => {
                        let mut cache = artifacts.entity_diff_cache.get_untracked();
                        cache.insert(rev_id, report);
                        artifacts.set_entity_diff_cache.set(cache);
                    }
                }
            }
        }
    });
}

pub(super) fn install_revision_artifact_effects(input: RevisionArtifactEffectsInput) {
    let RevisionArtifactEffectsInput {
        selected_rev_id,
        queue,
        selected_edit,
        set_selected_edit,
        all_edits,
        set_all_edits,
        set_action_status,
        artifacts,
    } = input;

    install_selected_diff_effect(
        selected_rev_id,
        queue,
        selected_edit,
        set_selected_edit,
        artifacts,
    );
    install_selected_media_diff_effect(selected_rev_id, selected_edit, artifacts);
    install_inline_edit_effect(selected_edit, set_action_status, artifacts);
    install_tag_action_effect(
        selected_edit,
        all_edits,
        set_all_edits,
        set_action_status,
        artifacts,
    );
}

/// Read `rev=N` from the URL hash fragment.
pub(in crate::pages::patrol) fn rev_id_from_hash() -> Option<u64> {
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

fn install_selected_diff_effect(
    selected_rev_id: ReadSignal<Option<u64>>,
    queue: Memo<Vec<QueuedEdit>>,
    selected_edit: ReadSignal<Option<QueuedEdit>>,
    set_selected_edit: WriteSignal<Option<QueuedEdit>>,
    artifacts: RevisionArtifactController,
) {
    Effect::new(move |prev_rev: Option<Option<u64>>| {
        let current_rev = selected_rev_id.get();
        let Some(rev_id) = current_rev else {
            return current_rev;
        };

        set_hash_rev(rev_id);

        if prev_rev == Some(current_rev) {
            return current_rev;
        }

        console::debug(&format!("[SP42] selection changed → rev {rev_id}"));

        let queue = queue.get_untracked();
        let edit = queue
            .iter()
            .find(|edit| edit.event.rev_id == rev_id)
            .cloned();
        set_selected_edit.set(edit);

        let cache = artifacts.diff_cache.get_untracked();
        if let Some(diff) = cache.get(&rev_id) {
            console::debug(&format!(
                "[SP42] diff cache HIT rev {rev_id} ({} segments)",
                diff.segments.len()
            ));
            artifacts.set_current_diff.set(Some(diff.clone()));
            artifacts.set_current_entity_diff.set(None);
            return current_rev;
        }
        let entity_cache = artifacts.entity_diff_cache.get_untracked();
        if let Some(report) = entity_cache.get(&rev_id) {
            console::debug(&format!("[SP42] entity diff cache HIT rev {rev_id}"));
            artifacts.set_current_entity_diff.set(Some(report.clone()));
            artifacts.set_current_diff.set(None);
            return current_rev;
        }

        console::debug(&format!("[SP42] diff cache MISS rev {rev_id} — fetching"));
        artifacts.set_diff_loading.set(true);
        artifacts.set_current_diff.set(None);
        artifacts.set_current_entity_diff.set(None);
        if let Some(edit) = selected_edit.get_untracked() {
            let old_rev_id = edit.event.old_rev_id.unwrap_or(0);
            let wiki_id = edit.event.wiki_id.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let report = fetch_content_diff(&wiki_id, rev_id, old_rev_id)
                    .await
                    .unwrap_or(None);
                apply_content_diff(artifacts, rev_id, report);
                artifacts.set_diff_loading.set(false);
            });
        }
        current_rev
    });
}

fn install_selected_media_diff_effect(
    selected_rev_id: ReadSignal<Option<u64>>,
    selected_edit: ReadSignal<Option<QueuedEdit>>,
    artifacts: RevisionArtifactController,
) {
    Effect::new(move |prev_rev: Option<Option<u64>>| {
        let current_rev = selected_rev_id.get();
        let Some(rev_id) = current_rev else {
            artifacts.set_current_media_diff.set(None);
            artifacts.set_media_diff_loading.set(false);
            return current_rev;
        };

        if prev_rev == Some(current_rev) {
            return current_rev;
        }

        let cache = artifacts.media_diff_cache.get_untracked();
        if let Some(report) = cache.get(&rev_id) {
            artifacts.set_current_media_diff.set(Some(report.clone()));
            artifacts.set_media_diff_loading.set(false);
            return current_rev;
        }

        artifacts.set_media_diff_loading.set(true);
        artifacts.set_current_media_diff.set(None);
        if let Some(edit) = selected_edit.get_untracked() {
            let old_rev_id = edit.event.old_rev_id.unwrap_or(0);
            if old_rev_id == 0 {
                artifacts.set_media_diff_loading.set(false);
                return current_rev;
            }
            let wiki_id = edit.event.wiki_id.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match fetch_media_diff(&wiki_id, rev_id, old_rev_id).await {
                    Ok(report) => {
                        if let Some(ref media_diff) = report {
                            let mut cache = artifacts.media_diff_cache.get_untracked();
                            cache.insert(rev_id, media_diff.clone());
                            artifacts.set_media_diff_cache.set(cache);
                        }
                        artifacts.set_current_media_diff.set(report);
                    }
                    Err(_) => {
                        artifacts.set_current_media_diff.set(None);
                    }
                }
                artifacts.set_media_diff_loading.set(false);
            });
        } else {
            artifacts.set_media_diff_loading.set(false);
        }
        current_rev
    });
}

fn install_inline_edit_effect(
    selected_edit: ReadSignal<Option<QueuedEdit>>,
    set_action_status: WriteSignal<String>,
    artifacts: RevisionArtifactController,
) {
    Effect::new(move |_| {
        let Some(action) = artifacts.edit_action.get() else {
            return;
        };
        artifacts.set_edit_action.set(None);
        let Some(edit) = selected_edit.get_untracked() else {
            return;
        };
        let request = SessionActionExecutionRequest {
            wiki_id: edit.event.wiki_id.clone(),
            kind: SessionActionKind::InlineEdit,
            rev_id: edit.event.rev_id,
            title: Some(edit.event.title.clone()),
            target_user: None,
            undo_after_rev_id: None,
            summary: Some("SP42: inline edit".to_string()),
            selected_text: Some(action.original_text),
            batch_rev_ids: None,
            replacement_text: Some(action.new_text),
            node_locator: None,
        };
        console::info(&format!("[SP42] inline edit on rev {}", request.rev_id));
        set_action_status.set("Saving inline edit...".to_string());
        wasm_bindgen_futures::spawn_local(async move {
            match execute_dev_auth_action(&request).await {
                Ok(response) if response.accepted => {
                    set_action_status.set(format!("Edit saved on rev {}", request.rev_id));
                    invalidate_current_diff(selected_edit, artifacts, request.rev_id).await;
                }
                Ok(response) => set_action_status.set(format!(
                    "Edit rejected: {}",
                    response.message.unwrap_or_default()
                )),
                Err(error) => set_action_status.set(format!("Edit error: {error}")),
            }
        });
    });
}

fn install_tag_action_effect(
    selected_edit: ReadSignal<Option<QueuedEdit>>,
    all_edits: ReadSignal<Vec<QueuedEdit>>,
    set_all_edits: WriteSignal<Vec<QueuedEdit>>,
    set_action_status: WriteSignal<String>,
    artifacts: RevisionArtifactController,
) {
    Effect::new(move |_| {
        let Some(action) = artifacts.tag_action.get() else {
            return;
        };
        artifacts.set_tag_action.set(None);

        let Some(edit) = selected_edit.get_untracked() else {
            return;
        };

        let request = SessionActionExecutionRequest {
            wiki_id: edit.event.wiki_id.clone(),
            kind: SessionActionKind::TagCitationNeeded,
            rev_id: edit.event.rev_id,
            title: Some(edit.event.title.clone()),
            target_user: None,
            undo_after_rev_id: None,
            summary: Some("SP42: added {{refnec}}".to_string()),
            selected_text: Some(action.text),
            batch_rev_ids: None,
            replacement_text: None,
            node_locator: None,
        };

        set_action_status.set("Adding citation needed...".to_string());
        wasm_bindgen_futures::spawn_local(async move {
            match execute_dev_auth_action(&request).await {
                Ok(response) if response.accepted => {
                    set_action_status.set(format!(
                        "Citation needed + patrolled rev {}",
                        request.rev_id
                    ));
                    let mut edits = all_edits.get_untracked();
                    if let Some(pos) = edits
                        .iter()
                        .position(|edit| edit.event.rev_id == request.rev_id)
                    {
                        edits.remove(pos);
                        set_all_edits.set(edits);
                    }
                    invalidate_current_diff(selected_edit, artifacts, request.rev_id).await;
                }
                Ok(response) => {
                    set_action_status.set(format!(
                        "Citation needed rejected: {}",
                        response.message.unwrap_or_default()
                    ));
                }
                Err(error) => {
                    set_action_status.set(format!("Citation error: {error}"));
                }
            }
        });
    });
}

async fn invalidate_current_diff(
    selected_edit: ReadSignal<Option<QueuedEdit>>,
    artifacts: RevisionArtifactController,
    rev_id: u64,
) {
    let mut cache = artifacts.diff_cache.get_untracked();
    cache.remove(&rev_id);
    artifacts.set_diff_cache.set(cache);
    let mut entity_cache = artifacts.entity_diff_cache.get_untracked();
    entity_cache.remove(&rev_id);
    artifacts.set_entity_diff_cache.set(entity_cache);
    artifacts.set_diff_loading.set(true);
    if let Some(item) = selected_edit.get_untracked() {
        let old_rev_id = item.event.old_rev_id.unwrap_or(0);
        if let Ok(report) =
            fetch_content_diff(&item.event.wiki_id, item.event.rev_id, old_rev_id).await
        {
            apply_content_diff(artifacts, item.event.rev_id, report);
        }
    }
    artifacts.set_diff_loading.set(false);
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
