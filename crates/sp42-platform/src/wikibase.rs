//! Wikibase entity content-model support (ADR-0016): the shared read model for
//! Wikidata entities.
//!
//! This module owns the platform half of the Wikidata read path — the typed
//! entity/statement model, the endpoint-agnostic entity parser, the label
//! lookup and statement→claim rendering promoted from `sp42-mcp`'s
//! `verify_wikidata_statement` (PR 103, its first consumer), the full-depth
//! [`EntityDiff`], and the [`ContentDiff`] sum that routes review surfaces by a
//! revision's content model. Everything here is pure: request *builders* return
//! [`HttpRequest`] values and *parsers* consume bytes, so shells inject the
//! `HttpClient` and tests replay fixtures with no live network (ADR-0004/0009).
//!
//! Two ADR-0016 invariants are load-bearing:
//!
//! - **Routing keys on the revision's content model, never the wiki id**
//!   (`wikidata.org` has wikitext talk pages; Wikipedias carry non-wikitext
//!   pages). See [`classify_content_model`].
//! - **The honesty invariant:** every change in modeled review-relevant fields
//!   surfaces as a classified change, and an unmodeled top-level entity delta
//!   surfaces as an explicit [`UnknownEntityChange`] instead of being silently
//!   dropped. An edit touching only a qualifier, rank, or reference is never
//!   rendered as a no-op — statements retain their canonical JSON (`raw`) so
//!   change detection is exact even for datatypes the typed model does not
//!   cover.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use url::Url;

use crate::diff_engine::StructuredDiff;
use crate::types::{HttpMethod, HttpRequest};

/// The `MediaWiki` content model for ordinary wikitext pages.
pub const WIKITEXT_CONTENT_MODEL: &str = "wikitext";
/// The content model of a Wikidata item (`Q…`) main-namespace revision.
pub const WIKIBASE_ITEM_CONTENT_MODEL: &str = "wikibase-item";
/// The content model of a Wikidata property (`P…`) revision.
pub const WIKIBASE_PROPERTY_CONTENT_MODEL: &str = "wikibase-property";

/// The Wikidata property id for "reference URL" — the URL-citation reference
/// snak. One supported reference property among several (ADR-0017 keeps the
/// full snak set so non-URL references are not flattened into this case).
pub const REFERENCE_URL_PROPERTY: &str = "P854";

/// Errors from Wikibase request building and payload parsing.
#[derive(Debug, Error)]
pub enum WikibaseError {
    /// The request inputs are invalid (bad entity id, empty label id list, …).
    #[error("wikibase request is invalid: {message}")]
    InvalidRequest {
        /// Human-readable description of the invalid input.
        message: String,
    },
    /// The payload parsed as JSON but is not a usable entity document.
    #[error("wikibase entity payload is invalid: {message}")]
    InvalidEntity {
        /// Human-readable description of the invalid payload.
        message: String,
    },
    /// The payload is not valid JSON at all.
    #[error("wikibase payload is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
}

// ---------------------------------------------------------------------------
// Content-model classification and capability gating (ADR-0016 D1/D4/D5)
// ---------------------------------------------------------------------------

/// Coarse classification of a revision's content model, for routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentModelClass {
    /// Ordinary wikitext — the existing review path, byte-for-byte untouched.
    Wikitext,
    /// A Wikibase entity (`wikibase-item` / `wikibase-property`) — routes to
    /// the entity diff path.
    WikibaseEntity,
    /// Any other named content model (JSON pages, Scribunto, CSS, …) — falls
    /// back to the text path with a note, never a hard error.
    Other,
    /// The content model is not known (older streams and snapshots carry
    /// none). Treated like wikitext, matching pre-ADR-0016 behavior.
    Unknown,
}

/// Classify a revision's content model for routing.
///
/// Routing keys on this value, never on the wiki id (ADR-0016 Decision 1).
#[must_use]
pub fn classify_content_model(content_model: Option<&str>) -> ContentModelClass {
    match content_model {
        None => ContentModelClass::Unknown,
        Some(WIKITEXT_CONTENT_MODEL) => ContentModelClass::Wikitext,
        Some(WIKIBASE_ITEM_CONTENT_MODEL | WIKIBASE_PROPERTY_CONTENT_MODEL) => {
            ContentModelClass::WikibaseEntity
        }
        Some(_) => ContentModelClass::Other,
    }
}

/// Which content-model-specific features apply to a revision.
///
/// This is the content axis of the capability model (ADR-0016 Decision 5): a
/// property of the *content*, separate from the OAuth-grant/rights/token axis
/// derived per account. Gated paths are **not invoked** for content that does
/// not support them, rather than invoked and discarded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentModelCapabilities {
    /// Wikitext-only signals apply: media-reference extraction, talk-page
    /// warning parsing, citation extraction/Parsoid rendering.
    pub wikitext_signals: bool,
    /// The Wikipedia-trained `LiftWing` revertrisk score is meaningful. Off for
    /// entity content: scoring is skipped, not faked (ADR-0016 Decision 7).
    pub revertrisk_scoring: bool,
    /// The revision routes to the structured entity diff path.
    pub entity_diff: bool,
}

/// The per-namespace *default* content model for a site, mirroring
/// `MediaWiki`'s own namespace configuration (`$wgNamespaceContentModels`):
/// on Wikidata, the main namespace holds items and namespace 120 holds
/// properties. This seeds `EditEvent.content_model` at ingestion — the
/// change feeds carry no content model — and routing still keys on the
/// per-revision value it seeds, so a Wikidata talk-page edit stays wikitext
/// (`None` here = the wiki's ordinary default). Scoped to the wikidata.org
/// family; other Wikibase hosts can be added as they are supported.
#[must_use]
pub fn default_namespace_content_model(api_url: &Url, namespace: i32) -> Option<&'static str> {
    let host = api_url.host_str()?;
    let is_wikidata = host == "wikidata.org" || host.ends_with(".wikidata.org");
    if !is_wikidata {
        return None;
    }
    wikidata_namespace_default(namespace)
}

/// The wikidata-family namespace → content-model defaults shared by both
/// keyed forms above (mirrors `$wgNamespaceContentModels`).
fn wikidata_namespace_default(namespace: i32) -> Option<&'static str> {
    match namespace {
        0 => Some(WIKIBASE_ITEM_CONTENT_MODEL),
        120 => Some(WIKIBASE_PROPERTY_CONTENT_MODEL),
        _ => None,
    }
}

/// [`default_namespace_content_model`], keyed by wiki dbname instead of the
/// api url — the form browser surfaces hold (a stream event carries the
/// `wiki_id`, not its api url). Same wikidata-family scope; other Wikibase
/// hosts are additive here too.
#[must_use]
pub fn default_namespace_content_model_for_wiki(
    wiki_id: &str,
    namespace: i32,
) -> Option<&'static str> {
    if !matches!(wiki_id, "wikidatawiki" | "testwikidatawiki") {
        return None;
    }
    wikidata_namespace_default(namespace)
}

/// Derive the content-model capability axis for a revision.
#[must_use]
pub fn derive_content_model_capabilities(content_model: Option<&str>) -> ContentModelCapabilities {
    match classify_content_model(content_model) {
        // Unknown keeps pre-ADR-0016 behavior; Other degrades to the text
        // path but keeps existing signals (honest fallback, not a hard gate).
        ContentModelClass::Wikitext | ContentModelClass::Other | ContentModelClass::Unknown => {
            ContentModelCapabilities {
                wikitext_signals: true,
                revertrisk_scoring: true,
                entity_diff: false,
            }
        }
        ContentModelClass::WikibaseEntity => ContentModelCapabilities {
            wikitext_signals: false,
            revertrisk_scoring: false,
            entity_diff: true,
        },
    }
}

// ---------------------------------------------------------------------------
// The typed entity model
// ---------------------------------------------------------------------------

/// A statement's rank.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StatementRank {
    /// The preferred statement among several for a property.
    Preferred,
    /// The default rank.
    #[default]
    Normal,
    /// Deprecated — known-wrong or superseded values kept for the record.
    Deprecated,
}

/// A typed Wikibase data value. Datatypes outside the modeled set are
/// preserved verbatim in [`WikibaseValue::Other`] — never a parse failure, and
/// still exactly diffable via each statement's retained `raw` JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WikibaseValue {
    /// A plain string value (URLs, external identifiers, …).
    String(String),
    /// A link to another entity, by id (`Q…` / `P…`).
    EntityId(String),
    /// A single-language text value.
    Monolingual {
        /// The language code.
        language: String,
        /// The text.
        text: String,
    },
    /// A point in time, in Wikibase's `+YYYY-MM-DD…` form (sign preserved).
    Time {
        /// The timestamp string as stored.
        time: String,
        /// The Wikibase precision code (9 = year, 10 = month, 11 = day),
        /// when the payload carries one.
        precision: Option<u8>,
    },
    /// A quantity, kept as the decimal string Wikibase stores.
    Quantity {
        /// The signed decimal amount as stored (sign preserved).
        amount: String,
        /// The unit entity URI; `None` for the dimensionless unit `"1"`.
        unit: Option<String>,
        /// The lower uncertainty bound, when the payload carries one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lower_bound: Option<String>,
        /// The upper uncertainty bound, when the payload carries one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        upper_bound: Option<String>,
    },
    /// Any other datatype, preserved as raw JSON.
    Other(Value),
}

/// What a snak asserts about its property.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WikibaseSnakKind {
    /// A concrete value.
    Value(WikibaseValue),
    /// "Unknown value" — the property applies but the value is unknown.
    SomeValue,
    /// "No value" — the property is known not to apply.
    NoValue,
}

/// One property→value assertion (a main value, qualifier, or reference part).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikibaseSnak {
    /// The property id (`P…`).
    pub property: String,
    /// The asserted value kind.
    pub kind: WikibaseSnakKind,
}

/// One reference on a statement: the full snak set, not only URL references
/// (ADR-0017 needs "stated in"/pages/identifier reference snaks intact).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikibaseReference {
    /// The reference's snaks, in `snaks-order` where present.
    pub snaks: Vec<WikibaseSnak>,
    /// The reference's canonical JSON, for exact change detection.
    pub raw: Value,
}

