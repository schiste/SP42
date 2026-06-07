use std::time::Instant;

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use sp42_coordination::{CoordinationRoomSummary, CoordinationSnapshot, CoordinationState};
use sp42_core::{
    DevAuthCapabilityReport, DevAuthSessionStatus, LocalOAuthConfigStatus, LocalOAuthSourceReport,
    routes as route_contracts,
};
use sp42_live::{BacklogRuntime, BacklogRuntimeConfig, BacklogRuntimeStatus, StreamRuntimeStatus};
use sp42_reporting::ServerDebugSummary;
use sp42_types::{FileStorage, Storage};

use crate::coordination::{
    CoordinationRegistry, CoordinationRoomInspection, CoordinationRoomMetrics,
};
use crate::endpoint_manifest::{OperatorEndpointDescriptor, operator_endpoint_manifest};
use crate::session_runtime::{bootstrap_status, current_status, prune_expired_sessions};
use crate::{
    AppState, OPERATOR_REPORT_PATH, cache_is_fresh, capability_report_for_request,
    resolved_wiki_config, supervisor_snapshot_for_wiki,
};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct DevAuthBootstrapStatus {
    pub(crate) bootstrap_ready: bool,
    pub(crate) oauth: LocalOAuthConfigStatus,
    pub(crate) session: DevAuthSessionStatus,
    pub(crate) source_path: Option<String>,
    pub(crate) source_report: LocalOAuthSourceReport,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct CapabilityProbeHint {
    pub(crate) wiki_id: String,
    pub(crate) endpoint: String,
    pub(crate) available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct CapabilityCacheStatus {
    pub(crate) present: bool,
    pub(crate) fresh: bool,
    pub(crate) age_ms: Option<u64>,
    pub(crate) wiki_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct ServerHealthStatus {
    pub(crate) project: String,
    pub(crate) ready_for_local_testing: bool,
    pub(crate) readiness_issues: Vec<String>,
    pub(crate) uptime_ms: u64,
    pub(crate) session_count: usize,
    pub(crate) coordination_room_count: usize,
    pub(crate) auth: DevAuthSessionStatus,
    pub(crate) oauth: LocalOAuthConfigStatus,
    pub(crate) bootstrap: DevAuthBootstrapStatus,
    pub(crate) capability_probe: CapabilityProbeHint,
    pub(crate) capability_cache: CapabilityCacheStatus,
    pub(crate) operator_report_path: String,
    pub(crate) coordination: CoordinationSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct RuntimeDebugStatus {
    pub(crate) project: String,
    pub(crate) uptime_ms: u64,
    pub(crate) session_count: usize,
    pub(crate) coordination_room_count: usize,
    pub(crate) auth: DevAuthSessionStatus,
    pub(crate) oauth: LocalOAuthConfigStatus,
    pub(crate) bootstrap: DevAuthBootstrapStatus,
    pub(crate) capabilities: DevAuthCapabilityReport,
    pub(crate) capability_cache: CapabilityCacheStatus,
    pub(crate) operator_report_path: String,
    pub(crate) coordination: CoordinationSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct OperatorReport {
    pub(crate) project: String,
    pub(crate) readiness: ServerHealthStatus,
    pub(crate) runtime: RuntimeDebugStatus,
    pub(crate) bootstrap: DevAuthBootstrapStatus,
    pub(crate) debug_summary: ServerDebugSummary,
    pub(crate) endpoints: Vec<OperatorEndpointDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct RoomInspectionCollection {
    pub(crate) rooms: Vec<CoordinationRoomInspection>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct OperatorRuntimeInspection {
    pub(crate) wiki_id: String,
    pub(crate) storage_root: String,
    pub(crate) backlog: BacklogRuntimeStatus,
    pub(crate) stream_checkpoint_key: String,
    pub(crate) stream_last_event_id: Option<String>,
    pub(crate) notes: Vec<String>,
}

pub(crate) async fn get_debug_summary(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Json<ServerDebugSummary> {
    Json(server_debug_summary(&state, &headers).await)
}

pub(crate) async fn get_operator_readiness(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Json<ServerHealthStatus> {
    Json(server_readiness(&state, &headers).await)
}

pub(crate) async fn get_operator_report(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Json<OperatorReport> {
    Json(operator_report(&state, &headers).await)
}

pub(crate) async fn get_runtime_debug(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Json<RuntimeDebugStatus> {
    Json(runtime_debug(&state, &headers).await)
}

pub(crate) async fn get_operator_runtime(
    Path(wiki_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<OperatorRuntimeInspection>, (StatusCode, Json<serde_json::Value>)> {
    operator_runtime_inspection(&state, &wiki_id)
        .await
        .map(Json)
        .map_err(|error| {
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": error })),
            )
        })
}

pub(crate) async fn get_healthz(State(state): State<AppState>) -> Json<ServerHealthStatus> {
    Json(server_readiness(&state, &HeaderMap::new()).await)
}

pub(crate) async fn server_debug_summary(
    state: &AppState,
    headers: &HeaderMap,
) -> ServerDebugSummary {
    let default_wiki_id = state.default_wiki_id();
    let auth = current_status(state, headers, true).await;
    let oauth = state.local_oauth.status();
    let capabilities = capability_report_for_request(state, headers, default_wiki_id, false).await;
    ServerDebugSummary {
        project: sp42_core::branding::PROJECT_NAME.to_string(),
        auth,
        oauth,
        capabilities,
        coordination: state.coordination.snapshot().await,
    }
}

pub(crate) async fn server_readiness(state: &AppState, headers: &HeaderMap) -> ServerHealthStatus {
    let default_wiki_id = state.default_wiki_id();
    let auth = current_status(state, headers, false).await;
    let bootstrap = bootstrap_status(state, &auth);
    let capability_probe = CapabilityProbeHint {
        wiki_id: default_wiki_id.to_string(),
        endpoint: route_contracts::dev_auth_capabilities_path(default_wiki_id),
        available: state.local_oauth.access_token().is_some(),
    };
    let capability_cache = capability_cache_status(state, default_wiki_id).await;
    let session_count = session_count(state).await;
    let coordination_snapshot = state.coordination.snapshot().await;
    let coordination_room_count = coordination_snapshot.rooms.len();
    let mut readiness_issues = Vec::new();
    let session_ready = auth.authenticated;
    let bootstrap_ready = bootstrap.bootstrap_ready;

    if !bootstrap_ready {
        readiness_issues
            .push("Local token bootstrap is unavailable because WIKIMEDIA_ACCESS_TOKEN is not set in process environment, .env.wikimedia.local, or .env.".to_string());
    }
    if !session_ready && !bootstrap_ready {
        readiness_issues.push("No dev session is bootstrapped yet.".to_string());
    }
    if !capability_cache.present {
        readiness_issues.push("No capability cache has been populated yet.".to_string());
    }

    ServerHealthStatus {
        project: sp42_core::branding::PROJECT_NAME.to_string(),
        ready_for_local_testing: session_ready || bootstrap_ready,
        readiness_issues,
        uptime_ms: uptime_ms(&state.started_at),
        session_count,
        coordination_room_count,
        auth,
        oauth: state.local_oauth.status(),
        bootstrap,
        capability_probe,
        capability_cache,
        operator_report_path: OPERATOR_REPORT_PATH.to_string(),
        coordination: coordination_snapshot,
    }
}

pub(crate) async fn runtime_debug(state: &AppState, headers: &HeaderMap) -> RuntimeDebugStatus {
    let default_wiki_id = state.default_wiki_id();
    let auth = current_status(state, headers, true).await;
    let bootstrap = bootstrap_status(state, &auth);
    let capabilities = capability_report_for_request(state, headers, default_wiki_id, false).await;
    let capability_cache = capability_cache_status(state, default_wiki_id).await;
    let coordination = state.coordination.snapshot().await;

    RuntimeDebugStatus {
        project: sp42_core::branding::PROJECT_NAME.to_string(),
        uptime_ms: uptime_ms(&state.started_at),
        session_count: session_count(state).await,
        coordination_room_count: coordination.rooms.len(),
        bootstrap,
        auth,
        oauth: state.local_oauth.status(),
        capabilities,
        capability_cache,
        operator_report_path: OPERATOR_REPORT_PATH.to_string(),
        coordination,
    }
}

pub(crate) async fn operator_report(state: &AppState, headers: &HeaderMap) -> OperatorReport {
    let readiness = server_readiness(state, headers).await;
    let runtime = runtime_debug(state, headers).await;
    let bootstrap = readiness.bootstrap.clone();
    let debug_summary = server_debug_summary(state, headers).await;

    OperatorReport {
        project: sp42_core::branding::PROJECT_NAME.to_string(),
        readiness,
        runtime,
        bootstrap,
        debug_summary,
        endpoints: operator_endpoint_manifest(state.default_wiki_id()),
    }
}

pub(crate) fn runtime_storage_for(state: &AppState) -> FileStorage {
    FileStorage::new(state.runtime_storage_root.clone())
}

pub(crate) async fn operator_runtime_inspection(
    state: &AppState,
    wiki_id: &str,
) -> Result<OperatorRuntimeInspection, String> {
    if let Some(snapshot) = supervisor_snapshot_for_wiki(state, wiki_id).await {
        let mut notes = snapshot.status.notes.clone();
        notes.push(
            "Runtime inspection is backed by the continuous ingestion supervisor.".to_string(),
        );
        return Ok(OperatorRuntimeInspection {
            wiki_id: wiki_id.to_string(),
            storage_root: state.runtime_storage_root.display().to_string(),
            backlog: snapshot
                .status
                .backlog_status
                .unwrap_or(BacklogRuntimeStatus {
                    checkpoint_key: format!("recentchanges.rccontinue.{wiki_id}"),
                    next_continue: snapshot.next_continue,
                    last_batch_size: 0,
                    total_events: 0,
                    poll_count: 0,
                }),
            stream_checkpoint_key: format!("stream.last_event_id.{wiki_id}"),
            stream_last_event_id: snapshot
                .status
                .stream_status
                .and_then(|status| status.last_event_id),
            notes,
        });
    }

    let config = resolved_wiki_config(state, wiki_id)?;
    let storage = runtime_storage_for(state);
    let mut backlog = BacklogRuntime::new(
        config,
        storage.clone(),
        BacklogRuntimeConfig {
            limit: 15,
            include_bots: false,
        },
        format!("recentchanges.rccontinue.{wiki_id}"),
    );
    backlog
        .initialize()
        .await
        .map_err(|error| format!("backlog runtime init failed: {error}"))?;
    let backlog_status = backlog.status();
    let stream_checkpoint_key = format!("stream.last_event_id.{wiki_id}");
    let stream_last_event_id = storage
        .get(&stream_checkpoint_key)
        .await
        .map_err(|error| format!("stream checkpoint read failed: {error}"))?
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .filter(|value| !value.trim().is_empty());

    let mut notes = vec![format!("runtime_storage_root={}", storage.root().display())];
    notes.push(format!(
        "backlog_next_continue={}",
        backlog_status.next_continue.as_deref().unwrap_or("none")
    ));
    notes.push(format!(
        "stream_last_event_id={}",
        stream_last_event_id.as_deref().unwrap_or("none")
    ));

    Ok(OperatorRuntimeInspection {
        wiki_id: wiki_id.to_string(),
        storage_root: storage.root().display().to_string(),
        backlog: backlog_status,
        stream_checkpoint_key,
        stream_last_event_id,
        notes,
    })
}

pub(crate) async fn persisted_stream_status(
    state: &AppState,
    wiki_id: &str,
) -> Result<StreamRuntimeStatus, String> {
    let checkpoint_key = format!("stream.last_event_id.{wiki_id}");
    let storage = runtime_storage_for(state);
    let last_event_id = storage
        .get(&checkpoint_key)
        .await
        .map_err(|error| format!("stream checkpoint read failed: {error}"))?
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .filter(|value| !value.trim().is_empty());

    Ok(StreamRuntimeStatus {
        checkpoint_key,
        last_event_id,
        delivered_events: 0,
        filtered_events: 0,
        reconnect_attempts: 0,
    })
}

pub(crate) async fn session_count(state: &AppState) -> usize {
    let mut sessions = state.sessions.write().await;
    prune_expired_sessions(&mut sessions, state.clock.now_ms());
    sessions.len()
}

pub(crate) async fn capability_cache_status(
    state: &AppState,
    wiki_id: &str,
) -> CapabilityCacheStatus {
    let guard = state.capability_cache.read().await;
    let Some(cache) = guard.as_ref() else {
        return CapabilityCacheStatus {
            present: false,
            fresh: false,
            age_ms: None,
            wiki_id: None,
        };
    };
    let age_ms = state.clock.now_ms().saturating_sub(cache.fetched_at_ms);
    let valid =
        cache.report.wiki_id == wiki_id && cache.report.checked && cache.report.error.is_none();
    CapabilityCacheStatus {
        present: valid,
        fresh: valid && cache_is_fresh(cache, state.clock.now_ms()),
        age_ms: Some(u64::try_from(age_ms).unwrap_or(u64::MAX)),
        wiki_id: Some(cache.report.wiki_id.clone()),
    }
}

pub(crate) async fn room_inspection(
    coordination: &CoordinationRegistry,
    wiki_id: &str,
) -> Option<CoordinationRoomInspection> {
    coordination.room_inspection(wiki_id).await
}

pub(crate) fn empty_room_inspection(wiki_id: &str) -> CoordinationRoomInspection {
    CoordinationRoomInspection {
        room: CoordinationRoomSummary {
            wiki_id: wiki_id.to_string(),
            connected_clients: 0,
            published_messages: 0,
            claim_count: 0,
            presence_count: 0,
            flagged_edit_count: 0,
            score_delta_count: 0,
            race_resolution_count: 0,
            recent_action_count: 0,
        },
        state: Some(CoordinationState::new(wiki_id).summary()),
        metrics: CoordinationRoomMetrics {
            last_activity_ms: None,
            published_messages: 0,
            accepted_messages: 0,
            invalid_messages: 0,
        },
    }
}

pub(crate) fn uptime_ms(started_at: &Instant) -> u64 {
    u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX)
}
