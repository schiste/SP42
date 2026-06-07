//! Fixture-backed developer preview builders shared by CLI and desktop shells.

use std::collections::BTreeMap;

use sp42_core::backlog_runtime::{BacklogRuntime, BacklogRuntimeConfig, BacklogRuntimeStatus};
use sp42_core::coordination_codec::{decode_message, encode_message};
use sp42_core::coordination_state::{CoordinationState, CoordinationStateSummary};
use sp42_core::errors::{BacklogRuntimeError, CodecError, ConfigError, StreamRuntimeError};
use sp42_core::recent_changes::RecentChangesBatch;
use sp42_core::stream_runtime::{StreamRuntime, StreamRuntimeStatus};
use sp42_core::traits::{MemoryStorage, ReplayEventSource, StubHttpClient};
use sp42_core::types::{
    Action, ActionBroadcast, CoordinationMessage, EditClaim, EditEvent, FlaggedEdit, HttpRequest,
    HttpResponse, PresenceHeartbeat, RaceResolution, ScoreDelta, ServerSentEvent, WikiConfig,
};

pub const DEV_PREVIEW_WIKI_ID: &str = "frwiki";
pub const DEV_PREVIEW_ACTOR: &str = "LocalUser";
pub const DEV_PREVIEW_REV_ID: u64 = 123_456;
pub const DEV_PREVIEW_DEFAULT_CONFIG: &str = include_str!("../../../configs/frwiki.yaml");
pub const DEV_PREVIEW_SAMPLE_EVENTS: &str =
    include_str!("../../../fixtures/frwiki_recentchanges_batch.jsonl");
pub const DEV_PREVIEW_SAMPLE_BACKLOG_RESPONSE: &str = r#"{
  "continue": { "rccontinue": "20260324010202|456" },
  "query": {
    "recentchanges": [
      {
        "type": "edit",
        "title": "Exemple",
        "ns": 0,
        "revid": 123460,
        "old_revid": 123459,
        "user": "192.0.2.11",
        "timestamp": "2026-03-24T01:02:03Z",
        "bot": false,
        "minor": false,
        "new": false,
        "oldlen": 120,
        "newlen": 90,
        "comment": "backlog sample",
        "tags": ["mw-reverted"]
      }
    ]
  }
}"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevStreamPreview {
    pub status: StreamRuntimeStatus,
    pub edits: Vec<EditEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevBacklogPreview {
    pub status: BacklogRuntimeStatus,
    pub request: HttpRequest,
    pub batch: RecentChangesBatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevCoordinationPreview {
    pub summary: CoordinationStateSummary,
    pub roundtrips: Vec<String>,
}

/// Parse the shared developer preview wiki configuration.
///
/// # Errors
///
/// Returns [`ConfigError`] when the embedded fixture is invalid.
pub fn parse_default_dev_wiki_config() -> Result<WikiConfig, ConfigError> {
    sp42_core::parse_wiki_config(DEV_PREVIEW_DEFAULT_CONFIG)
}

/// Build a stream runtime preview from JSONL recentchanges events.
///
/// # Errors
///
/// Returns [`StreamRuntimeError`] when stream initialization, event ingestion,
/// checkpoint storage, or reconnect handling fails.
pub async fn build_dev_stream_preview(
    config: &WikiConfig,
    payload: &str,
    event_id_prefix: &str,
) -> Result<DevStreamPreview, StreamRuntimeError> {
    let source = ReplayEventSource::new(replay_events_from_jsonl(payload, event_id_prefix));
    let storage = MemoryStorage::default();
    let mut runtime = StreamRuntime::from_config(config, source, storage);

    runtime.initialize().await?;
    let mut edits = Vec::new();
    while let Some(edit) = runtime.next_actionable_event().await? {
        edits.push(edit);
    }
    runtime.reconnect_from_checkpoint().await?;

    Ok(DevStreamPreview {
        status: runtime.status(),
        edits,
    })
}

/// Build a backlog polling preview using the shared fixture response.
///
/// # Errors
///
/// Returns [`BacklogRuntimeError`] when runtime initialization, request
/// construction, response parsing, or checkpoint storage fails.
pub async fn build_dev_backlog_preview(
    config: &WikiConfig,
) -> Result<DevBacklogPreview, BacklogRuntimeError> {
    let storage = MemoryStorage::default();
    let client = StubHttpClient::new([Ok(HttpResponse {
        status: 200,
        headers: BTreeMap::new(),
        body: DEV_PREVIEW_SAMPLE_BACKLOG_RESPONSE.as_bytes().to_vec(),
    })]);
    let mut runtime = BacklogRuntime::from_config(
        config,
        storage,
        BacklogRuntimeConfig {
            limit: 5,
            include_bots: false,
        },
    );

    runtime.initialize().await?;
    let request = runtime.build_next_request()?;
    let batch = runtime.poll(&client).await?;

    Ok(DevBacklogPreview {
        status: runtime.status(),
        request,
        batch,
    })
}

/// Build a coordination roundtrip preview for a wiki.
///
/// # Errors
///
/// Returns [`CodecError`] when message encoding or decoding fails.
pub fn build_dev_coordination_preview(wiki_id: &str) -> Result<DevCoordinationPreview, CodecError> {
    let messages = dev_coordination_preview_messages(wiki_id);
    let mut state = CoordinationState::new(wiki_id);
    let mut roundtrips = Vec::new();

    for message in messages {
        let bytes = encode_message(&message)?;
        let byte_len = bytes.len();
        let decoded = decode_message(&bytes)?;
        let label = dev_coordination_message_label(&decoded);
        let _ = state.apply(decoded);
        roundtrips.push(format!("roundtrip {label} bytes={byte_len}"));
    }

    Ok(DevCoordinationPreview {
        summary: state.summary(),
        roundtrips,
    })
}

#[must_use]
pub fn dev_coordination_preview_messages(wiki_id: &str) -> Vec<CoordinationMessage> {
    vec![
        CoordinationMessage::EditClaim(EditClaim {
            wiki_id: wiki_id.to_string(),
            rev_id: DEV_PREVIEW_REV_ID,
            actor: DEV_PREVIEW_ACTOR.to_string(),
        }),
        CoordinationMessage::PresenceHeartbeat(PresenceHeartbeat {
            wiki_id: wiki_id.to_string(),
            actor: DEV_PREVIEW_ACTOR.to_string(),
            active_edit_count: 1,
        }),
        CoordinationMessage::ScoreDelta(ScoreDelta {
            wiki_id: wiki_id.to_string(),
            rev_id: DEV_PREVIEW_REV_ID,
            delta: 8,
            reason: "LiftWing + warning history".to_string(),
        }),
        CoordinationMessage::FlaggedEdit(FlaggedEdit {
            wiki_id: wiki_id.to_string(),
            rev_id: DEV_PREVIEW_REV_ID,
            score: 95,
            reason: "possible vandalism".to_string(),
        }),
        CoordinationMessage::ActionBroadcast(ActionBroadcast {
            wiki_id: wiki_id.to_string(),
            rev_id: DEV_PREVIEW_REV_ID,
            action: Action::Rollback,
            actor: DEV_PREVIEW_ACTOR.to_string(),
        }),
        CoordinationMessage::RaceResolution(RaceResolution {
            wiki_id: wiki_id.to_string(),
            rev_id: DEV_PREVIEW_REV_ID,
            winning_actor: DEV_PREVIEW_ACTOR.to_string(),
        }),
    ]
}

#[must_use]
pub fn dev_coordination_message_label(message: &CoordinationMessage) -> &'static str {
    match message {
        CoordinationMessage::ActionBroadcast(_) => "ActionBroadcast",
        CoordinationMessage::EditClaim(_) => "EditClaim",
        CoordinationMessage::ScoreDelta(_) => "ScoreDelta",
        CoordinationMessage::PresenceHeartbeat(_) => "PresenceHeartbeat",
        CoordinationMessage::FlaggedEdit(_) => "FlaggedEdit",
        CoordinationMessage::RaceResolution(_) => "RaceResolution",
    }
}

