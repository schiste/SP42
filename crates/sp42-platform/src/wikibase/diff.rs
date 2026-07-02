use serde::{Deserialize, Serialize};

use super::model::{Lang, Sitelink, Statement};

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
    /// No classified changes at all. With the honesty invariant this is
    /// equivalent to "the two revisions are byte-identical entities".
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
#[allow(clippy::large_enum_variant)] // https://github.com/schiste/SP42/blob/main/docs/adr/0016-wikibase-read-model.md
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
#[allow(clippy::struct_excessive_bools)] // https://github.com/schiste/SP42/blob/main/docs/adr/0016-wikibase-read-model.md
pub struct StatementChangeParts {
    pub value: bool,
    pub qualifiers: bool,
    pub rank: bool,
    pub references: bool,
}
