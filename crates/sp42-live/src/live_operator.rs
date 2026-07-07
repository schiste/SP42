//! Shared live operator payload for the browser review surface.

use serde::{Deserialize, Serialize};

use crate::{BacklogRuntimeStatus, StreamRuntimeStatus};
use sp42_platform::{
    ActionExecutionStatusReport, DevAuthCapabilityReport, DevAuthSessionStatus, EditorIdentity,
    FlagState, LocalOAuthConfigStatus, LocalOAuthSourceReport, PublicRuleSetDocument,
    PublicTeamDefinitionDocument, PublicTeamRegistryDocument, PublicUserPreferencesDocument,
    QueuedEdit, ScoringSignal, SessionActionExecutionRequest, SessionActionKind,
    build_session_action_execution_requests,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

pub const DEFAULT_LIVE_OPERATOR_LIMIT: u16 = 15;
pub const MAX_LIVE_OPERATOR_LIMIT: u16 = 500;

impl LiveOperatorQuery {
    #[must_use]
    pub fn normalized(mut self) -> Self {
        self.limit = self.limit.clamp(1, MAX_LIVE_OPERATOR_LIMIT);
        self.tag_filter = normalized_optional_text(self.tag_filter);
        self.rccontinue = normalized_optional_text(self.rccontinue);
        self.namespaces.sort_unstable();
        self.namespaces.dedup();
        self
    }

    #[must_use]
    pub fn from_query_pairs<'a>(pairs: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let mut query = Self::default();
        for (key, value) in pairs {
            query.apply_query_pair(key, value);
        }
        query.normalized()
    }

    #[must_use]
    pub fn to_query_pairs(&self) -> Vec<(String, String)> {
        let query = self.clone().normalized();
        let mut pairs = Vec::new();
        pairs.push(("limit".to_string(), query.limit.to_string()));
        if query.include_bots.is_enabled() {
            pairs.push(("include_bots".to_string(), "true".to_string()));
        }
        if query.unpatrolled_only.is_enabled() {
            pairs.push(("unpatrolled_only".to_string(), "true".to_string()));
        }
        if !query.include_minor.is_enabled() {
            pairs.push(("include_minor".to_string(), "false".to_string()));
        }
        if !query.include_registered.is_enabled() {
            pairs.push(("include_registered".to_string(), "false".to_string()));
        }
        if !query.include_anonymous.is_enabled() {
            pairs.push(("include_anonymous".to_string(), "false".to_string()));
        }
        if !query.include_temporary.is_enabled() {
            pairs.push(("include_temporary".to_string(), "false".to_string()));
        }
        if !query.include_new_pages.is_enabled() {
            pairs.push(("include_new_pages".to_string(), "false".to_string()));
        }
        if !query.namespaces.is_empty() {
            pairs.push((
                "namespaces".to_string(),
                query
                    .namespaces
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            ));
        }
        if let Some(score) = query.min_score {
            pairs.push(("min_score".to_string(), score.to_string()));
        }
        if let Some(tag) = query.tag_filter {
            pairs.push(("tag_filter".to_string(), tag));
        }
        if let Some(token) = query.rccontinue {
            pairs.push(("rccontinue".to_string(), token));
        }
        pairs
    }

    #[must_use]
    pub fn to_query_string(&self) -> String {
        self.to_query_pairs()
            .into_iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join("&")
    }

    #[must_use]
    pub fn uses_default_filters(&self) -> bool {
        let query = self.clone().normalized();
        query.limit == DEFAULT_LIVE_OPERATOR_LIMIT
            && query.rccontinue.is_none()
            && !query.include_bots.is_enabled()
            && query.unpatrolled_only.is_enabled()
            && query.include_minor.is_enabled()
            && !query.include_registered.is_enabled()
            && query.include_anonymous.is_enabled()
            && query.include_temporary.is_enabled()
            && query.include_new_pages.is_enabled()
            && query.tag_filter.is_none()
            && query.namespaces.is_empty()
            && query.min_score.is_none()
    }

    #[must_use]
    pub fn can_use_supervisor_snapshot(&self) -> bool {
        let query = self.clone().normalized();
        query.rccontinue.is_none()
            && !query.include_bots.is_enabled()
            && !query.unpatrolled_only.is_enabled()
            && query.include_minor.is_enabled()
            && query.include_anonymous.is_enabled()
            && query.include_registered.is_enabled()
            && query.include_temporary.is_enabled()
            && query.include_new_pages.is_enabled()
            && query.tag_filter.is_none()
            && query.namespaces.is_empty()
            && query.min_score.is_none()
    }

    fn apply_query_pair(&mut self, key: &str, value: &str) {
        match key {
            "limit" => {
                if let Ok(limit) = value.parse::<u16>() {
                    self.limit = limit;
                }
            }
            "include_bots" => self.include_bots = FlagState::from(parse_bool(value)),
            "unpatrolled_only" => self.unpatrolled_only = FlagState::from(parse_bool(value)),
            "include_minor" => self.include_minor = FlagState::from(parse_bool(value)),
            "include_registered" => self.include_registered = FlagState::from(parse_bool(value)),
            "include_anonymous" => self.include_anonymous = FlagState::from(parse_bool(value)),
            "include_temporary" => self.include_temporary = FlagState::from(parse_bool(value)),
            "include_new_pages" => self.include_new_pages = FlagState::from(parse_bool(value)),
            "namespaces" => self.namespaces = parse_namespaces(value),
            "min_score" => self.min_score = value.parse::<i32>().ok(),
            "tag_filter" => self.tag_filter = normalized_optional_text(Some(value.to_string())),
            "rccontinue" => self.rccontinue = normalized_optional_text(Some(value.to_string())),
            _ => {}
        }
    }
}

