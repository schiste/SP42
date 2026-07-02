use std::collections::BTreeMap;

use serde_json::Value;
use thiserror::Error;

use super::model::{
    Entity, EntityId, Lang, PropertyId, Reference, Sitelink, Snak, Statement, StatementId,
    StatementRank, TermMap, WikibaseValue,
};

#[derive(Debug, Error)]
pub enum WikibaseParseError {
    #[error("entity payload is not valid JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("entity {id} not found in payload")]
    EntityNotFound { id: EntityId },
    #[error("entity payload for {id} has no recognizable shape")]
    UnrecognizedShape { id: EntityId },
}

/// Parse a Wikibase entity from EITHER a `Special:EntityData` `{"entities":{id:{…}}}`
/// document OR the bare entity object found inside an Action-API revision slot.
/// Endpoint-agnostic: the entity JSON body is identical, only the wrapper differs.
///
/// # Errors
///
/// Returns `WikibaseParseError` if:
/// - The JSON payload is invalid (`InvalidJson`)
/// - The entity with the given ID is not found (`EntityNotFound`)
/// - The payload has no recognizable shape (`UnrecognizedShape`)
pub fn parse_entity(id: &EntityId, body: &[u8]) -> Result<Entity, WikibaseParseError> {
    let value: Value = serde_json::from_slice(body)?;
    parse_entity_from_value(id, &value)
}

fn parse_entity_from_value(id: &EntityId, value: &Value) -> Result<Entity, WikibaseParseError> {
    let entity_obj = if let Some(entities) = value.get("entities") {
        // Special:EntityData format: {"entities": {id: {...}}}
        entities
            .get(id.as_str())
            .ok_or_else(|| WikibaseParseError::EntityNotFound { id: id.clone() })?
    } else if value.get("id").and_then(|v| v.as_str()) == Some(id.as_str()) {
        // Bare entity object format
        value
    } else if value.is_object() && value.get("id").is_some() {
        // Has "id" field but it doesn't match
        return Err(WikibaseParseError::EntityNotFound { id: id.clone() });
    } else {
        return Err(WikibaseParseError::UnrecognizedShape { id: id.clone() });
    };

    // Parse last_revid
    let last_revid = entity_obj.get("lastrevid").and_then(Value::as_u64);

    // Parse labels
    let labels = parse_term_map(entity_obj.get("labels"));

    // Parse descriptions
    let descriptions = parse_term_map(entity_obj.get("descriptions"));

    // Parse aliases
    let aliases = parse_aliases(entity_obj.get("aliases"));

    // Parse sitelinks
    let sitelinks = parse_sitelinks(entity_obj.get("sitelinks"));

    // Parse statements
    let statements = parse_statements(entity_obj.get("claims"));

    Ok(Entity {
        id: id.clone(),
        last_revid,
        labels,
        descriptions,
        aliases,
        statements,
        sitelinks,
    })
}

fn parse_term_map(value: Option<&Value>) -> TermMap {
    let mut map = TermMap::new();
    if let Some(obj) = value.and_then(|v| v.as_object()) {
        for (lang, term_obj) in obj {
            if let Some(term_val) = term_obj.get("value").and_then(|v| v.as_str()) {
                map.insert(lang.clone(), term_val.to_string());
            }
        }
    }
    map
}

fn parse_aliases(value: Option<&Value>) -> BTreeMap<Lang, Vec<String>> {
    let mut map = BTreeMap::new();
    if let Some(obj) = value.and_then(|v| v.as_object()) {
        for (lang, alias_array) in obj {
            let mut aliases = Vec::new();
            if let Some(arr) = alias_array.as_array() {
                for alias_obj in arr {
                    if let Some(alias_val) = alias_obj.get("value").and_then(|v| v.as_str()) {
                        aliases.push(alias_val.to_string());
                    }
                }
            }
            map.insert(lang.clone(), aliases);
        }
    }
    map
}

fn parse_sitelinks(value: Option<&Value>) -> BTreeMap<String, Sitelink> {
    let mut map = BTreeMap::new();
    if let Some(obj) = value.and_then(|v| v.as_object()) {
        for (site, sitelink_obj) in obj {
            if let (Some(site_val), Some(title_val)) = (
                sitelink_obj.get("site").and_then(|v| v.as_str()),
                sitelink_obj.get("title").and_then(|v| v.as_str()),
            ) {
                map.insert(
                    site.clone(),
                    Sitelink {
                        site: site_val.to_string(),
                        title: title_val.to_string(),
                    },
                );
            }
        }
    }
    map
}

fn parse_statements(value: Option<&Value>) -> BTreeMap<PropertyId, Vec<Statement>> {
    let mut map = BTreeMap::new();
    if let Some(obj) = value.and_then(|v| v.as_object()) {
        for (prop, statement_array) in obj {
            let property = PropertyId::new(prop.clone());
            let mut statements = Vec::new();
            if let Some(arr) = statement_array.as_array() {
                for stmt_value in arr {
                    statements.push(parse_statement(&property, stmt_value));
                }
            }
            if !statements.is_empty() {
                map.insert(property, statements);
            }
        }
    }
    map
}

