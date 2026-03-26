//! Shared types used across all SP42 targets.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(from = "bool", into = "bool")]
pub enum FlagState {
    #[default]
    Disabled,
    Enabled,
}

impl FlagState {
    #[must_use]
    pub const fn is_enabled(self) -> bool {
        matches!(self, Self::Enabled)
    }

    #[must_use]
    pub const fn from_bool(value: bool) -> Self {
        if value { Self::Enabled } else { Self::Disabled }
    }
}

impl From<bool> for FlagState {
    fn from(value: bool) -> Self {
        Self::from_bool(value)
    }
}

impl From<FlagState> for bool {
    fn from(value: FlagState) -> Self {
        value.is_enabled()
    }
}

impl fmt::Display for FlagState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.is_enabled() { "true" } else { "false" })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EditorIdentity {
    Registered { username: String },
    Anonymous { label: String },
    Temporary { label: String },
}

impl EditorIdentity {
    #[must_use]
    pub fn stable_label(&self) -> &str {
        match self {
            Self::Registered { username }
            | Self::Anonymous { label: username }
            | Self::Temporary { label: username } => username,
        }
    }

    #[must_use]
    pub const fn is_registered(&self) -> bool {
        matches!(self, Self::Registered { .. })
    }

    #[must_use]
    pub const fn is_anonymous(&self) -> bool {
        matches!(self, Self::Anonymous { .. })
    }

    #[must_use]
    pub const fn is_temporary(&self) -> bool {
        matches!(self, Self::Temporary { .. })
    }

