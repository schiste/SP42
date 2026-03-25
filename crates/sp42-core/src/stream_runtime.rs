//! Shared stream runtime around `EventSource` checkpointing and ingestion.

use crate::errors::{StorageError, StreamRuntimeError};
use crate::stream_ingestor::StreamIngestor;
use crate::traits::{EventSource, Storage};
use crate::types::{EditEvent, WikiConfig};
use serde::{Deserialize, Serialize};

const DEFAULT_CURSOR_KEY_PREFIX: &str = "stream.last_event_id";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamRuntimeStatus {
    pub checkpoint_key: String,
    pub last_event_id: Option<String>,
    pub delivered_events: u64,
    pub filtered_events: u64,
    pub reconnect_attempts: u64,
}

#[derive(Debug)]
pub struct StreamRuntime<E, S> {
    event_source: E,
    storage: S,
    ingestor: StreamIngestor,
    checkpoint_key: String,
    last_event_id: Option<String>,
    delivered_events: u64,
    filtered_events: u64,
    reconnect_attempts: u64,
}

impl<E, S> StreamRuntime<E, S>
where
    E: EventSource,
    S: Storage,
{
    #[must_use]
    pub fn from_config(config: &WikiConfig, event_source: E, storage: S) -> Self {
        Self::new(
            StreamIngestor::from_config(config),
            event_source,
            storage,
            format!("{}.{}", DEFAULT_CURSOR_KEY_PREFIX, config.wiki_id),
        )
    }

    #[must_use]
    pub fn new(
        ingestor: StreamIngestor,
        event_source: E,
        storage: S,
        checkpoint_key: String,
    ) -> Self {
        Self {
            event_source,
            storage,
            ingestor,
            checkpoint_key,
            last_event_id: None,
            delivered_events: 0,
            filtered_events: 0,
            reconnect_attempts: 0,
        }
    }

    /// Initialize the runtime from the persisted stream cursor.
    ///
    /// # Errors
    ///
    /// Returns [`StreamRuntimeError`] when the cursor cannot be loaded from
    /// storage or the event source cannot reconnect to the saved checkpoint.
    pub async fn initialize(&mut self) -> Result<(), StreamRuntimeError> {
        self.restore_checkpoint().await?;
        if self.last_event_id.is_some() {
            self.reconnect_from_checkpoint().await?;
        }
        Ok(())
    }

    /// Consume stream events until an actionable edit is produced or the source
    /// is exhausted.
    ///
    /// # Errors
    ///
    /// Returns [`StreamRuntimeError`] when the event source fails, cursor
    /// persistence fails, or the event payload is not valid recentchange JSON.
    pub async fn next_actionable_event(&mut self) -> Result<Option<EditEvent>, StreamRuntimeError> {
        loop {
            let Some(event) = self.event_source.next_event().await? else {
                return Ok(None);
            };

            if let Some(edit) = self.ingestor.ingest(&event.data)? {
                if let Some(id) = event.id.as_deref() {
                    self.store_checkpoint(id).await?;
                }
                self.delivered_events = self.delivered_events.saturating_add(1);
                return Ok(Some(edit));
            }

            if let Some(id) = event.id.as_deref() {
                self.store_checkpoint(id).await?;
            }
            self.filtered_events = self.filtered_events.saturating_add(1);
        }
    }

    /// Reconnect the underlying event source from the latest persisted cursor.
    ///
    /// # Errors
    ///
    /// Returns [`StreamRuntimeError`] when the checkpoint cannot be read from
    /// storage or the event source rejects the reconnect attempt.
    pub async fn reconnect_from_checkpoint(&mut self) -> Result<(), StreamRuntimeError> {
        let checkpoint = match &self.last_event_id {
            Some(value) => Some(value.clone()),
            None => self.load_checkpoint().await?,
        };

        self.event_source.reconnect(checkpoint.clone()).await?;
        self.reconnect_attempts = self.reconnect_attempts.saturating_add(1);
        self.last_event_id = checkpoint;
        Ok(())
    }

    /// Restore the persisted checkpoint without reconnecting the source.
    ///
    /// # Errors
    ///
    /// Returns [`StreamRuntimeError`] when the checkpoint cannot be read from
    /// storage.
    pub async fn restore_checkpoint(&mut self) -> Result<(), StreamRuntimeError> {
        self.last_event_id = self.load_checkpoint().await?;
        Ok(())
    }

    /// Load the persisted `Last-Event-ID` cursor from storage.
    ///
    /// # Errors
    ///
    /// Returns [`StreamRuntimeError`] when the underlying storage read fails or
    /// the persisted checkpoint is not valid UTF-8.
    pub async fn load_checkpoint(&self) -> Result<Option<String>, StreamRuntimeError> {
        let value = self.storage.get(&self.checkpoint_key).await?;
        decode_checkpoint(value)
    }

    pub fn status(&self) -> StreamRuntimeStatus {
        StreamRuntimeStatus {
            checkpoint_key: self.checkpoint_key.clone(),
            last_event_id: self.last_event_id.clone(),
            delivered_events: self.delivered_events,
            filtered_events: self.filtered_events,
            reconnect_attempts: self.reconnect_attempts,
        }
    }

    #[must_use]
    pub fn checkpoint_key(&self) -> &str {
        &self.checkpoint_key
    }

    #[must_use]
    pub fn last_event_id(&self) -> Option<&str> {
        self.last_event_id.as_deref()
    }

    /// Drain up to `limit` actionable events from the source.
    ///
    /// # Errors
    ///
    /// Returns [`StreamRuntimeError`] when stream consumption or persistence
    /// fails.
    pub async fn drain_actionable_events(
        &mut self,
        limit: usize,
    ) -> Result<Vec<EditEvent>, StreamRuntimeError> {
        let mut events = Vec::new();

        while events.len() < limit {
            match self.next_actionable_event().await? {
                Some(event) => events.push(event),
                None => break,
            }
        }

        Ok(events)
    }

    async fn store_checkpoint(&mut self, event_id: &str) -> Result<(), StreamRuntimeError> {
        self.storage
            .set(self.checkpoint_key.clone(), event_id.as_bytes().to_vec())
            .await?;
        self.last_event_id = Some(event_id.to_string());
        Ok(())
    }
}

