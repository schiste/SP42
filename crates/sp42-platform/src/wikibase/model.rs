use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// A Wikibase entity identifier (e.g., `Q42`, `P123`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EntityId(String);

impl EntityId {
    /// Create a new entity ID from a string.
    pub fn new(id: impl Into<String>) -> Self {
        EntityId(id.into())
    }

    /// Get the entity ID as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A Wikibase property identifier (e.g., `P31`, `P813`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PropertyId(String);

impl PropertyId {
    /// Create a new property ID from a string.
    pub fn new(id: impl Into<String>) -> Self {
        PropertyId(id.into())
    }

    /// Get the property ID as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PropertyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A statement identifier (e.g., `Q42$0E9C4724-C954-4698-84A7-5CE0D296A6F2`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct StatementId(String);

impl StatementId {
    /// Create a new statement ID from a string.
    pub fn new(id: impl Into<String>) -> Self {
        StatementId(id.into())
    }

    /// Get the statement ID as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for StatementId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A language code (e.g., `"en"`, `"fr"`).
pub type Lang = String;

/// A map of language codes to term values.
pub type TermMap = BTreeMap<Lang, String>;

/// A Wikibase entity with labels, descriptions, aliases, statements, and sitelinks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entity {
    pub id: EntityId,
    /// Drift baseline (ADR-0017); `None` if the read endpoint didn't carry a revision.
    pub last_revid: Option<u64>,
    pub labels: TermMap,
    pub descriptions: TermMap,
    pub aliases: BTreeMap<Lang, Vec<String>>,
    pub statements: BTreeMap<PropertyId, Vec<Statement>>,
    pub sitelinks: BTreeMap<String, Sitelink>,
}

/// A sitelink to a page on a wiki (e.g., Wikipedia article).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sitelink {
    pub site: String,
    pub title: String,
}

/// A statement (claim) in a Wikibase entity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Statement {
    pub id: Option<StatementId>,
    pub property: PropertyId,
    pub value: Snak,
    pub qualifiers: Vec<Snak>,
    pub rank: StatementRank,
    pub references: Vec<Reference>,
    /// Canonical JSON of the statement — change detection stays exact even for
    /// datatypes we don't richly model (never-a-no-op, ADR-0016 Decision 3).
    pub raw: serde_json::Value,
}

/// A reference (source) for a statement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reference {
    pub snaks: Vec<Snak>,
    pub raw: serde_json::Value,
}

/// A Wikibase snak (property-value pair).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Snak {
    Value {
        property: PropertyId,
        value: WikibaseValue,
    },
    SomeValue {
        property: PropertyId,
    },
    NoValue {
        property: PropertyId,
    },
}

/// A Wikibase value (various datatype representations).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WikibaseValue {
    String(String),
    EntityId(EntityId),
    Monolingual {
        lang: Lang,
        text: String,
    },
    Time {
        time: String,
        precision: u8,
    },
    Quantity {
        amount: String,
        unit: Option<EntityId>,
    },
    GlobeCoordinate {
        lat: f64,
        lon: f64,
    },
    /// Forward-compat: unknown datatypes preserved, never a parse failure,
    /// still diffable via `Statement.raw`.
    Other(serde_json::Value),
}

/// The rank of a statement (preferred, normal, or deprecated).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StatementRank {
    #[serde(rename = "preferred")]
    Preferred,
    #[serde(rename = "normal")]
    Normal,
    #[serde(rename = "deprecated")]
    Deprecated,
}
