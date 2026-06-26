#![forbid(unsafe_code)]

//! Shared SP42 contracts for transport, storage, and platform dependencies.

pub mod errors;
pub mod model;
pub mod traits;
pub mod transport;

pub use errors::{
    EventSourceError, HttpClientError, ModelClientError, StorageError, WebSocketError,
};
pub use model::{
    ChatMessage, ChatRole, EndpointMode, ModelClient, ModelCompletion, ModelCompletionRequest,
    ModelEndpointConfig, ModelInvocation, ModelRef, SamplingParams, StubModelClient,
};
pub use traits::{
    Clock, EventSource, FileStorage, FixedClock, HttpClient, LoopbackWebSocket, MemoryStorage,
    ReplayEventSource, Rng, SequenceRng, Storage, StubHttpClient, SystemClock, WebSocket,
};
pub use transport::{HttpMethod, HttpRequest, HttpResponse, ServerSentEvent, WebSocketFrame};
