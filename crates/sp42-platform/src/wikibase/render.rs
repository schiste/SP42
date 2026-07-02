use super::model::{Entity, EntityId, Snak, Statement, WikibaseValue};
use super::read::Labels;

/// One rendered value; carries the item id when the value is an entity so the
/// caller can resolve its label. (#103's `render_value` tuple, typed.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueDisplay {
    pub text: String,
    pub item: Option<EntityId>,
}

#[must_use]
pub fn render_value(value: &WikibaseValue) -> ValueDisplay {
    match value {
        WikibaseValue::String(s) => ValueDisplay {
            text: s.clone(),
            item: None,
        },
        WikibaseValue::EntityId(id) => ValueDisplay {
            text: id.as_str().to_string(),
            item: Some(id.clone()),
        },
        WikibaseValue::Monolingual { text, .. } => ValueDisplay {
            text: text.clone(),
            item: None,
        },
        WikibaseValue::Time { time, .. } => ValueDisplay {
            text: time.clone(),
            item: None,
        },
        WikibaseValue::Quantity { amount, .. } => ValueDisplay {
            text: amount.clone(),
            item: None,
        },
        WikibaseValue::GlobeCoordinate { lat, lon } => ValueDisplay {
            text: format!("{lat}, {lon}"),
            item: None,
        },
        WikibaseValue::Other(v) => ValueDisplay {
            text: v.to_string(),
            item: None,
        },
    }
}

