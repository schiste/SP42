//! Shared Wikibase (Wikidata) read model — ADR-0016.
//!
//! Typed entity parse, render, and diff shared by the citation MCP verb
//! (PRD-0010), patrol's `EntityDiff`, and the statement write lane (ADR-0017).
//! Design: docs/design-plans/2026-07-01-wikidata-read-model.md.
//!
//! The module is structured around the read-model's five concerns:
//!
//! - **Model** (`Entity`, `Statement`, `Snak`, `WikibaseValue`, `Reference`, etc.):
//!   Typed representation of Wikibase entities and claims, keeping raw JSON for exact
//!   change detection (the "never a no-op" invariant per ADR-0016 Decision 3).
//!
//! - **Reading** (`parse_entity`, `build_entity_request`, `build_label_request`, `Labels`):
//!   Endpoint-agnostic parsing from Special:EntityData or Action API JSON, plus HTTP
//!   request builders and label resolution (ADR-0016 Decision 1).
//!
//! - **Rendering** (`render_value`, `render_statement_claim`):
//!   Display text for Wikibase values and natural-language claim strings, used by
//!   PRD-0010 cite verification and rendering domains.
//!
//! - **Diffing** (`EntityDiff`, `StatementChange`, `ContentDiff`, `diff_entities`,
//!   `route_content_diff`, `ContentCapabilityProfile`):
//!   Change detection and content-model routing for patrol (ADR-0016 Decisions 4–5).
//!
//! - **Identity + write payload** (`StatementRef`, `StatementProposal`, `StatementGrounding`):
//!   Statement selectors and ADR-0017 propose/confirm types (propose-side only; the
//!   write lane is not implemented in platform).

mod capability;
mod diff;
mod model;
mod parse;
mod proposal;
mod read;
mod render;
mod select;

pub use capability::{ContentCapabilityProfile, derive_content_capability_profile};
pub use diff::{
    AliasChange, ContentDiff, EntityDiff, SitelinkChange, StatementChange, StatementChangeParts,
    TermChange, diff_entities, route_content_diff,
};
pub use model::{
    Entity, EntityId, Lang, PropertyId, Reference, Sitelink, Snak, Statement, StatementId,
    StatementRank, TermMap, WikibaseValue,
};
pub use parse::{WikibaseParseError, parse_entity};
pub use proposal::{StatementGrounding, StatementProposal};
pub use read::{
    Labels, RevisionContent, build_entity_request, build_label_request,
    build_revision_pair_request, parse_labels, parse_revision_contents,
};
pub use render::{ValueDisplay, render_statement_claim, render_value};
pub use select::StatementRef;
