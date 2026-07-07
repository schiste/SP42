use std::sync::{Arc, Mutex};

use leptos::prelude::*;
use send_wrapper::SendWrapper;
use sp42_core::{
    CompositeScore, EditEvent, EditorIdentity, FlagState, QueuedEdit, ScoringSignal,
    SignalContribution,
};

use crate::platform::console;
use crate::platform::eventstream::{EventStreamHandle, StreamEvent, start_eventstream};

pub(super) fn install_patrol_eventstream(
    active_wiki_id: String,
    all_edits: ReadSignal<Vec<QueuedEdit>>,
    set_all_edits: WriteSignal<Vec<QueuedEdit>>,
) {
    let eventstream_handle = Arc::new(Mutex::new(SendWrapper::new(None::<EventStreamHandle>)));
    let cleanup_eventstream_handle = Arc::clone(&eventstream_handle);
    on_cleanup(move || {
        if let Ok(mut handle) = cleanup_eventstream_handle.lock() {
            let _ = (**handle).take();
        }
    });

    Effect::new(move |started: Option<bool>| {
        if started.is_none() {
            match start_eventstream(&active_wiki_id, move |event: StreamEvent| {
                let queued = stream_event_to_queued_edit(&event);
                let mut edits = all_edits.get_untracked();
                if edits
                    .iter()
                    .any(|edit| edit.event.rev_id == queued.event.rev_id)
                {
                    return;
                }
                console::debug(&format!(
                    "[SP42] SSE: rev {} \"{}\" by {} (score {})",
                    queued.event.rev_id, queued.event.title, event.user, queued.score.total
                ));
                edits.insert(0, queued);
                if edits.len() > 200 {
                    edits.truncate(200);
                }
                set_all_edits.set(edits);
            }) {
                Ok(handle) => {
                    if let Ok(mut eventstream_handle) = eventstream_handle.lock() {
                        **eventstream_handle = Some(handle);
                    }
                }
                Err(error) => {
                    console::warn(&format!("[SP42] EventStreams unavailable: {error}"));
                }
            }
        }
        true
    });
}

fn stream_event_to_queued_edit(event: &StreamEvent) -> QueuedEdit {
    let is_anon = event
        .user
        .chars()
        .all(|c| c.is_ascii_digit() || c == '.' || c == ':');
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
            content_model: None,
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