    #[must_use]
    pub const fn is_newcomer_like(&self) -> bool {
        matches!(self, Self::Anonymous { .. } | Self::Temporary { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditEvent {
    pub wiki_id: String,
    pub title: String,
    pub namespace: i32,
    pub rev_id: u64,
    pub old_rev_id: Option<u64>,
    pub performer: EditorIdentity,
    pub timestamp_ms: i64,
    pub is_bot: FlagState,
    pub is_minor: FlagState,
    pub is_new_page: FlagState,
    pub tags: Vec<String>,
    pub comment: Option<String>,
    pub byte_delta: i32,
    #[serde(default)]
    pub is_patrolled: FlagState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScoreWeights {
    pub anonymous_user: i32,
    pub temporary_account: i32,
    pub new_page: i32,
    pub reverted_before: i32,
    pub large_content_removal: i32,
    pub profanity: i32,
    pub link_spam: i32,
    pub trusted_user: i32,
    pub bot_like_edit: i32,
    pub liftwing_risk: i32,
    pub warning_history: i32,
    pub obvious_vandalism: i32,
    pub duplicate_pattern: i32,
}

impl Default for ScoreWeights {
    fn default() -> Self {
        Self {
            anonymous_user: 25,
            temporary_account: 20,
            new_page: 20,
            reverted_before: 30,
            large_content_removal: 15,
            profanity: 30,
            link_spam: 20,
            trusted_user: -40,
            bot_like_edit: -50,
            liftwing_risk: 35,
            warning_history: 25,
            obvious_vandalism: 35,
            duplicate_pattern: 18,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScoringConfig {
    pub base_score: i32,
    pub max_score: i32,
    pub weights: ScoreWeights,
}

impl Default for ScoringConfig {
    fn default() -> Self {
        Self {
            base_score: 0,
            max_score: 100,
            weights: ScoreWeights::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScoringSignal {
    AnonymousUser,
    TemporaryAccount,
    NewPage,
    RevertedBefore,
    LargeContentRemoval,
    Profanity,
    LinkSpam,
    TrustedUser,
    BotLikeEdit,
    LiftWingRisk,
    WarningHistory,
    ObviousVandalism,
    DuplicatePattern,
}

impl std::fmt::Display for ScoringSignal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::AnonymousUser => "Anonymous user",
            Self::TemporaryAccount => "Temporary account",
            Self::NewPage => "New page",
            Self::RevertedBefore => "Reverted before",
            Self::LargeContentRemoval => "Large content removal",
            Self::Profanity => "Profanity",
            Self::LinkSpam => "Link spam",
            Self::TrustedUser => "Trusted user",
            Self::BotLikeEdit => "Bot-like edit",
            Self::LiftWingRisk => "LiftWing risk",
            Self::WarningHistory => "Warning history",
            Self::ObviousVandalism => "Obvious vandalism",
            Self::DuplicatePattern => "Duplicate pattern",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignalContribution {
    pub signal: ScoringSignal,
    pub weight: i32,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompositeScore {
    pub total: i32,
    pub contributions: Vec<SignalContribution>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WarningLevel {
    None,
    Level1,
    Level2,
    Level3,
    Level4,
    Final,
}

impl std::fmt::Display for WarningLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::None => "None",
            Self::Level1 => "Level 1",
            Self::Level2 => "Level 2",
            Self::Level3 => "Level 3",
            Self::Level4 => "Level 4",
            Self::Final => "Final",
        })
    }
}

impl WarningLevel {
    #[must_use]
    pub const fn severity(self) -> i32 {
        match self {
            Self::None => 0,
            Self::Level1 => 1,
            Self::Level2 => 2,
            Self::Level3 => 3,
            Self::Level4 | Self::Final => 4,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserRiskProfile {
    pub warning_level: WarningLevel,
    pub warning_count: u32,
    pub has_recent_vandalism_templates: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ScoringContext {
    pub user_risk: Option<UserRiskProfile>,
    pub liftwing_risk: Option<f32>,
    #[serde(default)]
    pub trust_override: FlagState,
    pub duplicate_cluster_size: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueHeuristicPolicy {
    #[serde(default)]
    pub trusted_usernames: Vec<String>,
    #[serde(default = "queue_heuristics_enabled")]
    pub duplicate_cluster_boost: FlagState,
}

impl Default for QueueHeuristicPolicy {
    fn default() -> Self {
        Self {
            trusted_usernames: Vec::new(),
            duplicate_cluster_boost: FlagState::Enabled,
        }
    }
}

fn queue_heuristics_enabled() -> FlagState {
    FlagState::Enabled
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedEdit {
    pub event: EditEvent,
    pub score: CompositeScore,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    Rollback,
    Revert,
    Warn,
    Report,
    MarkPatrolled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikiConfig {
    pub wiki_id: String,
    pub display_name: String,
    pub api_url: Url,
    pub eventstreams_url: Url,
    pub oauth_authorize_url: Url,
    pub oauth_token_url: Url,
    pub liftwing_url: Option<Url>,
    pub coordination_url: Option<Url>,
    #[serde(default)]
    pub namespace_allowlist: Vec<i32>,
    #[serde(default)]
    pub scoring: ScoringConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub url: Url,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerSentEvent {
    pub event_type: Option<String>,
    pub id: Option<String>,
    pub data: String,
    pub retry_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WebSocketFrame {
    Text(String),
    Binary(Vec<u8>),
    Close,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CoordinationMessage {
    ActionBroadcast(ActionBroadcast),
    EditClaim(EditClaim),
    ScoreDelta(ScoreDelta),
    PresenceHeartbeat(PresenceHeartbeat),
    FlaggedEdit(FlaggedEdit),
    RaceResolution(RaceResolution),
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LocalOAuthSourceReport {
    pub file_name: String,
    pub source_path: Option<String>,
    pub loaded_from_source: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerDebugSummary {
    pub project: String,
    pub auth: crate::dev_auth::DevAuthSessionStatus,
    pub oauth: crate::dev_auth::LocalOAuthConfigStatus,
    pub capabilities: crate::dev_auth::DevAuthCapabilityReport,
    pub coordination: CoordinationSnapshot,
}

#[cfg(test)]
mod tests {
    use super::{LocalOAuthSourceReport, ScoringSignal, WarningLevel};

    #[test]
    fn local_oauth_source_report_serializes_without_requiring_path_disclosure() {
        let report = LocalOAuthSourceReport {
            file_name: ".env.wikimedia.local".to_string(),
            source_path: None,
            loaded_from_source: true,
        };

        let encoded = serde_json::to_value(&report).expect("report should serialize");

        assert_eq!(
            encoded.get("file_name").and_then(serde_json::Value::as_str),
            Some(".env.wikimedia.local")
        );
        assert_eq!(
            encoded
                .get("loaded_from_source")
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn scoring_signal_display_produces_human_readable_labels() {
        assert_eq!(ScoringSignal::AnonymousUser.to_string(), "Anonymous user");
        assert_eq!(ScoringSignal::NewPage.to_string(), "New page");
        assert_eq!(ScoringSignal::RevertedBefore.to_string(), "Reverted before");
        assert_eq!(
            ScoringSignal::LargeContentRemoval.to_string(),
            "Large content removal"
        );
        assert_eq!(ScoringSignal::Profanity.to_string(), "Profanity");
        assert_eq!(ScoringSignal::LinkSpam.to_string(), "Link spam");
        assert_eq!(ScoringSignal::TrustedUser.to_string(), "Trusted user");
        assert_eq!(ScoringSignal::BotLikeEdit.to_string(), "Bot-like edit");
        assert_eq!(ScoringSignal::LiftWingRisk.to_string(), "LiftWing risk");
        assert_eq!(ScoringSignal::WarningHistory.to_string(), "Warning history");
    }

    #[test]
    fn warning_level_display_produces_human_readable_labels() {
        assert_eq!(WarningLevel::None.to_string(), "None");
        assert_eq!(WarningLevel::Level1.to_string(), "Level 1");
        assert_eq!(WarningLevel::Level2.to_string(), "Level 2");
        assert_eq!(WarningLevel::Level3.to_string(), "Level 3");
        assert_eq!(WarningLevel::Level4.to_string(), "Level 4");
        assert_eq!(WarningLevel::Final.to_string(), "Final");
    }
}
