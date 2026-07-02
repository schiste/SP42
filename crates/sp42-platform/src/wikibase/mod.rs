//! Shared Wikibase (Wikidata) read model — ADR-0016.
//!
//! Typed entity parse, render, and diff shared by the citation MCP verb
//! (PRD-0010), patrol's `EntityDiff`, and the statement write lane (ADR-0017).
//! Design: docs/design-plans/2026-07-01-wikidata-read-model.md.

mod model;
mod parse;
mod read;
mod select;

pub use model::{
    Entity, EntityId, Lang, PropertyId, Reference, Sitelink, Snak, Statement, StatementId,
    StatementRank, TermMap, WikibaseValue,
};
pub use parse::{WikibaseParseError, parse_entity};
pub use read::{
    Labels, RevisionContent, build_entity_request, build_label_request,
    build_revision_pair_request, parse_labels, parse_revision_contents,
};
pub use select::StatementRef;