impl WikibaseReference {
    /// The reference-URL (P854) string values on this reference, in order.
    pub fn urls(&self) -> impl Iterator<Item = &str> {
        self.snaks.iter().filter_map(|snak| {
            if snak.property == REFERENCE_URL_PROPERTY
                && let WikibaseSnakKind::Value(WikibaseValue::String(url)) = &snak.kind
            {
                Some(url.as_str())
            } else {
                None
            }
        })
    }
}

/// One statement on an entity, at full depth (PRD-0011 Q4): main value,
/// qualifiers, rank, and references.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikibaseStatement {
    /// The statement GUID (`Q42$…`), when the payload carries one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The property this statement is about (`P…`).
    pub property: String,
    /// The main value snak.
    pub value: WikibaseSnak,
    /// Qualifier snaks, in `qualifiers-order` where present.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub qualifiers: Vec<WikibaseSnak>,
    /// The statement rank.
    #[serde(default)]
    pub rank: StatementRank,
    /// The statement's references, each with its full snak set.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<WikibaseReference>,
    /// The statement's canonical JSON. Retained so change detection is exact
    /// even for datatypes the typed model does not cover — this is what makes
    /// the never-a-no-op invariant hold unconditionally.
    pub raw: Value,
}

/// A parsed Wikibase entity (item or property).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikibaseEntity {
    /// The entity id actually returned (`Q…` / `P…`; may differ from the
    /// requested id when the request followed a redirect).
    pub id: String,
    /// The entity's latest revision id when the payload carries one — the
    /// drift baseline ADR-0017 proposals record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_revid: Option<u64>,
    /// Labels by language code.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
    /// Descriptions by language code.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub descriptions: BTreeMap<String, String>,
    /// Aliases by language code, in payload order.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub aliases: BTreeMap<String, Vec<String>>,
    /// Statements by property id, in payload order.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub statements: BTreeMap<String, Vec<WikibaseStatement>>,
    /// Sitelinks by site dbname: linked title plus badge item ids, so a
    /// badge-only edit (title unchanged) is still a detectable change and
    /// the never-a-no-op invariant holds.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sitelinks: BTreeMap<String, WikibaseSitelink>,
    /// Unmodeled top-level entity fields (`datatype`, lexeme forms, …),
    /// preserved so [`diff_entities`] can surface changes to them as
    /// [`UnknownEntityChange`]s instead of silently dropping them.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, Value>,
}

/// One sitelink: the linked page title and the badge item ids
/// (`Q17437796` "featured article" and friends). Badges are part of the
/// sitelink payload, so a badge-only edit must not diff as a no-op.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikibaseSitelink {
    pub title: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub badges: Vec<String>,
}

impl WikibaseSitelink {
    /// The display form sitelink diff rows carry: the title, plus the badge
    /// ids in brackets when any are set (`"Douglas Adams [Q17437796]"`).
    #[must_use]
    pub fn display(&self) -> String {
        if self.badges.is_empty() {
            self.title.clone()
        } else {
            format!("{} [{}]", self.title, self.badges.join(", "))
        }
    }
}

impl WikibaseEntity {
    /// Select a statement by property, and optionally by statement GUID.
    ///
    /// Without a GUID the first statement for the property is returned — the
    /// selection rule `verify_wikidata_statement` (PR 103) established.
    #[must_use]
    pub fn statement(
        &self,
        property: &str,
        statement_id: Option<&str>,
    ) -> Option<&WikibaseStatement> {
        let statements = self.statements.get(property)?;
        match statement_id {
            Some(id) => statements
                .iter()
                .find(|statement| statement.id.as_deref() == Some(id)),
            None => statements.first(),
        }
    }
}

// ---------------------------------------------------------------------------
// Request builders (pure; shells inject the HttpClient)
// ---------------------------------------------------------------------------

fn site_root(api_url: &Url) -> Result<Url, WikibaseError> {
    let mut root = api_url.clone();
    root.set_path("/");
    root.set_query(None);
    root.set_fragment(None);
    if root.host_str().is_none() {
        return Err(WikibaseError::InvalidRequest {
            message: format!("api url {api_url} has no host"),
        });
    }
    Ok(root)
}

fn validate_entity_id(entity_id: &str) -> Result<(), WikibaseError> {
    let mut chars = entity_id.chars();
    let leads_ok = matches!(chars.next(), Some('Q' | 'P' | 'L'));
    if leads_ok && chars.clone().next().is_some() && chars.all(|c| c.is_ascii_digit()) {
        Ok(())
    } else {
        Err(WikibaseError::InvalidRequest {
            message: format!("`{entity_id}` is not a Wikibase entity id"),
        })
    }
}

/// Build a keyless `Special:EntityData/{id}.json` read for an entity,
/// optionally pinned to a specific revision (the parent read of a diff).
///
/// The host is derived from the wiki's configured `api_url`, so the builder
/// works against `www.wikidata.org`, `test.wikidata.org`, or any Wikibase
/// host without hardcoding one. The entity id is validated before it is
/// interpolated into the path.
///
/// # Errors
///
/// Returns [`WikibaseError::InvalidRequest`] when the entity id is not a
/// Wikibase id or the api url carries no host.
pub fn build_entity_request(
    api_url: &Url,
    entity_id: &str,
    revision: Option<u64>,
) -> Result<HttpRequest, WikibaseError> {
    validate_entity_id(entity_id)?;
    let root = site_root(api_url)?;
    let mut url = root
        .join(&format!("wiki/Special:EntityData/{entity_id}.json"))
        .map_err(|error| WikibaseError::InvalidRequest {
            message: format!("entity url did not build: {error}"),
        })?;
    if let Some(revision) = revision {
        url.query_pairs_mut()
            .append_pair("revision", &revision.to_string());
    }
    Ok(HttpRequest {
        method: HttpMethod::Get,
        url,
        headers: BTreeMap::new(),
        body: Vec::new(),
    })
}

/// Build a `wbgetentities` label lookup for a set of entity/property ids.
///
/// # Errors
///
/// Returns [`WikibaseError::InvalidRequest`] when `ids` is empty or contains
/// a non-Wikibase id.
pub fn build_label_request(
    api_url: &Url,
    ids: &[String],
    language: &str,
) -> Result<HttpRequest, WikibaseError> {
    if ids.is_empty() {
        return Err(WikibaseError::InvalidRequest {
            message: "label lookup needs at least one id".to_string(),
        });
    }
    for id in ids {
        validate_entity_id(id)?;
    }
    let mut url = api_url.clone();
    url.query_pairs_mut()
        .append_pair("action", "wbgetentities")
        .append_pair("ids", &ids.join("|"))
        .append_pair("props", "labels")
        .append_pair("languages", language)
        .append_pair("format", "json");
    Ok(HttpRequest {
        method: HttpMethod::Get,
        url,
        headers: BTreeMap::new(),
        body: Vec::new(),
    })
}

// ---------------------------------------------------------------------------
// Parsing (endpoint-agnostic)
// ---------------------------------------------------------------------------

/// Entity/property labels resolved for one language.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikibaseLabels(BTreeMap<String, String>);

impl WikibaseLabels {
    /// The label for an id, when one was returned.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&str> {
        self.0.get(id).map(String::as_str)
    }

    /// The resolved labels as an id → label map.
    #[must_use]
    pub fn into_map(self) -> BTreeMap<String, String> {
        self.0
    }
}

/// Parse a `wbgetentities` labels response for one language.
///
/// # Errors
///
/// Returns [`WikibaseError::Json`] when the body is not valid JSON.
pub fn parse_labels(body: &[u8], language: &str) -> Result<WikibaseLabels, WikibaseError> {
    let doc: Value = serde_json::from_slice(body)?;
    let mut labels = BTreeMap::new();
    if let Some(entities) = doc.get("entities").and_then(Value::as_object) {
        for (id, entity) in entities {
            if let Some(label) = entity
                .get("labels")
                .and_then(|l| l.get(language))
                .and_then(|l| l.get("value"))
                .and_then(Value::as_str)
            {
                labels.insert(id.clone(), label.to_owned());
            }
        }
    }
    Ok(WikibaseLabels(labels))
}

/// Parse an entity from either wrapper the two read endpoints produce: a
/// `Special:EntityData` document (`{"entities": {"Q42": {…}}}`) or a bare
/// entity object (the `prop=revisions` slot content). The entity JSON body is
/// identical either way (ADR-0016 Decision 2), so the parser is
/// endpoint-agnostic.
///
/// When the document wraps a *different* id than requested (a redirect), the
/// single wrapped entity is returned and its own id is preserved, so the
/// caller can see the redirect rather than silently assuming identity.
///
/// # Errors
///
/// Returns [`WikibaseError::Json`] for non-JSON bodies and
/// [`WikibaseError::InvalidEntity`] when no entity object can be located.
pub fn parse_entity(entity_id: &str, body: &[u8]) -> Result<WikibaseEntity, WikibaseError> {
    let doc: Value = serde_json::from_slice(body)?;
    let entity = locate_entity_object(entity_id, &doc)?;
    Ok(parse_entity_object(entity_id, entity))
}

/// Parse a bare entity body whose own `id` field is authoritative — the
/// `prop=revisions` slot-content case, where no id was requested up front.
///
/// # Errors
///
/// Returns [`WikibaseError::Json`] for non-JSON bodies and
/// [`WikibaseError::InvalidEntity`] when the body carries no `id`.
pub fn parse_entity_content(body: &[u8]) -> Result<WikibaseEntity, WikibaseError> {
    let entity = parse_entity("", body)?;
    if entity.id.is_empty() {
        return Err(WikibaseError::InvalidEntity {
            message: "entity content carries no id".to_string(),
        });
    }
    Ok(entity)
}

/// One revision's main-slot content, as returned by the Action API
/// `prop=revisions&rvslots=main&rvprop=ids|content|contentmodel` read — the
/// diff fetch ADR-0016 Decision 2 names (both revisions in one call, with the
/// content model Decision 1 routes on).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionContent {
    /// The main-slot content (wikitext, entity JSON, …).
    pub content: String,
    /// The revision's content model, when the payload carries one.
    pub content_model: Option<String>,
}

