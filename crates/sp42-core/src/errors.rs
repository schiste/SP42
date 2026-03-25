//! Domain-specific error types used across the core library.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum StreamIngestorError {
    #[error("stream event is not valid recentchange payload: {message}")]
    InvalidPayload { message: String },
    #[error("stream event serialization failed: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Error)]
pub enum StreamRuntimeError {
    #[error(transparent)]
    EventSource(#[from] EventSourceError),
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(transparent)]
    Ingestor(#[from] StreamIngestorError),
}

#[derive(Debug, Error)]
pub enum ScoringError {
    #[error("scoring failed: {message}")]
    Computation { message: String },
}

#[derive(Debug, Error)]
pub enum RecentChangesError {
    #[error("recentchanges request is invalid: {message}")]
    InvalidRequest { message: String },
    #[error("recentchanges response is invalid: {message}")]
    InvalidResponse { message: String },
    #[error("recentchanges serialization failed: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Error)]
pub enum BacklogRuntimeError {
    #[error(transparent)]
    RecentChanges(#[from] RecentChangesError),
    #[error(transparent)]
    Storage(#[from] StorageError),
}

#[derive(Debug, Error)]
pub enum LiftWingError {
    #[error("liftwing request is invalid: {message}")]
    InvalidRequest { message: String },
    #[error("liftwing response is invalid: {message}")]
    InvalidResponse { message: String },
    #[error("liftwing serialization failed: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Error)]
pub enum DiffError {
    #[error("diff generation failed: {message}")]
    Computation { message: String },
}

#[derive(Debug, Error)]
pub enum ActionError {
    #[error("wiki action failed: {message}")]
    Execution {
        message: String,
        code: Option<String>,
        http_status: Option<u16>,
        retryable: bool,
    },
}

#[derive(Debug, Error)]
pub enum UserAnalysisError {
    #[error("user analysis failed: {message}")]
    Analysis { message: String },
}

#[derive(Debug, Error)]
pub enum TrainingDataError {
    #[error("training data serialization failed: {0}")]
    Serialize(#[from] serde_json::Error),
}

#[derive(Debug, Error)]
pub enum ReviewWorkbenchError {
    #[error(transparent)]
    Action(#[from] ActionError),
    #[error(transparent)]
    Training(#[from] TrainingDataError),
    #[error("review workbench is incomplete: {message}")]
    Incomplete { message: String },
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("configuration is not valid YAML: {0}")]
    InvalidYaml(#[from] serde_yaml::Error),
    #[error("configuration field `{field}` is invalid: {message}")]
    InvalidField {
        field: &'static str,
        message: String,
    },
    #[error("configuration namespace_allowlist contains duplicate namespace {namespace}")]
    DuplicateNamespace { namespace: i32 },
}

#[derive(Debug, Error)]
pub enum OAuthError {
    #[error("oauth configuration is invalid: {message}")]
    InvalidConfig { message: String },
    #[error("pkce verifier is invalid: {message}")]
    InvalidVerifier { message: String },
    #[error("oauth callback is invalid: {message}")]
    InvalidCallback { message: String },
    #[error("oauth callback state mismatch")]
    StateMismatch,
    #[error("oauth authorization failed: {message}")]
    AuthorizationFailed { message: String },
}

#[derive(Debug, Error)]
pub enum DevAuthError {
    #[error("dev auth configuration is invalid: {message}")]
    InvalidConfig { message: String },
    #[error("dev auth payload is invalid: {message}")]
    InvalidPayload { message: String },
}

#[derive(Debug, Error)]
pub enum CoordinationError {
    #[error(transparent)]
    Codec(#[from] CodecError),
    #[error(transparent)]
    WebSocket(#[from] WebSocketError),
    #[error("unexpected websocket frame: {message}")]
    InvalidFrame { message: String },
}

#[derive(Debug, Error)]
pub enum CodecError {
    #[error("message pack encode failed: {0}")]
    Encode(#[from] rmp_serde::encode::Error),
    #[error("message pack decode failed: {0}")]
    Decode(#[from] rmp_serde::decode::Error),
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum HttpClientError {
    #[error("transport failed: {message}")]
    Transport { message: String },
    #[error("response was invalid: {message}")]
    InvalidResponse { message: String },
    #[error("stub state is poisoned: {resource}")]
    StatePoisoned { resource: &'static str },
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum EventSourceError {
    #[error("event source disconnected: {message}")]
    Disconnected { message: String },
    #[error("stub state is poisoned: {resource}")]
    StatePoisoned { resource: &'static str },
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum StorageError {
    #[error("storage failed: {message}")]
    Operation { message: String },
    #[error("stub state is poisoned: {resource}")]
    StatePoisoned { resource: &'static str },
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum WebSocketError {
    #[error("websocket failed: {message}")]
    Transport { message: String },
    #[error("stub state is poisoned: {resource}")]
    StatePoisoned { resource: &'static str },
}

#[derive(Debug, Error)]
pub enum WikiStorageError {
    #[error("wiki storage input is invalid: {message}")]
    InvalidInput { message: String },
    #[error("wiki storage serialization failed: {message}")]
    Serialize { message: String },
    #[error("wiki storage transport failed: {message}")]
    Transport { message: String },
    #[error("wiki storage write conflict on `{title}`: {message}")]
    Conflict { title: String, message: String },
}
