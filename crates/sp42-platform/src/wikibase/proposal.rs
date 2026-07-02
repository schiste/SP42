use serde::{Deserialize, Serialize};

use super::model::{EntityId, Statement};

/// The ADR-0017 propose/confirm payload for adding a referenced statement.
/// Propose-side only: applying it is the write lane (ADR-0017, not implemented here).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatementProposal {
    pub entity: EntityId,
    /// Drift baseline — `Entity.last_revid` at propose time. Confirm refuses on drift
    /// (ADR-0010 discipline).
    pub base_revid: u64,
    /// Property + value + qualifiers + rank + the citation reference, as parsed/built.
    pub statement: Statement,
    pub grounding: StatementGrounding,
}

/// ADR-0007 grounding for the proposed fact: a verbatim source passage.
/// Structural twin of the citation domain's grounding (which platform cannot
/// depend on, ADR-0013); converge if a shared home materializes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatementGrounding {
    pub source_url: String,
    /// Verbatim quote — surfaced only when it re-locates in the source (anti-fabrication).
    pub quote: String,
    pub source_hash: String,
}

#[cfg(test)]
mod tests {
    use crate::wikibase::{EntityId, PropertyId, parse_entity};

    use super::*;

    #[test]
    fn proposal_round_trips_through_json() {
        let entity = parse_entity(
            &EntityId::new("Q42"),
            include_str!("../../../../fixtures/wikibase/q42_entitydata.json").as_bytes(),
        )
        .unwrap();
        let proposal = StatementProposal {
            entity: EntityId::new("Q42"),
            base_revid: entity.last_revid.unwrap(),
            statement: entity.statements[&PropertyId::new("P69")][0].clone(),
            grounding: StatementGrounding {
                source_url: "https://example.org/adams-bio".into(),
                quote: "Adams was educated at St John's College.".into(),
                source_hash: "sha256:abc".into(),
            },
        };
        let json = serde_json::to_string(&proposal).unwrap();
        let back: StatementProposal = serde_json::from_str(&json).unwrap();
        assert_eq!(back, proposal);
        assert_eq!(back.base_revid, 2_000_341);
    }
}
