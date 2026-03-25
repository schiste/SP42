//! Trait boundaries for every external dependency the core needs.

use std::collections::{BTreeMap, VecDeque};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;

use crate::errors::{EventSourceError, HttpClientError, StorageError, WebSocketError};
use crate::types::{HttpRequest, HttpResponse, ServerSentEvent, WebSocketFrame};

#[async_trait]
pub trait HttpClient: Send + Sync {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, HttpClientError>;
}

#[async_trait]
pub trait EventSource: Send {
    async fn next_event(&mut self) -> Result<Option<ServerSentEvent>, EventSourceError>;
    async fn reconnect(&mut self, last_event_id: Option<String>) -> Result<(), EventSourceError>;
}

#[async_trait]
pub trait Storage: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError>;
    async fn set(&self, key: String, value: Vec<u8>) -> Result<(), StorageError>;
    async fn remove(&self, key: &str) -> Result<(), StorageError>;
}

pub trait Clock: Send + Sync {
    fn now_ms(&self) -> i64;
}

pub trait Rng: Send {
    fn next_u64(&mut self) -> u64;
}

#[async_trait]
pub trait WebSocket: Send {
    async fn send(&mut self, frame: WebSocketFrame) -> Result<(), WebSocketError>;
    async fn receive(&mut self) -> Result<Option<WebSocketFrame>, WebSocketError>;
}

#[derive(Debug)]
pub struct StubHttpClient {
    responses: Mutex<VecDeque<Result<HttpResponse, HttpClientError>>>,
}

impl StubHttpClient {
    #[must_use]
    pub fn new<I>(responses: I) -> Self
    where
        I: IntoIterator<Item = Result<HttpResponse, HttpClientError>>,
    {
        Self {
            responses: Mutex::new(responses.into_iter().collect()),
        }
    }
}

#[async_trait]
impl HttpClient for StubHttpClient {
    async fn execute(&self, _request: HttpRequest) -> Result<HttpResponse, HttpClientError> {
        let mut responses = self
            .responses
            .lock()
            .map_err(|_| HttpClientError::StatePoisoned {
                resource: "stub_http_client.responses",
            })?;

        responses.pop_front().unwrap_or_else(|| {
            Err(HttpClientError::InvalidResponse {
                message: "stub client has no queued response".to_string(),
            })
        })
    }
}

#[derive(Debug, Default)]
pub struct ReplayEventSource {
    events: VecDeque<ServerSentEvent>,
    last_reconnect_id: Option<String>,
}

impl ReplayEventSource {
    #[must_use]
    pub fn new<I>(events: I) -> Self
    where
        I: IntoIterator<Item = ServerSentEvent>,
    {
        Self {
            events: events.into_iter().collect(),
            last_reconnect_id: None,
        }
    }

    #[must_use]
    pub fn last_reconnect_id(&self) -> Option<&str> {
        self.last_reconnect_id.as_deref()
    }
}

#[async_trait]
impl EventSource for ReplayEventSource {
    async fn next_event(&mut self) -> Result<Option<ServerSentEvent>, EventSourceError> {
        Ok(self.events.pop_front())
    }

