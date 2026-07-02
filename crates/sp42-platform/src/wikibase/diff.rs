use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::diff_engine::StructuredDiff;
use crate::types::ContentModel;

use super::model::{Entity, EntityId, Lang, PropertyId, Sitelink, Statement};

/// Structured diff of two Wikibase entity revisions (ADR-0016 Decision 3).
/// Sibling of `StructuredDiff` (`diff_engine`), selected by `ContentDiff` (Phase 6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct EntityDiff {
    pub labels: Vec<TermChange>,
    pub descriptions: Vec<TermChange>,
    pub aliases: Vec<AliasChange>,
    pub sitelinks: Vec<SitelinkChange>,
    pub statements: Vec<StatementChange>,
}

impl EntityDiff {
    /// No classified changes between the two parsed entities. Statement identity is
    /// GUID-based, so a pure reordering of GUID-stable statements within a property
    /// is intentionally not surfaced (yields an empty diff).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.labels.is_empty()
            && self.descriptions.is_empty()
            && self.aliases.is_empty()
            && self.sitelinks.is_empty()
            && self.statements.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TermChange {
    pub lang: Lang,
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AliasChange {
    pub lang: Lang,
    pub before: Vec<String>,
    pub after: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SitelinkChange {
    pub site: String,
    pub before: Option<Sitelink>,
    pub after: Option<Sitelink>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[allow(clippy::large_enum_variant)] // #105: Changed is the common case; boxing would hurt ergonomics for a short-lived diff value
pub enum StatementChange {
    Added(Statement),
    Removed(Statement),
    Changed {
        before: Statement,
        after: Statement,
        parts: StatementChangeParts,
    },
}

/// Which sub-parts of a statement moved — powers "an edit touching only a
/// qualifier / rank / reference is never a no-op". Computed from raw-JSON equality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)] // #105: Four independent semantically-distinct change flags, not a state machine
pub struct StatementChangeParts {
    pub value: bool,
    pub qualifiers: bool,
    pub rank: bool,
    pub references: bool,
}

/// Compute union-diff of two key-value collections. For each key in the union of both
/// collections, returns (key, `before_value`, `after_value`) where before/after are `Option`.
fn union_diff_pairs<K, V>(
    old_keys: Vec<K>,
    new_keys: Vec<K>,
    old_getter: impl Fn(&K) -> Option<V>,
    new_getter: impl Fn(&K) -> Option<V>,
) -> Vec<(K, Option<V>, Option<V>)>
where
    K: Ord + Clone,
    V: Clone,
{
    let mut all_keys = BTreeMap::new();
    for key in old_keys {
        all_keys.insert(key.clone(), (old_getter(&key), None));
    }
    for key in new_keys {
        let entry = all_keys.entry(key.clone()).or_insert((None, None));
        entry.1 = new_getter(&key);
    }
    all_keys
        .into_iter()
        .map(|(k, (before, after))| (k, before, after))
        .collect()
}

/// Diff two entity revisions. `old = None` = first revision (everything Added).
/// Change detection uses `Statement.raw` equality, so unknown datatypes still
/// register (the honesty invariant, ADR-0016 Decision 3).
#[must_use]
#[allow(clippy::too_many_lines)] // #105: Five parallel union-diff blocks with statement processing; helper extracts duplication
pub fn diff_entities(old: Option<&Entity>, new: &Entity) -> EntityDiff {
    let mut diff = EntityDiff::default();

    // Handle labels: union of language keys
    {
        let old_keys: Vec<_> = old
            .map(|e| e.labels.keys().cloned().collect())
            .unwrap_or_default();
        let new_keys: Vec<_> = new.labels.keys().cloned().collect();
        for (lang, before, after) in union_diff_pairs(
            old_keys,
            new_keys,
            |lang| old.and_then(|e| e.labels.get(lang).cloned()),
            |lang| new.labels.get(lang).cloned(),
        ) {
            if before != after {
                diff.labels.push(TermChange {
                    lang,
                    before,
                    after,
                });
            }
        }
    }

    // Handle descriptions: union of language keys
    {
        let old_keys: Vec<_> = old
            .map(|e| e.descriptions.keys().cloned().collect())
            .unwrap_or_default();
        let new_keys: Vec<_> = new.descriptions.keys().cloned().collect();
        for (lang, before, after) in union_diff_pairs(
            old_keys,
            new_keys,
            |lang| old.and_then(|e| e.descriptions.get(lang).cloned()),
            |lang| new.descriptions.get(lang).cloned(),
        ) {
            if before != after {
                diff.descriptions.push(TermChange {
                    lang,
                    before,
                    after,
                });
            }
        }
    }

    // Handle aliases: union of language keys
    {
        let old_keys: Vec<_> = old
            .map(|e| e.aliases.keys().cloned().collect())
            .unwrap_or_default();
        let new_keys: Vec<_> = new.aliases.keys().cloned().collect();
        for (lang, before, after) in union_diff_pairs(
            old_keys,
            new_keys,
            |lang| old.and_then(|e| e.aliases.get(lang).cloned()),
            |lang| new.aliases.get(lang).cloned(),
        ) {
            let before_vec = before.unwrap_or_default();
            let after_vec = after.unwrap_or_default();
            if before_vec != after_vec {
                diff.aliases.push(AliasChange {
                    lang,
                    before: before_vec,
                    after: after_vec,
                });
            }
        }
    }

    // Handle sitelinks: union of site keys
    {
        let old_keys: Vec<_> = old
            .map(|e| e.sitelinks.keys().cloned().collect())
            .unwrap_or_default();
        let new_keys: Vec<_> = new.sitelinks.keys().cloned().collect();
        for (site, before, after) in union_diff_pairs(
            old_keys,
            new_keys,
            |site| old.and_then(|e| e.sitelinks.get(site).cloned()),
            |site| new.sitelinks.get(site).cloned(),
        ) {
            if before != after {
                diff.sitelinks.push(SitelinkChange {
                    site,
                    before,
                    after,
                });
            }
        }
    }

    // Handle statements: key by GUID, falling back to (property, index) position
    {
        let mut old_by_guid: BTreeMap<String, &Statement> = BTreeMap::new();
        let mut old_by_position: BTreeMap<(PropertyId, usize), &Statement> = BTreeMap::new();
        if let Some(old_entity) = old {
            for (prop_id, stmts) in &old_entity.statements {
                for (idx, stmt) in stmts.iter().enumerate() {
                    if let Some(id) = &stmt.id {
                        old_by_guid.insert(id.as_str().to_string(), stmt);
                    } else {
                        old_by_position.insert((prop_id.clone(), idx), stmt);
                    }
                }
            }
        }

        let mut new_by_guid: BTreeMap<String, &Statement> = BTreeMap::new();
        let mut new_by_position: BTreeMap<(PropertyId, usize), &Statement> = BTreeMap::new();
        for (prop_id, stmts) in &new.statements {
            for (idx, stmt) in stmts.iter().enumerate() {
                if let Some(id) = &stmt.id {
                    new_by_guid.insert(id.as_str().to_string(), stmt);
                } else {
                    new_by_position.insert((prop_id.clone(), idx), stmt);
                }
            }
        }

        // Process GUID-keyed statements
        {
            let mut all_guids = BTreeMap::new();
            for guid in old_by_guid.keys() {
                all_guids.insert(guid.clone(), (old_by_guid.get(guid).copied(), None));
            }
            for guid in new_by_guid.keys() {
                let entry = all_guids.entry(guid.clone()).or_insert((None, None));
                entry.1 = new_by_guid.get(guid).copied();
            }
            let guid_pairs: Vec<_> = all_guids
                .into_iter()
                .map(|(_, (old, new))| (old, new))
                .collect();
            diff.statements.extend(emit_statement_changes(guid_pairs));
        }

        // Process position-keyed statements
        {
            let mut all_positions = BTreeMap::new();
            for pos in old_by_position.keys() {
                all_positions.insert(pos.clone(), (old_by_position.get(pos).copied(), None));
            }
            for pos in new_by_position.keys() {
                let entry = all_positions.entry(pos.clone()).or_insert((None, None));
                entry.1 = new_by_position.get(pos).copied();
            }
            let position_pairs: Vec<_> = all_positions
                .into_iter()
                .map(|(_, (old, new))| (old, new))
                .collect();
            diff.statements
                .extend(emit_statement_changes(position_pairs));
        }
    }

    diff
}

/// Process statement pairs and emit `StatementChange` for each (old, new) combination.
/// Handles both GUID-keyed and position-keyed statements with identical logic.
fn emit_statement_changes(
    stmt_pairs: Vec<(Option<&Statement>, Option<&Statement>)>,
) -> Vec<StatementChange> {
    let mut changes = Vec::new();
    for (old_stmt, new_stmt) in stmt_pairs {
        match (old_stmt, new_stmt) {
            (Some(old), Some(new)) => {
                if old.raw != new.raw {
                    let parts = diff_statement_parts(old, new);
                    changes.push(StatementChange::Changed {
                        before: old.clone(),
                        after: new.clone(),
                        parts,
                    });
                }
            }
            (Some(old), None) => {
                changes.push(StatementChange::Removed(old.clone()));
            }
            (None, Some(new)) => {
                changes.push(StatementChange::Added(new.clone()));
            }
            (None, None) => unreachable!(),
        }
    }
    changes
}

fn diff_statement_parts(old: &Statement, new: &Statement) -> StatementChangeParts {
    let old_raw = &old.raw;
    let new_raw = &new.raw;

    let mainsnak_changed = old_raw.get("mainsnak") != new_raw.get("mainsnak");
    let qualifiers_changed = old_raw.get("qualifiers") != new_raw.get("qualifiers");
    let rank_changed = old_raw.get("rank") != new_raw.get("rank");
    let references_changed = old_raw.get("references") != new_raw.get("references");

    // If raw differs but all four sub-parts are equal, set value: true as the
    // catch-all so the change is visible (honesty invariant).
    if !mainsnak_changed && !qualifiers_changed && !rank_changed && !references_changed {
        return StatementChangeParts {
            value: true,
            qualifiers: false,
            rank: false,
            references: false,
        };
    }

    StatementChangeParts {
        value: mainsnak_changed,
        qualifiers: qualifiers_changed,
        rank: rank_changed,
        references: references_changed,
    }
}

/// The diff a consumer receives, selected by the revision's content model
/// (ADR-0016 Decision 4). Wikitext is byte-for-byte the existing path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ContentDiff {
    Text {
        diff: StructuredDiff,
        /// Set when this is a degradation (unknown model, unparseable entity):
        /// honest fallback, not a silent lie (D4).
        note: Option<String>,
    },
    Entity(EntityDiff),
}

/// Route two revision bodies to the right diff. `entity_id` is required only for
/// the entity path (parse needs the id); pass the page title (Q-id) from patrol.
#[must_use]
pub fn route_content_diff(
    model: &ContentModel,
    entity_id: Option<&EntityId>,
    before: Option<&str>,
    after: &str,
) -> ContentDiff {
    match model {
        ContentModel::Wikitext => {
            let before_text = before.unwrap_or("");
            let diff = crate::diff_engine::diff_lines(before_text, after);
            ContentDiff::Text { diff, note: None }
        }
        ContentModel::WikibaseItem | ContentModel::WikibaseProperty => {
            // entity_id is required for parsing; if missing, degrade to text
            let Some(id) = entity_id else {
                let diff = crate::diff_engine::diff_lines(before.unwrap_or(""), after);
                return ContentDiff::Text {
                    diff,
                    note: Some(
                        "entity revision could not be parsed; showing text diff".to_string(),
                    ),
                };
            };

            // Parse both sides (before `None` → first revision)
            let before_entity = if let Some(body) = before {
                if let Ok(entity) = crate::wikibase::parse_entity(id, body.as_bytes()) {
                    Some(entity)
                } else {
                    // before exists but is unparseable → degrade to text diff
                    let diff = crate::diff_engine::diff_lines(body, after);
                    return ContentDiff::Text {
                        diff,
                        note: Some(
                            "entity revision could not be parsed; showing text diff".to_string(),
                        ),
                    };
                }
            } else {
                None
            };
            let Ok(after_entity) = crate::wikibase::parse_entity(id, after.as_bytes()) else {
                // Unparseable entity → text with note
                let diff = crate::diff_engine::diff_lines(before.unwrap_or(""), after);
                return ContentDiff::Text {
                    diff,
                    note: Some(
                        "entity revision could not be parsed; showing text diff".to_string(),
                    ),
                };
            };

            // Both parses OK → compute entity diff
            let entity_diff = diff_entities(before_entity.as_ref(), &after_entity);
            ContentDiff::Entity(entity_diff)
        }
        ContentModel::Other(m) => {
            let before_text = before.unwrap_or("");
            let diff = crate::diff_engine::diff_lines(before_text, after);
            ContentDiff::Text {
                diff,
                note: Some(format!(
                    "content model {m} is not specially handled; showing text diff"
                )),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wikibase::{EntityId, parse_entity};

    fn entity(claims_json: &str, label: &str) -> crate::wikibase::Entity {
        let doc = format!(
            r#"{{"id":"Q1","type":"item","labels":{{"en":{{"language":"en","value":"{label}"}}}},
               "descriptions":{{}},"aliases":{{}},"claims":{claims_json},"sitelinks":{{}}}}"#
        );
        parse_entity(&EntityId::new("Q1"), doc.as_bytes()).unwrap()
    }

    const STMT: &str = r#"{"P569":[{"id":"Q1$a","mainsnak":{"snaktype":"value","property":"P569",
        "datavalue":{"value":"x","type":"string"}},"type":"statement","rank":"normal","references":[]}]}"#;

