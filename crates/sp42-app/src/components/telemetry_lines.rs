use sp42_core::{
    CoordinationRoomSummary, DevAuthCapabilityReport, DevAuthSessionStatus, LocalOAuthConfigStatus,
    ServerDebugSummary,
};

use crate::platform::coordination::{RoomInspectionCollection, room_inspection_lines};

#[must_use]
pub fn coordination_inspection_lines(collection: &RoomInspectionCollection) -> Vec<String> {
    let mut lines = vec![format!(
        "coordination_inspections rooms={}",
        collection.rooms.len()
    )];

    for (index, report) in collection.rooms.iter().enumerate() {
        lines.push(format!("inspection_index={}", index + 1));
        lines.extend(room_inspection_lines(report));
    }

    lines
}

#[must_use]
pub fn server_debug_summary_lines(summary: &ServerDebugSummary) -> Vec<String> {
    let mut lines = vec![
        format!("project={}", summary.project),
        auth_summary_line(&summary.auth),
        oauth_summary_line(&summary.oauth),
        capability_summary_line(&summary.capabilities),
        format!("coordination_rooms={}", summary.coordination.rooms.len()),
    ];

    lines.extend(summary.coordination.rooms.iter().map(room_summary_line));

    lines
}

fn auth_summary_line(status: &DevAuthSessionStatus) -> String {
    format!(
        "auth_authenticated={} auth_user={} auth_bridge_mode={} auth_scopes={}",
        status.authenticated,
        status.username.as_deref().unwrap_or("unknown"),
        status.bridge_mode,
        status.scopes.join(",")
    )
}

fn oauth_summary_line(status: &LocalOAuthConfigStatus) -> String {
    format!(
        "oauth_client_id_present={} oauth_client_secret_present={} oauth_access_token_present={}",
        status.client_id_present, status.client_secret_present, status.access_token_present
    )
}

fn capability_summary_line(report: &DevAuthCapabilityReport) -> String {
    format!(
        "capabilities_checked={} capabilities_user={} capabilities_edit={} capabilities_undo={} capabilities_patrol={} capabilities_rollback={}",
        report.checked,
        report.username.as_deref().unwrap_or("unknown"),
        report.capabilities.editing.can_edit,
        report.capabilities.editing.can_undo,
        report.capabilities.moderation.can_patrol,
        report.capabilities.moderation.can_rollback
    )
}

fn room_summary_line(room: &CoordinationRoomSummary) -> String {
    format!(
        "room={} connected_clients={} published_messages={} claims={} presence={} flags={} deltas={} resolutions={} actions={}",
        room.wiki_id,
        room.connected_clients,
        room.published_messages,
        room.claim_count,
        room.presence_count,
        room.flagged_edit_count,
        room.score_delta_count,
        room.race_resolution_count,
        room.recent_action_count
    )
}

#[cfg(test)]
mod tests {
    use super::{coordination_inspection_lines, server_debug_summary_lines};
    use crate::platform::coordination::{RoomInspectionCollection, RoomInspectionReport};
    use sp42_core::{
        CoordinationRoomSummary, CoordinationSnapshot, DevAuthActionTokenAvailability,
        DevAuthCapabilityReadiness, DevAuthCapabilityReport, DevAuthDerivedCapabilities,
        DevAuthEditCapabilities, DevAuthModerationCapabilities, DevAuthProbeAcceptance,
        DevAuthSessionStatus, LocalOAuthConfigStatus, ServerDebugSummary,
    };

    #[test]
    fn coordination_inspection_lines_include_room_and_state_counts() {
        let lines = coordination_inspection_lines(&RoomInspectionCollection {
            rooms: vec![RoomInspectionReport {
                room: CoordinationRoomSummary {
                    wiki_id: "frwiki".to_string(),
                    connected_clients: 2,
                    published_messages: 11,
                    claim_count: 0,
                    presence_count: 0,
                    flagged_edit_count: 0,
                    score_delta_count: 0,
                    race_resolution_count: 0,
                    recent_action_count: 0,
                },
                state: None,
            }],
        });

        assert!(
            lines
                .iter()
                .any(|line| line.contains("coordination_inspections rooms=1"))
        );
        assert!(lines.iter().any(|line| line.contains("inspection_index=1")));
        assert!(lines.iter().any(|line| line.contains("wiki_id=frwiki")));
    }

    #[test]
    fn server_debug_summary_lines_include_key_statuses() {
        let lines = server_debug_summary_lines(&ServerDebugSummary {
            project: "SP42".to_string(),
            auth: DevAuthSessionStatus {
                authenticated: true,
                username: Some("Tester".to_string()),
                scopes: vec!["rollback".to_string()],
                expires_at_ms: None,
                token_present: true,
                bridge_mode: "local".to_string(),
                local_token_available: true,
            },
            oauth: LocalOAuthConfigStatus {
                client_id_present: true,
                client_secret_present: false,
                access_token_present: true,
            },
            capabilities: DevAuthCapabilityReport {
                checked: true,
                wiki_id: "frwiki".to_string(),
                username: Some("Tester".to_string()),
                oauth_grants: vec!["basic".to_string()],
                wiki_groups: vec!["user".to_string()],
                wiki_rights: vec!["edit".to_string()],
                acceptance: DevAuthProbeAcceptance {
                    profile_accepted: true,
                    userinfo_accepted: true,
                },
                token_availability: DevAuthActionTokenAvailability {
                    csrf_token_available: true,
                    patrol_token_available: false,
                    rollback_token_available: false,
                },
                capabilities: DevAuthDerivedCapabilities {
                    read: DevAuthCapabilityReadiness {
                        can_authenticate: true,
                        can_query_userinfo: true,
                        can_read_recent_changes: true,
                    },
                    editing: DevAuthEditCapabilities {
                        can_edit: true,
                        can_undo: true,
                    },
                    moderation: DevAuthModerationCapabilities {
                        can_patrol: false,
                        can_rollback: false,
                    },
                },
                notes: vec![],
                error: None,
            },
            coordination: CoordinationSnapshot {
                rooms: vec![CoordinationRoomSummary {
                    wiki_id: "frwiki".to_string(),
                    connected_clients: 1,
                    published_messages: 9,
                    claim_count: 0,
                    presence_count: 0,
                    flagged_edit_count: 0,
                    score_delta_count: 0,
                    race_resolution_count: 0,
                    recent_action_count: 0,
                }],
            },
        });

        assert!(lines.iter().any(|line| line.contains("project=SP42")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("auth_authenticated=true"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("oauth_client_id_present=true"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("capabilities_checked=true"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("coordination_rooms=1"))
        );
        assert!(lines.iter().any(|line| line.contains("room=frwiki")));
    }
}
