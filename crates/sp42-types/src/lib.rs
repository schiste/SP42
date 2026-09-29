#![forbid(unsafe_code)]

//! Shared SP42 contracts for transport, storage, and platform dependencies.

pub mod errors;
pub mod intake;
pub mod model;
pub mod reason;
pub mod reviewable;
pub mod timestamp;
pub mod traits;
pub mod transport;

pub use errors::{
    EventSourceError, HttpClientError, ModelClientError, ReasonError, StorageError, WebSocketError,
};
pub use intake::{
    IntakeActor, IntakeCondition, IntakeDecision, IntakeField, IntakeFieldRegistry,
    IntakeFieldResolver, IntakeItem, IntakeOp, IntakeOutcome, IntakePipeline, IntakeResolveError,
    IntakeRule, IntakeValue, NoCustomFields, ResolvedField,
};
pub use model::{
    ChatMessage, ChatRole, EndpointMode, ModelClient, ModelCompletion, ModelCompletionRequest,
    ModelEndpointConfig, ModelInvocation, ModelRef, SamplingParams, StubModelClient,
};
pub use reason::{Reason, ReasonCode, ReasonParam, ReasonParams};
pub use reviewable::{
    REVIEWABLE_ITEM_ID_VERSION, ReviewableItemId, ReviewableItemIdError, ReviewableItemKind,
};
pub use timestamp::Timestamp;
pub use traits::{
    Clock, EventSource, FileStorage, FixedClock, HttpClient, LoopbackWebSocket, MemoryStorage,
    ReplayEventSource, Rng, SequenceRng, Storage, StubHttpClient, SystemClock, WebSocket,
    WikiRegistryView,
};
pub use transport::{HttpMethod, HttpRequest, HttpResponse, ServerSentEvent, WebSocketFrame};
