//! Coordination protocol payloads and room summary contracts.

use serde::{Deserialize, Serialize};
use sp42_platform::{Action, ReviewSessionSnapshot};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CoordinationMessage {
    ActionBroadcast(ActionBroadcast),
    EditClaim(EditClaim),
    ScoreDelta(ScoreDelta),
    PresenceHeartbeat(PresenceHeartbeat),
    FlaggedEdit(FlaggedEdit),
    RaceResolution(RaceResolution),
    ReviewSignal(ReviewSignal),
}

/// Server-originated hint that a review session changed (PRD-0017,
/// ADR-0018 §8): a live re-fetch cue for operator panels watching the
/// wiki's room. Advisory by design — the review-session store behind the
/// gated `/dev/review` routes stays the source of truth, the reducer
/// relays this kind without folding any room state, and a spoofed signal
/// can only cause a harmless re-fetch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewSignal {
    pub wiki_id: String,
    /// Session summary so a panel can badge (status, pending prompts,
    /// findings count) without a round-trip. Never authoritative.
    pub session: ReviewSessionSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionBroadcast {
    pub wiki_id: String,
    pub rev_id: u64,
    pub action: Action,
    pub actor: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditClaim {
    pub wiki_id: String,
    pub rev_id: u64,
    pub actor: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScoreDelta {
    pub wiki_id: String,
    pub rev_id: u64,
    pub delta: i32,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresenceHeartbeat {
    pub wiki_id: String,
    pub actor: String,
    pub active_edit_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlaggedEdit {
    pub wiki_id: String,
    pub rev_id: u64,
    pub score: i32,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RaceResolution {
    pub wiki_id: String,
    pub rev_id: u64,
    pub winning_actor: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoordinationRoomSummary {
    pub wiki_id: String,
    pub connected_clients: u32,
    pub published_messages: u64,
    pub claim_count: usize,
    pub presence_count: usize,
    pub flagged_edit_count: usize,
    pub score_delta_count: usize,
    pub race_resolution_count: usize,
    pub recent_action_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CoordinationSnapshot {
    pub rooms: Vec<CoordinationRoomSummary>,
}