/// Render a statement to a natural-language claim
/// ("&lt;subject-label&gt; &lt;property-label&gt; &lt;value&gt;."), resolving labels via `labels`
/// with fallback chain: en → mul → entity id. This IS #103's `claim_rendered`.
/// The mul fallback mirrors Wikidata's convention: mul is the language-neutral default,
/// specific languages (like en) override it.
#[must_use]
pub fn render_statement_claim(subject: &Entity, stmt: &Statement, labels: &Labels) -> String {
    // Get subject label: entity's "en" label, falling back to "mul", then to entity id
    let subject_label = subject
        .labels
        .get("en")
        .or_else(|| subject.labels.get("mul"))
        .map_or_else(|| subject.id.as_str(), String::as_str);

    // Get property label: labels.get(property), falling back to property id
    let property_label = labels
        .get(stmt.property.as_str())
        .unwrap_or_else(|| stmt.property.as_str());

    // Get value text
    let value_text = match &stmt.value {
        Snak::Value { value, .. } => {
            let display = render_value(value);
            // When item is Some, try to get label; otherwise use text
            if let Some(item) = display.item {
                labels
                    .get(item.as_str())
                    .unwrap_or(display.text.as_str())
                    .to_string()
            } else {
                display.text
            }
        }
        Snak::SomeValue { .. } => "unknown value".to_string(),
        Snak::NoValue { .. } => "no value".to_string(),
    };

    format!("{subject_label} {property_label} {value_text}.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wikibase::{EntityId, PropertyId, WikibaseValue, parse_entity, parse_labels};

    const ENTITYDATA: &str = include_str!("../../../../fixtures/wikibase/q42_entitydata.json");
    const LABELS: &str = include_str!("../../../../fixtures/wikibase/q42_labels.json");

    #[test]
    fn renders_scalar_values() {
        assert_eq!(render_value(&WikibaseValue::String("x".into())).text, "x");
        let time = render_value(&WikibaseValue::Time {
            time: "+1952-03-11T00:00:00Z".into(),
            precision: 11,
        });
        assert_eq!(time.text, "+1952-03-11T00:00:00Z");
        let qty = render_value(&WikibaseValue::Quantity {
            amount: "+1.96".into(),
            unit: None,
        });
        assert_eq!(qty.text, "+1.96");
    }

    #[test]
    fn renders_monolingual_and_coordinate_values() {
        let mono = render_value(&WikibaseValue::Monolingual {
            lang: "en".into(),
            text: "Douglas Noel Adams".into(),
        });
        assert_eq!(mono.text, "Douglas Noel Adams");
        assert_eq!(mono.item, None);
        let coord = render_value(&WikibaseValue::GlobeCoordinate {
            lat: 51.75194,
            lon: -0.33638,
        });
        assert_eq!(coord.text, "51.75194, -0.33638");
    }

    #[test]
    fn somevalue_and_novalue_render_in_claims() {
        let entity = parse_entity(&EntityId::new("Q42"), ENTITYDATA.as_bytes()).unwrap();
        let labels = crate::wikibase::Labels::default();
        let somevalue = &entity.statements[&PropertyId::new("P40")][0];
        assert_eq!(
            render_statement_claim(&entity, somevalue, &labels),
            "Douglas Adams P40 unknown value."
        );
        let novalue = &entity.statements[&PropertyId::new("P106")][0];
        assert_eq!(
            render_statement_claim(&entity, novalue, &labels),
            "Douglas Adams P106 no value."
        );
    }

    #[test]
    fn entity_values_carry_the_item_for_label_lookup() {
        let display = render_value(&WikibaseValue::EntityId(EntityId::new("Q691283")));
        assert_eq!(display.text, "Q691283");
        assert_eq!(display.item, Some(EntityId::new("Q691283")));
    }

    #[test]
    fn claim_renders_subject_property_value_with_labels() {
        let entity = parse_entity(&EntityId::new("Q42"), ENTITYDATA.as_bytes()).unwrap();
        let labels = parse_labels(LABELS.as_bytes()).unwrap();
        let stmt = &entity.statements[&PropertyId::new("P69")][0];
        assert_eq!(
            render_statement_claim(&entity, stmt, &labels),
            "Douglas Adams educated at St John's College."
        );
    }

    #[test]
    fn claim_falls_back_to_ids_when_labels_missing() {
        let entity = parse_entity(&EntityId::new("Q42"), ENTITYDATA.as_bytes()).unwrap();
        let labels = crate::wikibase::Labels::default();
        let stmt = &entity.statements[&PropertyId::new("P69")][0];
        assert_eq!(
            render_statement_claim(&entity, stmt, &labels),
            "Douglas Adams P69 Q691283."
        );
    }

    #[test]
    fn subject_label_falls_back_to_mul_when_en_missing() {
        // Entity with only mul label (like live Wikidata Q42 now)
        let mul_only_json = r#"{
            "entities": {
                "Q1234": {
                    "type": "item",
                    "id": "Q1234",
                    "labels": {
                        "mul": { "language": "mul", "value": "Default Label" }
                    },
                    "claims": {
                        "P69": [{
                            "id": "Q1234$abc",
                            "mainsnak": {
                                "snaktype": "value",
                                "property": "P69",
                                "datavalue": {
                                    "value": { "entity-type": "item", "numeric-id": 691283, "id": "Q691283" },
                                    "type": "wikibase-entityid"
                                }
                            },
                            "type": "statement",
                            "rank": "normal"
                        }]
                    }
                }
            }
        }"#;

        let entity = parse_entity(&EntityId::new("Q1234"), mul_only_json.as_bytes()).unwrap();
        let labels = crate::wikibase::Labels::default();
        let stmt = &entity.statements[&PropertyId::new("P69")][0];

        // Should use mul label when en is missing
        assert_eq!(
            render_statement_claim(&entity, stmt, &labels),
            "Default Label P69 Q691283."
        );
    }

    #[test]
    fn subject_label_prefers_en_over_mul() {
        // Entity with both en and mul labels: en should win
        let en_and_mul_json = r#"{
            "entities": {
                "Q5678": {
                    "type": "item",
                    "id": "Q5678",
                    "labels": {
                        "en": { "language": "en", "value": "English Label" },
                        "mul": { "language": "mul", "value": "Default Label" }
                    },
                    "claims": {
                        "P69": [{
                            "id": "Q5678$def",
                            "mainsnak": {
                                "snaktype": "value",
                                "property": "P69",
                                "datavalue": {
                                    "value": { "entity-type": "item", "numeric-id": 691283, "id": "Q691283" },
                                    "type": "wikibase-entityid"
                                }
                            },
                            "type": "statement",
                            "rank": "normal"
                        }]
                    }
                }
            }
        }"#;

        let entity = parse_entity(&EntityId::new("Q5678"), en_and_mul_json.as_bytes()).unwrap();
        let labels = crate::wikibase::Labels::default();
        let stmt = &entity.statements[&PropertyId::new("P69")][0];

        // Should prefer en label when both present
        assert_eq!(
            render_statement_claim(&entity, stmt, &labels),
            "English Label P69 Q691283."
        );
    }
}
