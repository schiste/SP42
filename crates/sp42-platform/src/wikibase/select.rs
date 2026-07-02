use serde::{Deserialize, Serialize};

use super::model::{Entity, EntityId, PropertyId, Statement, StatementId};

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
}
