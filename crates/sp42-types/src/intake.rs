//! Intake contracts — ADR-0026 §3–§6.
//!
//! Data, not mechanism: the item envelope, the typed field/op/value vocabulary,
//! the composable condition tree, and the routed decision. The evaluator that
//! walks this lives in `sp42-platform::intake_engine`.
//!
//! Two things in here are load-bearing and easy to undo by accident:
//!
//! - **`actor.rights` is `Option`.** A bare `Vec::new()` cannot distinguish "the
//!   adapter never looked the rights up" from "it looked and found none". That
//!   difference is the whole reason `ActorRights Has "sysop"` must not evaluate a
//!   confirmed `False` for an actor nobody checked — it would be a confirmed
//!   non-match, not an absence of one (ADR-0026 §3).
//! - **`on_unknown` is its own field, defaulting to `Drop`.** `on_fail` is itself
//!   an outcome and may be `Admit`, so folding `Unknown` into it would let
//!   missing data ride an admitting branch and admit exactly the item a
//!   restrictive rule meant to hold back (ADR-0026 §4, §6).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::timestamp::Timestamp;

/// A subject as intake sees it, before any candidacy decision.
///
/// `observed_at` is when *this event* happened; `created_at` and
/// `last_revision_at` are facts about the *page*. Conflating them would make
/// "how old is the page" and "how long ago was this edit" indistinguishable
/// (ADR-0026 §3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntakeItem {
    /// The project the event came from.
    pub wiki_id: String,
    /// The event kind, e.g. `edit`, `new`, `log`.
    pub event_type: String,
    /// The namespace of the subject page.
    pub namespace: i32,
    /// The subject page, when the adapter knows it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_id: Option<i64>,
    /// The revision the event refers to, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision_id: Option<i64>,
    /// When the subject page was created. `None` = not fetched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<Timestamp>,
    /// The subject page's most recent revision, as of the adapter's fetch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_revision_at: Option<Timestamp>,
    /// Who made the change.
    pub actor: IntakeActor,
    /// When this event happened.
    pub observed_at: Timestamp,
    /// Where the full payload lives, when it was fetched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_ref: Option<String>,
}

/// The actor behind an [`IntakeItem`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntakeActor {
    /// The account name, when the event carried one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// Rights held by the actor.
    ///
    /// `None` means *not fetched*; `Some(vec![])` means *fetched and confirmed
    /// empty*. A lightweight stream adapter that forwards an edit without a
    /// rights lookup is in the `None` case, and must not be treated as a
    /// confirmed non-match (ADR-0026 §3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rights: Option<Vec<String>>,
    /// Whether the account is flagged as a bot.
    pub is_bot: bool,
    /// When the account was created, when a lookup provided it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_created_at: Option<Timestamp>,
}

/// What a rule is about.
///
/// Closed, so configs can be linted against real fields, with
/// [`IntakeField::Custom`] as the priced-in escape hatch for a domain-local fact
/// the generic envelope does not carry. ADR-0026 §4 rejects free-form dot-paths
/// for exactly the linting reason.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntakeField {
    /// `IntakeItem::event_type`
    EventType,
    /// `IntakeItem::namespace`
    Namespace,
    /// `IntakeItem::page_id`
    PageId,
    /// `IntakeItem::revision_id`
    RevisionId,
    /// `IntakeItem::created_at`
    CreatedAt,
    /// `IntakeItem::last_revision_at`
    LastRevisionAt,
    /// `IntakeActor::username`
    ActorUsername,
    /// `IntakeActor::rights`
    ActorRights,
    /// `IntakeActor::is_bot`
    ActorIsBot,
    /// `IntakeActor::account_created_at`
    ActorAccountCreatedAt,
    /// `IntakeItem::observed_at`
    ObservedAt,
    /// A domain-registered field, resolved through a resolver registry.
    Custom(String),
}

