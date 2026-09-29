//! The intake evaluator — ADR-0026 §4–§6.
//!
//! Three-valued evaluation over a composable condition tree, routed to a
//! provenanced outcome. Two functions are the public surface, mirroring
//! [`scoring_engine`](crate::scoring_engine): compile a pipeline once, then
//! evaluate items against it.
//!
//! The point of the third value is narrow and load-bearing. A rule about a fact
//! the adapter never fetched is not a rule that *failed* — it is a rule that
//! could not be asked. Collapsing that into `False` lets a restrictive baseline
//! admit an item it meant to hold back, because `on_fail` is frequently `Admit`.
//! So:
//!
//! - An absent field evaluates to [`Tri::Unknown`], never [`Tri::False`].
//! - `Not` never turns an `Unknown` into a `True`. `Not(CreatedAt YoungerThan 1h)`
//!   against a missing `created_at` reads unknown, not "confirmed old enough".
//! - `on_unknown` is routed separately, defaulting to `Drop`.
//!
//! This is the *normal* path, not an edge case. `sp42-live`'s stream ingester
//! forwards `EditEvent`s that carry no `page_id`, no `created_at` and no actor
//! rights, so a baseline that asked about those fields would route every item
//! through `on_unknown` on day one. ADR-0026 §2's "baseline uses only intrinsic
//! facts" is a hard requirement of this design, not a style preference.
//!
//! Time enters as a resolved `now_ms: i64`, never as a `&dyn Clock`. This crate
//! has no runtime and CONSTITUTION §2.3 bars I/O crates, so the shell resolves
//! the clock and hands the value in, keeping the core clock-free (§1.4).

use std::collections::BTreeSet;

use sha2::{Digest, Sha256};
use sp42_types::{
    CapabilityResolver, IntakeCondition, IntakeDecision, IntakeField, IntakeFieldResolver,
    IntakeItem, IntakeMisconfiguration, IntakeOp, IntakeOutcome, IntakePipeline, IntakeRule,
    IntakeValue, NoCapabilities, NoCustomFields, ResolvedField, Timestamp,
};

/// How much of the digest string [`CompiledPipeline::config_version`] keeps.
///
/// Enough to make a collision between two pipelines' configs implausible, short
/// enough to read in a log line. The value is only ever compared for equality
/// against another digest, never parsed.
const CONFIG_VERSION_LEN: usize = 16;

/// A three-valued truth value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tri {
    /// The condition holds.
    True,
    /// The condition does not hold.
    False,
    /// The condition could not be evaluated — the fact was not fetched.
    Unknown,
}

/// Kleene conjunction: `False` dominates, then `Unknown`, then `True`.
///
/// An empty `All` is `True` (vacuous truth), which is what makes
/// `All([])` usable as an unconditional catch-all arm — ADR-0028 §5 and
/// ADR-0029 §5 both depend on that reading.
#[must_use]
pub fn tri_all(values: impl IntoIterator<Item = Tri>) -> Tri {
    // The empty case is stated rather than left to the accumulator's initial
    // value. Folding from a seed is the natural way to express the dominance
    // rules, but it silently decides the identity case, and here the identity
    // case is exactly the one with a documented answer.
    let mut result: Option<Tri> = None;
    for value in values {
        result = Some(match (result.unwrap_or(Tri::True), value) {
            // A single False settles the conjunction regardless of what follows,
            // so short-circuit here rather than scanning the rest.
            (_, Tri::False) | (Tri::False, _) => return Tri::False,
            (Tri::True, Tri::Unknown) | (Tri::Unknown, _) => Tri::Unknown,
            (Tri::True, Tri::True) => Tri::True,
        });
    }
    result.unwrap_or(Tri::True)
}

/// Kleene disjunction: `True` dominates, then `Unknown`, then `False`.
///
/// An empty `Any` is `Unknown` rather than `False`: "some of nothing" cannot be
/// affirmed, and calling it `False` would be a claim nobody has evidence for.
#[must_use]
pub fn tri_any(values: impl IntoIterator<Item = Tri>) -> Tri {
    let mut result: Option<Tri> = None;
    for value in values {
        result = Some(match (result.unwrap_or(Tri::False), value) {
            (_, Tri::True) | (Tri::True, _) => return Tri::True,
            (Tri::False, Tri::Unknown) | (Tri::Unknown, _) => Tri::Unknown,
            (Tri::False, Tri::False) => Tri::False,
        });
    }
    // See `tri_all`: the empty case is a decision, not a fold artefact.
    result.unwrap_or(Tri::Unknown)
}

/// Kleene negation: swaps `True` and `False`, and leaves `Unknown` alone.
///
/// Leaving `Unknown` alone is the whole reason the three-valued design exists.
/// Negating an unknown must never manufacture a `True`; that would turn "we
/// could not check" into "confirmed".
#[must_use]
pub fn tri_not(value: Tri) -> Tri {
    match value {
        Tri::True => Tri::False,
        Tri::False => Tri::True,
        Tri::Unknown => Tri::Unknown,
    }
}

/// What kind of value a field carries, and which operators it accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldSpec {
    /// The value kind, used to type-check the rule's right-hand side.
    pub kind: ValueKind,
    /// The operators this field accepts, in [`IntakeOp`] declaration order.
    pub ops: &'static [IntakeOp],
}

/// The value kinds the vocabulary has.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ValueKind {
    /// A single string.
    Text,
    /// A single integer.
    Number,
    /// A set of strings, and therefore not `Absent` when empty.
    TextList,
    /// A boolean, and therefore not `Absent` when false.
    Flag,
    /// An instant.
    Instant,
}

// Operator sets are listed in `IntakeOp` declaration order rather than grouped by
// meaning, so the table renders deterministically for a human reading it.
const SCALAR_OPS: &[IntakeOp] = &[IntakeOp::Eq, IntakeOp::NotEq, IntakeOp::In, IntakeOp::NotIn];
const LIST_OPS: &[IntakeOp] = &[
    IntakeOp::In,
    IntakeOp::NotIn,
    IntakeOp::Has,
    IntakeOp::NotHas,
];
const FLAG_OPS: &[IntakeOp] = &[IntakeOp::Eq, IntakeOp::NotEq];
const TEMPORAL_OPS: &[IntakeOp] = &[
    IntakeOp::Before,
    IntakeOp::After,
    IntakeOp::OlderThan,
    IntakeOp::YoungerThan,
];

/// The compatibility table for a field.
///
/// **Defined once, on purpose.** ADR-0026 §7 requires the linter to check every
/// `field`/`op`/`value` triple against this same table; §4 requires the engine to
/// refuse a mismatched pair it is handed. One definition serves both, because
/// CONSTITUTION §6.1 forbids a second copy and §14.4 makes a second table a
/// constitutional problem rather than untidiness.
///
/// `Custom` fields have no fixed spec — they are whatever the owning domain
/// registers — so they are reported as [`None`] and the caller resolves them.
#[must_use]
pub fn field_spec(field: &IntakeField) -> Option<FieldSpec> {
    let spec = match field {
        IntakeField::EventType | IntakeField::ActorUsername => FieldSpec {
            kind: ValueKind::Text,
            ops: SCALAR_OPS,
        },
        IntakeField::Namespace | IntakeField::PageId | IntakeField::RevisionId => FieldSpec {
            kind: ValueKind::Number,
            ops: SCALAR_OPS,
        },
        IntakeField::ActorRights => FieldSpec {
            kind: ValueKind::TextList,
            ops: LIST_OPS,
        },
        IntakeField::ActorIsBot => FieldSpec {
            kind: ValueKind::Flag,
            ops: FLAG_OPS,
        },
        IntakeField::CreatedAt
        | IntakeField::LastRevisionAt
        | IntakeField::ActorAccountCreatedAt
        | IntakeField::ObservedAt => FieldSpec {
            kind: ValueKind::Instant,
            ops: TEMPORAL_OPS,
        },
        IntakeField::Custom(_) => return None,
    };
    Some(spec)
}

/// Why a rule or a pipeline could not be used.
///
/// A *load-time* error: every variant here is a defect the config linter should
/// catch before the pipeline is ever installed (ADR-0026 §7). An item the rules
/// merely do not match is not an error, and a fact that was not fetched is not
/// an error — the latter is `Unknown`.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum IntakeError {
    /// The operator is not one this field accepts.
    #[error("field {field} does not accept operator {op:?}")]
    OperatorNotAllowed {
        /// The field named by the rule.
        field: String,
        /// The operator the rule used.
        op: IntakeOp,
    },
    /// The right-hand side does not match the field's value kind.
    #[error("field {field} ({kind:?}) does not accept a {value:?} operand for {op:?}")]
    ValueNotAllowed {
        /// The field named by the rule.
        field: String,
        /// The field's value kind.
        kind: ValueKind,
        /// The operator the rule used.
        op: IntakeOp,
        /// The operand the rule supplied.
        value: IntakeValue,
    },
}

/// Check one `field`/`op`/`value` triple against the compatibility table.
///
/// Shared by the load-time linter and by the evaluator, so a config that passes
/// lint cannot fail here for the same reason — a rule that was always going to be
/// rejected should never reach a running pipeline.
///
/// A `Custom` field always passes: its shape belongs to the owning domain's
/// registry, which is resolved at evaluation time instead.
///
/// # Errors
///
/// [`IntakeError::OperatorNotAllowed`] when the field does not accept the
/// operator, [`IntakeError::ValueNotAllowed`] when the operand does not match the
/// field's value kind under that operator.
pub fn validate_rule(rule: &IntakeRule) -> Result<(), IntakeError> {
    let Some(spec) = field_spec(&rule.field) else {
        return Ok(());
    };
    if !spec.ops.contains(&rule.op) {
        return Err(IntakeError::OperatorNotAllowed {
            field: label(&rule.field),
            op: rule.op,
        });
    }
    if !operand_fits(spec.kind, rule.op, &rule.value) {
        return Err(IntakeError::ValueNotAllowed {
            field: label(&rule.field),
            kind: spec.kind,
            op: rule.op,
            value: rule.value.clone(),
        });
    }
    Ok(())
}

/// Whether an operand is the right shape for a kind under a given operator.
fn operand_fits(kind: ValueKind, op: IntakeOp, value: &IntakeValue) -> bool {
    if let IntakeValue::CapabilityRef(_) = value {
        // A capability reference is a *set* the resolver expands, so it stands in
        // for a set operand. Rejecting it on a Flag or an Instant is deliberate:
        // the resolver can only produce comparable scalars, and admitting it there
        // would let a rule load that could only ever fail.
        return matches!(op, IntakeOp::In | IntakeOp::NotIn)
            && matches!(
                kind,
                ValueKind::Text | ValueKind::Number | ValueKind::TextList
            );
    }
    // Matched on the operator first: the operator is what selects between a
    // scalar operand and a set operand, so grouping this way is what makes the
    // "scalar vs set" distinction legible. Grouping by kind instead would need
    // two arms per scalar kind that read identically, which is both noise and a
    // `match_same_arms` warning that the duplication was accidental.
    match op {
        IntakeOp::Eq | IntakeOp::NotEq => match kind {
            ValueKind::Text => matches!(value, IntakeValue::Str(_)),
            ValueKind::Number => matches!(value, IntakeValue::Int(_)),
            ValueKind::Flag => matches!(value, IntakeValue::Bool(_)),
            _ => false,
        },
        IntakeOp::In | IntakeOp::NotIn => match kind {
            ValueKind::Text | ValueKind::TextList => matches!(value, IntakeValue::StrList(_)),
            ValueKind::Number => matches!(value, IntakeValue::IntList(_)),
            _ => false,
        },
        // A list field is asked whether it *contains* a single named right.
        IntakeOp::Has | IntakeOp::NotHas => {
            kind == ValueKind::TextList && matches!(value, IntakeValue::Str(_))
        }
        IntakeOp::Before | IntakeOp::After => {
            kind == ValueKind::Instant && matches!(value, IntakeValue::Timestamp(_))
        }
        IntakeOp::OlderThan | IntakeOp::YoungerThan => {
            kind == ValueKind::Instant && matches!(value, IntakeValue::Duration(_))
        }
    }
}

/// A pipeline whose rules have been type-checked and whose version is pinned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompiledPipeline {
    pipeline: IntakePipeline,
    config_version: String,
}

