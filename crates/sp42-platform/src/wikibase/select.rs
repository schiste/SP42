use serde::{Deserialize, Serialize};

use super::model::{
    Entity, EntityId, PropertyId, Reference, Snak, Statement, StatementId, WikibaseValue,
};

/// Names one statement on one entity. Promoted from PR #103's `sp42-mcp::StatementRef`
/// (design plan §Statement identity); #103 re-exports this after convergence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatementRef {
    pub entity: EntityId,
    pub property: PropertyId,
    pub statement_id: Option<StatementId>,
}

impl Entity {
    /// Select the statement a `StatementRef` names: by GUID when given, else the
    /// first statement for the property. Exactly #103's `parse_statement` selection.
    #[must_use]
    pub fn statement(&self, r: &StatementRef) -> Option<&Statement> {
        let statements = self.statements.get(&r.property)?;
        match &r.statement_id {
            Some(guid) => statements.iter().find(|s| s.id.as_ref() == Some(guid)),
            None => statements.first(),
        }
    }
}

impl Reference {
    /// P854 ("reference URL") snaks. This is #103's `extract_ref_url`, typed and
    /// plural — #103 takes `.next()` for its first-URL behavior.
    pub fn urls(&self) -> impl Iterator<Item = &str> {
        self.snaks.iter().filter_map(|snak| match snak {
            Snak::Value {
                property,
                value: WikibaseValue::String(url),
            } if property.as_str() == "P854" => Some(url.as_str()),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wikibase::{EntityId, PropertyId, StatementId, parse_entity};

    const ENTITYDATA: &str = include_str!("../../../../fixtures/wikibase/q42_entitydata.json");

    fn q42() -> crate::wikibase::Entity {
        parse_entity(&EntityId::new("Q42"), ENTITYDATA.as_bytes()).unwrap()
    }

    #[test]
    fn selects_first_statement_for_property_without_guid() {
        let entity = q42();
        let r = StatementRef {
            entity: EntityId::new("Q42"),
            property: PropertyId::new("P69"),
            statement_id: None,
        };
        let stmt = entity.statement(&r).expect("found");
        assert_eq!(stmt.property, PropertyId::new("P69"));
    }

    #[test]
    fn selects_by_guid_when_given() {
        let entity = q42();
        let r = StatementRef {
            entity: EntityId::new("Q42"),
            property: PropertyId::new("P69"),
            statement_id: Some(StatementId::new("Q42$0E9C4724-C954-4698-84A7-5CE0D296A6F2")),
        };
        assert!(entity.statement(&r).is_some());
        let miss = StatementRef {
            statement_id: Some(StatementId::new("Q42$nope")),
            ..r
        };
        assert!(entity.statement(&miss).is_none());
    }

    #[test]
    fn reference_urls_yields_p854_strings() {
        let entity = q42();
        let educated = &entity.statements[&PropertyId::new("P69")][0];
        let urls: Vec<&str> = educated.references[0].urls().collect();
        assert_eq!(urls, vec!["https://example.org/adams-bio"]);
    }

    #[test]
    fn reference_urls_empty_when_no_p854() {
        // Test the case where a reference exists but has no P854 snak.
        // Create a reference with P248 ("stated in") snak instead.
        let reference = Reference {
            snaks: vec![Snak::Value {
                property: PropertyId::new("P248"),
                value: WikibaseValue::EntityId(EntityId::new("Q1234567")),
            }],
            raw: serde_json::json!({"snaks": {"P248": [{"snaktype": "value"}]}}),
        };

        // Assert that urls() yields nothing when P854 is absent
        assert!(reference.urls().next().is_none());
    }
}
