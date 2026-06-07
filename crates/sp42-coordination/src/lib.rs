#![forbid(unsafe_code)]

//! Shared SP42 coordination protocol, codec, reducer, and client runtime.
//!
//! This crate owns the platform-independent collaboration contract:
//! message payloads, `MessagePack` encoding, deterministic room-state
//! reduction, and client-side optimistic runtime behavior.
//!
//! Server-owned concerns stay in `sp42-server`: Axum websocket upgrades,
//! broadcast-channel fanout, authenticated actor rewriting, room eviction, and
//! inspection metrics tied to live server process state.

pub mod client;
pub mod codec;
pub mod errors;
pub mod messages;
pub mod runtime;
pub mod state;

pub use client::CoordinationClient;
pub use codec::{decode_message, encode_message};
pub use errors::{CodecError, CoordinationError};
pub use messages::{
    ActionBroadcast, CoordinationMessage, CoordinationRoomSummary, CoordinationSnapshot, EditClaim,
    FlaggedEdit, PresenceHeartbeat, RaceResolution, ScoreDelta,
};
pub use runtime::{CoordinationRuntime, CoordinationRuntimeStatus};
pub use state::{CoordinationState, CoordinationStateSummary};