/// How a rule compares.
///
/// `Has`/`NotHas` are for list fields; the four temporal operators are for
/// timestamp fields. Neither set applies to the other, and the engine rejects a
/// mismatched pair rather than letting it silently never match (ADR-0026 §4, §7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntakeOp {
    /// Equality.
    Eq,
    /// Inequality.
    NotEq,
    /// Membership in a set operand.
    In,
    /// Absence from a set operand.
    NotIn,
    /// A list field contains the operand.
    Has,
    /// A list field does not contain the operand.
    NotHas,
    /// Strictly before an absolute instant.
    Before,
    /// Strictly after an absolute instant.
    After,
    /// Older than `now - duration`.
    OlderThan,
    /// Younger than `now - duration`.
    YoungerThan,
}

/// A rule's right-hand side.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntakeValue {
    /// A single string.
    Str(String),
    /// A single integer.
    Int(i64),
    /// A set of strings.
    StrList(Vec<String>),
    /// A set of integers.
    IntList(Vec<i64>),
    /// A boolean.
    Bool(bool),
    /// An absolute instant, for `Before`/`After`.
    Timestamp(i64),
    /// A span, for `OlderThan`/`YoungerThan`.
    Duration(i64),
    /// A named wiki-relative fact, resolved through the capability profile.
    CapabilityRef(String),
}

/// One predicate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntakeRule {
    /// What is being tested.
    pub field: IntakeField,
    /// How it is compared.
    pub op: IntakeOp,
    /// Against what.
    pub value: IntakeValue,
}

impl IntakeRule {
    /// A predicate over `field`.
    #[must_use]
    pub fn new(field: IntakeField, op: IntakeOp, value: IntakeValue) -> Self {
        Self { field, op, value }
    }
}

/// A composable predicate tree.
///
/// `All`/`Any`/`Not` are required from day one: ADR-0026's Alternatives records
/// that an AND-only list would force a breaking config migration the first time a
/// genuinely disjunctive policy appears, of which quickfail is the example.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntakeCondition {
    /// Every child holds. Vacuously true when empty.
    All(Vec<IntakeCondition>),
    /// Some child holds. Vacuously unknown when empty.
    Any(Vec<IntakeCondition>),
    /// The child's negation, three-valued.
    Not(Box<IntakeCondition>),
    /// A leaf predicate.
    Rule(IntakeRule),
}

impl IntakeCondition {
    /// An empty conjunction — vacuously `True`, and the unconditional arm
    /// ADR-0028 §5 / ADR-0029 §5 rely on for a ruleset's catch-all.
    #[must_use]
    pub fn always_true() -> Self {
        Self::All(Vec::new())
    }

    /// A single predicate.
    #[must_use]
    pub fn rule(rule: IntakeRule) -> Self {
        Self::Rule(rule)
    }

    /// A conjunction of `conditions`.
    #[must_use]
    pub fn all(conditions: impl IntoIterator<Item = Self>) -> Self {
        Self::All(conditions.into_iter().collect())
    }

    /// A disjunction of `conditions`.
    #[must_use]
    pub fn any(conditions: impl IntoIterator<Item = Self>) -> Self {
        Self::Any(conditions.into_iter().collect())
    }

    /// A three-valued negation.
    ///
    /// Named `negate` rather than `not` so it is not confused with
    /// [`std::ops::Not::not`]; the three-valued semantics are the whole point and
    /// deserve an unmistakable name.
    #[must_use]
    pub fn negate(condition: Self) -> Self {
        Self::Not(Box::new(condition))
    }
}

/// What a pipeline does with an item.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntakeOutcome {
    /// Admit, routing to a queue or workflow.
    Admit {
        /// Where the admitted item goes.
        route_to: String,
    },
    /// Discard.
    Drop,
    /// Hand the item to a different pipeline, which is then evaluated in turn.
    Reclassify {
        /// The pipeline to hand it to.
        pipeline: String,
    },
}