impl Default for LiveOperatorQuery {
    fn default() -> Self {
        Self {
            limit: DEFAULT_LIVE_OPERATOR_LIMIT,
            include_bots: FlagState::Disabled,
            unpatrolled_only: FlagState::Enabled,
            include_minor: FlagState::Enabled,
            include_registered: FlagState::Disabled,
            include_anonymous: FlagState::Enabled,
            include_temporary: FlagState::Enabled,
            include_new_pages: FlagState::Enabled,
            namespaces: Vec::new(),
            min_score: None,
            tag_filter: None,
            rccontinue: None,
        }
    }
}

#[must_use]
pub fn filter_live_operator_queue(
    queue: Vec<QueuedEdit>,
    query: &LiveOperatorQuery,
) -> Vec<QueuedEdit> {
    let query = query.clone().normalized();
    queue
        .into_iter()
        .filter(|item| live_operator_query_matches(item, &query))
        .take(usize::from(query.limit))
        .collect()
}

#[must_use]
pub fn live_operator_query_matches(item: &QueuedEdit, query: &LiveOperatorQuery) -> bool {
    if let Some(min_score) = query.min_score
        && item.score.total < min_score
    {
        return false;
    }
    if !query.include_registered.is_enabled()
        && matches!(item.event.performer, EditorIdentity::Registered { .. })
    {
        return false;
    }
    if !query.include_temporary.is_enabled()
        && matches!(item.event.performer, EditorIdentity::Temporary { .. })
    {
        return false;
    }
    if !query.include_anonymous.is_enabled()
        && matches!(item.event.performer, EditorIdentity::Anonymous { .. })
    {
        return false;
    }
    if !query.include_bots.is_enabled() && item.event.is_bot.is_enabled() {
        return false;
    }
    if !query.include_minor.is_enabled() && item.event.is_minor.is_enabled() {
        return false;
    }
    if !query.include_new_pages.is_enabled() && item.event.is_new_page.is_enabled() {
        return false;
    }
    if query.unpatrolled_only.is_enabled() && item.event.is_patrolled.is_enabled() {
        return false;
    }
    if !query.namespaces.is_empty() && !query.namespaces.contains(&item.event.namespace) {
        return false;
    }
    if let Some(tag_filter) = query.tag_filter.as_ref()
        && !item.event.tags.iter().any(|tag| tag == tag_filter)
    {
        return false;
    }
    true
}

