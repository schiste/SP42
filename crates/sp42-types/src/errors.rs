//! Error contracts for transport, storage, and platform dependency traits.

use thiserror::Error;

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
pub enum ModelClientError {
    #[error("model transport failed: {message}")]
    Transport { message: String },
    #[error("model response was invalid: {message}")]
    InvalidResponse { message: String },
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