/// A named filter with its routing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntakePipeline {
    /// The pipeline's id, referenced by `Reclassify` and by workflow configs.
    pub id: String,
    /// The predicate to evaluate.
    pub condition: IntakeCondition,
    /// Outcome when the condition is `True`.
    pub on_pass: IntakeOutcome,
    /// Outcome when the condition is `False`.
    pub on_fail: IntakeOutcome,
    /// Outcome when the condition is `Unknown`.
    ///
    /// `None` means the conservative default, `Drop`. It is deliberately a
    /// separate field rather than inherited from `on_fail`, because `on_fail` may
    /// itself be `Admit` and inheriting it would reopen the fail-open gap §4
    /// exists to close (ADR-0026 §4, §6).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_unknown: Option<IntakeOutcome>,
}

impl IntakePipeline {
    /// The outcome for a definite result.
    #[must_use]
    pub fn outcome_for(&self, known: bool) -> &IntakeOutcome {
        if known { &self.on_pass } else { &self.on_fail }
    }

    /// The outcome for an unknown result, defaulting to `Drop`.
    ///
    /// The default is applied here and nowhere else, so there is exactly one
    /// place the fail-closed choice is made.
    #[must_use]
    pub fn unknown_outcome(&self) -> IntakeOutcome {
        self.on_unknown.clone().unwrap_or(IntakeOutcome::Drop)
    }
}

/// The routed result of evaluating a pipeline.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntakeDecision {
    /// A definite routing decision, with the provenance to reproduce it.
    Routed {
        /// What the item should do.
        outcome: IntakeOutcome,
        /// Which pipeline decided.
        pipeline_id: String,
        /// The leaf predicates that fired, in evaluation order.
        matched_rule_path: Vec<String>,
        /// Which compiled pipeline decided — needed to replay the decision
        /// against the ruleset version that was live, not merely to audit which
        /// rules fired.
        config_version: String,
    },
    /// The pipeline could not be evaluated at all, for a reason that is a
    /// configuration fault rather than a property of the item.
    ///
    /// Deliberately *not* a `Drop`: a broken filter must be distinguishable from
    /// an intentional one (ADR-0026 §6). Collapsing the two would make a
    /// silently broken ruleset indistinguishable from a restrictive one, and the
    /// operator would see a clean stream rather than a fault.
    Misconfigured {
        /// Which pipeline could not be evaluated.
        pipeline_id: String,
        /// What was wrong with it.
        reason: IntakeMisconfiguration,
    },
}

/// Why a pipeline could not be evaluated.
///
/// Each variant is a defect in the *configuration* or the *environment*, never a
/// statement about the item. That is the distinction [`IntakeDecision`] turns on:
/// an item the rules reject is `Routed`, and a ruleset that cannot answer is
/// `Misconfigured`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "detail")]
pub enum IntakeMisconfiguration {
    /// A `CapabilityRef` did not resolve against the current discovered
    /// profile.
    ///
    /// Load-time-valid but evaluation-time-broken: a Tier-A drift removed a
    /// namespace the ruleset named (ADR-0026 §5).
    UnresolvedCapability {
        /// The reference that would not resolve.
        reference: String,
    },
    /// A rule named a `Custom` field that no registered domain supplies.
    UnregisteredCustomField {
        /// The field name the rule used.
        field: String,
    },
    /// A rule reached the evaluator that the compatibility table rejects, so it
    /// could never have evaluated as its author intended.
    ///
    /// Only reachable for a hand-built pipeline; [`compile_pipeline`] rejects
    /// these at load time, so this is defence in depth.
    ///
    /// [`compile_pipeline`]: https://docs.rs/sp42-platform
    InvalidRule {
        /// A human-readable description of the incompatibility.
        detail: String,
    },
    /// A `Custom` field resolved to a value shape its rule cannot compare.
    ///
    /// Distinct from [`Self::UnregisteredCustomField`]: the field exists, but the
    /// domain returns the wrong type for the operator the rule uses.
    UnusableCustomField {
        /// The field name the rule used.
        field: String,
        /// The operator the rule used.
        op: IntakeOp,
        /// The shape the registry actually produced.
        resolved: String,
    },
}