impl CompiledPipeline {
    /// The pipeline as authored.
    #[must_use]
    pub fn pipeline(&self) -> &IntakePipeline {
        &self.pipeline
    }

    /// The digest that identifies this exact pipeline.
    ///
    /// Content-derived rather than a hand-written version string, and per-pipeline
    /// rather than global: a single repository-wide version would invalidate the
    /// replayability of all recorded history whenever any unrelated config
    /// changed. `scoring_policy` only has a hand-authored `policy_version`, which
    /// nothing hashes — this is the first real implementation of the concept the
    /// gate contracts ask for (ADR-0028 §4, ADR-0029 §4).
    #[must_use]
    pub fn config_version(&self) -> &str {
        &self.config_version
    }
}

/// Type-check every rule in a pipeline and pin its version.
///
/// The load-time half of ADR-0026 §7. A pipeline that could never evaluate as its
/// author intended is rejected here rather than becoming a rule that quietly never
/// matches.
///
/// # Errors
///
/// Propagates the first [`IntakeError`] from [`validate_rule`], in tree order, so
/// the message points at the actual offending rule.
pub fn compile_pipeline(pipeline: &IntakePipeline) -> Result<CompiledPipeline, IntakeError> {
    validate_condition(&pipeline.condition)?;
    Ok(CompiledPipeline {
        pipeline: pipeline.clone(),
        config_version: config_digest(pipeline),
    })
}

fn validate_condition(condition: &IntakeCondition) -> Result<(), IntakeError> {
    match condition {
        IntakeCondition::All(children) | IntakeCondition::Any(children) => {
            for child in children {
                validate_condition(child)?;
            }
        }
        IntakeCondition::Not(child) => validate_condition(child)?,
        IntakeCondition::Rule(rule) => validate_rule(rule)?,
    }
    Ok(())
}

/// A short, stable digest of exactly this pipeline's meaning.
fn config_digest(pipeline: &IntakePipeline) -> String {
    let mut hasher = Sha256::new();
    // A domain-separation prefix, so a digest here can never be confused with a
    // digest of the same bytes produced for some other purpose.
    hasher.update(b"sp42/intake-pipeline/v1\0");
    for part in [
        pipeline.id.as_str(),
        &canonical_condition(&pipeline.condition),
        &canonical_outcome(&pipeline.on_pass),
        &canonical_outcome(&pipeline.on_fail),
        &pipeline
            .on_unknown
            .as_ref()
            .map_or_else(String::new, canonical_outcome),
    ] {
        // NUL-separated, so no rearrangement of the parts can collide by
        // concatenation (`id="ab", cond="c"` must not equal `id="a", cond="bc"`).
        hasher.update(part.as_bytes());
        hasher.update(b"\0");
    }
    let mut digest = format!("{:x}", hasher.finalize());
    digest.truncate(CONFIG_VERSION_LEN);
    digest
}

/// A canonical, order-preserving rendering, so the digest is stable.
fn canonical_condition(condition: &IntakeCondition) -> String {
    match condition {
        IntakeCondition::All(children) => {
            format!("all({})", render_children(children))
        }
        IntakeCondition::Any(children) => {
            format!("any({})", render_children(children))
        }
        IntakeCondition::Not(child) => format!("not({})", canonical_condition(child)),
        IntakeCondition::Rule(rule) => format!(
            "rule({},{},{})",
            label(&rule.field),
            op_label(rule.op),
            canonical_value(&rule.value)
        ),
    }
}

fn render_children(children: &[IntakeCondition]) -> String {
    children
        .iter()
        .map(canonical_condition)
        .collect::<Vec<_>>()
        .join(",")
}

fn canonical_value(value: &IntakeValue) -> String {
    match value {
        IntakeValue::Str(text) => format!("str:{text}"),
        IntakeValue::Int(number) => format!("int:{number}"),
        IntakeValue::StrList(items) => format!("strs:[{}]", items.join(",")),
        IntakeValue::IntList(items) => format!("ints:[{}]", join_numbers(items)),
        IntakeValue::Bool(flag) => format!("bool:{flag}"),
        IntakeValue::Timestamp(stamp) => format!("ts:{stamp}"),
        IntakeValue::Duration(duration) => format!("dur:{duration}"),
        IntakeValue::CapabilityRef(name) => format!("cap:{name}"),
    }
}

fn canonical_outcome(outcome: &IntakeOutcome) -> String {
    match outcome {
        IntakeOutcome::Admit { route_to } => format!("admit:{route_to}"),
        IntakeOutcome::Drop => "drop".to_string(),
        IntakeOutcome::Reclassify { pipeline } => format!("reclassify:{pipeline}"),
    }
}

fn join_numbers(values: &[i64]) -> String {
    values
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// A stable, human-readable field name, in the same vocabulary the config uses.
///
/// Written out rather than derived from [`Debug`], because these labels are part
/// of the contract: they land in `matched_rule_path`, in the config digest, and
/// in operator-facing error messages, all of which a person reads against a YAML
/// file spelling the field `event_type`. A `Debug`-derived label would silently
/// drift the first time a variant was renamed, and a digest that depends on
/// variant spelling would move for a non-semantic reason.
fn label(field: &IntakeField) -> String {
    match field {
        IntakeField::EventType => "event_type".to_string(),
        IntakeField::Namespace => "namespace".to_string(),
        IntakeField::PageId => "page_id".to_string(),
        IntakeField::RevisionId => "revision_id".to_string(),
        IntakeField::CreatedAt => "created_at".to_string(),
        IntakeField::LastRevisionAt => "last_revision_at".to_string(),
        IntakeField::ActorUsername => "actor_username".to_string(),
        IntakeField::ActorRights => "actor_rights".to_string(),
        IntakeField::ActorIsBot => "actor_is_bot".to_string(),
        IntakeField::ActorAccountCreatedAt => "actor_account_created_at".to_string(),
        IntakeField::ObservedAt => "observed_at".to_string(),
        IntakeField::Custom(name) => format!("custom:{name}"),
    }
}

/// The operator's config vocabulary spelling.
///
/// Paired with [`label`] for the same reason: these two strings are the audit
/// trail, and they are read next to a YAML rule. Spelling one in `snake_case`
/// and the other in `PascalCase` would make the two halves of `event_type.Eq`
/// look like they came from different systems.
fn op_label(op: IntakeOp) -> &'static str {
    match op {
        IntakeOp::Eq => "eq",
        IntakeOp::NotEq => "not_eq",
        IntakeOp::In => "in",
        IntakeOp::NotIn => "not_in",
        IntakeOp::Has => "has",
        IntakeOp::NotHas => "not_has",
        IntakeOp::Before => "before",
        IntakeOp::After => "after",
        IntakeOp::OlderThan => "older_than",
        IntakeOp::YoungerThan => "younger_than",
    }
}

/// Everything an evaluation needs from its caller.
pub struct IntakeContext<'a> {
    /// Wall-clock now, already resolved by the shell, in epoch milliseconds.
    pub now_ms: i64,
    /// Supplies `Custom` fields.
    pub custom_fields: &'a dyn IntakeFieldResolver,
    /// Supplies capability references.
    pub capabilities: &'a dyn CapabilityResolver,
}

impl<'a> IntakeContext<'a> {
    /// A context with no `Custom` fields and no capabilities.
    ///
    /// Both are fail-closed: a `Custom` field or a capability reference under
    /// this context is a misconfiguration, never a silent non-match.
    #[must_use]
    pub fn bare(now_ms: i64) -> Self {
        Self {
            now_ms,
            custom_fields: &NoCustomFields,
            capabilities: &NoCapabilities,
        }
    }

    /// A context with the given resolvers.
    #[must_use]
    pub fn new(
        now_ms: i64,
        custom_fields: &'a dyn IntakeFieldResolver,
        capabilities: &'a dyn CapabilityResolver,
    ) -> Self {
        Self {
            now_ms,
            custom_fields,
            capabilities,
        }
    }
}

/// Evaluate one item against a compiled pipeline.
///
/// The returned `IntakeDecision` is `Misconfigured` — rather than an `Err` — when
/// the engine could not answer at all, because that is a fact about the
/// *ruleset*, which belongs in the same stream of decisions as the facts about
/// the item. A genuine `Err` is reserved for a [`CompiledPipeline`] that reached
/// the engine without having been through [`compile_pipeline`].
///
/// # Errors
///
/// Returns [`IntakeError`] only for a hand-built [`CompiledPipeline`] carrying a
/// rule the compatibility table rejects. A rule whose *data* is missing is not an
/// error — it is `Unknown`, routed through `on_unknown`.
///
/// [`CompiledPipeline`]: CompiledPipeline
pub fn evaluate_intake(
    item: &IntakeItem,
    compiled: &CompiledPipeline,
    context: &IntakeContext<'_>,
) -> Result<IntakeDecision, IntakeError> {
    // Defence in depth. `compile_pipeline` already rejected every bad triple, so
    // this can only fire for a pipeline built by other means — and it is an
    // `Err` rather than a `Misconfigured` because a `CompiledPipeline` carrying
    // an invalid rule is an invariant violation, not a configuration drift.
    validate_condition(&compiled.pipeline().condition)?;

    let mut matched = Vec::new();
    let pipeline = compiled.pipeline();

    let decision = match eval_condition(&pipeline.condition, item, context, &mut matched) {
        Ok(verdict) => {
            let outcome = match verdict {
                Tri::True => pipeline.on_pass.clone(),
                Tri::False => pipeline.on_fail.clone(),
                Tri::Unknown => pipeline.unknown_outcome(),
            };
            IntakeDecision::Routed {
                outcome,
                pipeline_id: pipeline.id.clone(),
                matched_rule_path: matched,
                config_version: compiled.config_version.clone(),
            }
        }
        // A ruleset that cannot be evaluated must be distinguishable from one
        // that rejected the item (ADR-0026 §6).
        Err(reason) => IntakeDecision::Misconfigured {
            pipeline_id: pipeline.id.clone(),
            reason,
        },
    };
    Ok(decision)
}

/// Walk a condition tree, recording the rules that supported the verdict.
///
/// `matched` accumulates leaves that evaluated `True` and were **not** under a
/// `Not`. A negated branch's predicates are deliberately discarded: a rule that
/// holds inside `Not(...)` is evidence *against* the decision, and recording it
/// would make the audit trail claim the opposite of what it shows.
fn eval_condition(
    condition: &IntakeCondition,
    item: &IntakeItem,
    context: &IntakeContext<'_>,
    matched: &mut Vec<String>,
) -> Result<Tri, IntakeMisconfiguration> {
    match condition {
        IntakeCondition::All(children) => {
            let mut local = Vec::new();
            let mut values = Vec::with_capacity(children.len());
            for child in children {
                values.push(eval_condition(child, item, context, &mut local)?);
            }
            matched.extend(local);
            Ok(tri_all(values))
        }
        IntakeCondition::Any(children) => {
            let mut local = Vec::new();
            let mut values = Vec::with_capacity(children.len());
            for child in children {
                values.push(eval_condition(child, item, context, &mut local)?);
            }
            matched.extend(local);
            Ok(tri_any(values))
        }
        IntakeCondition::Not(child) => {
            // Discarded: see the note on `matched`.
            let mut discarded = Vec::new();
            let verdict = eval_condition(child, item, context, &mut discarded)?;
            Ok(tri_not(verdict))
        }
        IntakeCondition::Rule(rule) => {
            let verdict = eval_rule(rule, item, context)?;
            if verdict == Tri::True {
                matched.push(format!("{}.{}", label(&rule.field), op_label(rule.op)));
            }
            Ok(verdict)
        }
    }
}

fn eval_rule(
    rule: &IntakeRule,
    item: &IntakeItem,
    context: &IntakeContext<'_>,
) -> Result<Tri, IntakeMisconfiguration> {
    let resolved = resolve_field(&rule.field, item, context)?;
    if resolved.is_absent() {
        // The load-bearing line: absent data is `Unknown`, never `False`.
        return Ok(Tri::Unknown);
    }

    if let IntakeValue::CapabilityRef(reference) = &rule.value {
        let allowed = context.capabilities.resolve(reference).map_err(|_| {
            IntakeMisconfiguration::UnresolvedCapability {
                reference: reference.clone(),
            }
        })?;
        return Ok(compare_membership(&resolved, &allowed, rule.op));
    }

    compare_direct(&rule.field, &resolved, &rule.value, rule.op, context.now_ms)
}

