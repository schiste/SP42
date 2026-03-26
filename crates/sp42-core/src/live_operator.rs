//! Shared live operator payload for the browser review surface.

use serde::{Deserialize, Serialize};

use crate::{
    ActionExecutionHistoryReport, ActionExecutionStatusReport, BacklogRuntimeStatus,
    CoordinationRoomSummary, CoordinationStateSummary, DebugSnapshot, DevAuthCapabilityReport,
    DevAuthSessionStatus, FlagState, LocalOAuthConfigStatus, LocalOAuthSourceReport,
    PatrolScenarioReport, PatrolSessionDigest, PublicRuleSetDocument, PublicTeamDefinitionDocument,
    PublicTeamRegistryDocument, PublicUserPreferencesDocument, QueuedEdit, ReviewWorkbench,
    ScoringContext, SessionActionExecutionRequest, SessionActionKind, ShellStateModel,
    StreamRuntimeStatus, StructuredDiff, build_session_action_execution_requests,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveOperatorBackendStatus {
    pub ready_for_local_testing: FlagState,
    pub readiness_issues: Vec<String>,
    pub bootstrap_ready: FlagState,
    pub oauth: LocalOAuthConfigStatus,
    pub session: DevAuthSessionStatus,
    pub source_report: LocalOAuthSourceReport,
    pub capability_cache_present: FlagState,
    pub capability_cache_fresh: FlagState,
    pub capability_cache_age_ms: Option<u64>,
    pub capability_cache_wiki_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LiveOperatorQuery {
    pub limit: u16,
    #[serde(default)]
    pub include_bots: FlagState,
    #[serde(default)]
    pub unpatrolled_only: FlagState,
    #[serde(default)]
    pub include_minor: FlagState,
    #[serde(default)]
    pub include_registered: FlagState,
    #[serde(default)]
    pub include_anonymous: FlagState,
    #[serde(default)]
    pub include_temporary: FlagState,
    #[serde(default)]
    pub include_new_pages: FlagState,
    #[serde(default)]
    pub namespaces: Vec<i32>,
    pub min_score: Option<i32>,
    pub tag_filter: Option<String>,
    pub rccontinue: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveOperatorPhaseTiming {
    pub phase: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LiveOperatorTelemetry {
    pub total_duration_ms: u64,
    #[serde(default)]
    pub phase_timings: Vec<LiveOperatorPhaseTiming>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LiveOperatorRetryClass {
    NotNeeded,
    AfterSessionRefresh,
    AfterBackoff,
    AfterOperatorChange,
    Never,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveOperatorActionRecommendation {
    pub kind: SessionActionKind,
    pub request: Option<SessionActionExecutionRequest>,
    pub available: bool,
    pub recommended: bool,
    pub retry_class: LiveOperatorRetryClass,
    #[serde(default)]
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LiveOperatorActionPreflight {
    pub selected_rev_id: Option<u64>,
    pub recommended_kind: Option<SessionActionKind>,
    #[serde(default)]
    pub recommendations: Vec<LiveOperatorActionRecommendation>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveIngestionSupervisorStatus {
    pub wiki_id: String,
    pub active: bool,
    pub poll_interval_ms: u64,
    pub run_count: u64,
    pub latest_queue_depth: usize,
    pub last_started_at_ms: Option<i64>,
    pub last_success_at_ms: Option<i64>,
    pub last_error: Option<String>,
    pub stream_status: Option<StreamRuntimeStatus>,
    pub backlog_status: Option<BacklogRuntimeStatus>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LiveOperatorPublicDocuments {
    pub preferences: Option<PublicUserPreferencesDocument>,
    pub preferences_defaulted: FlagState,
    pub registry: Option<PublicTeamRegistryDocument>,
    pub registry_defaulted: FlagState,
    pub active_team: Option<PublicTeamDefinitionDocument>,
    pub active_team_defaulted: FlagState,
    pub active_rule_set: Option<PublicRuleSetDocument>,
    pub active_rule_set_defaulted: FlagState,
    pub audit_period_slug: Option<String>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveOperatorView {
    pub project: String,
    pub fetched_at_ms: i64,
    pub wiki_id: String,
    pub query: LiveOperatorQuery,
    pub queue: Vec<QueuedEdit>,
    pub selected_index: Option<usize>,
    pub scoring_context: Option<ScoringContext>,
    pub diff: Option<StructuredDiff>,
    pub review_workbench: Option<ReviewWorkbench>,
    pub stream_status: Option<StreamRuntimeStatus>,
    pub backlog_status: Option<BacklogRuntimeStatus>,
    pub scenario_report: PatrolScenarioReport,
    pub session_digest: PatrolSessionDigest,
    pub shell_state: ShellStateModel,
    pub capabilities: DevAuthCapabilityReport,
    pub auth: DevAuthSessionStatus,
    pub backend: LiveOperatorBackendStatus,
    pub action_status: ActionExecutionStatusReport,
    pub action_history: ActionExecutionHistoryReport,
    pub action_preflight: LiveOperatorActionPreflight,
    #[serde(default)]
    pub public_documents: LiveOperatorPublicDocuments,
    pub ingestion_supervisor: Option<LiveIngestionSupervisorStatus>,
    pub coordination_room: Option<CoordinationRoomSummary>,
    pub coordination_state: Option<CoordinationStateSummary>,
    #[serde(default)]
    pub debug_snapshot: DebugSnapshot,
    #[serde(default)]
    pub telemetry: LiveOperatorTelemetry,
    pub notes: Vec<String>,
    pub next_continue: Option<String>,
}

#[must_use]
pub fn build_live_operator_action_preflight(
    selected: Option<&QueuedEdit>,
    capabilities: &DevAuthCapabilityReport,
    action_status: &ActionExecutionStatusReport,
) -> LiveOperatorActionPreflight {
    let Some(item) = selected else {
        return LiveOperatorActionPreflight {
            selected_rev_id: None,
            recommended_kind: None,
            recommendations: Vec::new(),
            notes: vec!["Select an edit from the live queue to unlock patrol actions.".to_string()],
        };
    };

    let requests =
        build_session_action_execution_requests(item, None).unwrap_or_else(|_| Vec::new());
    let rollback_request = requests
        .iter()
        .find(|request| matches!(request.kind, SessionActionKind::Rollback))
        .cloned();
    let patrol_request = requests
        .iter()
        .find(|request| matches!(request.kind, SessionActionKind::Patrol))
        .cloned();
    let undo_request = requests
        .iter()
        .find(|request| matches!(request.kind, SessionActionKind::Undo))
        .cloned();

    let rollback = recommendation_for_kind(
        item,
        SessionActionKind::Rollback,
        rollback_request,
        capabilities,
    );
    let patrol = recommendation_for_kind(
        item,
        SessionActionKind::Patrol,
        patrol_request,
        capabilities,
    );
    let undo = recommendation_for_kind(item, SessionActionKind::Undo, undo_request, capabilities);
    let recommendations = vec![rollback, patrol, undo];
    let recommended_kind = recommendations
        .iter()
        .find(|recommendation| recommendation.recommended && recommendation.available)
        .map(|recommendation| recommendation.kind);

    let mut notes = Vec::new();
    if let Some(last_execution) = action_status.last_execution.as_ref()
        && !last_execution.accepted
    {
        notes.push(format!(
            "Last {} failed and is classified as {:?}.",
            last_execution.kind.label(),
            classify_retry(last_execution.api_code.as_deref(), last_execution.retryable)
        ));
    }
    if recommendations
        .iter()
        .all(|recommendation| !recommendation.available)
    {
        notes.push(
            "No live patrol actions are currently available with the active session rights."
                .to_string(),
        );
    }

    LiveOperatorActionPreflight {
        selected_rev_id: Some(item.event.rev_id),
        recommended_kind,
        recommendations,
        notes,
    }
}

fn recommendation_for_kind(
    item: &QueuedEdit,
    kind: SessionActionKind,
    request: Option<SessionActionExecutionRequest>,
    capabilities: &DevAuthCapabilityReport,
) -> LiveOperatorActionRecommendation {
    let (available, reasons, retry_class) = action_availability(kind, item, capabilities);
    let recommended = available && is_recommended(kind, item);

    LiveOperatorActionRecommendation {
        kind,
        request,
        available,
        recommended,
        retry_class,
        reasons,
    }
}

fn action_availability(
    kind: SessionActionKind,
    item: &QueuedEdit,
    capabilities: &DevAuthCapabilityReport,
) -> (bool, Vec<String>, LiveOperatorRetryClass) {
    let mut reasons = Vec::new();

    if !capabilities.checked {
        reasons.push("Capability probe has not completed yet.".to_string());
        return (
            false,
            reasons,
            if capabilities.error.is_some() {
                LiveOperatorRetryClass::AfterBackoff
            } else {
                LiveOperatorRetryClass::AfterSessionRefresh
            },
        );
    }

    if capabilities.error.is_some() {
        reasons.push("Capability probe is currently degraded.".to_string());
        return (false, reasons, LiveOperatorRetryClass::AfterBackoff);
    }

    match kind {
        SessionActionKind::Rollback => {
            if !capabilities.capabilities.moderation.can_rollback {
                reasons.push("Rollback right is unavailable for the active account.".to_string());
            }
            if !capabilities.token_availability.rollback_token_available {
                reasons.push("Rollback token is unavailable.".to_string());
            }
        }
        SessionActionKind::Patrol => {
            if item.event.is_patrolled.is_enabled() {
                reasons.push("The selected edit is already patrolled.".to_string());
            }
            if !capabilities.capabilities.moderation.can_patrol {
                reasons.push("Patrol right is unavailable for the active account.".to_string());
            }
            if !capabilities.token_availability.patrol_token_available {
                reasons.push("Patrol token is unavailable.".to_string());
            }
        }
        SessionActionKind::Undo => {
            if item.event.old_rev_id.is_none() && item.event.rev_id <= 1 {
                reasons.push("Undo requires a prior revision reference.".to_string());
            }
            if !capabilities.capabilities.editing.can_undo {
                reasons.push(
                    "Undo/edit capability is unavailable for the active account.".to_string(),
                );
            }
            if !capabilities.token_availability.csrf_token_available {
                reasons.push("CSRF token is unavailable.".to_string());
            }
        }
    }

    let available = reasons.is_empty();
    let retry_class = if available {
        LiveOperatorRetryClass::NotNeeded
    } else if reasons
        .iter()
        .any(|reason| reason.contains("token is unavailable"))
    {
        LiveOperatorRetryClass::AfterSessionRefresh
    } else if reasons.iter().any(|reason| {
        reason.contains("already patrolled")
            || reason.contains("prior revision")
            || reason.contains("right is unavailable")
            || reason.contains("capability is unavailable")
    }) {
        LiveOperatorRetryClass::AfterOperatorChange
    } else {
        LiveOperatorRetryClass::Never
    };

    (available, reasons, retry_class)
}

fn is_recommended(kind: SessionActionKind, item: &QueuedEdit) -> bool {
    match kind {
        SessionActionKind::Rollback => item.score.total >= 70,
        SessionActionKind::Patrol => !item.event.is_patrolled.is_enabled(),
        SessionActionKind::Undo => item.score.total >= 40,
    }
}

#[must_use]
pub fn classify_retry(api_code: Option<&str>, retryable: bool) -> LiveOperatorRetryClass {
    if !retryable {
        return LiveOperatorRetryClass::Never;
    }

    match api_code.unwrap_or_default() {
        "badtoken" | "notloggedin" | "assertuserfailed" => {
            LiveOperatorRetryClass::AfterSessionRefresh
        }
        "readonly" | "ratelimited" | "maxlag" | "internal_api_error_MWException" => {
            LiveOperatorRetryClass::AfterBackoff
        }
        _ => LiveOperatorRetryClass::AfterOperatorChange,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        LiveOperatorBackendStatus, LiveOperatorRetryClass, build_live_operator_action_preflight,
        classify_retry,
    };
    use crate::{
        ActionExecutionStatusReport, DevAuthActionTokenAvailability, DevAuthCapabilityReadiness,
        DevAuthCapabilityReport, DevAuthDerivedCapabilities, DevAuthEditCapabilities,
        DevAuthModerationCapabilities, DevAuthProbeAcceptance, DevAuthSessionStatus, EditEvent,
        EditorIdentity, FlagState, LocalOAuthConfigStatus, LocalOAuthSourceReport, QueuedEdit,
    };

    #[test]
    fn live_operator_backend_status_serializes_authoritative_local_env_state() {
        let status = LiveOperatorBackendStatus {
            ready_for_local_testing: FlagState::Enabled,
            readiness_issues: vec!["capability cache cold".to_string()],
            bootstrap_ready: FlagState::Enabled,
            oauth: LocalOAuthConfigStatus {
                client_id_present: true,
                client_secret_present: true,
                access_token_present: true,
            },
            session: DevAuthSessionStatus {
                authenticated: true,
                username: Some("Tester".to_string()),
                scopes: vec!["basic".to_string()],
                expires_at_ms: Some(123),
                token_present: true,
                bridge_mode: "local-env-token".to_string(),
                local_token_available: true,
            },
            source_report: LocalOAuthSourceReport {
                file_name: ".env.wikimedia.local".to_string(),
                source_path: None,
                loaded_from_source: true,
            },
            capability_cache_present: FlagState::Enabled,
            capability_cache_fresh: FlagState::Enabled,
            capability_cache_age_ms: Some(5),
            capability_cache_wiki_id: Some("frwiki".to_string()),
        };
        let encoded = serde_json::to_value(&status).expect("status should serialize");
        assert_eq!(
            encoded
                .get("source_report")
                .and_then(|value| value.get("loaded_from_source"))
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
        assert_eq!(
            encoded
                .get("capability_cache_present")
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
    }

    fn sample_item() -> QueuedEdit {
        QueuedEdit {
            score: crate::types::CompositeScore {
                total: 100,
                contributions: vec![],
            },
            event: EditEvent {
                wiki_id: "frwiki".to_string(),
                title: "Example".to_string(),
                namespace: 0,
                rev_id: 42,
                old_rev_id: Some(41),
                performer: EditorIdentity::Registered {
                    username: "ExampleUser".to_string(),
                },
                timestamp_ms: 1,
                is_bot: FlagState::Disabled,
                is_minor: FlagState::Disabled,
                is_new_page: FlagState::Disabled,
                tags: vec![],
                comment: None,
                byte_delta: 12,
                is_patrolled: FlagState::Disabled,
            },
        }
    }

    fn full_capabilities() -> DevAuthCapabilityReport {
        DevAuthCapabilityReport {
            checked: true,
            wiki_id: "frwiki".to_string(),
            username: Some("Reviewer".to_string()),
            oauth_grants: vec![],
            wiki_groups: vec![],
            wiki_rights: vec![],
            acceptance: DevAuthProbeAcceptance {
                profile_accepted: true,
                userinfo_accepted: true,
            },
            token_availability: DevAuthActionTokenAvailability {
                csrf_token_available: true,
                patrol_token_available: true,
                rollback_token_available: true,
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
                    can_patrol: true,
                    can_rollback: true,
                },
            },
            notes: vec![],
            error: None,
        }
    }

    #[test]
    fn preflight_recommends_rollback_for_high_score_edit() {
        let preflight = build_live_operator_action_preflight(
            Some(&sample_item()),
            &full_capabilities(),
            &ActionExecutionStatusReport {
                authenticated: true,
                session_id: Some("session".to_string()),
                username: Some("Reviewer".to_string()),
                total_actions: 0,
                successful_actions: 0,
                failed_actions: 0,
                retryable_failures: 0,
                last_execution: None,
                shell_feedback: vec![],
            },
        );

        assert_eq!(
            preflight.recommended_kind,
            Some(crate::SessionActionKind::Rollback)
        );
        assert!(
            preflight
                .recommendations
                .iter()
                .all(|entry| entry.available)
        );
    }

    #[test]
    fn preflight_classifies_missing_tokens_as_session_refresh() {
        let mut capabilities = full_capabilities();
        capabilities.token_availability.rollback_token_available = false;

        let preflight = build_live_operator_action_preflight(
            Some(&sample_item()),
            &capabilities,
            &ActionExecutionStatusReport {
                authenticated: true,
                session_id: Some("session".to_string()),
                username: Some("Reviewer".to_string()),
                total_actions: 0,
                successful_actions: 0,
                failed_actions: 0,
                retryable_failures: 0,
                last_execution: None,
                shell_feedback: vec![],
            },
        );

        let rollback = preflight
            .recommendations
            .iter()
            .find(|entry| matches!(entry.kind, crate::SessionActionKind::Rollback))
            .expect("rollback recommendation should exist");
        assert!(!rollback.available);
        assert_eq!(
            rollback.retry_class,
            LiveOperatorRetryClass::AfterSessionRefresh
        );
    }

    #[test]
    fn retry_classifier_maps_codes_to_classes() {
        assert_eq!(
            classify_retry(Some("badtoken"), true),
            LiveOperatorRetryClass::AfterSessionRefresh
        );
        assert_eq!(
            classify_retry(Some("maxlag"), true),
            LiveOperatorRetryClass::AfterBackoff
        );
        assert_eq!(classify_retry(None, false), LiveOperatorRetryClass::Never);
    }
}