impl IntakeDecision {
    /// The routed outcome, or `None` when misconfigured.
    #[must_use]
    pub fn outcome(&self) -> Option<&IntakeOutcome> {
        match self {
            Self::Routed { outcome, .. } => Some(outcome),
            Self::Misconfigured { .. } => None,
        }
    }
}

/// A resolved field value, as seen by the evaluator.
///
/// Field resolution is separate from rule evaluation so that the closed
/// [`IntakeField`] enum and domain-registered [`IntakeField::Custom`] fields
/// produce the same shape, and so the registry can be swapped per domain without
/// touching the tree walker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolvedField {
    /// A single string.
    Text(String),
    /// A single integer.
    Number(i64),
    /// A list of strings.
    TextList(Vec<String>),
    /// A boolean.
    Flag(bool),
    /// An instant.
    Instant(Timestamp),
    /// Not available on this item — the absent case, distinct from a present
    /// empty or zero value.
    Absent,
}

impl ResolvedField {
    /// Whether the field carries no usable value.
    ///
    /// This is what makes the three-valued evaluation meaningful: `Absent` is
    /// the only thing that produces `Unknown`. A present-but-empty list is a
    /// *known* empty list.
    #[must_use]
    pub fn is_absent(&self) -> bool {
        matches!(self, Self::Absent)
    }
}

/// The resolver registry a caller supplies for `Custom` fields.
///
/// Declared here rather than in `sp42-platform` so that a domain can implement
/// it without the engine owning a trait it never needs to know the shape of.
pub trait IntakeFieldResolver: Send + Sync {
    /// Resolve a `Custom(String)` field for this item.
    ///
    /// Returning `Ok(ResolvedField::Absent)` is a legitimate answer — the field
    /// is registered but this item does not carry it — and evaluates to `Unknown`
    /// rather than `False`.
    ///
    /// # Errors
    ///
    /// Returns [`IntakeResolveError::UnregisteredField`] when the name is not
    /// registered at all, which is a configuration bug the linter should have
    /// caught (§7) and which must not read as a non-match.
    fn resolve(&self, field: &str, item: &IntakeItem) -> Result<ResolvedField, IntakeResolveError>;
}

/// Why a `Custom` field could not be resolved.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum IntakeResolveError {
    /// The field name is not registered by any domain.
    #[error("custom intake field {0:?} is not registered by any domain")]
    UnregisteredField(String),
    /// A capability reference did not resolve against the current profile.
    ///
    /// Distinct from an empty resolution on purpose: an empty set would make a
    /// rule written to exclude something exclude nothing, and read as a
    /// confirmed answer.
    #[error("capability reference {0:?} did not resolve against the current wiki profile")]
    UnresolvedCapability(String),
}

/// Wiki-relative facts a rule can name, e.g. "the mainspace namespaces for
/// this wiki" (ADR-0026 §5).
///
/// A capability reference resolves against a *discovered* profile, so a value can
/// legitimately fail to resolve at evaluation time — a Tier-A drift broke a
/// reference that was valid at load time. That is a `Misconfigured` decision,
/// not a non-match (ADR-0026 §6).
pub trait CapabilityResolver: Send + Sync {
    /// Resolve a reference to a set of comparable values.
    ///
    /// # Errors
    ///
    /// Returns [`IntakeResolveError::UnresolvedCapability`] when the name does
    /// not resolve against the current profile. The caller must turn that into
    /// `IntakeDecision::Misconfigured` rather than evaluating it as `False`.
    fn resolve(&self, reference: &str) -> Result<BTreeSet<IntakeValue>, IntakeResolveError>;
}

/// A resolver for deployments with no capability references, which rejects them
/// rather than resolving to an empty set — an empty set would silently admit
/// everything a rule was written to exclude.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoCapabilities;

impl CapabilityResolver for NoCapabilities {
    fn resolve(&self, reference: &str) -> Result<BTreeSet<IntakeValue>, IntakeResolveError> {
        Err(IntakeResolveError::UnresolvedCapability(
            reference.to_string(),
        ))
    }
}