    async fn reconnect(&mut self, last_event_id: Option<String>) -> Result<(), EventSourceError> {
        self.last_reconnect_id = last_event_id;
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct MemoryStorage {
    values: Mutex<BTreeMap<String, Vec<u8>>>,
}

#[async_trait]
impl Storage for MemoryStorage {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let values = self
            .values
            .lock()
            .map_err(|_| StorageError::StatePoisoned {
                resource: "memory_storage.values",
            })?;

        Ok(values.get(key).cloned())
    }

    async fn set(&self, key: String, value: Vec<u8>) -> Result<(), StorageError> {
        let mut values = self
            .values
            .lock()
            .map_err(|_| StorageError::StatePoisoned {
                resource: "memory_storage.values",
            })?;

        values.insert(key, value);
        Ok(())
    }

    async fn remove(&self, key: &str) -> Result<(), StorageError> {
        let mut values = self
            .values
            .lock()
            .map_err(|_| StorageError::StatePoisoned {
                resource: "memory_storage.values",
            })?;

        values.remove(key);
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStorage {
    root: PathBuf,
}

static FILE_STORAGE_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

impl FileStorage {
    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn key_path(&self, key: &str) -> PathBuf {
        let mut encoded = String::with_capacity(key.len() * 2);
        for byte in key.as_bytes() {
            let _ = write!(&mut encoded, "{byte:02x}");
        }
        self.root.join(format!("{encoded}.bin"))
    }
}

#[async_trait]
impl Storage for FileStorage {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let path = self.key_path(key);
        match fs::read(path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(StorageError::Operation {
                message: error.to_string(),
            }),
        }
    }

    async fn set(&self, key: String, value: Vec<u8>) -> Result<(), StorageError> {
        fs::create_dir_all(&self.root).map_err(|error| StorageError::Operation {
            message: error.to_string(),
        })?;
        let target = self.key_path(&key);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| StorageError::Operation {
                message: error.to_string(),
            })?
            .as_nanos();
        let sequence = FILE_STORAGE_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temp_path = self.root.join(format!(
            ".{}.{}.{}.tmp",
            target
                .file_name()
                .map_or_else(|| "checkpoint".into(), |name| name.to_string_lossy()),
            nanos,
            sequence
        ));
        fs::write(&temp_path, value).map_err(|error| StorageError::Operation {
            message: error.to_string(),
        })?;
        fs::rename(temp_path, target).map_err(|error| StorageError::Operation {
            message: error.to_string(),
        })
    }

    async fn remove(&self, key: &str) -> Result<(), StorageError> {
        let path = self.key_path(key);
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(StorageError::Operation {
                message: error.to_string(),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedClock {
    now_ms: i64,
}

impl FixedClock {
    #[must_use]
    pub const fn new(now_ms: i64) -> Self {
        Self { now_ms }
    }
}

impl Clock for FixedClock {
    fn now_ms(&self) -> i64 {
        self.now_ms
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceRng {
    values: VecDeque<u64>,
}

impl SequenceRng {
    #[must_use]
    pub fn new<I>(values: I) -> Self
    where
        I: IntoIterator<Item = u64>,
    {
        Self {
            values: values.into_iter().collect(),
        }
    }
}

impl Rng for SequenceRng {
    fn next_u64(&mut self) -> u64 {
        self.values.pop_front().unwrap_or(0)
    }
}

#[derive(Debug, Default)]
pub struct LoopbackWebSocket {
    incoming: VecDeque<WebSocketFrame>,
    outgoing: Vec<WebSocketFrame>,
}

impl LoopbackWebSocket {
    #[must_use]
    pub fn with_incoming<I>(frames: I) -> Self
    where
        I: IntoIterator<Item = WebSocketFrame>,
    {
        Self {
            incoming: frames.into_iter().collect(),
            outgoing: Vec::new(),
        }
    }

    #[must_use]
    pub fn sent_frames(&self) -> &[WebSocketFrame] {
        &self.outgoing
    }
}

#[async_trait]
impl WebSocket for LoopbackWebSocket {
    async fn send(&mut self, frame: WebSocketFrame) -> Result<(), WebSocketError> {
        self.outgoing.push(frame);
        Ok(())
    }

    async fn receive(&mut self) -> Result<Option<WebSocketFrame>, WebSocketError> {
        Ok(self.incoming.pop_front())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use futures::executor::block_on;
    use url::Url;

    use super::{
        Clock, EventSource, FileStorage, FixedClock, HttpClient, LoopbackWebSocket, MemoryStorage,
        ReplayEventSource, Rng, SequenceRng, Storage, StubHttpClient, WebSocket,
    };
    use crate::types::{HttpMethod, HttpRequest, HttpResponse, ServerSentEvent, WebSocketFrame};

    #[test]
    fn fixed_clock_is_deterministic() {
        let clock = FixedClock::new(42);
        assert_eq!(clock.now_ms(), 42);
    }

    #[test]
    fn sequence_rng_returns_seeded_values() {
        let mut rng = SequenceRng::new([7, 9]);
        assert_eq!(rng.next_u64(), 7);
        assert_eq!(rng.next_u64(), 9);
        assert_eq!(rng.next_u64(), 0);
    }

    #[test]
    fn memory_storage_round_trips_values() {
        let storage = MemoryStorage::default();

        block_on(storage.set("key".to_string(), b"value".to_vec())).expect("set should succeed");
        let value = block_on(storage.get("key")).expect("get should succeed");

        assert_eq!(value, Some(b"value".to_vec()));
    }

    #[test]
    fn file_storage_round_trips_values() {
        let root = std::env::temp_dir().join(format!(
            "sp42-file-storage-{}-{}",
            std::process::id(),
            FixedClock::new(42).now_ms()
        ));
        let storage = FileStorage::new(root.clone());

        block_on(storage.set("key".to_string(), b"value".to_vec())).expect("set should succeed");
        let value = block_on(storage.get("key")).expect("get should succeed");

        assert_eq!(value, Some(b"value".to_vec()));

        block_on(storage.remove("key")).expect("remove should succeed");
        assert_eq!(
            block_on(storage.get("key")).expect("get after remove should succeed"),
            None
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn replay_event_source_tracks_reconnect_cursor() {
        let event = ServerSentEvent {
            event_type: Some("message".to_string()),
            id: Some("abc".to_string()),
            data: "{}".to_string(),
            retry_ms: None,
        };
        let mut source = ReplayEventSource::new([event.clone()]);

        let first = block_on(source.next_event()).expect("read should succeed");
        block_on(source.reconnect(Some("abc".to_string()))).expect("reconnect should succeed");

        assert_eq!(first, Some(event));
        assert_eq!(source.last_reconnect_id(), Some("abc"));
    }

    #[test]
    fn stub_http_client_returns_queued_response() {
        let client = StubHttpClient::new([Ok(HttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: br#"{"ok":true}"#.to_vec(),
        })]);
        let request = HttpRequest {
            method: HttpMethod::Get,
            url: Url::parse("https://fr.wikipedia.org/w/api.php").expect("url fixture is valid"),
            headers: BTreeMap::new(),
            body: Vec::new(),
        };

        let response = block_on(client.execute(request)).expect("request should succeed");

        assert_eq!(response.status, 200);
    }

    #[test]
    fn loopback_websocket_records_sent_frames() {
        let mut socket =
            LoopbackWebSocket::with_incoming([WebSocketFrame::Text("hello".to_string())]);

        block_on(socket.send(WebSocketFrame::Text("claim".to_string())))
            .expect("send should succeed");
        let received = block_on(socket.receive()).expect("receive should succeed");

        assert_eq!(received, Some(WebSocketFrame::Text("hello".to_string())));
        assert_eq!(
            socket.sent_frames(),
            &[WebSocketFrame::Text("claim".to_string())]
        );
    }
}
