//! Shared recentchanges backlog paging runtime.

use serde::{Deserialize, Serialize};

use crate::errors::BacklogRuntimeError;
use crate::errors::StorageError;
use crate::recent_changes::{
    RecentChangesBatch, RecentChangesQuery, build_recent_changes_request, execute_recent_changes,
    normalize_continue_token,
};
use crate::traits::{HttpClient, Storage};
use crate::types::{HttpRequest, WikiConfig};

const DEFAULT_BACKLOG_KEY_PREFIX: &str = "recentchanges.rccontinue";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BacklogRuntimeStatus {
    pub checkpoint_key: String,
    pub next_continue: Option<String>,
    pub last_batch_size: usize,
    pub total_events: u64,
    pub poll_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BacklogRuntimeConfig {
    pub limit: u16,
    pub include_bots: bool,
}

#[derive(Debug)]
pub struct BacklogRuntime<S> {
    storage: S,
    wiki_config: WikiConfig,
    config: BacklogRuntimeConfig,
    checkpoint_key: String,
    next_continue: Option<String>,
    last_batch_size: usize,
    total_events: u64,
    poll_count: u64,
}

impl<S> BacklogRuntime<S>
where
    S: Storage,
{
    #[must_use]
    pub fn from_config(wiki_config: &WikiConfig, storage: S, config: BacklogRuntimeConfig) -> Self {
        Self::new(
            wiki_config.clone(),
            storage,
            config,
            format!("{}.{}", DEFAULT_BACKLOG_KEY_PREFIX, wiki_config.wiki_id),
        )
    }

    #[must_use]
    pub fn new(
        wiki_config: WikiConfig,
        storage: S,
        config: BacklogRuntimeConfig,
        checkpoint_key: String,
    ) -> Self {
        Self {
            storage,
            wiki_config,
            config,
            checkpoint_key,
            next_continue: None,
            last_batch_size: 0,
            total_events: 0,
            poll_count: 0,
        }
    }

    /// Load the persisted `rccontinue` checkpoint into the runtime state.
    ///
    /// # Errors
    ///
    /// Returns [`BacklogRuntimeError`] when the storage read fails.
    pub async fn initialize(&mut self) -> Result<(), BacklogRuntimeError> {
        self.next_continue = self.load_checkpoint().await?;
        Ok(())
    }

    #[must_use]
    pub fn checkpoint_key(&self) -> &str {
        &self.checkpoint_key
    }

    #[must_use]
    pub fn current_checkpoint(&self) -> Option<&str> {
        self.next_continue.as_deref()
    }

    #[must_use]
    pub fn query(&self) -> RecentChangesQuery {
        RecentChangesQuery {
            limit: self.config.limit,
            rccontinue: self.next_continue.clone(),
            include_bots: self.config.include_bots,
            unpatrolled_only: false,
            include_minor: true,
            namespace_override: None,
        }
    }

    /// Build the next recentchanges request using the current persisted cursor.
    ///
    /// # Errors
    ///
    /// Returns [`BacklogRuntimeError`] when request construction fails.
    pub fn build_next_request(&self) -> Result<HttpRequest, BacklogRuntimeError> {
        Ok(build_recent_changes_request(
            &self.wiki_config,
            &self.query(),
        )?)
    }

    /// Poll the `MediaWiki` API through the injected HTTP client and persist the
    /// returned `rccontinue` checkpoint.
    ///
    /// # Errors
    ///
    /// Returns [`BacklogRuntimeError`] when the request fails, the response is
    /// invalid, or checkpoint persistence fails.
    pub async fn poll<C>(&mut self, client: &C) -> Result<RecentChangesBatch, BacklogRuntimeError>
    where
        C: HttpClient + ?Sized,
    {
        let batch = execute_recent_changes(client, &self.wiki_config, &self.query()).await?;
        self.apply_batch(&batch).await?;
        Ok(batch)
    }

    /// Apply a parsed batch and update the persisted `rccontinue` checkpoint.
    ///
    /// # Errors
    ///
    /// Returns [`BacklogRuntimeError`] when storage cannot persist or clear the
    /// updated checkpoint.
    pub async fn apply_batch(
        &mut self,
        batch: &RecentChangesBatch,
    ) -> Result<(), BacklogRuntimeError> {
        let next_continue = batch.next_continue.clone();

        if let Some(token) = &next_continue {
            self.storage
                .set(self.checkpoint_key.clone(), token.as_bytes().to_vec())
                .await?;
        } else {
            self.storage.remove(&self.checkpoint_key).await?;
        }

        self.last_batch_size = batch.events.len();
        self.total_events = self
            .total_events
            .saturating_add(u64::try_from(batch.events.len()).unwrap_or(u64::MAX));
        self.poll_count = self.poll_count.saturating_add(1);
        self.next_continue = next_continue;

        Ok(())
    }

    /// Remove any persisted `rccontinue` value and clear local runtime state.
    ///
    /// # Errors
    ///
    /// Returns [`BacklogRuntimeError`] when the storage removal fails.
    pub async fn reset_checkpoint(&mut self) -> Result<(), BacklogRuntimeError> {
        self.storage.remove(&self.checkpoint_key).await?;
        self.next_continue = None;
        Ok(())
    }

    /// Load the persisted `rccontinue` token from storage.
    ///
    /// # Errors
    ///
    /// Returns [`BacklogRuntimeError`] when the storage read fails.
    pub async fn load_checkpoint(&self) -> Result<Option<String>, BacklogRuntimeError> {
        let value = self.storage.get(&self.checkpoint_key).await?;
        match value {
            Some(bytes) => {
                let token = String::from_utf8(bytes).map_err(|error| {
                    BacklogRuntimeError::Storage(StorageError::Operation {
                        message: format!("checkpoint data is not valid UTF-8: {error}"),
                    })
                })?;
                normalize_continue_token(Some(&token)).map_err(BacklogRuntimeError::from)
            }
            None => Ok(None),
        }
    }

    #[must_use]
    pub fn status(&self) -> BacklogRuntimeStatus {
        BacklogRuntimeStatus {
            checkpoint_key: self.checkpoint_key.clone(),
            next_continue: self.next_continue.clone(),
            last_batch_size: self.last_batch_size,
            total_events: self.total_events,
            poll_count: self.poll_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use futures::executor::block_on;

    use super::{BacklogRuntime, BacklogRuntimeConfig};
    use crate::config_parser::parse_wiki_config;
    use crate::errors::StorageError;
    use crate::recent_changes::RecentChangesBatch;
    use crate::traits::{MemoryStorage, Storage, StubHttpClient};
    use crate::types::{EditEvent, EditorIdentity, HttpResponse};

    const CONFIG: &str = include_str!("../../../configs/frwiki.yaml");

    #[test]
    fn initializes_from_persisted_rccontinue() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let storage = MemoryStorage::default();
        block_on(storage.set(
            "recentchanges.rccontinue.frwiki".to_string(),
            b"20260324010101|123".to_vec(),
        ))
        .expect("checkpoint should store");

        let mut runtime = BacklogRuntime::from_config(
            &config,
            storage,
            BacklogRuntimeConfig {
                limit: 10,
                include_bots: false,
            },
        );
        block_on(runtime.initialize()).expect("runtime should initialize");

        let request = runtime.build_next_request().expect("request should build");
        assert!(
            request
                .url
                .query()
                .expect("query should exist")
                .contains("rccontinue=20260324010101%7C123")
        );
    }

    #[test]
    fn initialize_rejects_invalid_utf8_checkpoint() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let storage = MemoryStorage::default();
        block_on(storage.set(
            "recentchanges.rccontinue.frwiki".to_string(),
            vec![0xff, 0xfe],
        ))
        .expect("checkpoint should store");

        let mut runtime = BacklogRuntime::from_config(
            &config,
            storage,
            BacklogRuntimeConfig {
                limit: 10,
                include_bots: false,
            },
        );

        let error = block_on(runtime.initialize()).expect_err("initialize should fail");

        assert!(
            error
                .to_string()
                .contains("checkpoint data is not valid UTF-8")
        );
    }

    #[test]
    fn initialize_rejects_blank_persisted_checkpoint() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let storage = MemoryStorage::default();
        block_on(storage.set(
            "recentchanges.rccontinue.frwiki".to_string(),
            b"   ".to_vec(),
        ))
        .expect("checkpoint should store");

        let mut runtime = BacklogRuntime::from_config(
            &config,
            storage,
            BacklogRuntimeConfig {
                limit: 10,
                include_bots: false,
            },
        );

        let error = block_on(runtime.initialize()).expect_err("initialize should fail");

        assert!(error.to_string().contains("rccontinue must not be empty"));
    }

    #[test]
    fn poll_updates_checkpoint_and_metrics() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let storage = MemoryStorage::default();
        let client = StubHttpClient::new([Ok(HttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: serde_json::json!({
                "continue": { "rccontinue": "20260324010202|456" },
                "query": {
                    "recentchanges": [{
                        "type": "edit",
                        "title": "Exemple",
                        "ns": 0,
                        "revid": 123_456,
                        "old_revid": 123_455,
                        "user": "192.0.2.10",
                        "timestamp": "2026-03-24T01:02:03Z",
                        "bot": false,
                        "minor": false,
                        "new": false,
                        "oldlen": 120,
                        "newlen": 90,
                        "comment": "demo",
                        "tags": ["mw-reverted"]
                    }]
                }
            })
            .to_string()
            .into_bytes(),
        })]);
        let mut runtime = BacklogRuntime::from_config(
            &config,
            storage,
            BacklogRuntimeConfig {
                limit: 10,
                include_bots: false,
            },
        );

        let batch = block_on(runtime.poll(&client)).expect("poll should succeed");

        assert_eq!(batch.events.len(), 1);
        assert_eq!(runtime.status().last_batch_size, 1);
        assert_eq!(runtime.status().poll_count, 1);
        assert_eq!(
            runtime.status().next_continue.as_deref(),
            Some("20260324010202|456")
        );
        assert_eq!(
            block_on(runtime.load_checkpoint()).expect("checkpoint should load"),
            Some("20260324010202|456".to_string())
        );
    }

    #[test]
    fn apply_batch_does_not_mutate_state_when_storage_set_fails() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let storage = FailingStorage::fail_set();
        let mut runtime = BacklogRuntime::from_config(
            &config,
            storage,
            BacklogRuntimeConfig {
                limit: 25,
                include_bots: false,
            },
        );

        let error = block_on(runtime.apply_batch(&RecentChangesBatch {
            events: vec![sample_event()],
            next_continue: Some("token-1".to_string()),
        }))
        .expect_err("batch should fail");

        assert!(error.to_string().contains("storage failed"));
        assert_eq!(runtime.status().poll_count, 0);
        assert_eq!(runtime.status().last_batch_size, 0);
        assert!(runtime.current_checkpoint().is_none());
        assert!(
            block_on(runtime.load_checkpoint())
                .expect("checkpoint should load")
                .is_none()
        );
    }

    #[test]
    fn apply_batch_does_not_mutate_state_when_storage_remove_fails() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let storage = FailingStorage::fail_remove();
        let mut runtime = BacklogRuntime::from_config(
            &config,
            storage,
            BacklogRuntimeConfig {
                limit: 25,
                include_bots: false,
            },
        );

        let error = block_on(runtime.apply_batch(&RecentChangesBatch {
            events: vec![sample_event()],
            next_continue: None,
        }))
        .expect_err("batch should fail");

        assert!(error.to_string().contains("storage failed"));
        assert_eq!(runtime.status().poll_count, 0);
        assert_eq!(runtime.status().last_batch_size, 0);
        assert!(runtime.current_checkpoint().is_none());
    }

    #[test]
    fn reset_checkpoint_clears_state() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let storage = MemoryStorage::default();
        let mut runtime = BacklogRuntime::from_config(
            &config,
            storage,
            BacklogRuntimeConfig {
                limit: 25,
                include_bots: false,
            },
        );

        block_on(runtime.apply_batch(&RecentChangesBatch {
            events: Vec::new(),
            next_continue: Some("token-1".to_string()),
        }))
        .expect("batch should apply");
        block_on(runtime.reset_checkpoint()).expect("reset should succeed");

        assert!(runtime.status().next_continue.is_none());
        assert!(
            block_on(runtime.load_checkpoint())
                .expect("checkpoint should load")
                .is_none()
        );
    }

    #[test]
    fn load_checkpoint_rejects_invalid_utf8() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let storage = MemoryStorage::default();
        block_on(storage.set(
            "recentchanges.rccontinue.frwiki".to_string(),
            vec![0xff, 0xfe, 0xfd],
        ))
        .expect("checkpoint should store");

        let runtime = BacklogRuntime::from_config(
            &config,
            storage,
            BacklogRuntimeConfig {
                limit: 25,
                include_bots: false,
            },
        );

        let error = block_on(runtime.load_checkpoint()).expect_err("checkpoint should fail");

        assert!(
            error
                .to_string()
                .contains("checkpoint data is not valid UTF-8")
        );
    }

    #[test]
    fn query_reflects_current_checkpoint_state() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let storage = MemoryStorage::default();
        let mut runtime = BacklogRuntime::from_config(
            &config,
            storage,
            BacklogRuntimeConfig {
                limit: 25,
                include_bots: true,
            },
        );

        assert!(runtime.query().is_initial_poll());
        assert_eq!(runtime.checkpoint_key(), "recentchanges.rccontinue.frwiki");
        assert!(runtime.current_checkpoint().is_none());

        block_on(runtime.apply_batch(&RecentChangesBatch {
            events: vec![sample_event()],
            next_continue: Some("token-2".to_string()),
        }))
        .expect("batch should apply");

        assert_eq!(runtime.current_checkpoint(), Some("token-2"));
        assert_eq!(runtime.query().rccontinue.as_deref(), Some("token-2"));
    }

    #[test]
    fn poll_does_not_mutate_state_when_recentchanges_execution_fails() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let storage = MemoryStorage::default();
        let client = StubHttpClient::new([Ok(HttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: br#"{"query":{"recentchanges":[{"timestamp":"invalid"}]}}"#.to_vec(),
        })]);
        let mut runtime = BacklogRuntime::from_config(
            &config,
            storage,
            BacklogRuntimeConfig {
                limit: 10,
                include_bots: false,
            },
        );

        let error = block_on(runtime.poll(&client)).expect_err("poll should fail");

        assert!(error.to_string().contains("serialization failed"));
        assert_eq!(runtime.status().poll_count, 0);
        assert_eq!(runtime.status().total_events, 0);
        assert!(runtime.current_checkpoint().is_none());
    }

    #[test]
    fn batch_without_continue_clears_stale_checkpoint_and_updates_metrics() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let storage = MemoryStorage::default();
        block_on(storage.set(
            "recentchanges.rccontinue.frwiki".to_string(),
            b"stale-token".to_vec(),
        ))
        .expect("checkpoint should store");

        let mut runtime = BacklogRuntime::from_config(
            &config,
            storage,
            BacklogRuntimeConfig {
                limit: 25,
                include_bots: true,
            },
        );
        block_on(runtime.initialize()).expect("initialize should succeed");
        block_on(runtime.apply_batch(&RecentChangesBatch {
            events: vec![sample_event(), sample_event()],
            next_continue: None,
        }))
        .expect("batch should apply");

        assert!(runtime.current_checkpoint().is_none());
        assert_eq!(runtime.status().last_batch_size, 2);
        assert_eq!(runtime.status().total_events, 2);
        assert_eq!(runtime.status().poll_count, 1);
        assert!(
            block_on(runtime.load_checkpoint())
                .expect("checkpoint should load")
                .is_none()
        );
    }

    #[derive(Debug, Default)]
    struct FailingStorage {
        fail_set: bool,
        fail_remove: bool,
    }

    impl FailingStorage {
        fn fail_set() -> Self {
            Self {
                fail_set: true,
                fail_remove: false,
            }
        }

        fn fail_remove() -> Self {
            Self {
                fail_set: false,
                fail_remove: true,
            }
        }
    }

    #[async_trait::async_trait]
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
            if self.fail_remove {
                Err(StorageError::Operation {
                    message: "simulated remove failure".to_string(),
                })
            } else {
                Ok(())
            }
        }
    }

    fn sample_event() -> EditEvent {
        EditEvent {
            wiki_id: "frwiki".to_string(),
            title: "Example".to_string(),
            namespace: 0,
            rev_id: 123_456,
            old_rev_id: Some(123_455),
            performer: EditorIdentity::Anonymous {
                label: "192.0.2.1".to_string(),
            },
            timestamp_ms: 1_774_366_920_000,
            is_bot: false,
            is_minor: false,
            is_new_page: false,
            tags: vec!["mw-reverted".to_string()],
            comment: Some("demo".to_string()),
            byte_delta: 20,
            is_patrolled: false,
        }
    }
}