fn replay_events_from_jsonl<'a>(
    payload: &'a str,
    event_id_prefix: &'a str,
) -> impl Iterator<Item = ServerSentEvent> + 'a {
    payload
        .lines()
        .enumerate()
        .filter_map(move |(index, line)| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }

            Some(ServerSentEvent {
                event_type: Some("message".to_string()),
                id: Some(format!("{event_id_prefix}-{}", index + 1)),
                data: trimmed.to_string(),
                retry_ms: None,
            })
        })
}

#[cfg(test)]
mod tests {
    use futures::executor::block_on;

    use super::{
        DEV_PREVIEW_SAMPLE_EVENTS, DEV_PREVIEW_WIKI_ID, build_dev_backlog_preview,
        build_dev_coordination_preview, build_dev_stream_preview, parse_default_dev_wiki_config,
    };

    #[test]
    fn parses_default_dev_wiki_config() {
        let config = parse_default_dev_wiki_config().expect("fixture config should parse");

        assert_eq!(config.wiki_id, DEV_PREVIEW_WIKI_ID);
    }

    #[test]
    fn builds_stream_preview_from_shared_fixture() {
        let config = parse_default_dev_wiki_config().expect("fixture config should parse");
        let preview = block_on(build_dev_stream_preview(
            &config,
            DEV_PREVIEW_SAMPLE_EVENTS,
            "fixture",
        ))
        .expect("stream preview should build");

        assert_eq!(preview.status.checkpoint_key, "stream.last_event_id.frwiki");
        assert_eq!(preview.status.last_event_id.as_deref(), Some("fixture-4"));
        assert_eq!(preview.edits.len(), 4);
    }

    #[test]
    fn builds_backlog_preview_from_shared_fixture() {
        let config = parse_default_dev_wiki_config().expect("fixture config should parse");
        let preview =
            block_on(build_dev_backlog_preview(&config)).expect("backlog preview should build");

        assert_eq!(
            preview.status.checkpoint_key,
            "recentchanges.rccontinue.frwiki"
        );
        assert_eq!(
            preview.status.next_continue.as_deref(),
            Some("20260324010202|456")
        );
        assert_eq!(preview.batch.events.len(), 1);
    }

    #[test]
    fn builds_coordination_preview_from_shared_fixture() {
        let preview = build_dev_coordination_preview(DEV_PREVIEW_WIKI_ID)
            .expect("coordination preview should build");

        assert_eq!(preview.summary.wiki_id, DEV_PREVIEW_WIKI_ID);
        assert_eq!(preview.summary.claims.len(), 1);
        assert_eq!(preview.roundtrips.len(), 6);
    }
}
