use std::collections::HashMap;

use leptos::prelude::*;
use sp42_core::{EditorIdentity, QueuedEdit};

use crate::components::filter_bar::PatrolFilterParams;

pub(super) struct PatrolQueueController {
    pub(super) filters: ReadSignal<PatrolFilterParams>,
    pub(super) set_filters: WriteSignal<PatrolFilterParams>,
    pub(super) all_edits: ReadSignal<Vec<QueuedEdit>>,
    pub(super) set_all_edits: WriteSignal<Vec<QueuedEdit>>,
    pub(super) group_rev_ids: ReadSignal<HashMap<u64, Vec<u64>>>,
    pub(super) queue: Memo<Vec<QueuedEdit>>,
    pub(super) selected_index: Memo<usize>,
}

pub(super) fn create_patrol_queue_controller(
    selected_rev_id: ReadSignal<Option<u64>>,
) -> PatrolQueueController {
    let (filters, set_filters) = signal(PatrolFilterParams::default());
    let (all_edits, set_all_edits) = signal(Vec::<QueuedEdit>::new());
    let (group_rev_ids, set_group_rev_ids) = signal(HashMap::<u64, Vec<u64>>::new());

    let queue = Memo::new(move |_: Option<&Vec<QueuedEdit>>| {
        let edits = all_edits.get();
        let current_filters = filters.get();
        let filtered = filter_edits(edits, &current_filters);

        if !current_filters.group_edits {
            set_group_rev_ids.set(HashMap::new());
            return filtered
                .into_iter()
                .take(current_filters.limit as usize)
                .collect();
        }

        let (grouped, rev_map) = group_edits(filtered);
        set_group_rev_ids.set(rev_map);
        grouped
            .into_iter()
            .take(current_filters.limit as usize)
            .collect()
    });

    let selected_index = Memo::new(move |_| {
        let queue = queue.get();
        let rev = selected_rev_id.get();
        rev.and_then(|r| queue.iter().position(|edit| edit.event.rev_id == r))
            .unwrap_or(0)
    });

    PatrolQueueController {
        filters,
        set_filters,
        all_edits,
        set_all_edits,
        group_rev_ids,
        queue,
        selected_index,
    }
}

fn filter_edits(edits: Vec<QueuedEdit>, filters: &PatrolFilterParams) -> Vec<QueuedEdit> {
    edits
        .into_iter()
        .filter(|item| {
            if filters.unpatrolled_only && item.event.is_patrolled.is_enabled() {
                return false;
            }
            if !filters.include_bots && item.event.is_bot.is_enabled() {
                return false;
            }
            if !filters.include_minor && item.event.is_minor.is_enabled() {
                return false;
            }
            if !filters.include_new_pages && item.event.is_new_page.is_enabled() {
                return false;
            }
            match &item.event.performer {
                EditorIdentity::Anonymous { .. } => {
                    if !filters.include_anonymous {
                        return false;
                    }
                }
                EditorIdentity::Temporary { .. } => {
                    if !filters.include_temporary {
                        return false;
                    }
                }
                EditorIdentity::Registered { .. } => {
                    if !filters.include_registered {
                        return false;
                    }
                }
            }
            if let Some(ref tag) = filters.tag_filter {
                if !tag.trim().is_empty() && !item.event.tags.iter().any(|t| t == tag.trim()) {
                    return false;
                }
            }
            if let Some(min) = filters.min_score {
                if item.score.total < min {
                    return false;
                }
            }
            true
        })
        .collect()
}

fn group_edits(edits: Vec<QueuedEdit>) -> (Vec<QueuedEdit>, HashMap<u64, Vec<u64>>) {
    let mut groups: Vec<(String, Vec<QueuedEdit>)> = Vec::new();
    for item in edits {
        let key = format!(
            "{}|{}",
            item.event.title,
            performer_key(&item.event.performer)
        );
        if let Some(group) = groups.iter_mut().find(|(k, _)| k == &key) {
            group.1.push(item);
        } else {
            groups.push((key, vec![item]));
        }
    }

    let mut grouped = Vec::new();
    let mut rev_map = HashMap::new();
    for (_key, mut members) in groups {
        if members.len() == 1 {
            grouped.push(members.remove(0));
            continue;
        }

        members.sort_by_key(|edit| edit.event.rev_id);
        let all_revs = members
            .iter()
            .map(|edit| edit.event.rev_id)
            .collect::<Vec<_>>();
        let oldest_old = members.first().and_then(|edit| edit.event.old_rev_id);
        let newest = members.last().expect("non-empty group");
        let max_score = members
            .iter()
            .map(|edit| edit.score.total)
            .max()
            .unwrap_or(0);
        let total_delta = members.iter().map(|edit| edit.event.byte_delta).sum();

        let mut merged = newest.clone();
        merged.event.old_rev_id = oldest_old;
        merged.event.byte_delta = total_delta;
        merged.score.total = max_score;

        rev_map.insert(merged.event.rev_id, all_revs);
        grouped.push(merged);
    }

    (grouped, rev_map)
}

fn performer_key(performer: &EditorIdentity) -> String {
    match performer {
        EditorIdentity::Registered { username } => username.clone(),
        EditorIdentity::Anonymous { label } => label.clone(),
        EditorIdentity::Temporary { label } => label.clone(),
    }
}
