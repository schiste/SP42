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
    // Seed the per-namespace default content model exactly like the server
    // ingestion paths (ADR-0016) — otherwise a live Wikidata edit is queued
    // as unknown/wikitext and mis-scored against the initial-load entries.
    let content_model =
        sp42_core::default_namespace_content_model_for_wiki(&event.wiki, event.namespace)
            .map(str::to_owned);
    // Entity content mirrors the scoring engine's gate (ADR-0016 Decision 5):
    // a uniform base with no signal contributions — the wikitext byte/identity
    // heuristics below misread entity JSON edits.
    let entity_content =
        sp42_core::derive_content_model_capabilities(content_model.as_deref()).entity_diff;
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
    if entity_content {
        // No contributions: keep entity inserts chronological (PRD-0011 Q3).
    } else {
        apply_wikitext_quick_signals(event, is_anon, &performer, &mut score, &mut contributions);
    }

    QueuedEdit {
        event: EditEvent {
            content_model,
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

/// The quick wikitext-content signals for a live insert (anon/temporary
/// identity, large removals). Skipped entirely for entity content.
fn apply_wikitext_quick_signals(
    event: &StreamEvent,
    is_anon: bool,
    performer: &EditorIdentity,
    score: &mut i32,
    contributions: &mut Vec<SignalContribution>,
) {
    if is_anon {
        *score += 20;
        contributions.push(SignalContribution {
            signal: ScoringSignal::AnonymousUser,
            weight: 20,
            note: None,
        });
    }
    if matches!(performer, EditorIdentity::Temporary { .. }) {
        *score += 20;
        contributions.push(SignalContribution {
            signal: ScoringSignal::AnonymousUser,
            weight: 20,
            note: Some("temporary account".to_string()),
        });
    }
    if event.byte_delta().abs() > 500 {
        *score += 15;
        contributions.push(SignalContribution {
            signal: ScoringSignal::LargeContentRemoval,
            weight: 15,
            note: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::stream_event_to_queued_edit;
    use crate::platform::eventstream::StreamEvent;

    fn event(wiki: &str, namespace: i32) -> StreamEvent {
        StreamEvent {
            wiki: wiki.to_string(),
            title: "Q42".to_string(),
            namespace,
            rev_id: 1,
            old_rev_id: Some(0),
            user: "127.0.0.1".to_string(), // anon + large removal: both signals fire on wikitext
            bot: false,
            minor: false,
            new_page: false,
            patrolled: false,
            timestamp_ms: 0,
            comment: None,
            length_old: 2000,
            length_new: 100,
        }
    }

    #[test]
    fn live_wikidata_insert_seeds_content_model_and_skips_wikitext_signals() {
        let queued = stream_event_to_queued_edit(&event("wikidatawiki", 0));
        assert_eq!(queued.event.content_model.as_deref(), Some("wikibase-item"));
        // Mirrors the scoring engine's entity gate: uniform base, no signals.
        assert_eq!(queued.score.total, 0);
        assert!(queued.score.contributions.is_empty());

        // A Wikidata talk-page edit stays on the wikitext path.
        let talk = stream_event_to_queued_edit(&event("wikidatawiki", 1));
        assert_eq!(talk.event.content_model, None);
        assert!(!talk.score.contributions.is_empty());
    }

    #[test]
    fn live_wikipedia_insert_keeps_the_quick_signals() {
        let queued = stream_event_to_queued_edit(&event("enwiki", 0));
        assert_eq!(queued.event.content_model, None);
        assert_eq!(queued.score.total, 35, "anon + large removal");
        assert_eq!(queued.score.contributions.len(), 2);
    }
}