    #[test]
    fn identical_entities_diff_empty() {
        assert!(diff_entities(Some(&entity(STMT, "A")), &entity(STMT, "A")).is_empty());
    }

    #[test]
    fn missing_parent_yields_all_added_not_an_error() {
        let diff = diff_entities(None, &entity(STMT, "A"));
        assert_eq!(diff.labels.len(), 1);
        assert!(matches!(diff.statements[0], StatementChange::Added(_)));
    }

    #[test]
    fn label_change_is_classified() {
        let diff = diff_entities(Some(&entity(STMT, "A")), &entity(STMT, "B"));
        assert_eq!(
            diff.labels,
            vec![TermChange {
                lang: "en".into(),
                before: Some("A".into()),
                after: Some("B".into()),
            }]
        );
        assert!(diff.statements.is_empty());
    }

    #[test]
    fn rank_only_change_is_never_a_no_op() {
        let promoted = STMT.replace(r#""rank":"normal""#, r#""rank":"preferred""#);
        let diff = diff_entities(Some(&entity(STMT, "A")), &entity(&promoted, "A"));
        let StatementChange::Changed { parts, .. } = &diff.statements[0] else {
            panic!("expected Changed, got {:?}", diff.statements)
        };
        assert!(parts.rank);
        assert!(!parts.value && !parts.qualifiers && !parts.references);
    }

    #[test]
    fn qualifiers_only_change_is_never_a_no_op() {
        let with_qualifier = STMT.replace(
            r#""type":"statement","rank":"normal""#,
            r#""type":"statement","qualifiers":{"P580":[{"snaktype":"value","property":"P580",
                "datavalue":{"value":{"time":"+1971-00-00T00:00:00Z","precision":9},"type":"time"}}]},"rank":"normal""#,
        );
        let diff = diff_entities(Some(&entity(STMT, "A")), &entity(&with_qualifier, "A"));
        let StatementChange::Changed { parts, .. } = &diff.statements[0] else {
            panic!()
        };
        assert!(parts.qualifiers);
        assert!(!parts.value && !parts.rank && !parts.references);
    }

    #[test]
    fn raw_difference_outside_the_four_sub_parts_still_surfaces() {
        // The honesty catch-all: a field none of the four classifiers cover
        // (e.g. a statement-level "hash") must still register as a change.
        let with_extra = STMT.replace(
            r#""type":"statement""#,
            r#""type":"statement","hash":"deadbeef""#,
        );
        let diff = diff_entities(Some(&entity(STMT, "A")), &entity(&with_extra, "A"));
        let StatementChange::Changed { parts, .. } = &diff.statements[0] else {
            panic!("difference outside sub-parts must never be a no-op")
        };
        assert!(
            parts.value,
            "catch-all marks value so the change is visible"
        );
    }

    #[test]
    fn reference_only_change_is_never_a_no_op() {
        let with_ref = STMT.replace(
            r#""references":[]"#,
            r#""references":[{"snaks":{"P854":[{"snaktype":"value","property":"P854",
                "datavalue":{"value":"https://e.org","type":"string"}}]}}]"#,
        );
        let diff = diff_entities(Some(&entity(STMT, "A")), &entity(&with_ref, "A"));
        let StatementChange::Changed { parts, .. } = &diff.statements[0] else {
            panic!()
        };
        assert!(parts.references && !parts.value);
    }

    #[test]
    fn statement_added_and_removed_by_guid() {
        let two = STMT.replace(
            r"}]}",
            r#"},{"id":"Q1$b","mainsnak":{"snaktype":"value","property":"P569",
               "datavalue":{"value":"y","type":"string"}},"type":"statement","rank":"normal","references":[]}]}"#,
        );
        let diff = diff_entities(Some(&entity(STMT, "A")), &entity(&two, "A"));
        assert!(matches!(
            diff.statements.as_slice(),
            [StatementChange::Added(s)] if s.id.as_ref().expect("statement has id").as_str() == "Q1$b"
        ));
        let reverse = diff_entities(Some(&entity(&two, "A")), &entity(STMT, "A"));
        assert!(matches!(
            reverse.statements.as_slice(),
            [StatementChange::Removed(_)]
        ));
    }

    #[test]
    fn unknown_datatype_change_still_registers() {
        // The honesty invariant for datatypes we model as Other: raw equality catches it.
        let odd = STMT.replace(
            r#""value":"x","type":"string""#,
            r#""value":{"z":1},"type":"musical-notation""#,
        );
        let odd2 = odd.replace(r#"{"z":1}"#, r#"{"z":2}"#);
        let diff = diff_entities(Some(&entity(&odd, "A")), &entity(&odd2, "A"));
        assert!(
            matches!(&diff.statements[0], StatementChange::Changed { parts, .. } if parts.value)
        );
    }

    #[test]
    fn reordering_statements_within_property_yields_empty_diff() {
        // Pure reordering of GUID-stable statements within a property should yield
        // an empty diff, since statement identity is GUID-based, not position-based.
        let stmt_a = r#"{"id":"Q1$a","mainsnak":{"snaktype":"value","property":"P569","datavalue":{"value":"x","type":"string"}},"type":"statement","rank":"normal","references":[]}"#;
        let stmt_b = r#"{"id":"Q1$b","mainsnak":{"snaktype":"value","property":"P569","datavalue":{"value":"y","type":"string"}},"type":"statement","rank":"normal","references":[]}"#;
        let original = format!(r#"{{"P569":[{stmt_a},{stmt_b}]}}"#);
        let reordered = format!(r#"{{"P569":[{stmt_b},{stmt_a}]}}"#);
        let old_ent = entity(&original, "A");
        let new_ent = entity(&reordered, "A");
        let diff = diff_entities(Some(&old_ent), &new_ent);
        assert!(
            diff.is_empty(),
            "pure reordering of GUID-stable statements should yield empty diff"
        );
    }

    #[test]
    fn wikitext_routes_to_text_diff() {
        let diff = route_content_diff(&ContentModel::Wikitext, None, Some("old text"), "new text");
        assert!(matches!(diff, ContentDiff::Text { note: None, .. }));
    }

    #[test]
    fn wikitext_with_no_before_uses_empty_string() {
        let diff = route_content_diff(&ContentModel::Wikitext, None, None, "new text");
        assert!(matches!(diff, ContentDiff::Text { note: None, .. }));
    }

    #[test]
    fn entity_models_route_to_entity_diff() {
        let entity_json = r#"{"id":"Q42","type":"item","labels":{"en":{"language":"en","value":"Answer"}},"descriptions":{},"aliases":{},"claims":{},"sitelinks":{}}"#;
        let entity_json_updated = r#"{"id":"Q42","type":"item","labels":{"en":{"language":"en","value":"The Answer"}},"descriptions":{},"aliases":{},"claims":{},"sitelinks":{}}"#;

        let diff = route_content_diff(
            &ContentModel::WikibaseItem,
            Some(&EntityId::new("Q42")),
            Some(entity_json),
            entity_json_updated,
        );

        assert!(matches!(diff, ContentDiff::Entity(_)));
    }

    #[test]
    fn unparseable_entity_body_degrades_to_text_with_note() {
        let diff = route_content_diff(
            &ContentModel::WikibaseItem,
            Some(&EntityId::new("Q42")),
            Some(r#"{"id":"Q42"}"#),
            "garbage JSON {{{",
        );

        match diff {
            ContentDiff::Text { note: Some(n), .. } => {
                assert_eq!(n, "entity revision could not be parsed; showing text diff");
            }
            _ => panic!("Expected Text with note, got {diff:?}"),
        }
    }

    #[test]
    fn unparseable_before_with_valid_after_degrades_to_text_with_note() {
        let valid_entity_json = r#"{"id":"Q42","type":"item","labels":{"en":{"language":"en","value":"Answer"}},"descriptions":{},"aliases":{},"claims":{},"sitelinks":{}}"#;

        let diff = route_content_diff(
            &ContentModel::WikibaseItem,
            Some(&EntityId::new("Q42")),
            Some("garbage JSON {{{"),
            valid_entity_json,
        );

        match diff {
            ContentDiff::Text { note: Some(n), .. } => {
                assert_eq!(n, "entity revision could not be parsed; showing text diff");
            }
            _ => panic!("Expected Text with note for unparseable before, got {diff:?}"),
        }
    }

    #[test]
    fn entity_without_id_degrades_to_text_with_note() {
        let entity_json = r#"{"id":"Q42","type":"item","labels":{"en":{"language":"en","value":"Answer"}},"descriptions":{},"aliases":{},"claims":{},"sitelinks":{}}"#;

        let diff = route_content_diff(
            &ContentModel::WikibaseItem,
            None,
            Some(entity_json),
            entity_json,
        );

        match diff {
            ContentDiff::Text { note: Some(n), .. } => {
                assert_eq!(n, "entity revision could not be parsed; showing text diff");
            }
            _ => panic!("Expected Text with note, got {diff:?}"),
        }
    }

    #[test]
    fn unknown_model_degrades_to_text_with_note() {
        let diff = route_content_diff(
            &ContentModel::Other("Scribunto".into()),
            None,
            Some("old"),
            "new",
        );

        match diff {
            ContentDiff::Text { note: Some(n), .. } => {
                assert_eq!(
                    n,
                    "content model Scribunto is not specially handled; showing text diff"
                );
            }
            _ => panic!("Expected Text with note, got {diff:?}"),
        }
    }
}
