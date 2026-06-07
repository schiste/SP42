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
    pub link_addition: i32,
    pub reference_addition: i32,
    pub category_addition: i32,
    pub interwiki_addition: i32,
    pub mass_blanking: i32,
    pub inserted_profanity: i32,
    pub repeated_character_noise: i32,
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
        crate::scoring_policy::default_active_compiled_scoring_policy()
            .scoring_config
            .weights
            .clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScoringSignalParameters {
    pub large_content_removal_threshold: i32,
    pub massive_blanking_threshold: i32,
    pub repeated_character_run_threshold: u8,
    pub profanity_markers: Vec<String>,
    pub link_markers: Vec<String>,
    pub trusted_tags: Vec<String>,
    pub revert_tags: Vec<String>,
    pub suspicious_comment_markers: Vec<String>,
}

impl Default for ScoringSignalParameters {
    fn default() -> Self {
        crate::scoring_policy::default_active_compiled_scoring_policy()
            .scoring_config
            .signal_parameters
            .clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScoringIdentityConfig {
    pub contribution_cap: Option<i32>,
    pub anonymous_modifier_enabled: FlagState,
    pub temporary_modifier_enabled: FlagState,
    pub account_age_modifier_enabled: FlagState,
}

impl Default for ScoringIdentityConfig {
    fn default() -> Self {
        crate::scoring_policy::default_active_compiled_scoring_policy()
            .scoring_config
            .identity
            .clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScoringCombinationRule {
    pub slug: String,
    pub weight: i32,
    pub when_all: Vec<ScoringSignal>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScoringExternalEvaluationConfig {
    pub liftwing_enabled: FlagState,
    pub liftwing_max_contribution: i32,
}

impl Default for ScoringExternalEvaluationConfig {
    fn default() -> Self {
        crate::scoring_policy::default_active_compiled_scoring_policy()
            .scoring_config
            .external_evaluation
            .clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScoringConfig {
    pub base_score: i32,
    pub max_score: i32,
    pub identity: ScoringIdentityConfig,
    pub weights: ScoreWeights,
    #[serde(default)]
    pub signal_parameters: ScoringSignalParameters,
    #[serde(default)]
    pub combination_rules: Vec<ScoringCombinationRule>,
    #[serde(default)]
    pub external_evaluation: ScoringExternalEvaluationConfig,
}

impl Default for ScoringConfig {
    fn default() -> Self {
        crate::scoring_policy::default_active_compiled_scoring_policy()
            .scoring_config
            .clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScoringSignal {
    AnonymousUser,
    TemporaryAccount,
    NewPage,
    RevertedBefore,
    LargeContentRemoval,
    LinkAddition,
    ReferenceAddition,
    CategoryAddition,
    InterwikiAddition,
    MassBlanking,
    InsertedProfanity,
    RepeatedCharacterNoise,
    Profanity,
    LinkSpam,
    TrustedUser,
    BotLikeEdit,
    LiftWingRisk,
    WarningHistory,
    ObviousVandalism,
    DuplicatePattern,
    CombinationRule,
    IdentityCapAdjustment,
}

impl std::fmt::Display for ScoringSignal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::AnonymousUser => "Anonymous user",
            Self::TemporaryAccount => "Temporary account",
            Self::NewPage => "New page",
            Self::RevertedBefore => "Reverted before",
            Self::LargeContentRemoval => "Large content removal",
            Self::LinkAddition => "Link addition",
            Self::ReferenceAddition => "Reference addition",
            Self::CategoryAddition => "Category addition",
            Self::InterwikiAddition => "Interwiki addition",
            Self::MassBlanking => "Mass blanking",
            Self::InsertedProfanity => "Inserted profanity",
            Self::RepeatedCharacterNoise => "Repeated character noise",
            Self::Profanity => "Profanity",
            Self::LinkSpam => "Link spam",
            Self::TrustedUser => "Trusted user",
            Self::BotLikeEdit => "Bot-like edit",
            Self::LiftWingRisk => "LiftWing risk",
            Self::WarningHistory => "Warning history",
            Self::ObviousVandalism => "Obvious vandalism",
            Self::DuplicatePattern => "Duplicate pattern",
            Self::CombinationRule => "Combination rule",
            Self::IdentityCapAdjustment => "Identity cap adjustment",
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
    #[serde(default)]
    pub link_addition_only: FlagState,
    #[serde(default)]
    pub reference_addition_only: FlagState,
    #[serde(default)]
    pub category_addition_only: FlagState,
    #[serde(default)]
    pub interwiki_addition_only: FlagState,
    #[serde(default)]
    pub mass_blanking_detected: FlagState,
    #[serde(default)]
    pub inserted_profanity_detected: FlagState,
    #[serde(default)]
    pub repeated_character_noise_detected: FlagState,
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
    #[serde(default = "default_scoring_policy_ref")]
    pub scoring_policy_ref: String,
    #[serde(default)]
    pub scoring: ScoringConfig,
    #[serde(default)]
    pub templates: WikiTemplates,
}

fn default_scoring_policy_ref() -> String {
    "active/frwiki-vandalism".to_string()
}

/// Per-wiki template names for tagging actions. Each value is the short
/// template name without braces (e.g. `"refnec"` → `{{refnec}}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikiTemplates {
    #[serde(default = "default_citation_needed")]
    pub citation_needed: String,
}

impl Default for WikiTemplates {
    fn default() -> Self {
        Self {
            citation_needed: default_citation_needed(),
        }
    }
}

fn default_citation_needed() -> String {
    "Référence nécessaire".to_string()
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LocalOAuthSourceReport {
    pub file_name: String,
    pub source_path: Option<String>,
    pub loaded_from_source: bool,
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
