use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::{RwLock, broadcast};

use sp42_core::{
    CoordinationMessage, CoordinationRoomSummary, CoordinationSnapshot, CoordinationState,
    CoordinationStateSummary, decode_message,
};

const ROOM_CAPACITY: usize = 128;
const PRESENCE_STALE_AFTER_MS: i64 = 60_000;
const ROOM_IDLE_EVICT_AFTER_MS: i64 = 5 * 60_000;

#[derive(Debug, Clone, Default)]
pub struct CoordinationRegistry {
    rooms: Arc<RwLock<HashMap<String, CoordinationRoomState>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinationEnvelope {
    pub sender_id: u64,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CoordinationRoomMetrics {
    pub last_activity_ms: Option<i64>,
    pub published_messages: u64,
    pub accepted_messages: u64,
    pub invalid_messages: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CoordinationRoomInspection {
    pub room: CoordinationRoomSummary,
    pub state: Option<CoordinationStateSummary>,
    pub metrics: CoordinationRoomMetrics,
}

#[derive(Debug, Clone)]
struct CoordinationRoomState {
    sender: broadcast::Sender<CoordinationEnvelope>,
    state: CoordinationState,
    presence_last_seen_ms: HashMap<String, i64>,
    connected_clients: u32,
    published_messages: u64,
    accepted_messages: u64,
    invalid_messages: u64,
    last_activity_ms: Option<i64>,
}

impl CoordinationRegistry {
    pub async fn subscribe(&self, wiki_id: &str) -> broadcast::Receiver<CoordinationEnvelope> {
        let sender = self.room_sender(wiki_id).await;
        sender.subscribe()
    }

    pub async fn publish(&self, wiki_id: &str, envelope: CoordinationEnvelope) -> usize {
        let sender = self.room_sender(wiki_id).await;
        {
            let mut rooms = self.rooms.write().await;
            prune_inactive_rooms(&mut rooms, now_ms());
            if let Some(room) = rooms.get_mut(wiki_id) {
                let current_time_ms = now_ms();
                room.published_messages = room.published_messages.saturating_add(1);
                room.last_activity_ms = Some(current_time_ms);
                if let Ok(message) = decode_message(&envelope.payload) {
                    room.accepted_messages = room.accepted_messages.saturating_add(1);
                    track_presence_heartbeat(room, &message, current_time_ms);
                    let _applied = room.state.apply(message);
                    prune_stale_presence(room, current_time_ms);
                } else {
                    room.invalid_messages = room.invalid_messages.saturating_add(1);
                }
            }
        }
        sender.send(envelope).unwrap_or(0)
    }

    pub async fn connect_client(&self, wiki_id: &str) {
        let mut rooms = self.rooms.write().await;
        prune_inactive_rooms(&mut rooms, now_ms());
        let room = rooms
            .entry(wiki_id.to_string())
            .or_insert_with(|| new_room_state(wiki_id));
        let current_time_ms = now_ms();
        room.connected_clients = room.connected_clients.saturating_add(1);
        room.last_activity_ms = Some(current_time_ms);
        prune_stale_presence(room, current_time_ms);
    }

    pub async fn disconnect_client(&self, wiki_id: &str) {
        let mut rooms = self.rooms.write().await;
        let current_time_ms = now_ms();
        prune_inactive_rooms(&mut rooms, current_time_ms);
        let should_remove = if let Some(room) = rooms.get_mut(wiki_id) {
            room.connected_clients = room.connected_clients.saturating_sub(1);
            room.last_activity_ms = Some(current_time_ms);
            prune_stale_presence(room, current_time_ms);
            room.connected_clients == 0
                && room.published_messages == 0
                && room.accepted_messages == 0
                && room.invalid_messages == 0
        } else {
            false
        };

        if should_remove {
            rooms.remove(wiki_id);
        }
    }

    pub async fn snapshot(&self) -> CoordinationSnapshot {
        let current_time_ms = now_ms();
        let mut rooms = self.rooms.write().await;
        prune_inactive_rooms(&mut rooms, current_time_ms);
        for room in rooms.values_mut() {
            prune_stale_presence(room, current_time_ms);
        }
        let mut summaries = rooms
            .iter()
            .map(|(wiki_id, room)| room_summary(wiki_id, room))
            .collect::<Vec<_>>();
        summaries.sort_by(|left, right| left.wiki_id.cmp(&right.wiki_id));

        CoordinationSnapshot { rooms: summaries }
    }

    pub async fn room_state_summary(&self, wiki_id: &str) -> Option<CoordinationStateSummary> {
        let current_time_ms = now_ms();
        let mut rooms = self.rooms.write().await;
        prune_inactive_rooms(&mut rooms, current_time_ms);
        if let Some(room) = rooms.get_mut(wiki_id) {
            prune_stale_presence(room, current_time_ms);
        }
        rooms.get(wiki_id).map(|room| room.state.summary())
    }

    pub async fn room_inspection(&self, wiki_id: &str) -> Option<CoordinationRoomInspection> {
        let current_time_ms = now_ms();
        let mut rooms = self.rooms.write().await;
        prune_inactive_rooms(&mut rooms, current_time_ms);
        if let Some(room) = rooms.get_mut(wiki_id) {
            prune_stale_presence(room, current_time_ms);
        }
        rooms.get(wiki_id).map(|room| CoordinationRoomInspection {
            room: room_summary(wiki_id, room),
            state: Some(room.state.summary()),
            metrics: CoordinationRoomMetrics {
                last_activity_ms: room.last_activity_ms,
                published_messages: room.published_messages,
                accepted_messages: room.accepted_messages,
                invalid_messages: room.invalid_messages,
            },
        })
    }

    pub async fn room_inspections(&self) -> Vec<CoordinationRoomInspection> {
        let current_time_ms = now_ms();
        let mut rooms = self.rooms.write().await;
        prune_inactive_rooms(&mut rooms, current_time_ms);
        for room in rooms.values_mut() {
            prune_stale_presence(room, current_time_ms);
        }
        let mut inspections = rooms
            .iter()
            .map(|(wiki_id, room)| CoordinationRoomInspection {
                room: room_summary(wiki_id, room),
                state: Some(room.state.summary()),
                metrics: CoordinationRoomMetrics {
                    last_activity_ms: room.last_activity_ms,
                    published_messages: room.published_messages,
                    accepted_messages: room.accepted_messages,
                    invalid_messages: room.invalid_messages,
                },
            })
            .collect::<Vec<_>>();
        inspections.sort_by(|left, right| left.room.wiki_id.cmp(&right.room.wiki_id));
        inspections
    }

    #[cfg(test)]
    pub async fn room_count(&self) -> usize {
        self.rooms.read().await.len()
    }

    #[cfg(test)]
    pub async fn set_last_activity_for_test(&self, wiki_id: &str, last_activity_ms: i64) {
        let mut rooms = self.rooms.write().await;
        if let Some(room) = rooms.get_mut(wiki_id) {
            room.last_activity_ms = Some(last_activity_ms);
        }
    }

    #[cfg(test)]
    pub async fn set_presence_last_seen_for_test(
        &self,
        wiki_id: &str,
        actor: &str,
        last_seen_ms: i64,
    ) {
        let mut rooms = self.rooms.write().await;
        if let Some(room) = rooms.get_mut(wiki_id) {
            room.presence_last_seen_ms
                .insert(actor.to_string(), last_seen_ms);
        }
    }

    async fn room_sender(&self, wiki_id: &str) -> broadcast::Sender<CoordinationEnvelope> {
        {
            let rooms = self.rooms.read().await;
            if let Some(room) = rooms.get(wiki_id) {
                return room.sender.clone();
            }
        }

        let mut rooms = self.rooms.write().await;
        prune_inactive_rooms(&mut rooms, now_ms());
        rooms
            .entry(wiki_id.to_string())
            .or_insert_with(|| new_room_state(wiki_id))
            .sender
            .clone()
    }
}

fn new_room_state(wiki_id: &str) -> CoordinationRoomState {
    let (sender, _) = broadcast::channel(ROOM_CAPACITY);
    CoordinationRoomState {
        sender,
        state: CoordinationState::new(wiki_id),
        presence_last_seen_ms: HashMap::new(),
        connected_clients: 0,
        published_messages: 0,
        accepted_messages: 0,
        invalid_messages: 0,
        last_activity_ms: None,
    }
}

fn room_summary(wiki_id: &str, room: &CoordinationRoomState) -> CoordinationRoomSummary {
    let summary = room.state.summary();

    CoordinationRoomSummary {
        wiki_id: wiki_id.to_string(),
        connected_clients: room.connected_clients,
        published_messages: room.published_messages,
        claim_count: summary.claims.len(),
        presence_count: summary.presence.len(),
        flagged_edit_count: summary.flagged_edits.len(),
        score_delta_count: summary.score_deltas.len(),
        race_resolution_count: summary.race_resolutions.len(),
        recent_action_count: summary.recent_actions.len(),
    }
}

fn track_presence_heartbeat(
    room: &mut CoordinationRoomState,
    message: &CoordinationMessage,
    current_time_ms: i64,
) {
    if let CoordinationMessage::PresenceHeartbeat(heartbeat) = message {
        if heartbeat.active_edit_count == 0 {
            room.presence_last_seen_ms.remove(&heartbeat.actor);
        } else {
            room.presence_last_seen_ms
                .insert(heartbeat.actor.clone(), current_time_ms);
        }
    }
}

fn prune_stale_presence(room: &mut CoordinationRoomState, current_time_ms: i64) {
    let stale_actors = room
        .presence_last_seen_ms
        .iter()
        .filter_map(|(actor, last_seen_ms)| {
            let age_ms = current_time_ms.saturating_sub(*last_seen_ms);
            (age_ms >= PRESENCE_STALE_AFTER_MS).then(|| actor.clone())
        })
        .collect::<Vec<_>>();

    for actor in stale_actors {
        room.presence_last_seen_ms.remove(&actor);
        room.state.remove_presence(&actor);
    }
}

fn prune_inactive_rooms(rooms: &mut HashMap<String, CoordinationRoomState>, current_time_ms: i64) {
    rooms.retain(|_, room| !room_is_stale(room, current_time_ms));
}

fn room_is_stale(room: &CoordinationRoomState, current_time_ms: i64) -> bool {
    if room.connected_clients != 0 {
        return false;
    }

    let Some(last_activity_ms) = room.last_activity_ms else {
        return false;
    };

    current_time_ms.saturating_sub(last_activity_ms) >= ROOM_IDLE_EVICT_AFTER_MS
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use sp42_core::{CoordinationMessage, EditClaim, encode_message};

    use super::{CoordinationEnvelope, CoordinationRegistry, ROOM_IDLE_EVICT_AFTER_MS, now_ms};

    #[tokio::test]
    async fn publishes_to_all_subscribers_in_room() {
        let registry = CoordinationRegistry::default();
        let mut receiver = registry.subscribe("frwiki").await;

        let delivered = registry
            .publish(
                "frwiki",
                CoordinationEnvelope {
                    sender_id: 7,
                    payload: vec![1, 2, 3],
                },
            )
            .await;
        let envelope = receiver.recv().await.expect("message should be delivered");

        assert_eq!(delivered, 1);
        assert_eq!(envelope.sender_id, 7);
        assert_eq!(envelope.payload, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn isolates_rooms() {
        let registry = CoordinationRegistry::default();
        let _fr = registry.subscribe("frwiki").await;
        let _en = registry.subscribe("enwiki").await;

        assert_eq!(registry.room_count().await, 2);
    }

    #[tokio::test]
    async fn reports_room_snapshot() {
        let registry = CoordinationRegistry::default();
        registry.connect_client("frwiki").await;
        registry
            .publish(
                "frwiki",
                CoordinationEnvelope {
                    sender_id: 9,
                    payload: vec![7, 8],
                },
            )
            .await;

        let snapshot = registry.snapshot().await;

        assert_eq!(snapshot.rooms.len(), 1);
        assert_eq!(snapshot.rooms[0].wiki_id, "frwiki");
        assert_eq!(snapshot.rooms[0].connected_clients, 1);
        assert_eq!(snapshot.rooms[0].published_messages, 1);
    }

    #[tokio::test]
    async fn tracks_decoded_room_state() {
        let registry = CoordinationRegistry::default();
        let payload = encode_message(&CoordinationMessage::EditClaim(EditClaim {
            wiki_id: "frwiki".to_string(),
            rev_id: 123_456,
            actor: "Alice".to_string(),
        }))
        .expect("message should encode");

        registry
            .publish(
                "frwiki",
                CoordinationEnvelope {
                    sender_id: 1,
                    payload,
                },
            )
            .await;

        let summary = registry
            .room_state_summary("frwiki")
            .await
            .expect("room summary should exist");

        assert_eq!(summary.claims.len(), 1);
        assert_eq!(summary.claims[0].actor, "Alice");
    }

    #[tokio::test]
    async fn records_invalid_payloads_and_last_activity() {
        let registry = CoordinationRegistry::default();
        registry.connect_client("frwiki").await;
        registry
            .publish(
                "frwiki",
                CoordinationEnvelope {
                    sender_id: 1,
                    payload: b"not-msgpack".to_vec(),
                },
            )
            .await;

        let inspection = registry
            .room_inspection("frwiki")
            .await
            .expect("inspection should exist");

        assert_eq!(inspection.room.connected_clients, 1);
        assert_eq!(inspection.metrics.published_messages, 1);
        assert_eq!(inspection.metrics.accepted_messages, 0);
        assert_eq!(inspection.metrics.invalid_messages, 1);
        assert!(inspection.metrics.last_activity_ms.is_some());
    }

    #[tokio::test]
    async fn prunes_empty_rooms_after_disconnect() {
        let registry = CoordinationRegistry::default();
        registry.connect_client("frwiki").await;
        registry.disconnect_client("frwiki").await;

        assert_eq!(registry.room_count().await, 0);
    }

    #[tokio::test]
    async fn evicts_idle_rooms_with_no_connected_clients() {
        let registry = CoordinationRegistry::default();
        let payload = encode_message(&CoordinationMessage::EditClaim(EditClaim {
            wiki_id: "frwiki".to_string(),
            rev_id: 7,
            actor: "Alice".to_string(),
        }))
        .expect("message should encode");

        registry.connect_client("frwiki").await;
        registry
            .publish(
                "frwiki",
                CoordinationEnvelope {
                    sender_id: 1,
                    payload,
                },
            )
            .await;
        registry.disconnect_client("frwiki").await;
        registry
            .set_last_activity_for_test("frwiki", now_ms() - ROOM_IDLE_EVICT_AFTER_MS - 1)
            .await;

        let snapshot = registry.snapshot().await;

        assert!(snapshot.rooms.is_empty());
        assert_eq!(registry.room_count().await, 0);
    }
}