/// An empty resolver, for callers with no `Custom` fields.
///
/// Any `Custom` field then errors rather than silently matching, which is the
/// fail-closed direction.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoCustomFields;

impl IntakeFieldResolver for NoCustomFields {
    fn resolve(
        &self,
        field: &str,
        _item: &IntakeItem,
    ) -> Result<ResolvedField, IntakeResolveError> {
        Err(IntakeResolveError::UnregisteredField(field.to_string()))
    }
}

/// The registry a deployment actually wires up, keyed by field name.
#[derive(Clone, Debug, Default)]
pub struct IntakeFieldRegistry {
    entries: BTreeMap<String, ResolvedField>,
}

impl IntakeFieldRegistry {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a field's value for one item.
    #[must_use]
    pub fn with(mut self, field: impl Into<String>, value: ResolvedField) -> Self {
        self.entries.insert(field.into(), value);
        self
    }

    /// Every registered name, for the linter's benefit (ADR-0026 §7).
    pub fn registered(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        IntakeCondition, IntakeDecision, IntakeField, IntakeItem, IntakeMisconfiguration, IntakeOp,
        IntakeOutcome, IntakePipeline, IntakeRule, IntakeValue, ResolvedField,
    };
    use crate::timestamp::Timestamp;

    fn item() -> IntakeItem {
        IntakeItem {
            wiki_id: "frwiki".to_string(),
            event_type: "edit".to_string(),
            namespace: 0,
            page_id: Some(4242),
            revision_id: Some(99),
            created_at: Some(Timestamp::from_epoch_ms(0)),
            last_revision_at: Some(Timestamp::from_epoch_ms(1_000)),
            actor: super::IntakeActor {
                username: Some("Ada".to_string()),
                rights: None,
                is_bot: false,
                account_created_at: None,
            },
            observed_at: Timestamp::from_epoch_ms(2_000),
            payload_ref: None,
        }
    }

    #[test]
    fn actor_rights_distinguishes_not_fetched_from_confirmed_empty() {
        let mut subject = item();
        subject.actor.rights = None;
        assert_eq!(subject.actor.rights, None, "None means not fetched");

        subject.actor.rights = Some(Vec::new());
        assert_eq!(
            subject.actor.rights,
            Some(Vec::new()),
            "Some(empty) means fetched and confirmed empty"
        );
    }

    #[test]
    fn on_unknown_defaults_to_drop_and_never_inherits_on_fail() {
        let admit_on_fail = IntakePipeline {
            id: "restrictive-baseline".to_string(),
            condition: IntakeCondition::always_true(),
            on_pass: IntakeOutcome::Admit {
                route_to: "review".to_string(),
            },
            // The dangerous shape: an admitting failure branch.
            on_fail: IntakeOutcome::Admit {
                route_to: "admitted-by-default".to_string(),
            },
            on_unknown: None,
        };
        assert_eq!(
            admit_on_fail.unknown_outcome(),
            IntakeOutcome::Drop,
            "an Unknown must not ride an admitting on_fail"
        );
    }

    #[test]
    fn an_explicit_on_unknown_is_honoured_verbatim() {
        let explicit = IntakePipeline {
            id: "p".to_string(),
            condition: IntakeCondition::always_true(),
            on_pass: IntakeOutcome::Drop,
            on_fail: IntakeOutcome::Drop,
            on_unknown: Some(IntakeOutcome::Admit {
                route_to: "asked-for".to_string(),
            }),
        };
        assert_eq!(
            explicit.unknown_outcome(),
            IntakeOutcome::Admit {
                route_to: "asked-for".to_string()
            }
        );
    }

    #[test]
    fn absent_is_distinct_from_a_present_but_empty_value() {
        assert!(ResolvedField::Absent.is_absent());
        assert!(!ResolvedField::TextList(Vec::new()).is_absent());
        assert!(!ResolvedField::Flag(false).is_absent());
        assert!(!ResolvedField::Number(0).is_absent());
    }