fn decode_checkpoint(value: Option<Vec<u8>>) -> Result<Option<String>, StreamRuntimeError> {
    match value {
        Some(bytes) => String::from_utf8(bytes).map(Some).map_err(|error| {
            StreamRuntimeError::Storage(StorageError::Operation {
                message: format!("checkpoint data is not valid UTF-8: {error}"),
            })
        }),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use futures::executor::block_on;

    use super::StreamRuntime;
    use crate::config_parser::parse_wiki_config;
    use crate::errors::{EventSourceError, StorageError};
    use crate::traits::{EventSource, MemoryStorage, ReplayEventSource, Storage};
    use crate::types::ServerSentEvent;

    const CONFIG: &str = include_str!("../../../configs/frwiki.yaml");
    const SAMPLE_EVENT: &str = include_str!("../../../fixtures/frwiki_recentchange_edit.json");

    #[test]
    fn initializes_from_stored_checkpoint() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let source = ReplayEventSource::new([]);
        let storage = MemoryStorage::default();
        block_on(storage.set(
            "stream.last_event_id.frwiki".to_string(),
            b"cursor-123".to_vec(),
        ))
        .expect("checkpoint should store");

        let mut runtime = StreamRuntime::from_config(&config, source, storage);
        block_on(runtime.initialize()).expect("runtime should initialize");

        assert_eq!(
            runtime.status().last_event_id.as_deref(),
            Some("cursor-123")
        );
        assert_eq!(runtime.event_source.last_reconnect_id(), Some("cursor-123"));
    }

    #[test]
    fn streams_until_actionable_event_and_persists_cursor() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let unrelated = SAMPLE_EVENT.replace("\"frwiki\"", "\"enwiki\"");
        let source = ReplayEventSource::new([
            ServerSentEvent {
                event_type: Some("message".to_string()),
                id: Some("event-1".to_string()),
                data: unrelated,
                retry_ms: None,
            },
            ServerSentEvent {
                event_type: Some("message".to_string()),
                id: Some("event-2".to_string()),
                data: SAMPLE_EVENT.to_string(),
                retry_ms: None,
            },
        ]);
        let storage = MemoryStorage::default();
        let mut runtime = StreamRuntime::from_config(&config, source, storage);

        let edit = block_on(runtime.next_actionable_event())
            .expect("stream read should succeed")
            .expect("fixture should yield an actionable event");

        assert_eq!(edit.rev_id, 123_456);
        assert_eq!(runtime.status().filtered_events, 1);
        assert_eq!(runtime.status().delivered_events, 1);
        assert_eq!(runtime.status().last_event_id.as_deref(), Some("event-2"));
        assert_eq!(
            block_on(runtime.storage.get("stream.last_event_id.frwiki"))
                .expect("storage read should succeed"),
            Some(b"event-2".to_vec())
        );
    }

    #[test]
    fn reconnects_from_latest_checkpoint() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let source = ReplayEventSource::new([ServerSentEvent {
            event_type: Some("message".to_string()),
            id: Some("event-9".to_string()),
            data: SAMPLE_EVENT.to_string(),
            retry_ms: None,
        }]);
        let storage = MemoryStorage::default();
        let mut runtime = StreamRuntime::from_config(&config, source, storage);

        block_on(runtime.next_actionable_event()).expect("event should stream");
        block_on(runtime.reconnect_from_checkpoint()).expect("reconnect should work");

        assert_eq!(runtime.status().reconnect_attempts, 1);
        assert_eq!(runtime.event_source.last_reconnect_id(), Some("event-9"));
    }

    #[test]
    fn drains_actionable_events_up_to_limit() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let source = ReplayEventSource::new([
            ServerSentEvent {
                event_type: Some("message".to_string()),
                id: Some("event-1".to_string()),
                data: SAMPLE_EVENT.replace("\"frwiki\"", "\"enwiki\""),
                retry_ms: None,
            },
            ServerSentEvent {
                event_type: Some("message".to_string()),
                id: Some("event-2".to_string()),
                data: SAMPLE_EVENT.to_string(),
                retry_ms: None,
            },
            ServerSentEvent {
                event_type: Some("message".to_string()),
                id: Some("event-3".to_string()),
                data: SAMPLE_EVENT.to_string(),
                retry_ms: None,
            },
        ]);
        let storage = MemoryStorage::default();
        let mut runtime = StreamRuntime::from_config(&config, source, storage);

        let events = block_on(runtime.drain_actionable_events(2)).expect("drain should succeed");

        assert_eq!(events.len(), 2);
        assert_eq!(runtime.status().delivered_events, 2);
        assert_eq!(runtime.status().filtered_events, 1);
        assert_eq!(runtime.last_event_id(), Some("event-3"));
        assert_eq!(
            block_on(runtime.storage.get("stream.last_event_id.frwiki"))
                .expect("storage read should succeed"),
            Some(b"event-3".to_vec())
        );
    }

    #[test]
    fn restore_checkpoint_tracks_persisted_state_without_reconnect() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let source = ReplayEventSource::new([]);
        let storage = MemoryStorage::default();
        block_on(storage.set(
            "stream.last_event_id.frwiki".to_string(),
            b"cursor-555".to_vec(),
        ))
        .expect("checkpoint should store");

        let mut runtime = StreamRuntime::from_config(&config, source, storage);
        block_on(runtime.restore_checkpoint()).expect("checkpoint restore should work");

        assert_eq!(runtime.last_event_id(), Some("cursor-555"));
        assert_eq!(runtime.status().reconnect_attempts, 0);
        assert_eq!(runtime.event_source.last_reconnect_id(), None);
    }

    #[test]
    fn invalid_payload_does_not_advance_checkpoint() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let source = ReplayEventSource::new([ServerSentEvent {
            event_type: Some("message".to_string()),
            id: Some("event-bad".to_string()),
            data: serde_json::json!({
                "wiki": "frwiki",
                "namespace": 0,
                "title": "Broken event",
                "user": "192.0.2.44",
                "timestamp": "2026-03-24T01:02:03Z",
                "bot": false,
                "minor": false,
                "type": "edit",
                "comment": "missing revision object",
                "tags": []
            })
            .to_string(),
            retry_ms: None,
        }]);
        let storage = MemoryStorage::default();
        let mut runtime = StreamRuntime::from_config(&config, source, storage);

        let error =
            block_on(runtime.next_actionable_event()).expect_err("invalid payload should fail");

        assert!(error.to_string().contains("revision object is required"));
        assert!(runtime.last_event_id().is_none());
        assert!(
            block_on(runtime.storage.get("stream.last_event_id.frwiki"))
                .expect("storage read should succeed")
                .is_none()
        );
    }

    #[test]
    fn load_checkpoint_rejects_invalid_utf8() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let source = ReplayEventSource::new([]);
        let storage = MemoryStorage::default();
        block_on(storage.set("stream.last_event_id.frwiki".to_string(), vec![0xff, 0xfe]))
            .expect("checkpoint should store");
        let runtime = StreamRuntime::from_config(&config, source, storage);

        let error =
            block_on(runtime.load_checkpoint()).expect_err("invalid checkpoint should fail");

        assert!(
            error
                .to_string()
                .contains("checkpoint data is not valid UTF-8")
        );
    }

    #[test]
    fn reconnect_from_checkpoint_uses_storage_when_runtime_state_is_empty() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let source = ReplayEventSource::new([]);
        let storage = MemoryStorage::default();
        block_on(storage.set(
            "stream.last_event_id.frwiki".to_string(),
            b"cursor-from-storage".to_vec(),
        ))
        .expect("checkpoint should store");
        let mut runtime = StreamRuntime::from_config(&config, source, storage);

        block_on(runtime.reconnect_from_checkpoint()).expect("reconnect should succeed");

        assert_eq!(runtime.last_event_id(), Some("cursor-from-storage"));
        assert_eq!(
            runtime.event_source.last_reconnect_id(),
            Some("cursor-from-storage")
        );
    }

    #[test]
    fn reconnect_attempts_only_increment_on_success() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let source = FailingReconnectEventSource::default();
        let storage = MemoryStorage::default();
        block_on(storage.set(
            "stream.last_event_id.frwiki".to_string(),
            b"cursor-123".to_vec(),
        ))
        .expect("checkpoint should store");
        let mut runtime = StreamRuntime::from_config(&config, source, storage);

        let error =
            block_on(runtime.reconnect_from_checkpoint()).expect_err("reconnect should fail");

        assert!(error.to_string().contains("simulated reconnect failure"));
        assert_eq!(runtime.status().reconnect_attempts, 0);
        assert!(runtime.last_event_id().is_none());
    }

    #[test]
    fn actionable_event_without_id_preserves_existing_checkpoint() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let source = ReplayEventSource::new([ServerSentEvent {
            event_type: Some("message".to_string()),
            id: None,
            data: SAMPLE_EVENT.to_string(),
            retry_ms: None,
        }]);
        let storage = MemoryStorage::default();
        block_on(storage.set(
            "stream.last_event_id.frwiki".to_string(),
            b"cursor-keep".to_vec(),
        ))
        .expect("checkpoint should store");
        let mut runtime = StreamRuntime::from_config(&config, source, storage);
        block_on(runtime.restore_checkpoint()).expect("restore should succeed");

        let event = block_on(runtime.next_actionable_event())
            .expect("stream read should succeed")
            .expect("actionable event should arrive");

        assert_eq!(event.rev_id, 123_456);
        assert_eq!(runtime.last_event_id(), Some("cursor-keep"));
        assert_eq!(
            block_on(runtime.storage.get("stream.last_event_id.frwiki"))
                .expect("storage read should succeed"),
            Some(b"cursor-keep".to_vec())
        );
    }

    #[test]
    fn storage_write_failure_keeps_runtime_state_unadvanced() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let source = ReplayEventSource::new([ServerSentEvent {
            event_type: Some("message".to_string()),
            id: Some("event-1".to_string()),
            data: SAMPLE_EVENT.to_string(),
            retry_ms: None,
        }]);
        let storage = FailingStorage::fail_set();
        let mut runtime = StreamRuntime::from_config(&config, source, storage);

        let error = block_on(runtime.next_actionable_event()).expect_err("write should fail");

        assert!(error.to_string().contains("simulated set failure"));
        assert!(runtime.last_event_id().is_none());
        assert_eq!(runtime.status().delivered_events, 0);
        assert_eq!(runtime.status().filtered_events, 0);
    }

    #[derive(Debug, Default)]
    struct FailingReconnectEventSource {
        last_reconnect_id: Option<String>,
    }

    #[async_trait]
    impl EventSource for FailingReconnectEventSource {
        async fn next_event(&mut self) -> Result<Option<ServerSentEvent>, EventSourceError> {
            Ok(None)
        }

        async fn reconnect(
            &mut self,
            last_event_id: Option<String>,
        ) -> Result<(), EventSourceError> {
            self.last_reconnect_id = last_event_id;
            Err(EventSourceError::Disconnected {
                message: "simulated reconnect failure".to_string(),
            })
        }
    }

    #[derive(Debug, Default)]
    struct FailingStorage {
        fail_set: bool,
    }

    impl FailingStorage {
        fn fail_set() -> Self {
            Self { fail_set: true }
        }
    }

    #[async_trait]
    impl Storage for FailingStorage {
        async fn get(&self, _key: &str) -> Result<Option<Vec<u8>>, StorageError> {
            Ok(None)
        }

        async fn set(&self, _key: String, _value: Vec<u8>) -> Result<(), StorageError> {
            if self.fail_set {
                Err(StorageError::Operation {
                    message: "simulated set failure".to_string(),
                })
            } else {
                Ok(())
            }
        }

        async fn remove(&self, _key: &str) -> Result<(), StorageError> {
            Ok(())
        }
    }
}
