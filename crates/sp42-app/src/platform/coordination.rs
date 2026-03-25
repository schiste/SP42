use std::collections::BTreeSet;

use serde_json::Value;
use sp42_core::{CoordinationRoomSummary, CoordinationSnapshot, CoordinationStateSummary};

#[cfg(target_arch = "wasm32")]
use super::http::get_bytes;

const COORDINATION_ROOMS_URL: &str = "http://127.0.0.1:8788/coordination/rooms";
const COORDINATION_INSPECTIONS_URL: &str = "http://127.0.0.1:8788/coordination/inspections";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomInspectionReport {
    pub room: CoordinationRoomSummary,
    pub state: Option<CoordinationStateSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomInspectionCollection {
    pub rooms: Vec<RoomInspectionReport>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinationRoomNarrative {
    pub wiki_id: String,
    pub connected_clients: u32,
    pub published_messages: u64,
    pub state_available: bool,
    pub collaboration_mode: String,
    pub active_actors: Vec<String>,
    pub claimed_revisions: Vec<u64>,
    pub flagged_revisions: Vec<u64>,
    pub score_delta_revisions: Vec<u64>,
    pub race_resolution_revisions: Vec<u64>,
    pub latest_action: Option<String>,
}

#[must_use]
pub fn preview_coordination_snapshot() -> CoordinationSnapshot {
    CoordinationSnapshot { rooms: Vec::new() }
}

#[must_use]
pub fn coordination_snapshot_lines(snapshot: &CoordinationSnapshot) -> Vec<String> {
    let mut lines = vec![format!("room_count={}", snapshot.rooms.len())];
    for room in &snapshot.rooms {
        lines.push(format!(
            "room={} clients={} messages={} claims={} presence={} flags={} deltas={} resolutions={} actions={}",
            room.wiki_id,
            room.connected_clients,
            room.published_messages,
            room.claim_count,
            room.presence_count,
            room.flagged_edit_count,
            room.score_delta_count,
            room.race_resolution_count,
            room.recent_action_count
        ));
    }

    lines
}

#[must_use]
pub fn coordination_state_lines(state: &CoordinationStateSummary) -> Vec<String> {
    vec![
        format!("wiki_id={}", state.wiki_id),
        format!("claims={}", state.claims.len()),
        format!("presence={}", state.presence.len()),
        format!("flagged_edits={}", state.flagged_edits.len()),
        format!("score_deltas={}", state.score_deltas.len()),
        format!("race_resolutions={}", state.race_resolutions.len()),
        format!("recent_actions={}", state.recent_actions.len()),
    ]
}

#[must_use]
pub fn coordination_room_narrative(report: &RoomInspectionReport) -> CoordinationRoomNarrative {
    let state_available = report.state.is_some();
    let active_actors = report
        .state
        .as_ref()
        .map(|state| {
            let mut actors = BTreeSet::new();
            for entry in &state.presence {
                actors.insert(entry.actor.clone());
            }
            for claim in &state.claims {
                actors.insert(claim.actor.clone());
            }
            actors.into_iter().collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let claimed_revisions = report
        .state
        .as_ref()
        .map(|state| state.claims.iter().map(|entry| entry.rev_id).collect())
        .unwrap_or_default();
    let flagged_revisions = report
        .state
        .as_ref()
        .map(|state| {
            state
                .flagged_edits
                .iter()
                .map(|entry| entry.rev_id)
                .collect()
        })
        .unwrap_or_default();
    let score_delta_revisions = report
        .state
        .as_ref()
        .map(|state| {
            state
                .score_deltas
                .iter()
                .map(|entry| entry.rev_id)
                .collect()
        })
        .unwrap_or_default();
    let race_resolution_revisions = report
        .state
        .as_ref()
        .map(|state| {
            state
                .race_resolutions
                .iter()
                .map(|entry| entry.rev_id)
                .collect()
        })
        .unwrap_or_default();
    let latest_action = report.state.as_ref().and_then(|state| {
        state.recent_actions.last().map(|action| {
            format!(
                "{:?} rev={} by {}",
                action.action, action.rev_id, action.actor
            )
        })
    });

    CoordinationRoomNarrative {
        wiki_id: report.room.wiki_id.clone(),
        connected_clients: report.room.connected_clients,
        published_messages: report.room.published_messages,
        state_available,
        collaboration_mode: collaboration_mode(report),
        active_actors,
        claimed_revisions,
        flagged_revisions,
        score_delta_revisions,
        race_resolution_revisions,
        latest_action,
    }
}

#[must_use]
pub fn coordination_room_narrative_lines(report: &RoomInspectionReport) -> Vec<String> {
    let narrative = coordination_room_narrative(report);
    let mut lines = vec![
        format!(
            "room={} clients={} messages={} mode={}",
            narrative.wiki_id,
            narrative.connected_clients,
            narrative.published_messages,
            narrative.collaboration_mode
        ),
        format!("state_available={}", narrative.state_available),
    ];

    if let Some(state) = &report.state {
        lines.push(format!("state_wiki_id={}", state.wiki_id));
        lines.push(format!(
            "active_actors={}",
            if narrative.active_actors.is_empty() {
                "none".to_string()
            } else {
                narrative.active_actors.join(", ")
            }
        ));
        lines.push(format!(
            "claimed_revisions={}",
            if narrative.claimed_revisions.is_empty() {
                "none".to_string()
            } else {
                narrative
                    .claimed_revisions
                    .iter()
                    .map(|rev_id| rev_id.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        ));
        lines.push(format!(
            "flagged_revisions={}",
            if narrative.flagged_revisions.is_empty() {
                "none".to_string()
            } else {
                narrative
                    .flagged_revisions
                    .iter()
                    .map(|rev_id| rev_id.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        ));
        lines.push(format!(
            "score_delta_revisions={}",
            if narrative.score_delta_revisions.is_empty() {
                "none".to_string()
            } else {
                narrative
                    .score_delta_revisions
                    .iter()
                    .map(|rev_id| rev_id.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        ));
        lines.push(format!(
            "race_resolution_revisions={}",
            if narrative.race_resolution_revisions.is_empty() {
                "none".to_string()
            } else {
                narrative
                    .race_resolution_revisions
                    .iter()
                    .map(|rev_id| rev_id.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        ));
        lines.push(format!(
            "recent_action={}",
            narrative.latest_action.as_deref().unwrap_or("none")
        ));
        lines.push(format!(
            "state_counts claims={} presence={} flagged_edits={} score_deltas={} race_resolutions={} recent_actions={}",
            state.claims.len(),
            state.presence.len(),
            state.flagged_edits.len(),
            state.score_deltas.len(),
            state.race_resolutions.len(),
            state.recent_actions.len()
        ));
    } else {
        lines.push("state=missing".to_string());
    }

    lines
}

#[cfg(target_arch = "wasm32")]
pub async fn fetch_coordination_snapshot() -> Result<CoordinationSnapshot, String> {
    let bytes = get_bytes(COORDINATION_ROOMS_URL, "fetch coordination snapshot").await?;
    serde_json::from_slice(&bytes).map_err(|error| error.to_string())
}

#[cfg(target_arch = "wasm32")]
pub async fn fetch_coordination_room_state(
    wiki_id: &str,
) -> Result<CoordinationStateSummary, String> {
    let bytes = get_bytes(
        &format!("{COORDINATION_ROOMS_URL}/{wiki_id}"),
        "fetch coordination room state",
    )
    .await?;
    serde_json::from_slice(&bytes).map_err(|error| error.to_string())
}

#[cfg(target_arch = "wasm32")]
pub async fn fetch_coordination_inspections() -> Result<RoomInspectionCollection, String> {
    let bytes = get_bytes(
        COORDINATION_INSPECTIONS_URL,
        "fetch coordination inspections",
    )
    .await?;
    parse_room_inspection_collection(&bytes)
}

#[cfg(target_arch = "wasm32")]
pub async fn fetch_coordination_room_inspection(
    wiki_id: &str,
) -> Result<RoomInspectionReport, String> {
    let bytes = get_bytes(
        &format!("{COORDINATION_ROOMS_URL}/{wiki_id}/inspection"),
        "fetch coordination room inspection",
    )
    .await?;
    parse_room_inspection_report(&bytes)
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn fetch_coordination_snapshot() -> Result<CoordinationSnapshot, String> {
    Err("Coordination snapshot fetch is only available in the browser runtime.".to_string())
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn fetch_coordination_room_state(
    _wiki_id: &str,
) -> Result<CoordinationStateSummary, String> {
    Err("Coordination room fetch is only available in the browser runtime.".to_string())
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn fetch_coordination_inspections() -> Result<RoomInspectionCollection, String> {
    Err("Coordination inspections are only available in the browser runtime.".to_string())
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn fetch_coordination_room_inspection(
    _wiki_id: &str,
) -> Result<RoomInspectionReport, String> {
    Err("Coordination room inspection is only available in the browser runtime.".to_string())
}

#[must_use]
pub fn room_inspection_lines(report: &RoomInspectionReport) -> Vec<String> {
    coordination_room_narrative_lines(report)
}

fn parse_room_inspection_collection(bytes: &[u8]) -> Result<RoomInspectionCollection, String> {
    let value: Value = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    let object = value
        .as_object()
        .ok_or_else(|| "inspection collection response must be a JSON object".to_string())?;
    let rooms_value = object
        .get("rooms")
        .ok_or_else(|| "rooms field is missing".to_string())?;
    let rooms = rooms_value
        .as_array()
        .ok_or_else(|| "rooms field must be an array".to_string())?
        .iter()
        .map(parse_room_inspection_value)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(RoomInspectionCollection { rooms })
}

fn parse_room_inspection_report(bytes: &[u8]) -> Result<RoomInspectionReport, String> {
    let value: Value = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    parse_room_inspection_value(&value)
}

fn parse_room_inspection_value(value: &Value) -> Result<RoomInspectionReport, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "inspection response must be a JSON object".to_string())?;

    let room_value = object
        .get("room")
        .ok_or_else(|| "room field is missing".to_string())?
        .clone();
    let room = serde_json::from_value(room_value).map_err(|error| error.to_string())?;

    let state = match object.get("state") {
        Some(Value::Null) | None => None,
        Some(value) => {
            Some(serde_json::from_value(value.clone()).map_err(|error| error.to_string())?)
        }
    };

    Ok(RoomInspectionReport { room, state })
}

fn collaboration_mode(report: &RoomInspectionReport) -> String {
    let Some(state) = &report.state else {
        return "snapshot-only".to_string();
    };

    if !state.race_resolutions.is_empty() {
        "contested".to_string()
    } else if !state.flagged_edits.is_empty() || !state.score_deltas.is_empty() {
        "under-review".to_string()
    } else if !state.presence.is_empty() || !state.recent_actions.is_empty() {
        "active".to_string()
    } else if !state.claims.is_empty() {
        "claimed".to_string()
    } else {
        "quiet".to_string()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        coordination_room_narrative_lines, parse_room_inspection_collection, room_inspection_lines,
    };
    use sp42_core::{CoordinationRoomSummary, CoordinationStateSummary};

    #[test]
    fn room_inspection_lines_cover_presence_and_state() {
        let lines = room_inspection_lines(&super::RoomInspectionReport {
            room: CoordinationRoomSummary {
                wiki_id: "frwiki".to_string(),
                connected_clients: 3,
                published_messages: 7,
                claim_count: 0,
                presence_count: 0,
                flagged_edit_count: 0,
                score_delta_count: 0,
                race_resolution_count: 0,
                recent_action_count: 0,
            },
            state: Some(CoordinationStateSummary {
                wiki_id: "frwiki".to_string(),
                claims: vec![],
                presence: vec![],
                flagged_edits: vec![],
                score_deltas: vec![],
                race_resolutions: vec![],
                recent_actions: vec![],
            }),
        });

        assert!(lines.iter().any(|line| line.contains("wiki_id=frwiki")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("connected_clients=3"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("state_wiki_id=frwiki"))
        );
        assert!(lines.iter().any(|line| line.contains("mode=active")));
    }

    #[test]
    fn parse_room_inspection_collection_handles_multiple_rooms() {
        let value = json!({
            "rooms": [
                {
                    "room": {
                        "wiki_id": "frwiki",
                        "connected_clients": 2,
                        "published_messages": 11,
                        "claim_count": 0,
                        "presence_count": 0,
                        "flagged_edit_count": 0,
                        "score_delta_count": 0,
                        "race_resolution_count": 0,
                        "recent_action_count": 0
                    },
                    "state": null
                },
                {
                    "room": {
                        "wiki_id": "enwiki",
                        "connected_clients": 4,
                        "published_messages": 18,
                        "claim_count": 1,
                        "presence_count": 1,
                        "flagged_edit_count": 0,
                        "score_delta_count": 0,
                        "race_resolution_count": 0,
                        "recent_action_count": 0
                    },
                    "state": {
                        "wiki_id": "enwiki",
                        "claims": [],
                        "presence": [],
                        "flagged_edits": [],
                        "score_deltas": [],
                        "race_resolutions": [],
                        "recent_actions": []
                    }
                }
            ]
        });

        let parsed = parse_room_inspection_collection(
            &serde_json::to_vec(&value).expect("fixture should serialize"),
        )
        .expect("inspection collection should parse");

        assert_eq!(parsed.rooms.len(), 2);
        assert_eq!(parsed.rooms[0].room.wiki_id, "frwiki");
        assert!(parsed.rooms[0].state.is_none());
        assert_eq!(parsed.rooms[1].room.connected_clients, 4);
        assert_eq!(
            parsed.rooms[1]
                .state
                .as_ref()
                .expect("state should exist")
                .wiki_id,
            "enwiki"
        );
    }

    #[test]
    fn coordination_room_narrative_lines_surface_collaboration_details() {
        let lines = coordination_room_narrative_lines(&super::RoomInspectionReport {
            room: CoordinationRoomSummary {
                wiki_id: "frwiki".to_string(),
                connected_clients: 3,
                published_messages: 7,
                claim_count: 1,
                presence_count: 1,
                flagged_edit_count: 1,
                score_delta_count: 1,
                race_resolution_count: 0,
                recent_action_count: 1,
            },
            state: Some(CoordinationStateSummary {
                wiki_id: "frwiki".to_string(),
                claims: vec![sp42_core::EditClaim {
                    wiki_id: "frwiki".to_string(),
                    rev_id: 123,
                    actor: "Alice".to_string(),
                }],
                presence: vec![sp42_core::PresenceHeartbeat {
                    wiki_id: "frwiki".to_string(),
                    actor: "Alice".to_string(),
                    active_edit_count: 2,
                }],
                flagged_edits: vec![sp42_core::FlaggedEdit {
                    wiki_id: "frwiki".to_string(),
                    rev_id: 123,
                    score: 95,
                    reason: "possible vandalism".to_string(),
                }],
                score_deltas: vec![sp42_core::ScoreDelta {
                    wiki_id: "frwiki".to_string(),
                    rev_id: 123,
                    delta: 5,
                    reason: "recent action".to_string(),
                }],
                race_resolutions: vec![sp42_core::RaceResolution {
                    wiki_id: "frwiki".to_string(),
                    rev_id: 123,
                    winning_actor: "Alice".to_string(),
                }],
                recent_actions: vec![sp42_core::ActionBroadcast {
                    wiki_id: "frwiki".to_string(),
                    rev_id: 123,
                    action: sp42_core::Action::Rollback,
                    actor: "Alice".to_string(),
                }],
            }),
        });

        assert!(lines.iter().any(|line| line.contains("mode=contested")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("active_actors=Alice"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("recent_action=Rollback"))
        );
    }
}