fn parse_statement(property: &PropertyId, stmt_value: &Value) -> Statement {
    // Store the raw statement before extraction
    let raw = stmt_value.clone();

    // Parse mainsnak
    let value = stmt_value.get("mainsnak").and_then(parse_snak);

    // If mainsnak parsing failed completely, create a recovery snak
    let value = value.unwrap_or_else(|| Snak::Value {
        property: property.clone(),
        value: WikibaseValue::Other(stmt_value.get("mainsnak").cloned().unwrap_or(Value::Null)),
    });

    // Parse qualifiers
    let qualifiers = stmt_value
        .get("qualifiers")
        .and_then(|q| q.as_object())
        .map(|obj| {
            let mut snaks = Vec::new();
            for (_prop, snak_array) in obj {
                if let Some(arr) = snak_array.as_array() {
                    for snak_value in arr {
                        if let Some(snak) = parse_snak(snak_value) {
                            snaks.push(snak);
                        }
                    }
                }
            }
            snaks
        })
        .unwrap_or_default();

    // Parse rank
    let rank = stmt_value
        .get("rank")
        .and_then(Value::as_str)
        .map_or(StatementRank::Normal, |r| match r {
            "preferred" => StatementRank::Preferred,
            "deprecated" => StatementRank::Deprecated,
            _ => StatementRank::Normal,
        });

    // Parse references
    let references = stmt_value
        .get("references")
        .and_then(|r| r.as_array())
        .map(|arr| arr.iter().filter_map(parse_reference).collect())
        .unwrap_or_default();

    // Parse ID
    let id = stmt_value
        .get("id")
        .and_then(|id_val| id_val.as_str())
        .map(|id_str| StatementId::new(id_str.to_string()));

    Statement {
        id,
        property: property.clone(),
        value,
        qualifiers,
        rank,
        references,
        raw,
    }
}

fn parse_snak(snak_value: &Value) -> Option<Snak> {
    let snaktype = snak_value.get("snaktype")?.as_str()?;
    let property_str = snak_value.get("property")?.as_str()?;
    let property = PropertyId::new(property_str.to_string());

    match snaktype {
        "novalue" => Some(Snak::NoValue { property }),
        "somevalue" => Some(Snak::SomeValue { property }),
        "value" => {
            let datavalue = snak_value.get("datavalue")?;
            let value = parse_datavalue(datavalue)?;
            Some(Snak::Value { property, value })
        }
        _ => None,
    }
}

fn parse_datavalue(datavalue: &Value) -> Option<WikibaseValue> {
    let datatype = datavalue.get("type")?.as_str()?;
    let value_obj = datavalue.get("value")?;

    match datatype {
        "string" => value_obj.as_str().map(|s| WikibaseValue::String(s.into())),
        "wikibase-entityid" => {
            // Prefer "id" field, fall back to numeric-id
            let id = if let Some(id_str) = value_obj.get("id").and_then(Value::as_str) {
                EntityId::new(id_str.to_string())
            } else if let Some(numeric_id) = value_obj.get("numeric-id").and_then(Value::as_u64) {
                EntityId::new(format!("Q{numeric_id}"))
            } else {
                return None;
            };
            Some(WikibaseValue::EntityId(id))
        }
        "monolingualtext" => {
            let text = value_obj.get("text")?.as_str()?.to_string();
            let lang = value_obj.get("language")?.as_str()?.to_string();
            Some(WikibaseValue::Monolingual { lang, text })
        }
        "time" => {
            let time = value_obj.get("time")?.as_str()?.to_string();
            let precision = u8::try_from(value_obj.get("precision")?.as_u64()?).ok()?;
            Some(WikibaseValue::Time { time, precision })
        }
        "quantity" => {
            let amount = value_obj.get("amount")?.as_str()?.to_string();
            let unit = value_obj
                .get("unit")
                .and_then(|u| u.as_str())
                .and_then(|unit_str| {
                    if unit_str == "1" || unit_str.is_empty() {
                        None
                    } else {
                        // Extract the last segment of the URI
                        // SAFETY: unit_str is non-empty (checked above), so split().next_back()
                        // always yields Some.
                        let unit_id = unit_str
                            .split('/')
                            .next_back()
                            .expect("non-empty string always yields segment");
                        Some(EntityId::new(unit_id.to_string()))
                    }
                });
            Some(WikibaseValue::Quantity { amount, unit })
        }
        "globecoordinate" => {
            let lat = value_obj.get("latitude")?.as_f64()?;
            let lon = value_obj.get("longitude")?.as_f64()?;
            Some(WikibaseValue::GlobeCoordinate { lat, lon })
        }
        _ => {
            // Unknown datatype — preserve as Other for forward-compat
            Some(WikibaseValue::Other(value_obj.clone()))
        }
    }
}

