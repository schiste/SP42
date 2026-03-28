//! Human-readable scoring policy and evaluation-profile loaders.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::errors::{ScoringEvaluationError, ScoringPolicyError};
use crate::types::{
    FlagState, QueueHeuristicPolicy, ScoreWeights, ScoringCombinationRule, ScoringConfig,
    ScoringExternalEvaluationConfig, ScoringIdentityConfig, ScoringSignal, ScoringSignalParameters,
};

const ACTIVE_FRWIKI_VANDALISM_POLICY_YAML: &str =
    include_str!("../../../configs/scoring/active/frwiki-vandalism.yaml");
const CANDIDATE_FRWIKI_TIGHTEN_IDENTITY_CAP_YAML: &str =
    include_str!("../../../configs/scoring/candidate/frwiki-vandalism-tighten-identity-cap.yaml");

const REQUIRED_RULES: [&str; 14] = [
    "anonymous_user",
    "temporary_account",
    "new_page",
    "reverted_before",
    "large_content_removal",
    "link_addition",
    "profanity",
    "link_spam",
    "trusted_user",
    "bot_like_edit",
    "liftwing_risk",
    "warning_history",
    "obvious_vandalism",
    "duplicate_pattern",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScoringDomain {
    VandalismPatrol,
    ArticleQuality,
    SourcingReview,
    StructuredData,
    Maintenance,
    PolicyCompliance,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyLifecycle {
    Active,
    Candidate,
    Suggested,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoringDimensionWeights {
    pub content: f32,
    pub actor: f32,
    pub subject: f32,
    pub context: f32,
    pub temporal: f32,
    pub policy: f32,
    pub external_evaluation: f32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityPolicyConfig {
    pub contribution_cap: i32,
    pub anonymous_modifier_enabled: bool,
    pub temporary_modifier_enabled: bool,
    pub account_age_modifier_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuePolicyConfig {
    pub default_limit: u16,
    pub max_limit: u16,
    pub min_score_cutoff: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RulePolicyConfig {
    pub enabled: bool,
    pub weight: i32,
    pub threshold: Option<f64>,
    pub max_bonus: Option<i32>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignalParametersPolicyConfig {
    pub large_content_removal_threshold: i32,
    pub massive_blanking_threshold: i32,
    pub repeated_character_run_threshold: u8,
    pub profanity_markers: Vec<String>,
    pub link_markers: Vec<String>,
    pub trusted_tags: Vec<String>,
    pub revert_tags: Vec<String>,
    pub suspicious_comment_markers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CombinationRulePolicy {
    pub slug: String,
    pub enabled: bool,
    pub weight: i32,
    pub when_all: Vec<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiftWingPolicyConfig {
    pub enabled: bool,
    pub role: ExternalEvaluatorRole,
    pub max_contribution: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalEvaluatorRole {
    Disabled,
    SupportingSignal,
    TieBreaker,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ExternalEvaluationPolicyConfig {
    pub liftwing: Option<LiftWingPolicyConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FairnessPolicyConfig {
    pub max_newcomer_fp_regression_bps: u32,
    pub max_temporary_fp_regression_bps: u32,
    pub max_anonymous_fp_regression_bps: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoringPolicyDocument {
    pub domain: ScoringDomain,
    pub wiki_id: String,
    pub policy_version: String,
    pub evaluation_profile: String,
    pub inherits_from: Option<String>,
    pub lifecycle: PolicyLifecycle,
    pub dimensions: ScoringDimensionWeights,
    pub identity: IdentityPolicyConfig,
    pub queue: QueuePolicyConfig,
    pub signal_parameters: SignalParametersPolicyConfig,
    pub rules: BTreeMap<String, RulePolicyConfig>,
    #[serde(default)]
    pub combination_rules: Vec<CombinationRulePolicy>,
    #[serde(default)]
    pub external_evaluation_config: ExternalEvaluationPolicyConfig,
    pub fairness: FairnessPolicyConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvaluationFixtureSetPaths {
    pub regression: String,
    pub ranking: String,
    pub invariants: String,
    pub fairness: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvaluationFairnessProfile {
    pub track_newcomers: bool,
    pub track_temporary: bool,
    pub track_anonymous: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScoringEvaluationProfile {
    pub name: String,
    pub domain: String,
    pub required_gates: Vec<String>,
    pub fixture_sets: EvaluationFixtureSetPaths,
    pub fairness: EvaluationFairnessProfile,
    pub performance_budget_ms: u32,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompiledScoringPolicy {
    pub domain: ScoringDomain,
    pub wiki_id: String,
    pub policy_version: String,
    pub evaluation_profile: String,
    pub lifecycle: PolicyLifecycle,
    pub dimensions: ScoringDimensionWeights,
    pub scoring_config: ScoringConfig,
    pub queue_policy: QueueHeuristicPolicy,
    pub queue_defaults: QueuePolicyConfig,
    pub combination_rules: Vec<CombinationRulePolicy>,
    pub external_evaluation_config: ExternalEvaluationPolicyConfig,
    pub fairness: FairnessPolicyConfig,
}

/// Return the embedded compiled default active scoring policy used whenever the
/// runtime needs a stable hot-path baseline.
///
/// # Panics
///
/// Panics if the embedded active policy fixture is missing or fails validation
/// and compilation. That is treated as a build-time correctness failure.
#[must_use]
pub fn default_active_compiled_scoring_policy() -> &'static CompiledScoringPolicy {
    static DEFAULT_ACTIVE_POLICY: OnceLock<CompiledScoringPolicy> = OnceLock::new();
    DEFAULT_ACTIVE_POLICY.get_or_init(|| {
        load_embedded_compiled_scoring_policy("active/frwiki-vandalism")
            .expect("embedded active scoring policy should compile")
    })
}

/// Load and compile an embedded scoring policy by its repository reference.
///
/// # Errors
///
/// Returns [`ScoringPolicyError`] if the reference is unknown or the embedded
/// policy fails validation or compilation.
pub fn load_embedded_compiled_scoring_policy(
    reference: &str,
) -> Result<CompiledScoringPolicy, ScoringPolicyError> {
    let Some(yaml) = embedded_policy_source(reference) else {
        return Err(ScoringPolicyError::UnknownReference {
            reference: reference.to_string(),
        });
    };
    let document = parse_scoring_policy(yaml)?;
    compile_scoring_policy(&document)
}

/// Parse, deserialize, and validate a scoring policy document.
///
/// # Errors
///
/// Returns [`ScoringPolicyError`] if the YAML is invalid or if the decoded
/// policy fails constitutional validation.
pub fn parse_scoring_policy(yaml: &str) -> Result<ScoringPolicyDocument, ScoringPolicyError> {
    let document = serde_yaml::from_str::<ScoringPolicyDocument>(yaml)?;
    validate_scoring_policy(&document)?;
    Ok(document)
}

/// Compile a validated scoring policy into the hot-path runtime structures used
/// by the scorer.
///
/// # Errors
///
/// Returns [`ScoringPolicyError`] if the supplied document fails validation.
pub fn compile_scoring_policy(
    document: &ScoringPolicyDocument,
) -> Result<CompiledScoringPolicy, ScoringPolicyError> {
    validate_scoring_policy(document)?;
    let liftwing_weight = compile_liftwing_weight(document);
    let weights = ScoreWeights {
        anonymous_user: rule_weight(document, "anonymous_user"),
        temporary_account: rule_weight(document, "temporary_account"),
        new_page: rule_weight(document, "new_page"),
        reverted_before: rule_weight(document, "reverted_before"),
        large_content_removal: rule_weight(document, "large_content_removal"),
        link_addition: rule_weight(document, "link_addition"),
        profanity: rule_weight(document, "profanity"),
        link_spam: rule_weight(document, "link_spam"),
        trusted_user: rule_weight(document, "trusted_user"),
        bot_like_edit: rule_weight(document, "bot_like_edit"),
        liftwing_risk: liftwing_weight,
        warning_history: rule_weight(document, "warning_history"),
        obvious_vandalism: rule_weight(document, "obvious_vandalism"),
        duplicate_pattern: rule_weight(document, "duplicate_pattern"),
    };
    let scoring_config = ScoringConfig {
        base_score: 0,
        max_score: 100,
        identity: ScoringIdentityConfig {
            contribution_cap: Some(document.identity.contribution_cap),
            anonymous_modifier_enabled: document.identity.anonymous_modifier_enabled.into(),
            temporary_modifier_enabled: document.identity.temporary_modifier_enabled.into(),
            account_age_modifier_enabled: document.identity.account_age_modifier_enabled.into(),
        },
        weights,
        signal_parameters: ScoringSignalParameters {
            large_content_removal_threshold: document
                .signal_parameters
                .large_content_removal_threshold,
            massive_blanking_threshold: document.signal_parameters.massive_blanking_threshold,
            repeated_character_run_threshold: document
                .signal_parameters
                .repeated_character_run_threshold,
            profanity_markers: document.signal_parameters.profanity_markers.clone(),
            link_markers: document.signal_parameters.link_markers.clone(),
            trusted_tags: document.signal_parameters.trusted_tags.clone(),
            revert_tags: document.signal_parameters.revert_tags.clone(),
            suspicious_comment_markers: document
                .signal_parameters
                .suspicious_comment_markers
                .clone(),
        },
        combination_rules: compile_combination_rules(document)?,
        external_evaluation: ScoringExternalEvaluationConfig {
            liftwing_enabled: FlagState::from(
                document
                    .external_evaluation_config
                    .liftwing
                    .as_ref()
                    .is_some_and(|config| config.enabled),
            ),
            liftwing_max_contribution: document
                .external_evaluation_config
                .liftwing
                .as_ref()
                .map_or(0, |config| config.max_contribution),
        },
    };
    let queue_policy = QueueHeuristicPolicy {
        trusted_usernames: Vec::new(),
        duplicate_cluster_boost: FlagState::from(
            document
                .rules
                .get("duplicate_pattern")
                .is_none_or(|rule| rule.enabled),
        ),
    };

    Ok(CompiledScoringPolicy {
        domain: document.domain.clone(),
        wiki_id: document.wiki_id.clone(),
        policy_version: document.policy_version.clone(),
        evaluation_profile: document.evaluation_profile.clone(),
        lifecycle: document.lifecycle,
        dimensions: document.dimensions.clone(),
        scoring_config,
        queue_policy,
        queue_defaults: document.queue.clone(),
        combination_rules: document.combination_rules.clone(),
        external_evaluation_config: document.external_evaluation_config.clone(),
        fairness: document.fairness.clone(),
    })
}

/// Parse, deserialize, and validate a scoring evaluation profile.
///
/// # Errors
///
/// Returns [`ScoringEvaluationError`] if the YAML is invalid or the decoded
/// profile fails validation.
pub fn parse_scoring_evaluation_profile(
    yaml: &str,
) -> Result<ScoringEvaluationProfile, ScoringEvaluationError> {
    let profile = serde_yaml::from_str::<ScoringEvaluationProfile>(yaml)?;
    validate_scoring_evaluation_profile(&profile)?;
    Ok(profile)
}

/// Validate a scoring policy document against the constitutional structural
/// requirements used by the runtime compiler.
///
/// # Errors
///
/// Returns [`ScoringPolicyError`] when required metadata, queue settings, or
/// rule definitions are missing or inconsistent.
pub fn validate_scoring_policy(document: &ScoringPolicyDocument) -> Result<(), ScoringPolicyError> {
    if document.wiki_id.trim().is_empty() {
        return Err(ScoringPolicyError::InvalidField {
            field: "wiki_id",
            message: "must not be blank".to_string(),
        });
    }
    if document.policy_version.trim().is_empty() {
        return Err(ScoringPolicyError::InvalidField {
            field: "policy_version",
            message: "must not be blank".to_string(),
        });
    }
    if document.evaluation_profile.trim().is_empty() {
        return Err(ScoringPolicyError::InvalidField {
            field: "evaluation_profile",
            message: "must not be blank".to_string(),
        });
    }
    if document.identity.contribution_cap < 0 {
        return Err(ScoringPolicyError::InvalidField {
            field: "identity.contribution_cap",
            message: "must not be negative".to_string(),
        });
    }
    if document.queue.default_limit == 0 || document.queue.max_limit == 0 {
        return Err(ScoringPolicyError::InvalidField {
            field: "queue",
            message: "default_limit and max_limit must be positive".to_string(),
        });
    }
    if document.queue.default_limit > document.queue.max_limit {
        return Err(ScoringPolicyError::InvalidField {
            field: "queue",
            message: "default_limit must not exceed max_limit".to_string(),
        });
    }
    if document.signal_parameters.repeated_character_run_threshold < 2 {
        return Err(ScoringPolicyError::InvalidField {
            field: "signal_parameters.repeated_character_run_threshold",
            message: "must be at least 2".to_string(),
        });
    }
    for required in REQUIRED_RULES {
        if !document.rules.contains_key(required) {
            return Err(ScoringPolicyError::MissingRule { rule: required });
        }
    }
    validate_marker_list(
        "signal_parameters.profanity_markers",
        &document.signal_parameters.profanity_markers,
    )?;
    validate_marker_list(
        "signal_parameters.link_markers",
        &document.signal_parameters.link_markers,
    )?;
    validate_marker_list(
        "signal_parameters.trusted_tags",
        &document.signal_parameters.trusted_tags,
    )?;
    validate_marker_list(
        "signal_parameters.revert_tags",
        &document.signal_parameters.revert_tags,
    )?;
    validate_marker_list(
        "signal_parameters.suspicious_comment_markers",
        &document.signal_parameters.suspicious_comment_markers,
    )?;
    for combination in &document.combination_rules {
        if combination.slug.trim().is_empty() {
            return Err(ScoringPolicyError::InvalidField {
                field: "combination_rules.slug",
                message: "must not be blank".to_string(),
            });
        }
        if combination.when_all.is_empty() {
            return Err(ScoringPolicyError::InvalidField {
                field: "combination_rules.when_all",
                message: "must not be empty".to_string(),
            });
        }
    }
    Ok(())
}

/// Validate a scoring evaluation profile before it is admitted into CI or local
/// evaluation runs.
///
/// # Errors
///
/// Returns [`ScoringEvaluationError`] when required gate metadata or performance
/// budget fields are missing or invalid.
pub fn validate_scoring_evaluation_profile(
    profile: &ScoringEvaluationProfile,
) -> Result<(), ScoringEvaluationError> {
    if profile.name.trim().is_empty() {
        return Err(ScoringEvaluationError::InvalidField {
            field: "name",
            message: "must not be blank".to_string(),
        });
    }
    if profile.domain.trim().is_empty() {
        return Err(ScoringEvaluationError::InvalidField {
            field: "domain",
            message: "must not be blank".to_string(),
        });
    }
    if profile.required_gates.is_empty() {
        return Err(ScoringEvaluationError::InvalidField {
            field: "required_gates",
            message: "must not be empty".to_string(),
        });
    }
    if profile.performance_budget_ms == 0 {
        return Err(ScoringEvaluationError::InvalidField {
            field: "performance_budget_ms",
            message: "must be positive".to_string(),
        });
    }
    Ok(())
}

fn rule_weight(document: &ScoringPolicyDocument, slug: &'static str) -> i32 {
    let Some(rule) = document.rules.get(slug) else {
        return 0;
    };
    if rule.enabled { rule.weight } else { 0 }
}

fn embedded_policy_source(reference: &str) -> Option<&'static str> {
    match reference {
        "active/frwiki-vandalism" => Some(ACTIVE_FRWIKI_VANDALISM_POLICY_YAML),
        "candidate/frwiki-vandalism-tighten-identity-cap" => {
            Some(CANDIDATE_FRWIKI_TIGHTEN_IDENTITY_CAP_YAML)
        }
        _ => None,
    }
}

fn compile_combination_rules(
    document: &ScoringPolicyDocument,
) -> Result<Vec<ScoringCombinationRule>, ScoringPolicyError> {
    let mut compiled = Vec::new();
    for rule in &document.combination_rules {
        if !rule.enabled {
            continue;
        }
        let mut when_all = Vec::with_capacity(rule.when_all.len());
        for signal_slug in &rule.when_all {
            when_all.push(signal_from_slug(signal_slug)?);
        }
        compiled.push(ScoringCombinationRule {
            slug: rule.slug.clone(),
            weight: rule.weight,
            when_all,
            notes: rule.notes.clone(),
        });
    }
    Ok(compiled)
}

fn compile_liftwing_weight(document: &ScoringPolicyDocument) -> i32 {
    let configured = rule_weight(document, "liftwing_risk");
    let Some(liftwing) = &document.external_evaluation_config.liftwing else {
        return configured;
    };
    if !liftwing.enabled || matches!(liftwing.role, ExternalEvaluatorRole::Disabled) {
        return 0;
    }
    configured.clamp(-liftwing.max_contribution, liftwing.max_contribution)
}

fn signal_from_slug(slug: &str) -> Result<ScoringSignal, ScoringPolicyError> {
    match slug {
        "anonymous_user" => Ok(ScoringSignal::AnonymousUser),
        "temporary_account" => Ok(ScoringSignal::TemporaryAccount),
        "new_page" => Ok(ScoringSignal::NewPage),
        "reverted_before" => Ok(ScoringSignal::RevertedBefore),
        "large_content_removal" => Ok(ScoringSignal::LargeContentRemoval),
        "link_addition" => Ok(ScoringSignal::LinkAddition),
        "profanity" => Ok(ScoringSignal::Profanity),
        "link_spam" => Ok(ScoringSignal::LinkSpam),
        "trusted_user" => Ok(ScoringSignal::TrustedUser),
        "bot_like_edit" => Ok(ScoringSignal::BotLikeEdit),
        "liftwing_risk" => Ok(ScoringSignal::LiftWingRisk),
        "warning_history" => Ok(ScoringSignal::WarningHistory),
        "obvious_vandalism" => Ok(ScoringSignal::ObviousVandalism),
        "duplicate_pattern" => Ok(ScoringSignal::DuplicatePattern),
        _ => Err(ScoringPolicyError::InvalidField {
            field: "combination_rules.when_all",
            message: format!("unknown scoring signal `{slug}`"),
        }),
    }
}

fn validate_marker_list(field: &'static str, markers: &[String]) -> Result<(), ScoringPolicyError> {
    if markers.is_empty() {
        return Err(ScoringPolicyError::InvalidField {
            field,
            message: "must not be empty".to_string(),
        });
    }
    if markers.iter().any(|marker| marker.trim().is_empty()) {
        return Err(ScoringPolicyError::InvalidField {
            field,
            message: "must not contain blank entries".to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        PolicyLifecycle, ScoringDomain, compile_scoring_policy,
        default_active_compiled_scoring_policy, parse_scoring_evaluation_profile,
        parse_scoring_policy,
    };

    #[test]
    fn parses_and_compiles_scoring_policy() {
        let policy = parse_scoring_policy(include_str!(
            "../../../configs/scoring/active/frwiki-vandalism.yaml"
        ))
        .expect("policy should parse");
        let compiled = compile_scoring_policy(&policy).expect("policy should compile");

        assert_eq!(policy.domain, ScoringDomain::VandalismPatrol);
        assert_eq!(policy.lifecycle, PolicyLifecycle::Active);
        assert_eq!(compiled.queue_defaults.default_limit, 25);
        assert_eq!(compiled.scoring_config.identity.contribution_cap, Some(25));
        assert_eq!(
            compiled
                .scoring_config
                .signal_parameters
                .large_content_removal_threshold,
            -500
        );
        assert_eq!(compiled.scoring_config.combination_rules.len(), 2);
        assert!(compiled.queue_policy.duplicate_cluster_boost.is_enabled());
    }

    #[test]
    fn parses_evaluation_profile() {
        let profile = parse_scoring_evaluation_profile(include_str!(
            "../../../evals/scoring/profiles/vandalism_patrol_default.yaml"
        ))
        .expect("profile should parse");

        assert_eq!(profile.name, "vandalism_patrol_default");
        assert!(profile.fairness.track_newcomers);
        assert!(
            profile
                .required_gates
                .iter()
                .any(|gate| gate == "fairness_checks")
        );
    }

    #[test]
    fn loads_default_embedded_active_policy() {
        let compiled = default_active_compiled_scoring_policy();

        assert_eq!(compiled.wiki_id, "frwiki");
        assert_eq!(compiled.policy_version, "v0.1");
        assert_eq!(compiled.scoring_config.identity.contribution_cap, Some(25));
    }
}