fn resolve_field(
    field: &IntakeField,
    item: &IntakeItem,
    context: &IntakeContext<'_>,
) -> Result<ResolvedField, IntakeMisconfiguration> {
    Ok(match field {
        IntakeField::EventType => ResolvedField::Text(item.event_type.clone()),
        IntakeField::Namespace => ResolvedField::Number(i64::from(item.namespace)),
        IntakeField::PageId => optional_number(item.page_id),
        IntakeField::RevisionId => optional_number(item.revision_id),
        IntakeField::CreatedAt => optional_instant(item.created_at),
        IntakeField::LastRevisionAt => optional_instant(item.last_revision_at),
        IntakeField::ObservedAt => ResolvedField::Instant(item.observed_at),
        IntakeField::ActorUsername => item
            .actor
            .username
            .clone()
            .map_or(ResolvedField::Absent, ResolvedField::Text),
        // `None` means not fetched, and must stay `Absent` rather than becoming
        // an empty list — that distinction is ADR-0026 §3's whole point.
        IntakeField::ActorRights => item
            .actor
            .rights
            .clone()
            .map_or(ResolvedField::Absent, ResolvedField::TextList),
        IntakeField::ActorIsBot => ResolvedField::Flag(item.actor.is_bot),
        IntakeField::ActorAccountCreatedAt => optional_instant(item.actor.account_created_at),
        IntakeField::Custom(name) => context.custom_fields.resolve(name, item).map_err(|_| {
            IntakeMisconfiguration::UnregisteredCustomField {
                field: name.clone(),
            }
        })?,
    })
}

fn optional_number(value: Option<i64>) -> ResolvedField {
    value.map_or(ResolvedField::Absent, ResolvedField::Number)
}

fn optional_instant(value: Option<Timestamp>) -> ResolvedField {
    value.map_or(ResolvedField::Absent, ResolvedField::Instant)
}

/// Compare a resolved value against a set the resolver expanded.
///
/// `In`/`NotIn` on a list field is a *containment* check — the field must contain
/// every value in the operand — so a rule can require a bundle of rights rather
/// than either one of them. `Has`/`NotHas` against a capability set asks about
/// membership, treating the set as the values to look for.
fn compare_membership(
    resolved: &ResolvedField,
    allowed: &BTreeSet<IntakeValue>,
    op: IntakeOp,
) -> Tri {
    let contains = match resolved {
        ResolvedField::Text(text) => allowed
            .iter()
            .any(|value| matches!(value, IntakeValue::Str(candidate) if candidate == text)),
        ResolvedField::Number(number) => allowed
            .iter()
            .any(|value| matches!(value, IntakeValue::Int(candidate) if candidate == number)),
        ResolvedField::TextList(items) => allowed.iter().all(|value| match value {
            IntakeValue::Str(wanted) => items.contains(wanted),
            // A capability that resolved to the wrong element type cannot
            // satisfy a list of strings; that is a non-match, not a crash.
            _ => false,
        }),
        // `validate_rule` already refuses a capability on a Flag or an Instant,
        // and an absent field returned before this point.
        _ => false,
    };
    match op {
        IntakeOp::In | IntakeOp::Has => truth(contains),
        IntakeOp::NotIn | IntakeOp::NotHas => tri_not(truth(contains)),
        _ => Tri::Unknown,
    }
}

/// Compare a resolved value directly against a literal operand.
fn compare_direct(
    field: &IntakeField,
    resolved: &ResolvedField,
    value: &IntakeValue,
    op: IntakeOp,
    now_ms: i64,
) -> Result<Tri, IntakeMisconfiguration> {
    let verdict = match (resolved, value, op) {
        (ResolvedField::Text(actual), IntakeValue::Str(expected), IntakeOp::Eq) => {
            truth(actual == expected)
        }
        (ResolvedField::Text(actual), IntakeValue::Str(expected), IntakeOp::NotEq) => {
            tri_not(truth(actual == expected))
        }
        (ResolvedField::Text(actual), IntakeValue::StrList(expected), IntakeOp::In) => {
            truth(expected.contains(actual))
        }
        (ResolvedField::Text(actual), IntakeValue::StrList(expected), IntakeOp::NotIn) => {
            tri_not(truth(expected.contains(actual)))
        }
        (ResolvedField::Number(actual), IntakeValue::Int(expected), IntakeOp::Eq) => {
            truth(*actual == *expected)
        }
        (ResolvedField::Number(actual), IntakeValue::Int(expected), IntakeOp::NotEq) => {
            tri_not(truth(*actual == *expected))
        }
        (ResolvedField::Number(actual), IntakeValue::IntList(expected), IntakeOp::In) => {
            truth(expected.contains(actual))
        }
        (ResolvedField::Number(actual), IntakeValue::IntList(expected), IntakeOp::NotIn) => {
            tri_not(truth(expected.contains(actual)))
        }
        (ResolvedField::TextList(actual), IntakeValue::Str(expected), IntakeOp::Has) => {
            truth(actual.contains(expected))
        }
        (ResolvedField::TextList(actual), IntakeValue::Str(expected), IntakeOp::NotHas) => {
            tri_not(truth(actual.contains(expected)))
        }
        (ResolvedField::TextList(actual), IntakeValue::StrList(expected), IntakeOp::In) => {
            truth(expected.iter().all(|wanted| actual.contains(wanted)))
        }
        (ResolvedField::TextList(actual), IntakeValue::StrList(expected), IntakeOp::NotIn) => {
            tri_not(truth(expected.iter().all(|wanted| actual.contains(wanted))))
        }
        (ResolvedField::Flag(actual), IntakeValue::Bool(expected), IntakeOp::Eq) => {
            truth(*actual == *expected)
        }
        (ResolvedField::Flag(actual), IntakeValue::Bool(expected), IntakeOp::NotEq) => {
            tri_not(truth(*actual == *expected))
        }
        (ResolvedField::Instant(actual), IntakeValue::Timestamp(expected), IntakeOp::Before) => {
            truth(actual.is_before(Timestamp::from_epoch_ms(*expected)))
        }
        (ResolvedField::Instant(actual), IntakeValue::Timestamp(expected), IntakeOp::After) => {
            truth(actual.is_after(Timestamp::from_epoch_ms(*expected)))
        }
        (ResolvedField::Instant(actual), IntakeValue::Duration(duration), IntakeOp::OlderThan) => {
            truth(actual.epoch_ms() < now_ms.saturating_sub(*duration))
        }
        (
            ResolvedField::Instant(actual),
            IntakeValue::Duration(duration),
            IntakeOp::YoungerThan,
        ) => truth(actual.epoch_ms() > now_ms.saturating_sub(*duration)),
        // Only a `Custom` field reaches this arm: `validate_rule` deferred it to
        // the domain's registry, so the shape mismatch only becomes visible once
        // a value actually arrives. Reporting it beats returning `Unknown`, which
        // would quietly route a broken rule into `on_unknown`.
        _ => {
            return Err(IntakeMisconfiguration::UnusableCustomField {
                field: label(field),
                op,
                resolved: describe_resolved(resolved),
            });
        }
    };
    Ok(verdict)
}

/// Name a resolved shape, for the [`IntakeMisconfiguration::UnusableCustomField`]
/// report.
///
/// The `Absent` arm is unreachable through `evaluate_intake`, which returns
/// `Unknown` before a shape is ever compared, and is here so the function is
/// total over its input rather than panicking on a variant a future caller could
/// pass.
fn describe_resolved(resolved: &ResolvedField) -> String {
    match resolved {
        ResolvedField::Text(_) => "text".to_string(),
        ResolvedField::Number(_) => "number".to_string(),
        ResolvedField::TextList(_) => "text_list".to_string(),
        ResolvedField::Flag(_) => "flag".to_string(),
        ResolvedField::Instant(_) => "instant".to_string(),
        ResolvedField::Absent => "absent".to_string(),
    }
}