fn normalized_optional_text(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn parse_bool(value: &str) -> bool {
    matches!(value.trim(), "1" | "true" | "TRUE" | "True" | "yes" | "on")
}

fn parse_namespaces(value: &str) -> Vec<i32> {
    let mut namespaces = value
        .split(',')
        .filter_map(|entry| entry.trim().parse::<i32>().ok())
        .collect::<Vec<_>>();
    namespaces.sort_unstable();
    namespaces.dedup();
    namespaces
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LiveOperatorHeuristicProvenance {
    pub rev_id: u64,
    pub performer: String,
    pub resolved_team_slug: Option<String>,
    pub resolved_team_document_title: Option<String>,
    pub resolved_rule_set_slug: Option<String>,
    pub resolved_rule_set_document_title: Option<String>,
    #[serde(default)]
    pub applied_rule_sources: Vec<String>,
    #[serde(default)]
    pub matched_trusted_sources: Vec<String>,
    pub duplicate_cluster_size: Option<u32>,
    #[serde(default)]
    pub obvious_vandalism: FlagState,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[must_use]
pub fn build_live_operator_action_preflight(
    selected: Option<&QueuedEdit>,
    capabilities: &DevAuthCapabilityReport,
    action_status: &ActionExecutionStatusReport,
    note: Option<&str>,
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
        build_session_action_execution_requests(item, note).unwrap_or_else(|_| Vec::new());
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
        SessionActionKind::TagCitationNeeded | SessionActionKind::InlineEdit => {
            if !capabilities.capabilities.editing.can_edit {
                reasons.push("Edit capability is unavailable.".to_string());
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
    let has_obvious_vandalism = item
        .score
        .contributions
        .iter()
        .any(|entry| matches!(entry.signal, ScoringSignal::ObviousVandalism));
    let has_duplicate_pattern = item
        .score
        .contributions
        .iter()
        .any(|entry| matches!(entry.signal, ScoringSignal::DuplicatePattern));
    let has_trusted_suppression = item
        .score
        .contributions
        .iter()
        .any(|entry| matches!(entry.signal, ScoringSignal::TrustedUser));

    match kind {
        SessionActionKind::Rollback => {
            has_obvious_vandalism || has_duplicate_pattern || item.score.total >= 70
        }
        SessionActionKind::Patrol => {
            !item.event.is_patrolled.is_enabled()
                && !has_obvious_vandalism
                && !has_duplicate_pattern
                && item.score.total < 60
        }
        SessionActionKind::Undo => {
            !has_trusted_suppression && (has_duplicate_pattern || item.score.total >= 40)
        }
        SessionActionKind::TagCitationNeeded | SessionActionKind::InlineEdit => false,
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
        LiveOperatorBackendStatus, LiveOperatorQuery, LiveOperatorRetryClass,
        build_live_operator_action_preflight, classify_retry, filter_live_operator_queue,
    };
    use sp42_platform::{
        ActionExecutionStatusReport, CompositeScore, DevAuthActionTokenAvailability,
        DevAuthCapabilityReadiness, DevAuthCapabilityReport, DevAuthDerivedCapabilities,
        DevAuthEditCapabilities, DevAuthModerationCapabilities, DevAuthProbeAcceptance,
        DevAuthSessionStatus, EditEvent, EditorIdentity, FlagState, LocalOAuthConfigStatus,
        LocalOAuthSourceReport, QueuedEdit,
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
                csrf_token: None,
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
        sample_item_with(
            42,
            EditorIdentity::Registered {
                username: "ExampleUser".to_string(),
            },
            100,
            0,
            vec![],
        )
    }

    fn sample_item_with(
        rev_id: u64,
        performer: EditorIdentity,
        score: i32,
        namespace: i32,
        tags: Vec<&str>,
    ) -> QueuedEdit {
        QueuedEdit {
            score: CompositeScore {
                total: score,
                contributions: vec![],
            },
            event: EditEvent {
                content_model: None,
                wiki_id: "frwiki".to_string(),
                title: "Example".to_string(),
                namespace,
                rev_id,
                old_rev_id: Some(rev_id.saturating_sub(1)),
                performer,
                timestamp_ms: 1,
                is_bot: FlagState::Disabled,
                is_minor: FlagState::Disabled,
                is_new_page: FlagState::Disabled,
                tags: tags.into_iter().map(ToString::to_string).collect(),
                comment: None,
                byte_delta: 12,
                is_patrolled: FlagState::Disabled,
            },
        }
    }

    #[test]
    fn live_operator_query_defaults_match_patrol_surface_defaults() {
        let query = LiveOperatorQuery::default();

        assert_eq!(query.limit, 15);
        assert!(query.unpatrolled_only.is_enabled());
        assert!(query.include_minor.is_enabled());
        assert!(query.include_anonymous.is_enabled());
        assert!(query.include_temporary.is_enabled());
        assert!(query.include_new_pages.is_enabled());
        assert!(!query.include_bots.is_enabled());
        assert!(!query.include_registered.is_enabled());
        assert!(query.uses_default_filters());
        assert!(!query.can_use_supervisor_snapshot());
    }

    #[test]
    fn supervisor_snapshot_query_requires_broad_unfiltered_recentchanges_shape() {
        let query = LiveOperatorQuery {
            unpatrolled_only: FlagState::Disabled,
            include_registered: FlagState::Enabled,
            ..LiveOperatorQuery::default()
        };

        assert!(!query.uses_default_filters());
        assert!(query.can_use_supervisor_snapshot());
    }

    #[test]
    fn live_operator_query_serializes_existing_query_contract() {
        let query = LiveOperatorQuery {
            limit: 50,
            include_bots: FlagState::Enabled,
            include_minor: FlagState::Disabled,
            include_registered: FlagState::Disabled,
            include_anonymous: FlagState::Disabled,
            include_temporary: FlagState::Disabled,
            include_new_pages: FlagState::Disabled,
            namespaces: vec![2, 0],
            min_score: Some(30),
            tag_filter: Some("mw-reverted".to_string()),
            rccontinue: Some("20260325|abc".to_string()),
            ..LiveOperatorQuery::default()
        };

        let query_string = query.to_query_string();

        assert!(query_string.contains("limit=50"));
        assert!(query_string.contains("include_bots=true"));
        assert!(query_string.contains("include_minor=false"));
        assert!(query_string.contains("include_registered=false"));
        assert!(query_string.contains("include_anonymous=false"));
        assert!(query_string.contains("include_temporary=false"));
        assert!(query_string.contains("include_new_pages=false"));
        assert!(query_string.contains("namespaces=0,2"));
        assert!(query_string.contains("min_score=30"));
        assert!(query_string.contains("tag_filter=mw-reverted"));
        assert!(query_string.contains("rccontinue=20260325|abc"));
    }

    #[test]
    fn live_operator_query_parses_and_normalizes_pairs() {
        let query = LiveOperatorQuery::from_query_pairs([
            ("limit", "999"),
            ("include_bots", "true"),
            ("include_registered", "true"),
            ("namespaces", "2,0,2,bad"),
            ("tag_filter", " mw-reverted "),
        ]);

        assert_eq!(query.limit, 500);
        assert!(query.include_bots.is_enabled());
        assert!(query.include_registered.is_enabled());
        assert_eq!(query.namespaces, vec![0, 2]);
        assert_eq!(query.tag_filter.as_deref(), Some("mw-reverted"));
    }

    #[test]
    fn filter_live_operator_queue_applies_identity_score_namespace_and_tag_rules() {
        let queue = vec![
            sample_item_with(
                1,
                EditorIdentity::Registered {
                    username: "Registered".to_string(),
                },
                90,
                0,
                vec!["mw-reverted"],
            ),
            sample_item_with(
                2,
                EditorIdentity::Anonymous {
                    label: "192.0.2.1".to_string(),
                },
                45,
                0,
                vec!["mw-reverted"],
            ),
            sample_item_with(
                3,
                EditorIdentity::Temporary {
                    label: "~2026-1".to_string(),
                },
                80,
                2,
                vec!["visualeditor"],
            ),
        ];
        let query = LiveOperatorQuery {
            min_score: Some(40),
            namespaces: vec![0],
            tag_filter: Some("mw-reverted".to_string()),
            ..LiveOperatorQuery::default()
        };

        let filtered = filter_live_operator_queue(queue, &query);

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].event.rev_id, 2);
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
            None,
        );

        assert_eq!(
            preflight.recommended_kind,
            Some(sp42_platform::SessionActionKind::Rollback)
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
            None,
        );

        let rollback = preflight
            .recommendations
            .iter()
            .find(|entry| matches!(entry.kind, sp42_platform::SessionActionKind::Rollback))
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
