//! Compatibility re-exports for SP42 platform dependency traits.

pub use sp42_types::{
    Clock, EventSource, FileStorage, FixedClock, HttpClient, LoopbackWebSocket, MemoryStorage,
    ReplayEventSource, Rng, SequenceRng, Storage, StubHttpClient, SystemClock, WebSocket,
};
pub use crate::wikitext_editor::{ScriptedWikitextEditor, WikitextEditor};