fn parse_reference(ref_value: &Value) -> Option<Reference> {
    let snaks_obj = ref_value.get("snaks")?.as_object()?;
    let mut snaks = Vec::new();

    for (_prop, snak_array) in snaks_obj {
        if let Some(arr) = snak_array.as_array() {
            for snak_value in arr {
                if let Some(snak) = parse_snak(snak_value) {
                    snaks.push(snak);
                }
            }
        }
    }

    Some(Reference {
        snaks,
        raw: ref_value.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENTITYDATA: &str = include_str!("../../../../fixtures/wikibase/q42_entitydata.json");

    fn q42() -> Entity {
        parse_entity(&EntityId::new("Q42"), ENTITYDATA.as_bytes()).expect("parses")
    }

    #[test]
    fn parses_terms_aliases_and_sitelinks() {
        let entity = q42();
        assert_eq!(entity.id, EntityId::new("Q42"));
        assert_eq!(entity.last_revid, Some(2_000_341));
        assert_eq!(
            entity.labels.get("en").map(String::as_str),
            Some("Douglas Adams")
        );
        assert_eq!(entity.aliases["en"], vec!["Douglas Noel Adams", "DNA"]);
        assert_eq!(entity.sitelinks["enwiki"].title, "Douglas Adams");
    }

    #[test]
    fn parses_statement_depth_qualifiers_rank_and_references() {
        let entity = q42();
        let educated = &entity.statements[&PropertyId::new("P69")][0];
        assert!(matches!(
            &educated.value,
            Snak::Value { value: WikibaseValue::EntityId(id), .. } if id.as_str() == "Q691283"
        ));
        assert_eq!(educated.qualifiers.len(), 1);
        assert_eq!(educated.rank, StatementRank::Normal);
        assert_eq!(educated.references.len(), 1);
        assert!(!educated.raw.is_null());
    }

    #[test]
    fn unknown_datatype_parses_as_other_never_fails() {
        let entity = q42();
        let unknown = &entity.statements[&PropertyId::new("P9999")][0];
        assert!(matches!(
            &unknown.value,
            Snak::Value {
                value: WikibaseValue::Other(_),
                ..
            }
        ));
        assert_eq!(unknown.rank, StatementRank::Deprecated);
    }

    #[test]
    fn novalue_and_somevalue_snaks_parse() {
        let entity = q42();
        assert!(matches!(
            &entity.statements[&PropertyId::new("P106")][0].value,
            Snak::NoValue { .. }
        ));
        assert!(matches!(
            &entity.statements[&PropertyId::new("P40")][0].value,
            Snak::SomeValue { .. }
        ));
    }

    #[test]
    fn monolingual_and_coordinate_values_parse() {
        let entity = q42();
        assert!(matches!(
            &entity.statements[&PropertyId::new("P1477")][0].value,
            Snak::Value { value: WikibaseValue::Monolingual { lang, text }, .. }
                if lang == "en" && text == "Douglas Noel Adams"
        ));
        assert!(matches!(
            &entity.statements[&PropertyId::new("P625")][0].value,
            Snak::Value {
                value: WikibaseValue::GlobeCoordinate { .. },
                ..
            }
        ));
    }

    #[test]
    fn parses_bare_entity_object_endpoint_agnostic() {
        // The Action-API revision slot carries the bare entity object, no
        // {"entities": {...}} wrapper. Same body, different wrapper (design plan §Reading).
        let doc: serde_json::Value = serde_json::from_str(ENTITYDATA).unwrap();
        let bare = serde_json::to_vec(&doc["entities"]["Q42"]).unwrap();
        let entity = parse_entity(&EntityId::new("Q42"), &bare).expect("parses bare form");
        assert_eq!(
            entity.labels.get("en").map(String::as_str),
            Some("Douglas Adams")
        );
    }

    #[test]
    fn missing_entity_is_an_error() {
        let err = parse_entity(&EntityId::new("Q1"), ENTITYDATA.as_bytes()).unwrap_err();
        assert!(matches!(err, WikibaseParseError::EntityNotFound { .. }));
    }

    #[test]
    fn serde_roundtrip_preserves_entity() {
        let entity = q42();
        let serialized = serde_json::to_value(&entity).expect("serializes");
        let deserialized: Entity = serde_json::from_value(serialized).expect("deserializes");
        assert_eq!(entity, deserialized);
    }

    #[test]
    fn invalid_json_is_an_error() {
        let bad_json = b"{ invalid json }";
        let err = parse_entity(&EntityId::new("Q42"), bad_json).unwrap_err();
        assert!(matches!(err, WikibaseParseError::InvalidJson(_)));
    }

    #[test]
    fn unrecognized_shape_is_an_error() {
        let unrecognized = br#"{"not_an_entity": "structure"}"#;
        let err = parse_entity(&EntityId::new("Q42"), unrecognized).unwrap_err();
        assert!(matches!(err, WikibaseParseError::UnrecognizedShape { .. }));
    }
}
