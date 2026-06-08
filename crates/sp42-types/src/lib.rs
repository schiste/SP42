#![forbid(unsafe_code)]

//! Shared SP42 contracts for transport, storage, and platform dependencies.

pub mod errors;
pub mod traits;
pub mod transport;

pub use errors::{EventSourceError, HttpClientError, StorageError, WebSocketError};
pub use traits::{
    Clock, EventSource, FileStorage, FixedClock, HttpClient, LoopbackWebSocket, MemoryStorage,
    ReplayEventSource, Rng, SequenceRng, Storage, StubHttpClient, SystemClock, WebSocket,
};
pub use transport::{HttpMethod, HttpRequest, HttpResponse, ServerSentEvent, WebSocketFrame};