/// Parse an Action API `prop=revisions` response (formatversion 2) into
/// per-revision main-slot contents keyed by revision id. Revisions without
/// content (deleted/suppressed) are omitted, matching how the existing
/// text-pair fetch treats them.
///
/// # Errors
///
/// Returns [`WikibaseError::Json`] for non-JSON bodies and
/// [`WikibaseError::InvalidEntity`] when the payload has no `query.pages`.
pub fn parse_revision_contents(
    body: &[u8],
) -> Result<BTreeMap<u64, RevisionContent>, WikibaseError> {
    let parsed: Value = serde_json::from_slice(body)?;
    let pages = parsed
        .get("query")
        .and_then(|value| value.get("pages"))
        .and_then(Value::as_array)
        .ok_or_else(|| WikibaseError::InvalidEntity {
            message: "revision payload does not contain query.pages".to_string(),
        })?;

    let mut contents = BTreeMap::new();
    for page in pages {
        let Some(revisions) = page.get("revisions").and_then(Value::as_array) else {
            continue;
        };
        for revision in revisions {
            let Some(rev_id) = revision.get("revid").and_then(Value::as_u64) else {
                continue;
            };
            let main = revision.get("slots").and_then(|slots| slots.get("main"));
            let Some(content) = main
                .and_then(|main| main.get("content"))
                .and_then(Value::as_str)
            else {
                continue;
            };
            let content_model = main
                .and_then(|main| main.get("contentmodel"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            contents.insert(
                rev_id,
                RevisionContent {
                    content: content.to_owned(),
                    content_model,
                },
            );
        }
    }
    Ok(contents)
}

/// Route a revision pair to the content-model-appropriate diff (ADR-0016
/// Decision 4): Wikibase entity content parses and diffs structurally;
/// everything else — wikitext, unknown or other models, and entity content
/// that fails to parse — takes the existing text diff, degrading honestly
/// rather than erroring. `before` is `None` for a first revision.
#[must_use]
pub fn route_content_diff(
    content_model: Option<&str>,
    before: Option<&str>,
    after: &str,
) -> ContentDiff {
    if classify_content_model(content_model) == ContentModelClass::WikibaseEntity
        && let Ok(new_entity) = parse_entity_content(after.as_bytes())
    {
        let old_entity = before.and_then(|body| parse_entity_content(body.as_bytes()).ok());
        // A `before` that exists but does not parse must not silently become
        // an all-added diff; fall back to the text path instead.
        if before.is_none() || old_entity.is_some() {
            return ContentDiff::Entity {
                diff: diff_entities(old_entity.as_ref(), &new_entity),
            };
        }
    }
    ContentDiff::Text {
        diff: crate::diff_engine::diff_lines(before.unwrap_or(""), after),
    }
}

fn locate_entity_object<'doc>(
    entity_id: &str,
    doc: &'doc Value,
) -> Result<&'doc Value, WikibaseError> {
    match doc.get("entities").and_then(Value::as_object) {
        Some(entities) => {
            if let Some(entity) = entities.get(entity_id) {
                return Ok(entity);
            }
            // A redirect returns the target entity under its own id.
            if entities.len() == 1
                && let Some(entity) = entities.values().next()
            {
                return Ok(entity);
            }
            Err(WikibaseError::InvalidEntity {
                message: format!("entity {entity_id} not found"),
            })
        }
        None if doc.is_object() => Ok(doc),
        None => Err(WikibaseError::InvalidEntity {
            message: "payload is not an entity document".to_string(),
        }),
    }
}

/// Top-level entity keys that are either modeled or pure fetch bookkeeping.
/// Everything else lands in [`WikibaseEntity::extra`] for the honesty
/// invariant. `modified`/`pageid`/`ns`/`title`/`lastrevid` are bookkeeping:
/// they change on every edit and are not review content.
const MODELED_OR_BOOKKEEPING_KEYS: [&str; 12] = [
    "id",
    "type",
    "labels",
    "descriptions",
    "aliases",
    "claims",
    "statements",
    "sitelinks",
    "lastrevid",
    "modified",
    "pageid",
    "ns",
];

fn parse_entity_object(requested_id: &str, entity: &Value) -> WikibaseEntity {
    let id = entity
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or(requested_id)
        .to_owned();
    let last_revid = entity.get("lastrevid").and_then(Value::as_u64);

    let mut statements = BTreeMap::new();
    // Items/properties carry `claims`; some Wikibase serializations use
    // `statements` — accept either.
    let claims = entity.get("claims").or_else(|| entity.get("statements"));
    if let Some(claims) = claims.and_then(Value::as_object) {
        for (property, list) in claims {
            let parsed: Vec<WikibaseStatement> = list
                .as_array()
                .map(|list| {
                    list.iter()
                        .map(|statement| parse_statement_object(property, statement))
                        .collect()
                })
                .unwrap_or_default();
            statements.insert(property.clone(), parsed);
        }
    }

    let mut extra = BTreeMap::new();
    if let Some(object) = entity.as_object() {
        for (key, value) in object {
            if !MODELED_OR_BOOKKEEPING_KEYS.contains(&key.as_str()) && key != "title" {
                extra.insert(key.clone(), value.clone());
            }
        }
    }

    WikibaseEntity {
        id,
        last_revid,
        labels: parse_term_map(entity.get("labels")),
        descriptions: parse_term_map(entity.get("descriptions")),
        aliases: parse_alias_map(entity.get("aliases")),
        statements,
        sitelinks: parse_sitelinks(entity.get("sitelinks")),
        extra,
    }
}

fn parse_term_map(terms: Option<&Value>) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    if let Some(terms) = terms.and_then(Value::as_object) {
        for (language, term) in terms {
            if let Some(value) = term.get("value").and_then(Value::as_str) {
                map.insert(language.clone(), value.to_owned());
            }
        }
    }
    map
}