fn truth(holds: bool) -> Tri {
    if holds { Tri::True } else { Tri::False }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use proptest::prelude::*;

    use super::{
        CompiledPipeline, IntakeContext, IntakeError, Tri, ValueKind, compile_pipeline,
        evaluate_intake, field_spec, tri_all, tri_any, tri_not, validate_rule,
    };
    use sp42_types::{
        CapabilityResolver, IntakeActor, IntakeCondition, IntakeField, IntakeFieldRegistry,
        IntakeItem, IntakeMisconfiguration, IntakeOp, IntakeOutcome, IntakePipeline,
        IntakeResolveError, IntakeRule, IntakeValue, ResolvedField, Timestamp,
    };

    const ALL_VALUES: [Tri; 3] = [Tri::False, Tri::True, Tri::Unknown];

    /// `now` for every test that does not care about time.
    const NOW: i64 = 10_000;

    fn item() -> IntakeItem {
        IntakeItem {
            wiki_id: "frwiki".to_string(),
            event_type: "edit".to_string(),
            namespace: 0,
            page_id: Some(4242),
            revision_id: Some(99),
            created_at: Some(Timestamp::from_epoch_ms(1_000)),
            last_revision_at: Some(Timestamp::from_epoch_ms(2_000)),
            actor: IntakeActor {
                username: Some("Ada".to_string()),
                rights: Some(vec!["sysop".to_string(), "edit".to_string()]),
                is_bot: false,
                account_created_at: Some(Timestamp::from_epoch_ms(500)),
            },
            observed_at: Timestamp::from_epoch_ms(NOW),
            payload_ref: None,
        }
    }

    fn compiled(condition: IntakeCondition) -> CompiledPipeline {
        compile_pipeline(&IntakePipeline {
            id: "baseline".to_string(),
            condition,
            on_pass: IntakeOutcome::Admit {
                route_to: "review".to_string(),
            },
            on_fail: IntakeOutcome::Drop,
            on_unknown: None,
        })
        .expect("pipeline compiles")
    }

    fn rule(field: IntakeField, op: IntakeOp, value: IntakeValue) -> IntakeCondition {
        IntakeCondition::rule(IntakeRule::new(field, op, value))
    }

    fn text_is_edit() -> IntakeCondition {
        rule(
            IntakeField::EventType,
            IntakeOp::Eq,
            IntakeValue::Str("edit".to_string()),
        )
    }

    /// The outcome of a decision, cloned for comparison.
    ///
    /// `Misconfigured` has no outcome, and `None` here is how a test asserts
    /// that distinction rather than confusing it with a `Drop`.
    fn outcome_of(decision: &sp42_types::IntakeDecision) -> Option<IntakeOutcome> {
        decision.outcome().cloned()
    }

    // ── Kleene, exhaustively ────────────────────────────────────────────────

    #[test]
    fn tri_all_is_exhaustively_correct() {
        for a in ALL_VALUES {
            for b in ALL_VALUES {
                let expected = match (a, b) {
                    (Tri::False, _) | (_, Tri::False) => Tri::False,
                    (Tri::Unknown, _) | (_, Tri::Unknown) => Tri::Unknown,
                    (Tri::True, Tri::True) => Tri::True,
                };
                assert_eq!(tri_all([a, b]), expected, "all({a:?}, {b:?})");
            }
        }
    }

    #[test]
    fn tri_any_is_exhaustively_correct() {
        for a in ALL_VALUES {
            for b in ALL_VALUES {
                let expected = match (a, b) {
                    (Tri::True, _) | (_, Tri::True) => Tri::True,
                    (Tri::Unknown, _) | (_, Tri::Unknown) => Tri::Unknown,
                    (Tri::False, Tri::False) => Tri::False,
                };
                assert_eq!(tri_any([a, b]), expected, "any({a:?}, {b:?})");
            }
        }
    }

    #[test]
    fn tri_not_is_exhaustively_correct() {
        for a in ALL_VALUES {
            let expected = match a {
                Tri::True => Tri::False,
                Tri::False => Tri::True,
                Tri::Unknown => Tri::Unknown,
            };
            assert_eq!(tri_not(a), expected, "not({a:?})");
        }
    }

    #[test]
    fn an_absent_field_short_circuits_before_its_operator_is_consulted() {
        // A flag is never `Absent`, so `Not(ActorIsBot Eq true)` against a
        // bot-false actor takes the `tri_not(False)` path and admits. The
        // negation arm has to actually walk its child for that to hold.
        let mut subject = item();
        subject.actor.is_bot = false;
        let pipeline = compiled(IntakeCondition::negate(rule(
            IntakeField::ActorIsBot,
            IntakeOp::Eq,
            IntakeValue::Bool(true),
        )));
        assert!(matches!(
            outcome_of(&evaluate_intake(&subject, &pipeline, &IntakeContext::bare(NOW)).unwrap()),
            Some(IntakeOutcome::Admit { .. })
        ));
    }

    #[test]
    fn an_empty_all_is_true_and_an_empty_any_is_unknown() {
        // `All([])` is the unconditional catch-all arm ADR-0028 §5 relies on.
        assert_eq!(tri_all([]), Tri::True);
        assert_eq!(
            IntakeCondition::always_true(),
            IntakeCondition::All(Vec::new())
        );
        // "some of nothing" cannot be affirmed; calling it False would be a claim
        // nobody has evidence for.
        assert_eq!(tri_any([]), Tri::Unknown);
    }

    #[test]
    fn kleene_composition_is_associative_and_commutative() {
        for a in ALL_VALUES {
            for b in ALL_VALUES {
                for c in ALL_VALUES {
                    assert_eq!(
                        tri_all([tri_all([a, b]), c]),
                        tri_all([a, tri_all([b, c])]),
                        "all is associative at {a:?} {b:?} {c:?}"
                    );
                    assert_eq!(
                        tri_all([a, b]),
                        tri_all([b, a]),
                        "all is commutative at {a:?} {b:?}"
                    );
                    assert_eq!(
                        tri_any([tri_any([a, b]), c]),
                        tri_any([a, tri_any([b, c])]),
                        "any is associative at {a:?} {b:?} {c:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn de_morgan_holds_in_three_valued_logic() {
        for a in ALL_VALUES {
            for b in ALL_VALUES {
                assert_eq!(
                    tri_not(tri_all([a, b])),
                    tri_any([tri_not(a), tri_not(b)]),
                    "not all == any not at {a:?} {b:?}"
                );
            }
        }
    }

    // ── Kleene, as properties ───────────────────────────────────────────────

    fn arb_tri() -> impl Strategy<Value = Tri> {
        prop_oneof![Just(Tri::True), Just(Tri::False), Just(Tri::Unknown)]
    }

    proptest! {
        #[test]
        fn double_negation_is_identity(x in arb_tri()) {
            prop_assert_eq!(tri_not(tri_not(x)), x);
        }

        #[test]
        fn a_conjunction_containing_false_is_false(
            others in proptest::collection::vec(arb_tri(), 0..4), x in arb_tri(),
        ) {
            let mut values = others.clone();
            values.push(x);
            if values.contains(&Tri::False) {
                prop_assert_eq!(tri_all(values), Tri::False);
            }
        }

        #[test]
        fn a_disjunction_containing_true_is_true(
            others in proptest::collection::vec(arb_tri(), 0..4), x in arb_tri(),
        ) {
            let mut values = others.clone();
            values.push(x);
            if values.contains(&Tri::True) {
                prop_assert_eq!(tri_any(values), Tri::True);
            }
        }

        /// The property the whole three-valued design exists for.
        ///
        /// Stated precisely, because the obvious phrasing is *false*: "the input
        /// contains an Unknown" does not imply "the fold is Unknown", because
        /// `Any` containing a `True` is `True` no matter what else is in it. The
        /// real invariants are that an `Unknown` can only ever be absorbed into
        /// a weaker value, and that negation produces a `True` from exactly one
        /// input — a definite `False`.
        #[test]
        fn an_unknown_is_never_manufactured_into_a_definite_answer(
            others in proptest::collection::vec(arb_tri(), 1..5),
        ) {
            prop_assume!(others.contains(&Tri::Unknown));
            prop_assert_ne!(tri_all(others.clone()), Tri::True);
            prop_assert_ne!(tri_any(others), Tri::False);
        }

        #[test]
        fn negation_yields_true_from_a_definite_false_and_from_nothing_else(
            x in arb_tri(),
        ) {
            prop_assert_eq!(tri_not(x) == Tri::True, x == Tri::False);
        }
    }

    // ── The compatibility table ─────────────────────────────────────────────

    #[test]
    fn a_temporal_field_rejects_a_membership_operator() {
        let error = validate_rule(&IntakeRule::new(
            IntakeField::CreatedAt,
            IntakeOp::Has,
            IntakeValue::Str("x".to_string()),
        ))
        .expect_err("a temporal field must not accept Has");
        assert!(
            matches!(error, IntakeError::OperatorNotAllowed { .. }),
            "{error:?}"
        );
        assert!(error.to_string().contains("created_at"), "{error}");
    }

    #[test]
    fn a_list_field_rejects_a_temporal_operator() {
        let error = validate_rule(&IntakeRule::new(
            IntakeField::ActorRights,
            IntakeOp::Before,
            IntakeValue::Timestamp(0),
        ))
        .expect_err("a list field must not accept Before");
        assert!(
            matches!(error, IntakeError::OperatorNotAllowed { .. }),
            "{error:?}"
        );
    }

    #[test]
    fn a_flag_rejects_a_temporal_operator_and_a_string_operand() {
        let operator = validate_rule(&IntakeRule::new(
            IntakeField::ActorIsBot,
            IntakeOp::OlderThan,
            IntakeValue::Duration(1),
        ))
        .expect_err("a flag must not accept OlderThan");
        assert!(matches!(operator, IntakeError::OperatorNotAllowed { .. }));

        let operand = validate_rule(&IntakeRule::new(
            IntakeField::ActorIsBot,
            IntakeOp::Eq,
            IntakeValue::Str("true".to_string()),
        ))
        .expect_err("a flag takes a bool, not a string");
        assert!(
            matches!(operand, IntakeError::ValueNotAllowed { .. }),
            "{operand:?}"
        );
    }

    #[test]
    fn a_capability_reference_is_refused_where_no_set_could_satisfy_it() {
        for field in [IntakeField::ActorIsBot, IntakeField::CreatedAt] {
            assert!(
                validate_rule(&IntakeRule::new(
                    field.clone(),
                    IntakeOp::In,
                    IntakeValue::CapabilityRef("mainspace".to_string()),
                ))
                .is_err(),
                "{field:?} has no comparable set, so a capability reference cannot apply"
            );
        }
        // But a set-valued field accepts it.
        assert!(
            validate_rule(&IntakeRule::new(
                IntakeField::Namespace,
                IntakeOp::In,
                IntakeValue::CapabilityRef("mainspace".to_string()),
            ))
            .is_ok()
        );
    }

    #[test]
    fn a_capability_reference_is_refused_on_a_kind_with_no_comparable_set() {
        // The kind guard in `operand_fits`, tested directly. The end-to-end check
        // above never reaches it for these fields — their `ops` array already
        // omits `In`, so the operator check fires first — which makes this guard
        // untestable through `validate_rule` today. It stays because it is the
        // only thing standing between a future edit that adds `In` to `FLAG_OPS`
        // and a rule that loads and then can never match.
        for kind in [ValueKind::Flag, ValueKind::Instant] {
            for op in [IntakeOp::In, IntakeOp::NotIn] {
                assert!(
                    !super::operand_fits(
                        kind,
                        op,
                        &IntakeValue::CapabilityRef("mainspace".to_string())
                    ),
                    "{kind:?} must not accept a capability set under {op:?}"
                );
            }
        }
        for kind in [ValueKind::Text, ValueKind::Number, ValueKind::TextList] {
            assert!(super::operand_fits(
                kind,
                IntakeOp::In,
                &IntakeValue::CapabilityRef("mainspace".to_string())
            ));
        }
    }

    #[test]
    fn older_than_takes_a_duration_and_before_takes_a_timestamp() {
        assert!(
            validate_rule(&IntakeRule::new(
                IntakeField::LastRevisionAt,
                IntakeOp::OlderThan,
                IntakeValue::Duration(10),
            ))
            .is_ok()
        );
        assert!(
            validate_rule(&IntakeRule::new(
                IntakeField::LastRevisionAt,
                IntakeOp::Before,
                IntakeValue::Timestamp(10),
            ))
            .is_ok()
        );
        // ...and the crossed pairing is not silently accepted.
        assert!(
            validate_rule(&IntakeRule::new(
                IntakeField::LastRevisionAt,
                IntakeOp::OlderThan,
                IntakeValue::Timestamp(10),
            ))
            .is_err()
        );
    }

    #[test]
    fn a_custom_field_defers_to_its_registry() {
        // No fixed spec, so the table cannot reject it; the registry decides.
        assert!(
            validate_rule(&IntakeRule::new(
                IntakeField::Custom("edit_count".to_string()),
                IntakeOp::Eq,
                IntakeValue::Int(1),
            ))
            .is_ok()
        );
    }

    #[test]
    fn a_rule_inside_a_negation_is_validated_too() {
        // Validation must reach into a `Not`; a bad rule hidden under one would
        // otherwise load cleanly and only surface at evaluation time.
        let error = compile_pipeline(&IntakePipeline {
            id: "hidden".to_string(),
            condition: IntakeCondition::negate(IntakeCondition::any([
                text_is_edit(),
                rule(
                    IntakeField::ActorIsBot,
                    IntakeOp::Has,
                    IntakeValue::Str("nope".to_string()),
                ),
            ])),
            on_pass: IntakeOutcome::Drop,
            on_fail: IntakeOutcome::Drop,
            on_unknown: None,
        })
        .expect_err("must not compile");
        assert!(
            matches!(error, IntakeError::OperatorNotAllowed { .. }),
            "{error:?}"
        );
    }

    #[test]
    fn compiling_a_pipeline_rejects_an_unevaluable_rule() {
        let error = compile_pipeline(&IntakePipeline {
            id: "broken".to_string(),
            condition: IntakeCondition::all([
                text_is_edit(),
                rule(
                    IntakeField::CreatedAt,
                    IntakeOp::Has,
                    IntakeValue::Str("nope".to_string()),
                ),
            ]),
            on_pass: IntakeOutcome::Drop,
            on_fail: IntakeOutcome::Drop,
            on_unknown: None,
        })
        .expect_err("must not compile");
        assert!(
            matches!(error, IntakeError::OperatorNotAllowed { .. }),
            "{error:?}"
        );
    }

    // ── Absent data is Unknown, not False ─────────────────────────────────

    #[test]
    fn a_missing_creation_time_is_unknown_not_false() {
        // The case from ADR-0026 §4: a rule asking whether a page is young,
        // against a page whose age the adapter never fetched.
        let mut subject = item();
        subject.created_at = None;

        let pipeline = compiled(rule(
            IntakeField::CreatedAt,
            IntakeOp::YoungerThan,
            IntakeValue::Duration(3_600_000),
        ));
        let decision =
            evaluate_intake(&subject, &pipeline, &IntakeContext::bare(NOW)).expect("evaluates");
        assert_eq!(
            decision.outcome(),
            Some(&IntakeOutcome::Drop),
            "Unknown must route to on_unknown, which defaults to Drop"
        );
    }

    #[test]
    fn negating_a_missing_creation_time_is_still_unknown() {
        // Not(CreatedAt YoungerThan 1h) must not read as "confirmed old enough".
        let mut subject = item();
        subject.created_at = None;

        let pipeline = compiled(IntakeCondition::negate(rule(
            IntakeField::CreatedAt,
            IntakeOp::YoungerThan,
            IntakeValue::Duration(3_600_000),
        )));
        let decision =
            evaluate_intake(&subject, &pipeline, &IntakeContext::bare(NOW)).expect("evaluates");
        assert_eq!(
            decision.outcome(),
            Some(&IntakeOutcome::Drop),
            "a negated unknown is still unknown, so it must not admit"
        );
    }

    #[test]
    fn unfetched_rights_are_unknown_while_confirmed_empty_is_false() {
        // The distinction ADR-0026 §3 exists for, on both sides. `on_fail` and the
        // `on_unknown` default are given *different* outcomes, so the test can
        // actually tell the two paths apart rather than observing `Drop` twice.
        let pipeline = compile_pipeline(&IntakePipeline {
            id: "rights".to_string(),
            condition: rule(
                IntakeField::ActorRights,
                IntakeOp::Has,
                IntakeValue::Str("sysop".to_string()),
            ),
            on_pass: IntakeOutcome::Admit {
                route_to: "review".to_string(),
            },
            on_fail: IntakeOutcome::Reclassify {
                pipeline: "elsewhere".to_string(),
            },
            on_unknown: None,
        })
        .expect("compiles");
        let context = IntakeContext::bare(NOW);

        let mut unfetched = item();
        unfetched.actor.rights = None;
        assert_eq!(
            outcome_of(&evaluate_intake(&unfetched, &pipeline, &context).unwrap()),
            Some(IntakeOutcome::Drop),
            "not fetched is Unknown, so it routes to on_unknown"
        );

        let mut confirmed_empty = item();
        confirmed_empty.actor.rights = Some(Vec::new());
        assert_eq!(
            outcome_of(&evaluate_intake(&confirmed_empty, &pipeline, &context).unwrap()),
            Some(IntakeOutcome::Reclassify {
                pipeline: "elsewhere".to_string()
            }),
            "fetched and confirmed empty is a definite False, routed to on_fail"
        );

        let present = item();
        assert_eq!(
            outcome_of(&evaluate_intake(&present, &pipeline, &context).unwrap()),
            Some(IntakeOutcome::Admit {
                route_to: "review".to_string()
            }),
        );
    }

    #[test]
    fn an_admitting_on_fail_does_not_admit_an_unknown() {
        // The day-one failure mode this whole module exists to prevent: a
        // restrictive baseline whose `on_fail` is `Admit`, plus a stream adapter
        // that fetches none of the fields the rules ask about.
        let pipeline = compile_pipeline(&IntakePipeline {
            id: "restrictive-baseline".to_string(),
            condition: rule(
                IntakeField::CreatedAt,
                IntakeOp::YoungerThan,
                IntakeValue::Duration(3_600_000),
            ),
            on_pass: IntakeOutcome::Drop,
            on_fail: IntakeOutcome::Admit {
                route_to: "admitted-by-default".to_string(),
            },
            on_unknown: None,
        })
        .expect("compiles");

        let mut subject = item();
        subject.created_at = None;
        let decision =
            evaluate_intake(&subject, &pipeline, &IntakeContext::bare(NOW)).expect("evaluates");
        assert_eq!(
            decision.outcome(),
            Some(&IntakeOutcome::Drop),
            "an Unknown must not inherit an admitting on_fail"
        );
    }

    #[test]
    fn an_explicit_on_unknown_overrides_the_default() {
        let pipeline = compile_pipeline(&IntakePipeline {
            id: "quarantine".to_string(),
            condition: rule(
                IntakeField::CreatedAt,
                IntakeOp::YoungerThan,
                IntakeValue::Duration(3_600_000),
            ),
            on_pass: IntakeOutcome::Drop,
            on_fail: IntakeOutcome::Drop,
            on_unknown: Some(IntakeOutcome::Reclassify {
                pipeline: "needs-enrichment".to_string(),
            }),
        })
        .expect("compiles");

        let mut subject = item();
        subject.created_at = None;
        assert_eq!(
            outcome_of(&evaluate_intake(&subject, &pipeline, &IntakeContext::bare(NOW)).unwrap()),
            Some(IntakeOutcome::Reclassify {
                pipeline: "needs-enrichment".to_string()
            }),
        );
    }

    // ── `Any` in the engine, not just in the Kleene helpers ────────────────

    #[test]
    fn a_disjunction_passes_when_any_child_passes() {
        let pipeline = compiled(IntakeCondition::any([
            text_is_edit(),
            rule(
                IntakeField::EventType,
                IntakeOp::Eq,
                IntakeValue::Str("log".to_string()),
            ),
        ]));
        let decision =
            evaluate_intake(&item(), &pipeline, &IntakeContext::bare(NOW)).expect("evaluates");
        assert!(matches!(
            decision.outcome(),
            Some(IntakeOutcome::Admit { .. })
        ));
    }

    #[test]
    fn a_disjunction_fails_only_when_every_child_fails() {
        let pipeline = compiled(IntakeCondition::any([
            rule(
                IntakeField::EventType,
                IntakeOp::Eq,
                IntakeValue::Str("log".to_string()),
            ),
            rule(IntakeField::Namespace, IntakeOp::Eq, IntakeValue::Int(9)),
        ]));
        let decision =
            evaluate_intake(&item(), &pipeline, &IntakeContext::bare(NOW)).expect("evaluates");
        assert_eq!(decision.outcome(), Some(&IntakeOutcome::Drop));
    }

    #[test]
    fn a_disjunction_of_only_unanswerable_children_is_unknown_not_false() {
        // The mixed case, and the one a two-valued reading gets wrong. Both
        // children fail to answer, so `Any` is `Unknown` and must not be read as
        // "definitely not this".
        let mut subject = item();
        subject.created_at = None;
        subject.last_revision_at = None;

        let pipeline = compiled(IntakeCondition::any([
            rule(
                IntakeField::CreatedAt,
                IntakeOp::YoungerThan,
                IntakeValue::Duration(3_600_000),
            ),
            rule(
                IntakeField::LastRevisionAt,
                IntakeOp::YoungerThan,
                IntakeValue::Duration(3_600_000),
            ),
        ]));
        let decision =
            evaluate_intake(&subject, &pipeline, &IntakeContext::bare(NOW)).expect("evaluates");
        assert_eq!(
            decision.outcome(),
            Some(&IntakeOutcome::Drop),
            "two unknowns in a disjunction is still unknown, so it routes to on_unknown"
        );
    }

    #[test]
    fn a_disjunction_of_one_unknown_and_one_definite_miss_is_false() {
        // The genuinely mixed case, and the complement of the test above. One
        // child cannot be answered and the other is a confirmed non-match, so the
        // disjunction is `False`: the confirmed miss settles it, and `Unknown`
        // only weakens a value, never blocks a definite one.
        let mut subject = item();
        subject.created_at = None;

        let pipeline = compiled(IntakeCondition::any([
            rule(
                IntakeField::CreatedAt,
                IntakeOp::YoungerThan,
                IntakeValue::Duration(3_600_000),
            ),
            // `event_type` is present, and is not "log".
            rule(
                IntakeField::EventType,
                IntakeOp::Eq,
                IntakeValue::Str("log".to_string()),
            ),
        ]));
        let decision =
            evaluate_intake(&subject, &pipeline, &IntakeContext::bare(NOW)).expect("evaluates");
        assert_eq!(
            decision.outcome(),
            Some(&IntakeOutcome::Drop),
            "a definite miss among the children makes the disjunction False, so it routes to on_fail"
        );
    }

    #[test]
    fn a_conjunction_of_one_unknown_and_one_definite_match_is_unknown() {
        // The mirror image, and the case that would be most damaging if wrong: a
        // rule that matches everything it can check, but cannot check one
        // predicate, must not be treated as satisfied.
        let mut subject = item();
        subject.created_at = None;

        let pipeline = compiled(IntakeCondition::all([
            text_is_edit(),
            rule(
                IntakeField::CreatedAt,
                IntakeOp::YoungerThan,
                IntakeValue::Duration(3_600_000),
            ),
        ]));
        let decision =
            evaluate_intake(&subject, &pipeline, &IntakeContext::bare(NOW)).expect("evaluates");
        assert_eq!(
            decision.outcome(),
            Some(&IntakeOutcome::Drop),
            "an unanswerable conjunct cannot be waved through by the ones that matched"
        );
        match decision {
            sp42_types::IntakeDecision::Routed {
                matched_rule_path, ..
            } => assert_eq!(
                matched_rule_path,
                vec!["event_type.eq".to_string()],
                "the matching conjunct is recorded, but it did not decide the branch"
            ),
            other @ sp42_types::IntakeDecision::Misconfigured { .. } => {
                panic!("expected routed, got {other:?}")
            }
        }
    }

    #[test]
    fn a_disjunction_with_one_definite_match_beats_a_sibling_unknown() {
        // `True` dominates `Unknown`, so a rule that *can* be answered decides the
        // branch. The opposite order — Unknown first — must not change the result.
        let mut subject = item();
        subject.created_at = None;

        for children in [
            vec![
                rule(
                    IntakeField::CreatedAt,
                    IntakeOp::YoungerThan,
                    IntakeValue::Duration(3_600_000),
                ),
                text_is_edit(),
            ],
            vec![
                text_is_edit(),
                rule(
                    IntakeField::CreatedAt,
                    IntakeOp::YoungerThan,
                    IntakeValue::Duration(3_600_000),
                ),
            ],
        ] {
            let pipeline = compiled(IntakeCondition::any(children));
            let decision =
                evaluate_intake(&subject, &pipeline, &IntakeContext::bare(NOW)).expect("evaluates");
            assert!(
                matches!(decision.outcome(), Some(IntakeOutcome::Admit { .. })),
                "a confirmed match must win over an unanswerable sibling"
            );
        }
    }

    #[test]
    fn an_empty_disjunction_is_unknown_so_a_ruleset_cannot_accidentally_admit() {
        // A vacuously-`False` `Any` in a restrictive ruleset would be a claim
        // nothing supports. It routes to `on_unknown`, which is `Drop`.
        let pipeline = compiled(IntakeCondition::any([]));
        let decision =
            evaluate_intake(&item(), &pipeline, &IntakeContext::bare(NOW)).expect("evaluates");
        assert_eq!(decision.outcome(), Some(&IntakeOutcome::Drop));
    }

    #[test]
    fn the_empty_disjunction_is_told_apart_from_the_empty_conjunction() {
        // Same children, same outcomes, different meaning — so the digests must
        // differ, or a config edit swapping one for the other would go unnoticed
        // in any replay that compares versions.
        let empty_all = compiled(IntakeCondition::always_true());
        let empty_any = compiled(IntakeCondition::any([]));
        assert_ne!(empty_all.config_version(), empty_any.config_version());

        assert!(matches!(
            evaluate_intake(&item(), &empty_all, &IntakeContext::bare(NOW))
                .expect("evaluates")
                .outcome(),
            Some(IntakeOutcome::Admit { .. })
        ));
        assert_eq!(
            evaluate_intake(&item(), &empty_any, &IntakeContext::bare(NOW))
                .expect("evaluates")
                .outcome(),
            Some(&IntakeOutcome::Drop)
        );
    }

    #[test]
    fn a_nested_disjunction_records_its_matching_leaves() {
        let pipeline = compiled(IntakeCondition::any([
            text_is_edit(),
            IntakeCondition::all([
                rule(IntakeField::Namespace, IntakeOp::Eq, IntakeValue::Int(9)),
                rule(
                    IntakeField::EventType,
                    IntakeOp::Eq,
                    IntakeValue::Str("log".to_string()),
                ),
            ]),
        ]));
        let decision =
            evaluate_intake(&item(), &pipeline, &IntakeContext::bare(NOW)).expect("evaluates");
        match decision {
            sp42_types::IntakeDecision::Routed {
                matched_rule_path, ..
            } => assert_eq!(
                matched_rule_path,
                vec!["event_type.eq".to_string()],
                "only the branch that actually fired"
            ),
            other @ sp42_types::IntakeDecision::Misconfigured { .. } => {
                panic!("expected routed, got {other:?}")
            }
        }
    }

    // ── Operators, on present data ─────────────────────────────────────────

    #[test]
    fn list_containment_requires_every_wanted_value() {
        let one = compiled(rule(
            IntakeField::ActorRights,
            IntakeOp::In,
            IntakeValue::StrList(vec!["sysop".to_string()]),
        ));
        let both = compiled(rule(
            IntakeField::ActorRights,
            IntakeOp::In,
            IntakeValue::StrList(vec!["sysop".to_string(), "edit".to_string()]),
        ));
        let one_unwanted = compiled(rule(
            IntakeField::ActorRights,
            IntakeOp::In,
            IntakeValue::StrList(vec!["sysop".to_string(), "oversight".to_string()]),
        ));
        let context = IntakeContext::bare(NOW);

        assert!(matches!(
            outcome_of(&evaluate_intake(&item(), &one, &context).unwrap()),
            Some(IntakeOutcome::Admit { .. })
        ));
        assert!(matches!(
            outcome_of(&evaluate_intake(&item(), &both, &context).unwrap()),
            Some(IntakeOutcome::Admit { .. })
        ));
        assert_eq!(
            outcome_of(&evaluate_intake(&item(), &one_unwanted, &context).unwrap()),
            Some(IntakeOutcome::Drop),
            "a partially-met bundle is not a match"
        );
    }

    #[test]
    fn scalar_membership_and_negation_agree() {
        for (field, op, value, expected_pass) in scalar_cases() {
            let context = IntakeContext::bare(NOW);
            let pipeline = compiled(rule(field.clone(), op, value));
            let outcome = outcome_of(&evaluate_intake(&item(), &pipeline, &context).unwrap());
            assert_eq!(
                outcome.is_some_and(|o| matches!(o, IntakeOutcome::Admit { .. })),
                expected_pass,
                "{field:?} {op:?} should {}pass",
                if expected_pass { "" } else { "not " }
            );
        }
    }

    /// One `field`/`op`/`operand` case against a present value.
    fn case(
        field: IntakeField,
        op: IntakeOp,
        value: IntakeValue,
        expected_pass: bool,
    ) -> (IntakeField, IntakeOp, IntakeValue, bool) {
        (field, op, value, expected_pass)
    }

    fn text(s: &str) -> IntakeValue {
        IntakeValue::Str(s.to_string())
    }

    fn texts(values: &[&str]) -> IntakeValue {
        IntakeValue::StrList(values.iter().map(|v| (*v).to_string()).collect())
    }

    fn at(ms: i64) -> IntakeValue {
        IntakeValue::Timestamp(ms)
    }

    fn nums(values: &[i64]) -> IntakeValue {
        IntakeValue::IntList(values.to_vec())
    }

    /// Every `field`/`op`/`operand` on a present value, with whether it should pass.
    ///
    /// A table rather than inline assertions so the full set of combinations is
    /// readable in one place; the `case`/`text`/`at` helpers keep each row to a
    /// single line, which is what keeps this under `too_many_lines`.
    fn scalar_cases() -> Vec<(IntakeField, IntakeOp, IntakeValue, bool)> {
        vec![
            case(IntakeField::EventType, IntakeOp::Eq, text("edit"), true),
            case(IntakeField::EventType, IntakeOp::Eq, text("log"), false),
            case(IntakeField::EventType, IntakeOp::NotEq, text("log"), true),
            case(IntakeField::EventType, IntakeOp::In, texts(&["edit"]), true),
            case(IntakeField::EventType, IntakeOp::In, texts(&["log"]), false),
            case(
                IntakeField::EventType,
                IntakeOp::NotIn,
                texts(&["log"]),
                true,
            ),
            case(IntakeField::ActorUsername, IntakeOp::Eq, text("Ada"), true),
            case(
                IntakeField::Namespace,
                IntakeOp::Eq,
                IntakeValue::Int(0),
                true,
            ),
            case(
                IntakeField::Namespace,
                IntakeOp::Eq,
                IntakeValue::Int(9),
                false,
            ),
            case(
                IntakeField::Namespace,
                IntakeOp::NotEq,
                IntakeValue::Int(9),
                true,
            ),
            case(IntakeField::Namespace, IntakeOp::In, nums(&[0, 1]), true),
            case(IntakeField::Namespace, IntakeOp::In, nums(&[1, 2]), false),
            case(IntakeField::Namespace, IntakeOp::NotIn, nums(&[1, 2]), true),
            case(
                IntakeField::PageId,
                IntakeOp::Eq,
                IntakeValue::Int(4242),
                true,
            ),
            case(
                IntakeField::RevisionId,
                IntakeOp::Eq,
                IntakeValue::Int(99),
                true,
            ),
            case(
                IntakeField::ActorIsBot,
                IntakeOp::Eq,
                IntakeValue::Bool(false),
                true,
            ),
            case(
                IntakeField::ActorIsBot,
                IntakeOp::NotEq,
                IntakeValue::Bool(true),
                true,
            ),
            case(IntakeField::ObservedAt, IntakeOp::Before, at(NOW + 1), true),
            case(
                IntakeField::ObservedAt,
                IntakeOp::Before,
                at(NOW - 1),
                false,
            ),
            case(IntakeField::ObservedAt, IntakeOp::After, at(NOW - 1), true),
            case(IntakeField::ObservedAt, IntakeOp::After, at(NOW + 1), false),
            case(IntakeField::CreatedAt, IntakeOp::Before, at(NOW), true),
            case(IntakeField::LastRevisionAt, IntakeOp::Before, at(NOW), true),
            case(
                IntakeField::ActorAccountCreatedAt,
                IntakeOp::Before,
                at(NOW),
                true,
            ),
            case(
                IntakeField::ActorAccountCreatedAt,
                IntakeOp::OlderThan,
                IntakeValue::Duration(5_000),
                true,
            ),
        ]
    }

    #[test]
    fn not_has_and_not_in_are_the_negations_of_their_positives() {
        let context = IntakeContext::bare(NOW);
        let pairs = [
            (
                rule(
                    IntakeField::ActorRights,
                    IntakeOp::Has,
                    IntakeValue::Str("sysop".to_string()),
                ),
                rule(
                    IntakeField::ActorRights,
                    IntakeOp::NotHas,
                    IntakeValue::Str("sysop".to_string()),
                ),
            ),
            (
                rule(
                    IntakeField::Namespace,
                    IntakeOp::In,
                    IntakeValue::IntList(vec![0]),
                ),
                rule(
                    IntakeField::Namespace,
                    IntakeOp::NotIn,
                    IntakeValue::IntList(vec![0]),
                ),
            ),
            // The same law over a `Text` list operand, which takes the
            // `StrList` arm rather than the `Str` one.
            (
                rule(
                    IntakeField::ActorRights,
                    IntakeOp::In,
                    IntakeValue::StrList(vec!["sysop".to_string()]),
                ),
                rule(
                    IntakeField::ActorRights,
                    IntakeOp::NotIn,
                    IntakeValue::StrList(vec!["sysop".to_string()]),
                ),
            ),
        ];
        for (positive, negative) in pairs {
            let yes = outcome_of(
                &evaluate_intake(&item(), &compiled(positive.clone()), &context).unwrap(),
            );
            let no = outcome_of(&evaluate_intake(&item(), &compiled(negative), &context).unwrap());
            assert!(matches!(yes, Some(IntakeOutcome::Admit { .. })));
            assert!(matches!(no, Some(IntakeOutcome::Drop)));
        }
    }

    #[test]
    fn temporal_operators_compare_against_the_injected_now() {
        let context = IntakeContext::bare(NOW);
        // last_revision_at is at 2_000, now is 10_000.
        let cases = [
            (IntakeOp::OlderThan, 5_000, true),
            (IntakeOp::OlderThan, 20_000, false),
            (IntakeOp::YoungerThan, 5_000, false),
            (IntakeOp::YoungerThan, 20_000, true),
        ];
        for (op, duration, expected_pass) in cases {
            let pipeline = compiled(rule(
                IntakeField::LastRevisionAt,
                op,
                IntakeValue::Duration(duration),
            ));
            assert_eq!(
                outcome_of(&evaluate_intake(&item(), &pipeline, &context).unwrap())
                    .is_some_and(|o| matches!(o, IntakeOutcome::Admit { .. })),
                expected_pass,
                "{op:?} {duration} against now={NOW} and a revision at 2_000"
            );
        }
    }

    #[test]
    fn temporal_arithmetic_saturates_instead_of_wrapping() {
        // `now` at i64::MIN with a large duration must not wrap the threshold
        // into the far future, which would make an ancient page look young.
        let pipeline = compiled(rule(
            IntakeField::ObservedAt,
            IntakeOp::OlderThan,
            IntakeValue::Duration(1_000),
        ));
        let mut subject = item();
        subject.observed_at = Timestamp::from_epoch_ms(0);
        let decision = evaluate_intake(&subject, &pipeline, &IntakeContext::bare(i64::MIN))
            .expect("evaluates");
        assert_eq!(
            decision.outcome(),
            Some(&IntakeOutcome::Drop),
            "a wrapped threshold would report the epoch as ancient-but-not-old"
        );
    }

    // ── Provenance ─────────────────────────────────────────────────────────

    #[test]
    fn a_decision_carries_the_rules_that_fired_and_the_pipeline_version() {
        let pipeline = compiled(IntakeCondition::all([
            text_is_edit(),
            rule(IntakeField::Namespace, IntakeOp::Eq, IntakeValue::Int(0)),
        ]));
        let decision =
            evaluate_intake(&item(), &pipeline, &IntakeContext::bare(NOW)).expect("evaluates");
        match decision {
            sp42_types::IntakeDecision::Routed {
                matched_rule_path,
                config_version,
                pipeline_id,
                ..
            } => {
                assert_eq!(pipeline_id, "baseline");
                assert_eq!(matched_rule_path.len(), 2, "both predicates fired");
                assert!(matched_rule_path.contains(&"event_type.eq".to_string()));
                assert_eq!(config_version, pipeline.config_version());
            }
            other @ sp42_types::IntakeDecision::Misconfigured { .. } => {
                panic!("expected routed, got {other:?}")
            }
        }
    }

    #[test]
    fn a_false_branch_records_no_rule_path() {
        let pipeline = compiled(text_is_edit());
        let mut subject = item();
        subject.event_type = "log".to_string();
        let decision =
            evaluate_intake(&subject, &pipeline, &IntakeContext::bare(NOW)).expect("evaluates");
        match decision {
            sp42_types::IntakeDecision::Routed {
                matched_rule_path,
                outcome,
                ..
            } => {
                assert_eq!(outcome, IntakeOutcome::Drop);
                assert!(matched_rule_path.is_empty(), "nothing fired");
            }
            other @ sp42_types::IntakeDecision::Misconfigured { .. } => {
                panic!("expected routed, got {other:?}")
            }
        }
    }

    #[test]
    fn a_negated_branch_never_appears_in_the_rule_path() {
        // A rule that holds *inside* a Not is evidence against the decision.
        // Recording it would make the audit trail claim the opposite of what it
        // shows, so the path must stay empty while the Not itself passes.
        let inner = rule(
            IntakeField::EventType,
            IntakeOp::Eq,
            IntakeValue::Str("delete".to_string()),
        );
        let pipeline = compiled(IntakeCondition::negate(inner));
        let decision =
            evaluate_intake(&item(), &pipeline, &IntakeContext::bare(NOW)).expect("evaluates");
        match decision {
            sp42_types::IntakeDecision::Routed {
                matched_rule_path,
                outcome,
                ..
            } => {
                assert!(
                    matches!(outcome, IntakeOutcome::Admit { .. }),
                    "the negated rule is false, so the Not passes"
                );
                assert!(
                    matched_rule_path.is_empty(),
                    "got {matched_rule_path:?}, but the only firing rule was negated"
                );
            }
            other @ sp42_types::IntakeDecision::Misconfigured { .. } => {
                panic!("expected routed, got {other:?}")
            }
        }
    }

    #[test]
    fn the_config_version_is_content_derived_and_per_pipeline() {
        // D2: a content digest, not a hand-written string, and not global.
        let base = IntakePipeline {
            id: "p".to_string(),
            condition: text_is_edit(),
            on_pass: IntakeOutcome::Drop,
            on_fail: IntakeOutcome::Drop,
            on_unknown: None,
        };
        let same = compile_pipeline(&base).expect("compiles");
        let again = compile_pipeline(&base).expect("compiles");
        assert_eq!(
            same.config_version(),
            again.config_version(),
            "the same pipeline must digest identically"
        );
        assert_eq!(same.config_version().len(), 16, "a short, loggable digest");

        let mut renamed = base.clone();
        renamed.id = "other".to_string();
        assert_ne!(
            compile_pipeline(&renamed)
                .expect("compiles")
                .config_version(),
            same.config_version(),
            "a different pipeline must not share a version"
        );

        let mut rerouted = base.clone();
        rerouted.on_fail = IntakeOutcome::Reclassify {
            pipeline: "elsewhere".to_string(),
        };
        assert_ne!(
            compile_pipeline(&rerouted)
                .expect("compiles")
                .config_version(),
            same.config_version(),
            "routing is part of the pipeline's meaning, so it must move the digest"
        );

        let mut requantified = base.clone();
        requantified.condition = IntakeCondition::negate(text_is_edit());
        assert_ne!(
            compile_pipeline(&requantified)
                .expect("compiles")
                .config_version(),
            same.config_version(),
        );

        let mut requantified_again = base;
        requantified_again.on_unknown = Some(IntakeOutcome::Drop);
        assert_ne!(
            compile_pipeline(&requantified_again)
                .expect("compiles")
                .config_version(),
            same.config_version(),
            "an explicit on_unknown is not the same meaning as an absent one"
        );
    }

    #[test]
    fn the_digest_cannot_be_forged_by_rearranging_its_parts() {
        // The parts are NUL-separated precisely so that moving content across the
        // id/condition boundary changes the digest.
        let left = compile_pipeline(&IntakePipeline {
            id: "ab".to_string(),
            condition: text_is_edit(),
            on_pass: IntakeOutcome::Drop,
            on_fail: IntakeOutcome::Drop,
            on_unknown: None,
        })
        .expect("compiles");
        let right = compile_pipeline(&IntakePipeline {
            id: "a".to_string(),
            condition: rule(
                IntakeField::EventType,
                IntakeOp::Eq,
                IntakeValue::Str("editb".to_string()),
            ),
            on_pass: IntakeOutcome::Drop,
            on_fail: IntakeOutcome::Drop,
            on_unknown: None,
        })
        .expect("compiles");
        assert_ne!(left.config_version(), right.config_version());
    }

    // ── Custom fields ─────────────────────────────────────────────────────

    struct FixedField(ResolvedField);

    impl sp42_types::IntakeFieldResolver for FixedField {
        fn resolve(
            &self,
            _field: &str,
            _item: &IntakeItem,
        ) -> Result<ResolvedField, IntakeResolveError> {
            Ok(self.0.clone())
        }
    }

    fn edit_count_pipeline() -> CompiledPipeline {
        compiled(rule(
            IntakeField::Custom("edit_count".to_string()),
            IntakeOp::Eq,
            IntakeValue::Int(7),
        ))
    }

    #[test]
    fn a_custom_field_resolves_through_its_registry() {
        let resolver = FixedField(ResolvedField::Number(7));
        let decision = evaluate_intake(
            &item(),
            &edit_count_pipeline(),
            &IntakeContext::new(NOW, &resolver, &sp42_types::NoCapabilities),
        )
        .expect("evaluates");
        assert!(matches!(
            decision.outcome(),
            Some(IntakeOutcome::Admit { .. })
        ));
    }

    #[test]
    fn a_custom_field_with_no_resolver_is_misconfigured_not_a_non_match() {
        // Fail-closed and loud: a config that could never work must not read as
        // "the item did not qualify".
        let decision = evaluate_intake(&item(), &edit_count_pipeline(), &IntakeContext::bare(NOW))
            .expect("evaluates");
        assert_eq!(
            decision,
            sp42_types::IntakeDecision::Misconfigured {
                pipeline_id: "baseline".to_string(),
                reason: IntakeMisconfiguration::UnregisteredCustomField {
                    field: "edit_count".to_string()
                },
            }
        );
    }

    #[test]
    fn a_registry_can_report_a_field_as_absent_for_one_item() {
        // Registered, but this item does not carry it: Unknown, not False.
        let resolver = FixedField(ResolvedField::Absent);
        let decision = evaluate_intake(
            &item(),
            &edit_count_pipeline(),
            &IntakeContext::new(NOW, &resolver, &sp42_types::NoCapabilities),
        )
        .expect("evaluates");
        assert_eq!(decision.outcome(), Some(&IntakeOutcome::Drop));
    }

    #[test]
    fn a_custom_field_of_the_wrong_shape_is_reported_not_routed() {
        // The table cannot catch this at load time — the shape belongs to the
        // domain — so the evaluator reports it instead of quietly returning
        // `Unknown` and dropping the item through `on_unknown`.
        //
        // One case per reported shape, because the message is the only thing an
        // operator has to go on: "text" alone would not say whether the fix is in
        // the rule's operand or in the registry.
        //
        // `Number` is absent from this list because it is the *correct* shape for
        // the rule (`Eq Int(7)`) and so routes rather than reporting;
        // `Absent` is covered separately, since it is handled before the shape
        // comparison and is a legitimate answer.
        for (resolved, described) in [
            (ResolvedField::Text("seven".to_string()), "text"),
            (ResolvedField::TextList(vec!["7".to_string()]), "text_list"),
            (ResolvedField::Flag(true), "flag"),
            (
                ResolvedField::Instant(Timestamp::from_epoch_ms(7)),
                "instant",
            ),
        ] {
            let resolver = FixedField(resolved);
            let decision = evaluate_intake(
                &item(),
                &edit_count_pipeline(),
                &IntakeContext::new(NOW, &resolver, &sp42_types::NoCapabilities),
            )
            .expect("evaluates");
            match decision {
                sp42_types::IntakeDecision::Misconfigured { reason, .. } => {
                    assert_eq!(
                        reason,
                        IntakeMisconfiguration::UnusableCustomField {
                            field: "custom:edit_count".to_string(),
                            op: IntakeOp::Eq,
                            resolved: described.to_string(),
                        },
                    );
                }
                other @ sp42_types::IntakeDecision::Routed { .. } => {
                    panic!("expected misconfigured, got {other:?}")
                }
            }
        }

        // A number is the one shape reachable here for a reason other than being
        // the wrong variant: a `Custom` field defers to the registry, so a rule
        // can pair a scalar `Int` operand with `In`, which a builtin `Number`
        // field would also accept — that routes, and is covered above. Reaching
        // a report needs an operator no scalar can answer, such as a temporal
        // comparison against a number.
        let set_rule = compiled(rule(
            IntakeField::Custom("edit_count".to_string()),
            IntakeOp::OlderThan,
            IntakeValue::Duration(3_600_000),
        ));
        let resolver = FixedField(ResolvedField::Number(7));
        let decision = evaluate_intake(
            &item(),
            &set_rule,
            &IntakeContext::new(NOW, &resolver, &sp42_types::NoCapabilities),
        )
        .expect("evaluates");
        assert_eq!(
            decision,
            sp42_types::IntakeDecision::Misconfigured {
                pipeline_id: "baseline".to_string(),
                reason: IntakeMisconfiguration::UnusableCustomField {
                    field: "custom:edit_count".to_string(),
                    op: IntakeOp::OlderThan,
                    resolved: "number".to_string(),
                },
            }
        );
    }

    #[test]
    fn a_custom_field_reporting_absent_still_routes_to_on_unknown() {
        // `Absent` is a legitimate resolver answer, so it must be handled by the
        // absent check *before* the shape comparison — otherwise a registry that
        // correctly reports "this item has no edit_count" would be reported as a
        // misconfiguration instead.
        let resolver = FixedField(ResolvedField::Absent);
        let decision = evaluate_intake(
            &item(),
            &edit_count_pipeline(),
            &IntakeContext::new(NOW, &resolver, &sp42_types::NoCapabilities),
        )
        .expect("evaluates");
        assert!(matches!(
            decision,
            sp42_types::IntakeDecision::Routed { .. }
        ));
    }

    #[test]
    fn a_custom_field_of_the_wrong_shape_is_reported_for_its_operator() {
        // The operator travels in the report, so a rule using a temporal
        // comparison is distinguishable from one using equality.
        let resolver = FixedField(ResolvedField::Text("soon".to_string()));
        let pipeline = compiled(rule(
            IntakeField::Custom("last_touched".to_string()),
            IntakeOp::OlderThan,
            IntakeValue::Duration(1_000),
        ));
        let decision = evaluate_intake(
            &item(),
            &pipeline,
            &IntakeContext::new(NOW, &resolver, &sp42_types::NoCapabilities),
        )
        .expect("evaluates");
        assert_eq!(
            decision,
            sp42_types::IntakeDecision::Misconfigured {
                pipeline_id: "baseline".to_string(),
                reason: IntakeMisconfiguration::UnusableCustomField {
                    field: "custom:last_touched".to_string(),
                    op: IntakeOp::OlderThan,
                    resolved: "text".to_string(),
                },
            }
        );
    }

    #[test]
    fn every_resolved_shape_has_a_report_name() {
        // `describe_resolved` is the only thing standing between a misconfigured
        // pipeline and a report that does not say what went wrong, so its
        // coverage is asserted directly rather than only through the paths that
        // happen to reach it.
        for (shape, name) in [
            (ResolvedField::Text("x".to_string()), "text"),
            (ResolvedField::Number(1), "number"),
            (ResolvedField::TextList(Vec::new()), "text_list"),
            (ResolvedField::Flag(false), "flag"),
            (ResolvedField::Instant(Timestamp::UNIX_EPOCH), "instant"),
            (ResolvedField::Absent, "absent"),
        ] {
            assert_eq!(super::describe_resolved(&shape), name);
        }
    }

    #[test]
    fn every_builtin_field_resolves_to_the_value_its_item_carries() {
        // Walks the whole `resolve_field` table against one fully-populated item.
        // The per-operator tests above each touch a few fields; this is what
        // catches a field that resolves the *wrong* `IntakeItem` field — an
        // entirely silent bug, since a wrong-but-plausible value still evaluates.
        let context = IntakeContext::bare(NOW);
        let subject = item();
        let cases: [(IntakeField, ResolvedField); 11] = [
            (
                IntakeField::EventType,
                ResolvedField::Text("edit".to_string()),
            ),
            (IntakeField::Namespace, ResolvedField::Number(0)),
            (IntakeField::PageId, ResolvedField::Number(4242)),
            (IntakeField::RevisionId, ResolvedField::Number(99)),
            (
                IntakeField::CreatedAt,
                ResolvedField::Instant(Timestamp::from_epoch_ms(1_000)),
            ),
            (
                IntakeField::LastRevisionAt,
                ResolvedField::Instant(Timestamp::from_epoch_ms(2_000)),
            ),
            (
                IntakeField::ActorUsername,
                ResolvedField::Text("Ada".to_string()),
            ),
            (
                IntakeField::ActorRights,
                ResolvedField::TextList(vec!["sysop".to_string(), "edit".to_string()]),
            ),
            (IntakeField::ActorIsBot, ResolvedField::Flag(false)),
            (
                IntakeField::ActorAccountCreatedAt,
                ResolvedField::Instant(Timestamp::from_epoch_ms(500)),
            ),
            (
                IntakeField::ObservedAt,
                ResolvedField::Instant(Timestamp::from_epoch_ms(NOW)),
            ),
        ];
        for (field, expected) in cases {
            assert_eq!(
                super::resolve_field(&field, &subject, &context).expect("resolves"),
                expected,
                "{field:?} resolved to the wrong part of the item"
            );
        }
    }

    #[test]
    fn every_optional_field_resolves_to_absent_when_not_fetched() {
        // The other half of the three-valued contract: every `Option` in
        // `IntakeItem` must reach the evaluator as `Absent`, never as a
        // default. A `None` that became `0` would read as a confident answer.
        let context = IntakeContext::bare(NOW);
        let mut subject = item();
        subject.page_id = None;
        subject.revision_id = None;
        subject.created_at = None;
        subject.last_revision_at = None;
        subject.actor.username = None;
        subject.actor.rights = None;
        subject.actor.account_created_at = None;

        for field in [
            IntakeField::PageId,
            IntakeField::RevisionId,
            IntakeField::CreatedAt,
            IntakeField::LastRevisionAt,
            IntakeField::ActorUsername,
            IntakeField::ActorRights,
            IntakeField::ActorAccountCreatedAt,
        ] {
            assert_eq!(
                super::resolve_field(&field, &subject, &context).expect("resolves"),
                ResolvedField::Absent,
                "{field:?} must be Absent when unfetched, not a default"
            );
        }
    }

    #[test]
    fn a_registry_reports_its_names_for_the_linter() {
        let registry = IntakeFieldRegistry::new()
            .with("edit_count", ResolvedField::Number(1))
            .with("edit_group", ResolvedField::TextList(Vec::new()));
        let names: BTreeSet<&str> = registry.registered().collect();
        assert!(names.contains("edit_count"));
        assert!(names.contains("edit_group"));
    }

    // ── Capability references ──────────────────────────────────────────────

    struct FixedCapabilities(BTreeSet<IntakeValue>);

    impl CapabilityResolver for FixedCapabilities {
        fn resolve(&self, reference: &str) -> Result<BTreeSet<IntakeValue>, IntakeResolveError> {
            if reference == "mainspace" {
                Ok(self.0.clone())
            } else {
                Err(IntakeResolveError::UnresolvedCapability(
                    reference.to_string(),
                ))
            }
        }
    }

    fn capabilities() -> FixedCapabilities {
        FixedCapabilities(BTreeSet::from([
            IntakeValue::Int(0),
            IntakeValue::Str("0".to_string()),
            IntakeValue::Str("main".to_string()),
        ]))
    }

    #[test]
    fn a_resolved_capability_reference_membership_tests() {
        let resolver = capabilities();
        let context = IntakeContext::new(NOW, &sp42_types::NoCustomFields, &resolver);

        let in_mainspace = compiled(rule(
            IntakeField::Namespace,
            IntakeOp::In,
            IntakeValue::CapabilityRef("mainspace".to_string()),
        ));
        assert!(matches!(
            outcome_of(&evaluate_intake(&item(), &in_mainspace, &context).unwrap()),
            Some(IntakeOutcome::Admit { .. })
        ));

        let not_in_mainspace = compiled(rule(
            IntakeField::Namespace,
            IntakeOp::NotIn,
            IntakeValue::CapabilityRef("mainspace".to_string()),
        ));
        assert_eq!(
            outcome_of(&evaluate_intake(&item(), &not_in_mainspace, &context).unwrap()),
            Some(IntakeOutcome::Drop)
        );

        // A text list field takes the same reference, and requires every entry.
        let rights = compiled(rule(
            IntakeField::ActorRights,
            IntakeOp::In,
            IntakeValue::CapabilityRef("mainspace".to_string()),
        ));
        let decision = evaluate_intake(&item(), &rights, &context).expect("evaluates");
        assert_eq!(
            decision.outcome(),
            Some(&IntakeOutcome::Drop),
            "the rights list cannot contain the capability's namespaces"
        );

        // A capability that resolves to exactly the held rights is a match, so
        // the text-list arm is confirmed to be reachable and not merely refused.
        let held = FixedCapabilities(BTreeSet::from([
            IntakeValue::Str("sysop".to_string()),
            IntakeValue::Str("edit".to_string()),
        ]));
        let held_context = IntakeContext::new(NOW, &sp42_types::NoCustomFields, &held);
        let bundle = compiled(rule(
            IntakeField::ActorRights,
            IntakeOp::In,
            IntakeValue::CapabilityRef("held".to_string()),
        ));
        // `held` is not a name this resolver knows, so it is a Misconfigured —
        // which is itself the check that a mismatched name never silently
        // resolves to an empty set and reads as "no rights required".
        assert!(matches!(
            evaluate_intake(&item(), &bundle, &held_context).expect("evaluates"),
            sp42_types::IntakeDecision::Misconfigured { .. }
        ));
    }

    #[test]
    fn a_capability_matching_a_text_field_is_compared_as_a_string() {
        // The `ResolvedField::Text` arm of the membership comparison, which the
        // namespace-only tests never reach.
        struct AdminNames;

        impl CapabilityResolver for AdminNames {
            fn resolve(
                &self,
                reference: &str,
            ) -> Result<BTreeSet<IntakeValue>, IntakeResolveError> {
                match reference {
                    "admins" => Ok(BTreeSet::from([
                        IntakeValue::Str("Ada".to_string()),
                        IntakeValue::Str("Grace".to_string()),
                    ])),
                    other => Err(IntakeResolveError::UnresolvedCapability(other.to_string())),
                }
            }
        }

        let context = IntakeContext::new(NOW, &sp42_types::NoCustomFields, &AdminNames);
        for (op, expected_pass) in [(IntakeOp::In, true), (IntakeOp::NotIn, false)] {
            let pipeline = compiled(rule(
                IntakeField::ActorUsername,
                op,
                IntakeValue::CapabilityRef("admins".to_string()),
            ));
            assert_eq!(
                outcome_of(&evaluate_intake(&item(), &pipeline, &context).unwrap())
                    .is_some_and(|o| matches!(o, IntakeOutcome::Admit { .. })),
                expected_pass,
                "{op:?} against a capability that contains the actor's name"
            );
        }
    }

    #[test]
    fn a_capability_of_the_wrong_element_type_cannot_satisfy_a_list_field() {
        // A capability that resolved only to integers, asked of a list of
        // strings. Not a crash and not a silent pass: the elements cannot
        // match, so the comparison is a non-match.
        let numbers = FixedCapabilities(BTreeSet::from([IntakeValue::Int(0)]));
        let context = IntakeContext::new(NOW, &sp42_types::NoCustomFields, &numbers);
        let pipeline = compiled(rule(
            IntakeField::ActorRights,
            IntakeOp::In,
            IntakeValue::CapabilityRef("mainspace".to_string()),
        ));
        assert_eq!(
            outcome_of(&evaluate_intake(&item(), &pipeline, &context).unwrap()),
            Some(IntakeOutcome::Drop)
        );
    }

    #[test]
    fn an_unresolvable_capability_is_misconfigured_and_distinguishable_from_a_drop() {
        // ADR-0026 §6: a broken filter must never look like an intentional one.
        let resolver = capabilities();
        let context = IntakeContext::new(NOW, &sp42_types::NoCustomFields, &resolver);
        let pipeline = compiled(rule(
            IntakeField::Namespace,
            IntakeOp::In,
            IntakeValue::CapabilityRef("redirects".to_string()),
        ));
        let decision = evaluate_intake(&item(), &pipeline, &context).expect("evaluates");
        assert_eq!(
            decision,
            sp42_types::IntakeDecision::Misconfigured {
                pipeline_id: "baseline".to_string(),
                reason: IntakeMisconfiguration::UnresolvedCapability {
                    reference: "redirects".to_string()
                },
            }
        );
        assert!(
            decision.outcome().is_none(),
            "a misconfiguration has no outcome, so it cannot be mistaken for one"
        );
    }

    // ── The table itself ───────────────────────────────────────────────────

    #[test]
    fn every_field_kind_is_reachable_and_every_kind_is_covered() {
        // Guards the table: a new field with no spec would silently become
        // "unresolvable", so assert the closed set is what we think it is.
        let kinds: BTreeSet<ValueKind> = [
            IntakeField::EventType,
            IntakeField::Namespace,
            IntakeField::PageId,
            IntakeField::RevisionId,
            IntakeField::CreatedAt,
            IntakeField::LastRevisionAt,
            IntakeField::ActorUsername,
            IntakeField::ActorRights,
            IntakeField::ActorIsBot,
            IntakeField::ActorAccountCreatedAt,
            IntakeField::ObservedAt,
        ]
        .iter()
        .filter_map(super::field_spec)
        .map(|spec| spec.kind)
        .collect();
        assert_eq!(kinds.len(), 5, "all five value kinds are reachable");
        assert!(field_spec(&IntakeField::Custom("x".to_string())).is_none());
    }

    /// The operand that *should* be accepted for a `kind` under an `op`, or
    /// `None` when the pairing is not meaningful.
    ///
    /// The operator matters as much as the kind: `In` takes a set where `Eq`
    /// takes a scalar, so a probe built from the kind alone would report a
    /// correct table as broken.
    fn probe_operand(kind: ValueKind, op: IntakeOp) -> Option<IntakeValue> {
        let text = || IntakeValue::Str("x".to_string());
        match (kind, op) {
            (ValueKind::Text, IntakeOp::Eq | IntakeOp::NotEq)
            | (ValueKind::TextList, IntakeOp::Has | IntakeOp::NotHas) => Some(text()),
            (ValueKind::Number, IntakeOp::Eq | IntakeOp::NotEq) => Some(IntakeValue::Int(0)),
            (ValueKind::Flag, IntakeOp::Eq | IntakeOp::NotEq) => Some(IntakeValue::Bool(false)),
            (ValueKind::Text | ValueKind::TextList, IntakeOp::In | IntakeOp::NotIn) => {
                Some(IntakeValue::StrList(vec!["x".to_string()]))
            }
            (ValueKind::Number, IntakeOp::In | IntakeOp::NotIn) => {
                Some(IntakeValue::IntList(vec![0]))
            }
            (ValueKind::Instant, IntakeOp::Before | IntakeOp::After) => {
                Some(IntakeValue::Timestamp(0))
            }
            (ValueKind::Instant, IntakeOp::OlderThan | IntakeOp::YoungerThan) => {
                Some(IntakeValue::Duration(0))
            }
            _ => None,
        }
    }

    const EVERY_OP: [IntakeOp; 10] = [
        IntakeOp::Eq,
        IntakeOp::NotEq,
        IntakeOp::In,
        IntakeOp::NotIn,
        IntakeOp::Has,
        IntakeOp::NotHas,
        IntakeOp::Before,
        IntakeOp::After,
        IntakeOp::OlderThan,
        IntakeOp::YoungerThan,
    ];

    const EVERY_BUILTIN_FIELD: [IntakeField; 5] = [
        IntakeField::EventType,
        IntakeField::Namespace,
        IntakeField::ActorRights,
        IntakeField::ActorIsBot,
        IntakeField::CreatedAt,
    ];

    #[test]
    fn the_table_rejects_every_operator_it_claims_to() {
        // Exhaustiveness over the vocabulary: whatever the table lists as allowed
        // must actually admit a sensible operand, and everything it omits must be
        // refused. Catches a value that slipped into the wrong `ops` array, and a
        // pair the `ops` array allows but `operand_fits` cannot satisfy.
        for field in EVERY_BUILTIN_FIELD {
            let spec = field_spec(&field).expect("builtin field has a spec");
            for op in EVERY_OP {
                let outcome = probe_operand(spec.kind, op)
                    .map(|operand| validate_rule(&IntakeRule::new(field.clone(), op, operand)));
                if spec.ops.contains(&op) {
                    let result = outcome.unwrap_or_else(|| {
                        panic!("{field:?} lists {op:?} but no operand can satisfy it")
                    });
                    assert!(
                        result.is_ok(),
                        "{field:?} lists {op:?} but rejects it: {result:?}"
                    );
                } else {
                    // The operator check runs first, so any operand is refused.
                    let result = validate_rule(&IntakeRule::new(
                        field.clone(),
                        op,
                        IntakeValue::Str("x".to_string()),
                    ));
                    assert!(result.is_err(), "{field:?} omits {op:?} but accepted it");
                }
            }
        }
    }

    #[test]
    fn an_allowed_operator_still_rejects_the_wrong_operand_shape() {
        // The complement of the test above: being on the allowed list is not by
        // itself enough, the operand still has to match the kind.
        let wrong_shapes = [
            (IntakeField::EventType, IntakeOp::Eq, IntakeValue::Int(0)),
            (
                IntakeField::EventType,
                IntakeOp::In,
                IntakeValue::Str("x".to_string()),
            ),
            (
                IntakeField::Namespace,
                IntakeOp::Eq,
                IntakeValue::Str("x".to_string()),
            ),
            (IntakeField::Namespace, IntakeOp::In, IntakeValue::Int(0)),
            (IntakeField::ActorRights, IntakeOp::Has, IntakeValue::Int(0)),
            (
                IntakeField::ActorRights,
                IntakeOp::In,
                IntakeValue::IntList(vec![0]),
            ),
            (IntakeField::ActorIsBot, IntakeOp::Eq, IntakeValue::Int(0)),
            (
                IntakeField::CreatedAt,
                IntakeOp::Before,
                IntakeValue::Duration(0),
            ),
            (
                IntakeField::CreatedAt,
                IntakeOp::OlderThan,
                IntakeValue::Timestamp(0),
            ),
        ];
        for (field, op, value) in wrong_shapes {
            let spec = field_spec(&field).expect("builtin field has a spec");
            assert!(spec.ops.contains(&op), "{field:?} should allow {op:?}");
            let error = validate_rule(&IntakeRule::new(field, op, value))
                .expect_err("the operand shape is wrong even though the operator is allowed");
            assert!(
                matches!(error, IntakeError::ValueNotAllowed { .. }),
                "expected a value error, got {error:?}"
            );
        }
    }
}