    #[test]
    fn misconfigured_is_not_an_outcome() {
        let misconfigured = IntakeDecision::Misconfigured {
            pipeline_id: "p".to_string(),
            reason: IntakeMisconfiguration::UnresolvedCapability {
                reference: "mainspace".to_string(),
            },
        };
        assert!(misconfigured.outcome().is_none());

        let routed = IntakeDecision::Routed {
            outcome: IntakeOutcome::Drop,
            pipeline_id: "p".to_string(),
            matched_rule_path: vec!["a".to_string()],
            config_version: "abc".to_string(),
        };
        assert_eq!(routed.outcome(), Some(&IntakeOutcome::Drop));
    }

    #[test]
    fn every_misconfiguration_survives_a_round_trip() {
        // Recorded in the same stream as routed decisions, so the shape has to
        // survive storage and replay — an internally tagged enum is easy to get
        // subtly wrong and only shows up once a record is read back.
        for reason in [
            IntakeMisconfiguration::UnresolvedCapability {
                reference: "mainspace".to_string(),
            },
            IntakeMisconfiguration::UnregisteredCustomField {
                field: "edit_count".to_string(),
            },
            IntakeMisconfiguration::InvalidRule {
                detail: "field actor_is_bot does not accept operator OlderThan".to_string(),
            },
            IntakeMisconfiguration::UnusableCustomField {
                field: "custom:edit_count".to_string(),
                op: IntakeOp::Eq,
                resolved: "text".to_string(),
            },
        ] {
            let decision = IntakeDecision::Misconfigured {
                pipeline_id: "baseline".to_string(),
                reason: reason.clone(),
            };
            let json = serde_json::to_string(&decision).expect("serializes");
            assert_eq!(
                serde_json::from_str::<IntakeDecision>(&json).expect("deserializes"),
                decision,
                "round trip failed for {json}"
            );
        }
    }

    #[test]
    fn a_condition_tree_round_trips_through_serde() {
        // Configs are authored as data; the tree has to survive a round trip
        // exactly or a reload changes policy.
        let tree = IntakeCondition::any([
            IntakeCondition::rule(IntakeRule::new(
                IntakeField::EventType,
                IntakeOp::Eq,
                IntakeValue::Str("edit".to_string()),
            )),
            IntakeCondition::rule(IntakeRule::new(
                IntakeField::ActorRights,
                IntakeOp::Has,
                IntakeValue::Str("sysop".to_string()),
            )),
            IntakeCondition::negate(IntakeCondition::all([])),
        ]);
        let json = serde_json::to_string(&tree).expect("serializes");
        let back: IntakeCondition = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(back, tree);
    }

    #[test]
    fn an_absent_page_id_deserializes_to_none_rather_than_failing() {
        // Additive and back-compatible: a payload written before a field existed
        // must still load.
        let minimal = r#"{
            "wiki_id": "frwiki",
            "event_type": "edit",
            "namespace": 0,
            "actor": {"is_bot": false},
            "observed_at": 2000
        }"#;
        let parsed: IntakeItem = serde_json::from_str(minimal).expect("decodes");
        assert_eq!(parsed.page_id, None);
        assert_eq!(parsed.created_at, None);
        assert_eq!(parsed.actor.rights, None);
        assert_eq!(parsed.observed_at, Timestamp::from_epoch_ms(2_000));
    }

    #[test]
    fn a_pipeline_without_on_unknown_loads_with_the_default_applied_at_use() {
        let json = r#"{
            "id": "p",
            "condition": {"all": []},
            "on_pass": {"drop": null},
            "on_fail": {"admit": {"route_to": "x"}}
        }"#;
        let parsed: IntakePipeline = serde_json::from_str(json).expect("decodes");
        assert_eq!(parsed.on_unknown, None);
        assert_eq!(parsed.unknown_outcome(), IntakeOutcome::Drop);
    }
}