fn parse_alias_map(aliases: Option<&Value>) -> BTreeMap<String, Vec<String>> {
    let mut map = BTreeMap::new();
    if let Some(aliases) = aliases.and_then(Value::as_object) {
        for (language, list) in aliases {
            let values: Vec<String> = list
                .as_array()
                .map(|list| {
                    list.iter()
                        .filter_map(|alias| alias.get("value").and_then(Value::as_str))
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            map.insert(language.clone(), values);
        }
    }
    map
}

fn parse_sitelinks(sitelinks: Option<&Value>) -> BTreeMap<String, WikibaseSitelink> {
    let mut map = BTreeMap::new();
    if let Some(sitelinks) = sitelinks.and_then(Value::as_object) {
        for (site, link) in sitelinks {
            if let Some(title) = link.get("title").and_then(Value::as_str) {
                let badges = link
                    .get("badges")
                    .and_then(Value::as_array)
                    .map(|badges| {
                        badges
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default();
                map.insert(
                    site.clone(),
                    WikibaseSitelink {
                        title: title.to_owned(),
                        badges,
                    },
                );
            }
        }
    }
    map
}

fn parse_statement_object(property: &str, statement: &Value) -> WikibaseStatement {
    let id = statement
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let value = statement.get("mainsnak").map_or(
        WikibaseSnak {
            property: property.to_owned(),
            kind: WikibaseSnakKind::NoValue,
        },
        |mainsnak| parse_snak(property, mainsnak),
    );
    let rank = match statement.get("rank").and_then(Value::as_str) {
        Some("preferred") => StatementRank::Preferred,
        Some("deprecated") => StatementRank::Deprecated,
        _ => StatementRank::Normal,
    };
    WikibaseStatement {
        id,
        property: property.to_owned(),
        value,
        qualifiers: parse_snak_map(
            statement.get("qualifiers"),
            statement.get("qualifiers-order"),
        ),
        rank,
        references: parse_references(statement.get("references")),
        raw: statement.clone(),
    }
}

fn parse_references(references: Option<&Value>) -> Vec<WikibaseReference> {
    references
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .map(|reference| WikibaseReference {
                    snaks: parse_snak_map(reference.get("snaks"), reference.get("snaks-order")),
                    raw: reference.clone(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Flatten a `{property: [snak, …]}` map into a `Vec`, honoring the sibling
/// `…-order` list where present so payload order survives.
fn parse_snak_map(snaks: Option<&Value>, order: Option<&Value>) -> Vec<WikibaseSnak> {
    let Some(map) = snaks.and_then(Value::as_object) else {
        return Vec::new();
    };
    let ordered_properties: Vec<&str> = order.and_then(Value::as_array).map_or_else(
        || map.keys().map(String::as_str).collect(),
        |order| order.iter().filter_map(Value::as_str).collect(),
    );

    let mut snaks = Vec::new();
    for property in ordered_properties {
        if let Some(list) = map.get(property).and_then(Value::as_array) {
            for snak in list {
                snaks.push(parse_snak(property, snak));
            }
        }
    }
    snaks
}

fn parse_snak(fallback_property: &str, snak: &Value) -> WikibaseSnak {
    let property = snak
        .get("property")
        .and_then(Value::as_str)
        .unwrap_or(fallback_property)
        .to_owned();
    let kind = match snak.get("snaktype").and_then(Value::as_str) {
        Some("somevalue") => WikibaseSnakKind::SomeValue,
        Some("novalue") => WikibaseSnakKind::NoValue,
        // "value", or absent (sparse fixtures / older serializations carry a
        // datavalue without an explicit snaktype).
        _ => match snak.get("datavalue") {
            Some(datavalue) => WikibaseSnakKind::Value(parse_data_value(datavalue)),
            None => WikibaseSnakKind::NoValue,
        },
    };
    WikibaseSnak { property, kind }
}

fn parse_data_value(datavalue: &Value) -> WikibaseValue {
    let value = datavalue.get("value");
    match datavalue.get("type").and_then(Value::as_str) {
        Some("string") => {
            WikibaseValue::String(value.and_then(Value::as_str).unwrap_or_default().to_owned())
        }
        Some("wikibase-entityid") => WikibaseValue::EntityId(
            value
                .and_then(|inner| inner.get("id"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        ),
        Some("monolingualtext") => WikibaseValue::Monolingual {
            language: value
                .and_then(|inner| inner.get("language"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            text: value
                .and_then(|inner| inner.get("text"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        },
        Some("time") => WikibaseValue::Time {
            time: value
                .and_then(|inner| inner.get("time"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            precision: value
                .and_then(|inner| inner.get("precision"))
                .and_then(Value::as_u64)
                .and_then(|precision| u8::try_from(precision).ok()),
        },
        Some("quantity") => {
            let unit = value
                .and_then(|inner| inner.get("unit"))
                .and_then(Value::as_str)
                .filter(|unit| *unit != "1")
                .map(str::to_owned);
            let bound = |key: &str| {
                value
                    .and_then(|inner| inner.get(key))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            };
            WikibaseValue::Quantity {
                amount: value
                    .and_then(|inner| inner.get("amount"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                unit,
                lower_bound: bound("lowerBound"),
                upper_bound: bound("upperBound"),
            }
        }
        // No type at all but a bare string value: infer a string (sparse
        // fixture / legacy shape). Anything else is preserved verbatim.
        None if value.is_some_and(Value::is_string) => {
            WikibaseValue::String(value.and_then(Value::as_str).unwrap_or_default().to_owned())
        }
        _ => WikibaseValue::Other(datavalue.clone()),
    }
}

// ---------------------------------------------------------------------------
// Rendering (promoted from sp42-mcp's verify_wikidata_statement)
// ---------------------------------------------------------------------------

/// A snak value rendered for display. When the value is an entity link,
/// `item` carries the id so the caller can substitute a resolved label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueDisplay {
    /// The display text (for an entity link, the raw id until resolved).
    pub text: String,
    /// The linked entity id, when the value is an entity link.
    pub item: Option<String>,
}

/// The entity id tail of a unit URI (`…/entity/Q11573` → `Q11573`); the
/// full string when it has no path tail (already an id, or unrecognized).
fn unit_entity_id(unit: &str) -> &str {
    unit.rsplit('/').next().unwrap_or(unit)
}

/// Render a snak to a display string. Best-effort by design (PRD-0010 open
/// question 1): consumers surface the rendered form for inspection; richer
/// datatype rendering is follow-on work.
#[must_use]
pub fn render_snak_value(snak: &WikibaseSnak) -> ValueDisplay {
    match &snak.kind {
        WikibaseSnakKind::NoValue => ValueDisplay {
            text: "(no value)".to_owned(),
            item: None,
        },
        WikibaseSnakKind::SomeValue => ValueDisplay {
            text: "(unknown value)".to_owned(),
            item: None,
        },
        WikibaseSnakKind::Value(value) => match value {
            WikibaseValue::String(text) | WikibaseValue::Monolingual { text, .. } => ValueDisplay {
                text: text.clone(),
                item: None,
            },
            WikibaseValue::EntityId(id) => ValueDisplay {
                text: id.clone(),
                item: Some(id.clone()),
            },
            WikibaseValue::Time { time, .. } => ValueDisplay {
                text: time.trim_start_matches('+').to_owned(),
                item: None,
            },
            WikibaseValue::Quantity {
                amount,
                unit,
                lower_bound,
                upper_bound,
            } => {
                // Unit and bounds are part of the value: a unit-only or
                // bounds-only change must not render as `5 → 5`. The unit is
                // shown as its entity id (label resolution for units is
                // follow-on datatype rendering).
                use std::fmt::Write as _;
                let mut text = amount.trim_start_matches('+').to_owned();
                if let (Some(lower), Some(upper)) = (lower_bound, upper_bound) {
                    let _ = write!(
                        text,
                        " [{}..{}]",
                        lower.trim_start_matches('+'),
                        upper.trim_start_matches('+')
                    );
                }
                if let Some(unit) = unit.as_deref().map(unit_entity_id) {
                    let _ = write!(text, " {unit}");
                }
                ValueDisplay { text, item: None }
            }
            WikibaseValue::Other(value) => ValueDisplay {
                text: value.to_string(),
                item: None,
            },
        },
    }
}

/// Render a statement into a natural-language claim sentence
/// (`"<subject> <property> <value>."`), resolving the property and any
/// item-valued main snak through `labels` and falling back to raw ids.
#[must_use]
pub fn render_statement_claim(
    subject_label: &str,
    statement: &WikibaseStatement,
    labels: &WikibaseLabels,
) -> String {
    let property_label = labels
        .get(&statement.property)
        .unwrap_or(&statement.property);
    let display = render_snak_value(&statement.value);
    let value_label = display
        .item
        .as_deref()
        .and_then(|item| labels.get(item))
        .unwrap_or(&display.text);
    format!("{subject_label} {property_label} {value_label}.")
}

// ---------------------------------------------------------------------------
// EntityDiff (ADR-0016 Decision 3) and ContentDiff routing (Decision 4)
// ---------------------------------------------------------------------------

/// A change to a per-language term (label or description).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TermChange {
    /// The language code.
    pub language: String,
    /// The value before the edit; `None` when the term was added.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    /// The value after the edit; `None` when the term was removed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
}

/// A change to a language's alias list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AliasChange {
    /// The language code.
    pub language: String,
    /// The alias list before the edit.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub before: Vec<String>,
    /// The alias list after the edit.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub after: Vec<String>,
}

/// A change to a sitelink.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SitelinkChange {
    /// The site dbname (`enwiki`, `frwiki`, …).
    pub site: String,
    /// The sitelink display form ([`WikibaseSitelink::display`]: title plus
    /// badge ids when present) before the edit; `None` when the sitelink was
    /// added.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    /// The display form after the edit; `None` when the sitelink was removed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
}

/// Which parts of a changed statement moved — what powers "an edit touching
/// only a qualifier, rank, or reference is never a no-op".
// Five independent change flags, not a state machine (reviewed in PR 119);
// two-variant enums here would only rename `bool`.
#[allow(clippy::struct_excessive_bools)] // https://github.com/schiste/SP42/pull/119
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatementChangeParts {
    /// The main value changed.
    pub value: bool,
    /// The qualifiers changed.
    pub qualifiers: bool,
    /// The rank changed.
    pub rank: bool,
    /// The references changed.
    pub references: bool,
    /// The statements' raw JSON differs outside the four modeled parts —
    /// surfaced explicitly rather than dropped (the honesty invariant).
    pub other: bool,
}

impl StatementChangeParts {
    /// Whether any part is flagged.
    #[must_use]
    pub const fn any(&self) -> bool {
        self.value || self.qualifiers || self.rank || self.references || self.other
    }
}

/// One classified statement change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StatementChange {
    /// A statement present only after the edit.
    Added {
        /// The added statement.
        statement: Box<WikibaseStatement>,
    },
    /// A statement present only before the edit.
    Removed {
        /// The removed statement.
        statement: Box<WikibaseStatement>,
    },
    /// A statement whose GUID persisted but whose content moved.
    Changed {
        /// The statement before the edit.
        before: Box<WikibaseStatement>,
        /// The statement after the edit.
        after: Box<WikibaseStatement>,
        /// Which parts moved.
        parts: StatementChangeParts,
    },
    /// The statements for a property were reordered without content changes.
    /// Surfaced explicitly so a pure reorder is never a no-op (the honesty
    /// invariant) — statement order is reviewable content on Wikidata.
    Reordered {
        /// The property whose statement order changed.
        property: String,
    },
}

/// A change to an unmodeled top-level entity field, surfaced by key so the
/// review never silently drops a real change (the honesty invariant).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnknownEntityChange {
    /// The top-level entity key whose value differs.
    pub key: String,
}

/// A full-depth structured diff between two revisions of one entity.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityDiff {
    /// Label changes, by language.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<TermChange>,
    /// Description changes, by language.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub descriptions: Vec<TermChange>,
    /// Alias-list changes, by language.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<AliasChange>,
    /// Sitelink changes, by site.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sitelinks: Vec<SitelinkChange>,
    /// Statement changes, at full depth.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub statements: Vec<StatementChange>,
    /// Changes to unmodeled top-level entity fields.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub other: Vec<UnknownEntityChange>,
}

impl EntityDiff {
    /// Whether the diff records any change at all.
    #[must_use]
    pub fn has_changes(&self) -> bool {
        !(self.labels.is_empty()
            && self.descriptions.is_empty()
            && self.aliases.is_empty()
            && self.sitelinks.is_empty()
            && self.statements.is_empty()
            && self.other.is_empty())
    }
}

/// The entity/property ids a renderer wants human-readable labels for:
/// statement and qualifier property ids plus item-valued main snaks, across
/// every statement change in the diff. Deduplicated and ordered; feed to
/// [`build_label_request`] (best-effort — a failed lookup renders raw ids).
#[must_use]
pub fn collect_label_ids(diff: &EntityDiff) -> Vec<String> {
    let mut ids = std::collections::BTreeSet::new();
    let mut collect_statement = |statement: &WikibaseStatement| {
        ids.insert(statement.property.clone());
        for snak in &statement.qualifiers {
            ids.insert(snak.property.clone());
        }
        if let WikibaseSnakKind::Value(WikibaseValue::EntityId(id)) = &statement.value.kind {
            ids.insert(id.clone());
        }
    };
    for change in &diff.statements {
        match change {
            StatementChange::Added { statement } | StatementChange::Removed { statement } => {
                collect_statement(statement);
            }
            StatementChange::Changed { before, after, .. } => {
                collect_statement(before);
                collect_statement(after);
            }
            StatementChange::Reordered { .. } => {}
        }
    }
    for change in &diff.statements {
        if let StatementChange::Reordered { property } = change {
            ids.insert(property.clone());
        }
    }
    ids.into_iter().collect()
}

/// The content-model-routed diff a review surface consumes (ADR-0016
/// Decision 4): wikitext (and unknown/other models, with honest degradation)
/// renders through the existing [`StructuredDiff`]; Wikibase entities render
/// through [`EntityDiff`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContentDiff {
    /// A line/char text diff (the existing path, byte-for-byte untouched).
    Text {
        /// The structured text diff.
        diff: StructuredDiff,
    },
    /// A structured entity diff.
    Entity {
        /// The structured entity diff.
        diff: EntityDiff,
    },
}

/// How a rendered entity change row classifies, for badge/tone display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityChangeKind {
    /// Present only after the edit.
    Added,
    /// Present only before the edit.
    Removed,
    /// Present on both sides with different content.
    Changed,
}

/// One pre-rendered entity change row: everything a review surface needs,
/// with labels already resolved (raw ids where resolution failed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityChangeRowReport {
    /// The change classification (badge/tone).
    pub kind: EntityChangeKind,
    /// The fully rendered row text.
    pub text: String,
}

/// One titled section of rendered entity change rows (Labels, Statements, …).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityDiffSectionReport {
    /// The section title.
    pub title: String,
    /// The rendered rows, in diff order.
    pub rows: Vec<EntityChangeRowReport>,
}

/// The rendered entity diff a review surface consumes: flat, pre-rendered
/// sections in the ADR-0011 "the report is the contract" discipline. The
/// full-depth [`EntityDiff`] stays server/domain-side where the honesty
/// invariant needs it; shipping only rendered rows keeps the browser's
/// deserialize surface (and the wasm bundle) small.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityDiffReport {
    /// The rendered sections; empty when the diff records no changes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sections: Vec<EntityDiffSectionReport>,
}

impl EntityDiffReport {
    /// Whether the report carries any rendered change.
    #[must_use]
    pub fn has_changes(&self) -> bool {
        !self.sections.is_empty()
    }
}

/// The content-diff route's wire payload, routed by content model: wikitext
/// keeps the existing structured text diff; entity revisions carry the
/// pre-rendered [`EntityDiffReport`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContentDiffReport {
    /// A line/char text diff (the existing path, byte-for-byte untouched).
    Text {
        /// The structured text diff.
        diff: StructuredDiff,
    },
    /// A rendered entity diff.
    Entity {
        /// The rendered entity diff report.
        diff: EntityDiffReport,
    },
}

fn optional_term(value: Option<&String>) -> String {
    value.map_or_else(
        || "(none)".to_owned(),
        |value| format!("\u{201c}{value}\u{201d}"),
    )
}

fn term_row(change: &TermChange) -> EntityChangeRowReport {
    let kind = match (&change.before, &change.after) {
        (None, Some(_)) => EntityChangeKind::Added,
        (Some(_), None) => EntityChangeKind::Removed,
        _ => EntityChangeKind::Changed,
    };
    EntityChangeRowReport {
        kind,
        text: format!(
            "{}: {} \u{2192} {}",
            change.language,
            optional_term(change.before.as_ref()),
            optional_term(change.after.as_ref())
        ),
    }
}

fn labeled<'ids>(labels: &'ids BTreeMap<String, String>, id: &'ids str) -> &'ids str {
    labels.get(id).map_or(id, String::as_str)
}

fn statement_value_text(
    labels: &BTreeMap<String, String>,
    statement: &WikibaseStatement,
) -> String {
    let display = render_snak_value(&statement.value);
    match display.item {
        Some(item) => labeled(labels, &item).to_owned(),
        None => display.text,
    }
}

fn statement_row_text(labels: &BTreeMap<String, String>, statement: &WikibaseStatement) -> String {
    format!(
        "{}: {}",
        labeled(labels, &statement.property),
        statement_value_text(labels, statement)
    )
}

fn snak_list_text(labels: &BTreeMap<String, String>, snaks: &[WikibaseSnak]) -> String {
    let rendered: Vec<String> = snaks
        .iter()
        .map(|snak| {
            let display = render_snak_value(snak);
            let value = display
                .item
                .as_deref()
                .map_or(display.text.clone(), |item| {
                    labeled(labels, item).to_owned()
                });
            format!("{}: {}", labeled(labels, &snak.property), value)
        })
        .collect();
    format!("[{}]", rendered.join("; "))
}

const fn rank_text(rank: StatementRank) -> &'static str {
    match rank {
        StatementRank::Preferred => "preferred",
        StatementRank::Normal => "normal",
        StatementRank::Deprecated => "deprecated",
    }
}

fn references_text(labels: &BTreeMap<String, String>, references: &[WikibaseReference]) -> String {
    let rendered: Vec<String> = references
        .iter()
        .map(|reference| snak_list_text(labels, &reference.snaks))
        .collect();
    format!("[{}]", rendered.join("; "))
}

/// The changed-subpart deltas for a `Changed` statement row: reviewers must
/// see *what* moved in a qualifier/rank/reference-only edit, not just that
/// something did.
fn changed_part_details(
    labels: &BTreeMap<String, String>,
    before: &WikibaseStatement,
    after: &WikibaseStatement,
    parts: StatementChangeParts,
) -> Vec<String> {
    let mut details = Vec::new();
    // Honesty backstop: the raw value moved but the best-effort renderer
    // shows the same text on both sides (a datatype detail the renderer
    // does not cover yet). Say so instead of presenting `X → X` silently.
    if parts.value && statement_value_text(labels, before) == statement_value_text(labels, after) {
        details.push("value details beyond the rendered form changed".to_owned());
    }
    if parts.qualifiers {
        details.push(format!(
            "qualifiers {} \u{2192} {}",
            snak_list_text(labels, &before.qualifiers),
            snak_list_text(labels, &after.qualifiers)
        ));
    }
    if parts.rank {
        details.push(format!(
            "rank {} \u{2192} {}",
            rank_text(before.rank),
            rank_text(after.rank)
        ));
    }
    if parts.references {
        details.push(format!(
            "references {} \u{2192} {}",
            references_text(labels, &before.references),
            references_text(labels, &after.references)
        ));
    }
    if parts.other {
        details.push("other statement fields changed".to_owned());
    }
    details
}

fn statement_row(
    labels: &BTreeMap<String, String>,
    change: &StatementChange,
) -> EntityChangeRowReport {
    match change {
        StatementChange::Added { statement } => EntityChangeRowReport {
            kind: EntityChangeKind::Added,
            text: statement_row_text(labels, statement),
        },
        StatementChange::Removed { statement } => EntityChangeRowReport {
            kind: EntityChangeKind::Removed,
            text: statement_row_text(labels, statement),
        },
        StatementChange::Changed {
            before,
            after,
            parts,
        } => {
            let core = format!(
                "{}: {} \u{2192} {}",
                labeled(labels, &after.property),
                statement_value_text(labels, before),
                statement_value_text(labels, after),
            );
            let details = changed_part_details(labels, before, after, *parts);
            let text = if details.is_empty() {
                core
            } else {
                format!("{core} ({})", details.join("; "))
            };
            EntityChangeRowReport {
                kind: EntityChangeKind::Changed,
                text,
            }
        }
        StatementChange::Reordered { property } => EntityChangeRowReport {
            kind: EntityChangeKind::Changed,
            text: format!("{}: statement order changed", labeled(labels, property)),
        },
    }
}

fn report_section(
    title: &str,
    rows: Vec<EntityChangeRowReport>,
) -> Option<EntityDiffSectionReport> {
    (!rows.is_empty()).then(|| EntityDiffSectionReport {
        title: title.to_owned(),
        rows,
    })
}

/// Render a full-depth [`EntityDiff`] into the flat, pre-rendered report a
/// review surface consumes, substituting resolved `labels` (raw ids where
/// resolution failed). Pure and host-testable — this is the single renderer
/// every shell shares, so entity rows read identically everywhere.
#[must_use]
pub fn render_entity_diff_report(
    diff: &EntityDiff,
    labels: &BTreeMap<String, String>,
) -> EntityDiffReport {
    let sections = [
        report_section("Labels", diff.labels.iter().map(term_row).collect()),
        report_section(
            "Descriptions",
            diff.descriptions.iter().map(term_row).collect(),
        ),
        report_section(
            "Aliases",
            diff.aliases
                .iter()
                .map(|change| EntityChangeRowReport {
                    kind: EntityChangeKind::Changed,
                    text: format!(
                        "{}: [{}] \u{2192} [{}]",
                        change.language,
                        change.before.join(", "),
                        change.after.join(", ")
                    ),
                })
                .collect(),
        ),
        report_section(
            "Sitelinks",
            diff.sitelinks
                .iter()
                .map(|change| {
                    let kind = match (&change.before, &change.after) {
                        (None, Some(_)) => EntityChangeKind::Added,
                        (Some(_), None) => EntityChangeKind::Removed,
                        _ => EntityChangeKind::Changed,
                    };
                    EntityChangeRowReport {
                        kind,
                        text: format!(
                            "{}: {} \u{2192} {}",
                            change.site,
                            optional_term(change.before.as_ref()),
                            optional_term(change.after.as_ref())
                        ),
                    }
                })
                .collect(),
        ),
        report_section(
            "Statements",
            diff.statements
                .iter()
                .map(|change| statement_row(labels, change))
                .collect(),
        ),
        report_section(
            "Other fields",
            diff.other
                .iter()
                .map(|change| EntityChangeRowReport {
                    kind: EntityChangeKind::Changed,
                    text: format!("{} changed", change.key),
                })
                .collect(),
        ),
    ]
    .into_iter()
    .flatten()
    .collect();
    EntityDiffReport { sections }
}

fn diff_term_maps(
    before: &BTreeMap<String, String>,
    after: &BTreeMap<String, String>,
) -> Vec<TermChange> {
    let mut changes = Vec::new();
    for (language, value) in before {
        match after.get(language) {
            Some(new_value) if new_value == value => {}
            other => changes.push(TermChange {
                language: language.clone(),
                before: Some(value.clone()),
                after: other.cloned(),
            }),
        }
    }
    for (language, value) in after {
        if !before.contains_key(language) {
            changes.push(TermChange {
                language: language.clone(),
                before: None,
                after: Some(value.clone()),
            });
        }
    }
    changes
}

fn diff_statement_lists(
    property: &str,
    before: &[WikibaseStatement],
    after: &[WikibaseStatement],
    changes: &mut Vec<StatementChange>,
) {
    let mut consumed_after = vec![false; after.len()];
    let mut matched_positions = Vec::new();

    for old in before {
        // Pair by GUID first; a statement kept across the edit keeps its id.
        let paired = old.id.as_deref().and_then(|id| {
            after.iter().enumerate().find_map(|(index, new)| {
                (!consumed_after[index] && new.id.as_deref() == Some(id)).then_some(index)
            })
        });
        if let Some(index) = paired {
            consumed_after[index] = true;
            matched_positions.push(index);
            let new = &after[index];
            if old.raw != new.raw {
                changes.push(StatementChange::Changed {
                    before: Box::new(old.clone()),
                    after: Box::new(new.clone()),
                    parts: statement_change_parts(old, new),
                });
            }
            continue;
        }
        // No GUID pairing: an identical raw statement is unchanged; anything
        // else is a removal (an id-less edited statement honestly shows as
        // removed + added rather than guessed into a pairing).
        if let Some(index) = after.iter().enumerate().find_map(|(index, new)| {
            (!consumed_after[index] && new.raw == old.raw).then_some(index)
        }) {
            consumed_after[index] = true;
            matched_positions.push(index);
            continue;
        }
        changes.push(StatementChange::Removed {
            statement: Box::new(old.clone()),
        });
    }

    for (index, new) in after.iter().enumerate() {
        if !consumed_after[index] {
            changes.push(StatementChange::Added {
                statement: Box::new(new.clone()),
            });
        }
    }

    // Matched statements landing out of order is a real, reviewable edit
    // (statement order is content on Wikidata): surface it explicitly rather
    // than letting a pure reorder render as no changes.
    if matched_positions.windows(2).any(|pair| pair[0] > pair[1]) {
        changes.push(StatementChange::Reordered {
            property: property.to_owned(),
        });
    }
}

fn statement_change_parts(
    before: &WikibaseStatement,
    after: &WikibaseStatement,
) -> StatementChangeParts {
    let mut parts = StatementChangeParts {
        value: raw_part(before, "mainsnak") != raw_part(after, "mainsnak"),
        qualifiers: raw_part(before, "qualifiers") != raw_part(after, "qualifiers")
            || raw_part(before, "qualifiers-order") != raw_part(after, "qualifiers-order"),
        rank: before.rank != after.rank,
        references: raw_part(before, "references") != raw_part(after, "references"),
        other: false,
    };
    // The raws differ (the caller established that); if none of the modeled
    // parts explain it, say so explicitly instead of rendering a no-op.
    if !parts.any() {
        parts.other = true;
    }
    parts
}

fn raw_part<'statement>(
    statement: &'statement WikibaseStatement,
    key: &str,
) -> Option<&'statement Value> {
    statement.raw.get(key)
}

/// Diff two revisions of an entity into classified changes. `old` is `None`
/// for a first revision, which yields an all-added diff rather than an error.
#[must_use]
pub fn diff_entities(old: Option<&WikibaseEntity>, new: &WikibaseEntity) -> EntityDiff {
    let empty = WikibaseEntity {
        id: new.id.clone(),
        last_revid: None,
        labels: BTreeMap::new(),
        descriptions: BTreeMap::new(),
        aliases: BTreeMap::new(),
        statements: BTreeMap::new(),
        sitelinks: BTreeMap::new(),
        extra: BTreeMap::new(),
    };
    let old = old.unwrap_or(&empty);

    let mut diff = EntityDiff {
        labels: diff_term_maps(&old.labels, &new.labels),
        descriptions: diff_term_maps(&old.descriptions, &new.descriptions),
        ..EntityDiff::default()
    };

    for (language, before) in &old.aliases {
        let after = new.aliases.get(language);
        if after != Some(before) {
            diff.aliases.push(AliasChange {
                language: language.clone(),
                before: before.clone(),
                after: after.cloned().unwrap_or_default(),
            });
        }
    }
    for (language, after) in &new.aliases {
        if !old.aliases.contains_key(language) {
            diff.aliases.push(AliasChange {
                language: language.clone(),
                before: Vec::new(),
                after: after.clone(),
            });
        }
    }

    for (site, before) in &old.sitelinks {
        match new.sitelinks.get(site) {
            // Struct equality covers badges too: a badge-only edit (same
            // title) is a change, never a no-op.
            Some(after) if after == before => {}
            other => diff.sitelinks.push(SitelinkChange {
                site: site.clone(),
                before: Some(before.display()),
                after: other.map(WikibaseSitelink::display),
            }),
        }
    }
    for (site, after) in &new.sitelinks {
        if !old.sitelinks.contains_key(site) {
            diff.sitelinks.push(SitelinkChange {
                site: site.clone(),
                before: None,
                after: Some(after.display()),
            });
        }
    }

    let empty_statements: Vec<WikibaseStatement> = Vec::new();
    for (property, before) in &old.statements {
        let after = new.statements.get(property).unwrap_or(&empty_statements);
        diff_statement_lists(property, before, after, &mut diff.statements);
    }
    for (property, after) in &new.statements {
        if !old.statements.contains_key(property) {
            diff_statement_lists(property, &empty_statements, after, &mut diff.statements);
        }
    }

    for (key, before) in &old.extra {
        if new.extra.get(key) != Some(before) {
            diff.other.push(UnknownEntityChange { key: key.clone() });
        }
    }
    for key in new.extra.keys() {
        if !old.extra.contains_key(key) {
            diff.other.push(UnknownEntityChange { key: key.clone() });
        }
    }

    diff
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        ContentModelClass, StatementChange, StatementRank, WikibaseSnakKind, WikibaseValue,
        build_entity_request, build_label_request, classify_content_model,
        derive_content_model_capabilities, diff_entities, parse_entity, parse_labels,
        render_snak_value, render_statement_claim,
    };
    use url::Url;

    fn wikidata_api_url() -> Url {
        "https://www.wikidata.org/w/api.php"
            .parse()
            .expect("valid url")
    }

    fn full_entity_doc() -> String {
        json!({
            "entities": {
                "Q42": {
                    "id": "Q42",
                    "type": "item",
                    "lastrevid": 123_456,
                    "modified": "2026-07-01T00:00:00Z",
                    "labels": {"en": {"language": "en", "value": "Douglas Adams"}},
                    "descriptions": {"en": {"language": "en", "value": "English writer"}},
                    "aliases": {"en": [{"language": "en", "value": "Douglas Noel Adams"}]},
                    "claims": {
                        "P800": [{
                            "id": "Q42$s1",
                            "rank": "normal",
                            "mainsnak": {
                                "snaktype": "value",
                                "property": "P800",
                                "datavalue": {"type": "string", "value": "The Hitchhiker's Guide to the Galaxy"}
                            },
                            "qualifiers": {
                                "P585": [{
                                    "snaktype": "value",
                                    "property": "P585",
                                    "datavalue": {"type": "time", "value": {"time": "+1979-10-12T00:00:00Z", "precision": 11}}
                                }]
                            },
                            "qualifiers-order": ["P585"],
                            "references": [{
                                "snaks": {
                                    "P854": [{
                                        "snaktype": "value",
                                        "property": "P854",
                                        "datavalue": {"type": "string", "value": "https://example.org/ref"}
                                    }]
                                },
                                "snaks-order": ["P854"]
                            }]
                        }]
                    },
                    "sitelinks": {"enwiki": {"site": "enwiki", "title": "Douglas Adams"}}
                }
            }
        })
        .to_string()
    }

    #[test]
    fn classifies_content_models_per_revision() {
        assert_eq!(
            classify_content_model(Some("wikitext")),
            ContentModelClass::Wikitext
        );
        assert_eq!(
            classify_content_model(Some("wikibase-item")),
            ContentModelClass::WikibaseEntity
        );
        assert_eq!(
            classify_content_model(Some("wikibase-property")),
            ContentModelClass::WikibaseEntity
        );
        assert_eq!(
            classify_content_model(Some("Scribunto")),
            ContentModelClass::Other
        );
        assert_eq!(classify_content_model(None), ContentModelClass::Unknown);
    }

    #[test]
    fn namespace_defaults_seed_entity_content_models_for_wikidata_only() {
        let wikidata: Url = "https://www.wikidata.org/w/api.php".parse().expect("url");
        let test_wikidata: Url = "https://test.wikidata.org/w/api.php".parse().expect("url");
        let frwiki: Url = "https://fr.wikipedia.org/w/api.php".parse().expect("url");

        assert_eq!(
            super::default_namespace_content_model(&wikidata, 0),
            Some("wikibase-item")
        );
        assert_eq!(
            super::default_namespace_content_model(&wikidata, 120),
            Some("wikibase-property")
        );
        // Talk pages keep the ordinary (wikitext) default.
        assert_eq!(super::default_namespace_content_model(&wikidata, 1), None);
        assert_eq!(
            super::default_namespace_content_model(&test_wikidata, 0),
            Some("wikibase-item")
        );
        assert_eq!(super::default_namespace_content_model(&frwiki, 0), None);
    }

    #[test]
    fn entity_content_gates_wikitext_signals_and_scoring_off() {
        let entity = derive_content_model_capabilities(Some("wikibase-item"));
        assert!(!entity.wikitext_signals);
        assert!(!entity.revertrisk_scoring);
        assert!(entity.entity_diff);

        // Unknown keeps pre-ADR-0016 behavior; other models degrade honestly
        // to the text path.
        for model in [None, Some("wikitext"), Some("json")] {
            let capabilities = derive_content_model_capabilities(model);
            assert!(capabilities.wikitext_signals);
            assert!(capabilities.revertrisk_scoring);
            assert!(!capabilities.entity_diff);
        }
    }

    #[test]
    fn builds_entity_request_from_api_url_with_optional_revision() {
        let request =
            build_entity_request(&wikidata_api_url(), "Q42", None).expect("request builds");
        assert_eq!(
            request.url.as_str(),
            "https://www.wikidata.org/wiki/Special:EntityData/Q42.json"
        );

        let pinned =
            build_entity_request(&wikidata_api_url(), "Q42", Some(99)).expect("request builds");
        assert!(pinned.url.as_str().ends_with("Q42.json?revision=99"));
    }

    #[test]
    fn rejects_non_entity_ids_before_building_urls() {
        for bad in ["", "Q", "42", "Q42/../secret", "Talk:Q42"] {
            assert!(build_entity_request(&wikidata_api_url(), bad, None).is_err());
        }
    }

    #[test]
    fn builds_label_request_and_parses_labels() {
        let request = build_label_request(
            &wikidata_api_url(),
            &["P800".to_string(), "Q42".to_string()],
            "en",
        )
        .expect("request builds");
        let url = request.url.as_str();
        assert!(url.contains("action=wbgetentities"));
        assert!(url.contains("ids=P800%7CQ42"));
        assert!(url.contains("props=labels"));

        assert!(build_label_request(&wikidata_api_url(), &[], "en").is_err());

        let labels = parse_labels(
            br#"{"entities":{"P800":{"labels":{"en":{"value":"notable work"}}}}}"#,
            "en",
        )
        .expect("labels parse");
        assert_eq!(labels.get("P800"), Some("notable work"));
        assert_eq!(labels.get("P999"), None);
    }

    #[test]
    fn parses_a_full_entity_at_statement_depth() {
        let entity = parse_entity("Q42", full_entity_doc().as_bytes()).expect("entity parses");
        assert_eq!(entity.id, "Q42");
        assert_eq!(entity.last_revid, Some(123_456));
        assert_eq!(
            entity.labels.get("en").map(String::as_str),
            Some("Douglas Adams")
        );
        assert_eq!(
            entity
                .sitelinks
                .get("enwiki")
                .map(|link| link.title.as_str()),
            Some("Douglas Adams")
        );

        let statement = entity.statement("P800", None).expect("statement present");
        assert_eq!(statement.id.as_deref(), Some("Q42$s1"));
        assert_eq!(statement.rank, StatementRank::Normal);
        assert_eq!(statement.qualifiers.len(), 1);
        assert!(matches!(
            &statement.qualifiers[0].kind,
            WikibaseSnakKind::Value(WikibaseValue::Time { time, precision: Some(11) })
                if time == "+1979-10-12T00:00:00Z"
        ));
        let urls: Vec<&str> = statement
            .references
            .iter()
            .flat_map(super::WikibaseReference::urls)
            .collect();
        assert_eq!(urls, vec!["https://example.org/ref"]);
    }

    #[test]
    fn parses_bare_entity_objects_and_sparse_snaks() {
        // A bare entity object (the prop=revisions slot shape), with the
        // sparse reference shape PR 103's fixtures use: no snaktype, no inner
        // property, no datavalue type.
        let bare = json!({
            "id": "Q1",
            "claims": {
                "P854": [{
                    "mainsnak": {"datavalue": {"value": "https://bare.example/"}},
                    "references": [{"snaks": {"P854": [{"datavalue": {"value": "https://ref.example/"}}]}}]
                }]
            }
        })
        .to_string();
        let entity = parse_entity("Q1", bare.as_bytes()).expect("bare entity parses");
        let statement = entity.statement("P854", None).expect("statement present");
        assert!(matches!(
            &statement.value.kind,
            WikibaseSnakKind::Value(WikibaseValue::String(url)) if url == "https://bare.example/"
        ));
        let urls: Vec<&str> = statement
            .references
            .iter()
            .flat_map(super::WikibaseReference::urls)
            .collect();
        assert_eq!(urls, vec!["https://ref.example/"]);
    }

    #[test]
    fn follows_single_entity_redirects_and_reports_missing_entities() {
        let redirected = r#"{"entities":{"Q100":{"id":"Q100","labels":{}}}}"#;
        let entity = parse_entity("Q42", redirected.as_bytes()).expect("redirect tolerated");
        assert_eq!(entity.id, "Q100");

        assert!(parse_entity("Q42", br#"{"entities":{}}"#).is_err());
        assert!(parse_entity("Q42", b"[1,2]").is_err());
        assert!(parse_entity("Q42", b"not json").is_err());
    }

    #[test]
    fn renders_snak_values_and_claim_sentences() {
        let entity = parse_entity("Q42", full_entity_doc().as_bytes()).expect("entity parses");
        let statement = entity.statement("P800", None).expect("statement present");
        let labels = parse_labels(
            br#"{"entities":{"P800":{"labels":{"en":{"value":"notable work"}}}}}"#,
            "en",
        )
        .expect("labels parse");
        assert_eq!(
            render_statement_claim("Douglas Adams", statement, &labels),
            "Douglas Adams notable work The Hitchhiker's Guide to the Galaxy."
        );

        let display = render_snak_value(&statement.value);
        assert_eq!(display.text, "The Hitchhiker's Guide to the Galaxy");
        assert_eq!(display.item, None);
    }

    fn entity_with_statement(statement: &serde_json::Value) -> super::WikibaseEntity {
        let doc = json!({"id": "Q42", "claims": {"P800": [statement]}}).to_string();
        parse_entity("Q42", doc.as_bytes()).expect("entity parses")
    }

    #[test]
    fn qualifier_only_rank_only_and_reference_only_edits_are_never_no_ops() {
        let base = json!({
            "id": "Q42$s1",
            "rank": "normal",
            "mainsnak": {"snaktype": "value", "property": "P800", "datavalue": {"type": "string", "value": "v"}}
        });

        let mut with_qualifier = base.clone();
        with_qualifier["qualifiers"] = json!({"P585": [{"snaktype": "value", "property": "P585", "datavalue": {"type": "string", "value": "q"}}]});
        let mut with_rank = base.clone();
        with_rank["rank"] = json!("preferred");
        let mut with_reference = base.clone();
        with_reference["references"] =
            json!([{"snaks": {"P854": [{"datavalue": {"value": "https://ref.example/"}}]}}]);

        let old = entity_with_statement(&base);
        for (new_statement, expect) in [
            (with_qualifier, "qualifiers"),
            (with_rank, "rank"),
            (with_reference, "references"),
        ] {
            let new = entity_with_statement(&new_statement);
            let diff = diff_entities(Some(&old), &new);
            assert!(diff.has_changes(), "{expect}-only edit must not be a no-op");
            let [StatementChange::Changed { parts, .. }] = diff.statements.as_slice() else {
                panic!("{expect}-only edit should classify as Changed");
            };
            match expect {
                "qualifiers" => assert!(parts.qualifiers && !parts.value && !parts.rank),
                "rank" => assert!(parts.rank && !parts.value && !parts.qualifiers),
                "references" => assert!(parts.references && !parts.value && !parts.rank),
                _ => unreachable!(),
            }
        }
    }

    #[test]
    fn unmodeled_statement_deltas_flag_the_other_part() {
        let base = json!({
            "id": "Q42$s1",
            "mainsnak": {"snaktype": "value", "property": "P800", "datavalue": {"type": "string", "value": "v"}}
        });
        let mut tweaked = base.clone();
        tweaked["future-field"] = json!("something new");

        let diff = diff_entities(
            Some(&entity_with_statement(&base)),
            &entity_with_statement(&tweaked),
        );
        let [StatementChange::Changed { parts, .. }] = diff.statements.as_slice() else {
            panic!("unmodeled delta should classify as Changed");
        };
        assert!(parts.other, "unmodeled statement delta must be surfaced");
    }

    #[test]
    fn diffs_terms_aliases_sitelinks_and_unknown_entity_fields() {
        let old = parse_entity(
            "Q42",
            json!({
                "id": "Q42",
                "labels": {"en": {"value": "Old"}, "fr": {"value": "Ancien"}},
                "aliases": {"en": [{"value": "O."}]},
                "sitelinks": {"enwiki": {"title": "Old"}},
                "datatype": "string"
            })
            .to_string()
            .as_bytes(),
        )
        .expect("old parses");
        let new = parse_entity(
            "Q42",
            json!({
                "id": "Q42",
                "labels": {"en": {"value": "New"}},
                "aliases": {"en": [{"value": "O."}, {"value": "N."}]},
                "sitelinks": {"enwiki": {"title": "Old"}, "frwiki": {"title": "Nouveau"}},
                "datatype": "external-id"
            })
            .to_string()
            .as_bytes(),
        )
        .expect("new parses");

        let diff = diff_entities(Some(&old), &new);
        assert_eq!(diff.labels.len(), 2); // en changed, fr removed
        assert_eq!(diff.aliases.len(), 1);
        assert_eq!(diff.sitelinks.len(), 1); // frwiki added
        assert_eq!(diff.other.len(), 1);
        assert_eq!(diff.other[0].key, "datatype");
    }

    #[test]
    fn identical_revisions_diff_to_no_changes_and_first_revisions_are_all_added() {
        let entity = parse_entity("Q42", full_entity_doc().as_bytes()).expect("entity parses");
        assert!(!diff_entities(Some(&entity), &entity).has_changes());

        let first = diff_entities(None, &entity);
        assert!(first.has_changes());
        assert!(
            first
                .statements
                .iter()
                .all(|change| matches!(change, StatementChange::Added { .. }))
        );
    }

    #[test]
    fn statement_add_and_remove_classify_by_guid() {
        let one = json!({
            "id": "Q42$s1",
            "mainsnak": {"snaktype": "value", "property": "P800", "datavalue": {"type": "string", "value": "a"}}
        });
        let two = json!({
            "id": "Q42$s2",
            "mainsnak": {"snaktype": "value", "property": "P800", "datavalue": {"type": "string", "value": "b"}}
        });
        let old_doc = json!({"id": "Q42", "claims": {"P800": [one]}}).to_string();
        let new_doc = json!({"id": "Q42", "claims": {"P800": [two]}}).to_string();
        let old = parse_entity("Q42", old_doc.as_bytes()).expect("old parses");
        let new = parse_entity("Q42", new_doc.as_bytes()).expect("new parses");

        let diff = diff_entities(Some(&old), &new);
        assert_eq!(diff.statements.len(), 2);
        assert!(
            diff.statements
                .iter()
                .any(|change| matches!(change, StatementChange::Removed { .. }))
        );
        assert!(
            diff.statements
                .iter()
                .any(|change| matches!(change, StatementChange::Added { .. }))
        );
    }

    #[test]
    fn parses_revision_contents_with_content_models() {
        let body = json!({
            "query": {
                "pages": [{
                    "pageid": 1,
                    "revisions": [
                        {"revid": 10, "slots": {"main": {"contentmodel": "wikibase-item", "content": "{\"id\":\"Q42\"}"}}},
                        {"revid": 11, "slots": {"main": {"content": "plain text, no model"}}},
                        {"revid": 12, "slots": {"main": {}}}
                    ]
                }]
            }
        })
        .to_string();
        let contents = super::parse_revision_contents(body.as_bytes()).expect("revisions parse");
        assert_eq!(contents.len(), 2); // revid 12 has no content and is omitted
        assert_eq!(
            contents[&10].content_model.as_deref(),
            Some("wikibase-item")
        );
        assert_eq!(contents[&11].content_model, None);

        assert!(super::parse_revision_contents(b"{}").is_err());
        assert!(super::parse_revision_contents(b"not json").is_err());
    }

    #[test]
    fn routes_entity_content_to_entity_diffs_and_everything_else_to_text() {
        let old_entity = json!({"id": "Q42", "labels": {"en": {"value": "Old"}}}).to_string();
        let new_entity = json!({"id": "Q42", "labels": {"en": {"value": "New"}}}).to_string();

        // Entity content model with parseable bodies → structured diff.
        let diff = super::route_content_diff(Some("wikibase-item"), Some(&old_entity), &new_entity);
        let super::ContentDiff::Entity { diff } = diff else {
            panic!("entity content should route to the entity diff");
        };
        assert_eq!(diff.labels.len(), 1);

        // First revision → all-added entity diff, not an error.
        assert!(matches!(
            super::route_content_diff(Some("wikibase-item"), None, &new_entity),
            super::ContentDiff::Entity { .. }
        ));

        // Wikitext and unknown models → the text path.
        assert!(matches!(
            super::route_content_diff(Some("wikitext"), Some("a"), "b"),
            super::ContentDiff::Text { .. }
        ));
        assert!(matches!(
            super::route_content_diff(None, Some("a"), "b"),
            super::ContentDiff::Text { .. }
        ));

        // Entity model whose content does not parse → honest text fallback,
        // including when only the `before` side is broken (never a fabricated
        // all-added entity diff).
        assert!(matches!(
            super::route_content_diff(Some("wikibase-item"), Some(&old_entity), "not json"),
            super::ContentDiff::Text { .. }
        ));
        assert!(matches!(
            super::route_content_diff(Some("wikibase-item"), Some("not json"), &new_entity),
            super::ContentDiff::Text { .. }
        ));
    }

    #[test]
    fn pure_statement_reorders_are_never_no_ops() {
        let one = json!({
            "id": "Q42$s1",
            "mainsnak": {"snaktype": "value", "property": "P800", "datavalue": {"type": "string", "value": "a"}}
        });
        let two = json!({
            "id": "Q42$s2",
            "mainsnak": {"snaktype": "value", "property": "P800", "datavalue": {"type": "string", "value": "b"}}
        });
        let old_doc =
            json!({"id": "Q42", "claims": {"P800": [one.clone(), two.clone()]}}).to_string();
        let new_doc = json!({"id": "Q42", "claims": {"P800": [two, one]}}).to_string();
        let old = parse_entity("Q42", old_doc.as_bytes()).expect("old parses");
        let new = parse_entity("Q42", new_doc.as_bytes()).expect("new parses");

        let diff = diff_entities(Some(&old), &new);
        assert!(diff.has_changes(), "a pure reorder must not be a no-op");
        assert_eq!(
            diff.statements,
            vec![StatementChange::Reordered {
                property: "P800".to_string()
            }]
        );
    }

    #[test]
    fn renders_entity_diff_reports_with_labels_and_subpart_details() {
        use std::collections::BTreeMap;

        let base = json!({
            "id": "Q42$s1",
            "rank": "normal",
            "mainsnak": {"snaktype": "value", "property": "P800", "datavalue": {"type": "string", "value": "v"}}
        });
        let mut tweaked = base.clone();
        tweaked["rank"] = json!("preferred");
        tweaked["references"] =
            json!([{"snaks": {"P854": [{"datavalue": {"value": "https://ref.example/"}}]}}]);

        let old = entity_with_statement(&base);
        let new = entity_with_statement(&tweaked);
        let diff = diff_entities(Some(&old), &new);

        let labels = BTreeMap::from([("P800".to_string(), "notable work".to_string())]);
        let report = super::render_entity_diff_report(&diff, &labels);
        assert!(report.has_changes());
        assert_eq!(report.sections.len(), 1);
        assert_eq!(report.sections[0].title, "Statements");
        let row = &report.sections[0].rows[0];
        assert_eq!(row.kind, super::EntityChangeKind::Changed);
        assert!(row.text.starts_with("notable work: v \u{2192} v ("));
        assert!(
            row.text.contains("rank normal \u{2192} preferred"),
            "rank delta must be visible: {}",
            row.text
        );
        assert!(
            row.text.contains("https://ref.example/"),
            "reference delta must be visible: {}",
            row.text
        );

        // Empty diff → empty report; unresolved ids render raw.
        assert!(
            !super::render_entity_diff_report(&super::EntityDiff::default(), &labels).has_changes()
        );
        let unlabeled = super::render_entity_diff_report(&diff, &BTreeMap::new());
        assert!(unlabeled.sections[0].rows[0].text.starts_with("P800:"));
    }

    #[test]
    fn content_diff_report_wire_round_trips_both_arms() {
        let entity = super::ContentDiffReport::Entity {
            diff: super::EntityDiffReport {
                sections: vec![super::EntityDiffSectionReport {
                    title: "Statements".to_string(),
                    rows: vec![super::EntityChangeRowReport {
                        kind: super::EntityChangeKind::Added,
                        text: "notable work: v".to_string(),
                    }],
                }],
            },
        };
        let encoded = serde_json::to_string(&entity).expect("serializes");
        assert!(encoded.contains(r#""kind":"entity""#));
        let decoded: super::ContentDiffReport =
            serde_json::from_str(&encoded).expect("deserializes");
        assert_eq!(decoded, entity);

        let text = super::ContentDiffReport::Text {
            diff: crate::diff_engine::diff_lines("a", "b"),
        };
        let encoded = serde_json::to_string(&text).expect("serializes");
        assert!(encoded.contains(r#""kind":"text""#));
        let decoded: super::ContentDiffReport =
            serde_json::from_str(&encoded).expect("deserializes");
        assert_eq!(decoded, text);
    }

    #[test]
    fn entity_diff_round_trips_through_serde() {
        let entity = parse_entity("Q42", full_entity_doc().as_bytes()).expect("entity parses");
        let diff = diff_entities(None, &entity);
        let encoded = serde_json::to_string(&diff).expect("diff serializes");
        let decoded: super::EntityDiff = serde_json::from_str(&encoded).expect("diff deserializes");
        assert_eq!(decoded, diff);
    }

    #[test]
    fn badge_only_sitelink_edit_is_never_a_no_op() {
        // Same title, different badges: the honesty invariant requires a
        // sitelink change, and the display form shows what moved.
        let before_json = serde_json::json!({
            "id": "Q42", "type": "item",
            "sitelinks": {"enwiki": {"site": "enwiki", "title": "Douglas Adams", "badges": []}}
        });
        let after_json = serde_json::json!({
            "id": "Q42", "type": "item",
            "sitelinks": {"enwiki": {"site": "enwiki", "title": "Douglas Adams",
                                      "badges": ["Q17437796"]}}
        });
        let before = super::parse_entity(
            "Q42",
            serde_json::json!({"entities": {"Q42": before_json}})
                .to_string()
                .as_bytes(),
        )
        .expect("before parses");
        let after = super::parse_entity(
            "Q42",
            serde_json::json!({"entities": {"Q42": after_json}})
                .to_string()
                .as_bytes(),
        )
        .expect("after parses");

        let diff = super::diff_entities(Some(&before), &after);
        assert!(
            diff.has_changes(),
            "badge-only edit must not diff as a no-op"
        );
        assert_eq!(diff.sitelinks.len(), 1);
        assert_eq!(diff.sitelinks[0].before.as_deref(), Some("Douglas Adams"));
        assert_eq!(
            diff.sitelinks[0].after.as_deref(),
            Some("Douglas Adams [Q17437796]")
        );
    }

    #[test]
    fn invisible_value_change_is_annotated_not_silent() {
        // A calendar-model-only time edit: the raw mainsnak differs (parts
        // marks the value changed) but the rendered form is identical on
        // both sides. The changed row must say so, never present `X → X`.
        let statement = |calendar: &str| {
            super::parse_statement_object(
                "P569",
                &serde_json::json!({
                    "id": "Q42$abc",
                    "mainsnak": {
                        "snaktype": "value",
                        "property": "P569",
                        "datavalue": {
                            "type": "time",
                            "value": {
                                "time": "+1952-03-11T00:00:00Z",
                                "precision": 11,
                                "calendarmodel": calendar
                            }
                        }
                    },
                    "rank": "normal"
                }),
            )
        };
        let before = statement("http://www.wikidata.org/entity/Q1985727");
        let after = statement("http://www.wikidata.org/entity/Q1985786");
        let parts = super::statement_change_parts(&before, &after);
        assert!(parts.value, "raw mainsnak moved");
        let details =
            super::changed_part_details(&std::collections::BTreeMap::new(), &before, &after, parts);
        assert!(
            details
                .iter()
                .any(|detail| detail.contains("beyond the rendered form")),
            "equal-display value change must be annotated: {details:?}"
        );
    }

    #[test]
    fn quantity_rendering_includes_bounds_when_present() {
        let bounded = super::WikibaseSnak {
            property: "P2044".to_owned(),
            kind: super::WikibaseSnakKind::Value(super::WikibaseValue::Quantity {
                amount: "+5".to_owned(),
                unit: None,
                lower_bound: Some("+4.8".to_owned()),
                upper_bound: Some("+5.2".to_owned()),
            }),
        };
        assert_eq!(super::render_snak_value(&bounded).text, "5 [4.8..5.2]");
    }

    #[test]
    fn quantity_rendering_includes_the_unit() {
        // A unit-only change must not render as `5 → 5`.
        let with_unit = super::WikibaseSnak {
            property: "P2048".to_owned(),
            kind: super::WikibaseSnakKind::Value(super::WikibaseValue::Quantity {
                amount: "+5".to_owned(),
                unit: Some("http://www.wikidata.org/entity/Q11573".to_owned()),
                lower_bound: None,
                upper_bound: None,
            }),
        };
        assert_eq!(super::render_snak_value(&with_unit).text, "5 Q11573");

        // The dimensionless unit stays a bare amount.
        let unitless = super::WikibaseSnak {
            property: "P1114".to_owned(),
            kind: super::WikibaseSnakKind::Value(super::WikibaseValue::Quantity {
                amount: "+5".to_owned(),
                unit: None,
                lower_bound: None,
                upper_bound: None,
            }),
        };
        assert_eq!(super::render_snak_value(&unitless).text, "5");
    }
}
