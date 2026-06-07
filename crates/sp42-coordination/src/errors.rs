//! Coordination-specific error types.

use sp42_types::WebSocketError;
use thiserror::Error;

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
