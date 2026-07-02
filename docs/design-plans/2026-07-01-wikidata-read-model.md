# Shared Wikidata read model — convergence sketch

**Date:** 2026-07-01
**Status:** Implemented (sp42-platform::wikibase), to converge PR #103 (PRD-0010) and PRD-0011
**Governs the *how* for:** ADR-0016 (entity read + `EntityDiff`), ADR-0017 (statement
write contract). User-facing intent is PRD-0011; the verb surface is PRD-0010.

## Why this exists

PR #103 shipped the first Wikidata read in the codebase (`verify_wikidata_statement`,
`crates/sp42-mcp/src/wikidata.rs`): it reads an entity, parses a statement, resolves
labels, extracts the P854 reference URL, renders a claim, and verifies it. It does
this with ad-hoc `serde_json::Value` poking, in the `sp42-mcp` **shell**.

PRD-0011 adds two more consumers of the *same* read — patrol's `EntityDiff`
(ADR-0016) and the statement-proposal write lane (ADR-0017) — so by the
reuse-by-design rule the read model belongs in **platform**, not a shell. #103's own
design plan already anticipated this ("Statement→claim rendering module in
`sp42-core` … net-new"); the shell placement was the first-consumer shortcut. This
sketch is the typed platform model all three converge on, and the mapping from #103's
current code onto it.

## Placement

- **Entity model + parse + render + references + `EntityDiff` + `ContentDiff`** →
  `sp42-platform` (a new `wikibase` module), sitting next to `diff_engine.rs`
  (`StructuredDiff`) so `ContentDiff` can name both diff types without a cross-crate
  edge, and reachable by the patrol domain, `sp42-citation`, and the `sp42-mcp` shell
  alike (all are ≥ platform in `check-layering.sh`).
- **Content-model capability axis** (ADR-0016 Decision 5) → extends
  `sp42-wiki::capabilities` (where `WikiCapabilityProfile` already lives).
- **Fetch builders/parsers** are pure functions over the `sp42-types::HttpClient`
  trait (the Citoid/recentchanges precedent); shells inject the client.

## The model (type sketch)

```rust
// crates/sp42-platform/src/wikibase/mod.rs  (platform)

/// A parsed Wikibase entity (item or property): content model wikibase-item / -property.
pub struct Entity {
    pub id: EntityId,                              // "Q42", "P569"
    pub last_revid: Option<u64>,                   // drift baseline (ADR-0017); None if the
                                                   // read endpoint didn't carry a revision
    pub labels: TermMap,                           // lang -> value
    pub descriptions: TermMap,
    pub aliases: BTreeMap<Lang, Vec<String>>,
    pub statements: BTreeMap<PropertyId, Vec<Statement>>,
    pub sitelinks: BTreeMap<String, Sitelink>,     // wiki dbname -> sitelink
}

pub struct Statement {
    pub id: Option<StatementId>,                   // GUID, e.g. "Q42$..."
    pub property: PropertyId,
    pub value: Snak,                               // the mainsnak
    pub qualifiers: Vec<Snak>,
    pub rank: StatementRank,                       // Preferred | Normal | Deprecated
    pub references: Vec<Reference>,
    /// Canonical JSON of the statement, retained so change detection is exact even for
    /// datatypes we don't richly model (the "never a no-op" invariant, ADR-0016).
    pub raw: serde_json::Value,
}

pub struct Reference { pub snaks: Vec<Snak>, pub raw: serde_json::Value }

pub enum Snak {
    Value { property: PropertyId, value: WikibaseValue },
    SomeValue { property: PropertyId },            // "unknown value"
    NoValue  { property: PropertyId },             // "no value"
}

pub enum WikibaseValue {
    String(String),
    EntityId(EntityId),                            // wikibase-entityid
    Monolingual { lang: Lang, text: String },
    Time { time: String, precision: u8 },
    Quantity { amount: String, unit: Option<EntityId> },
    GlobeCoordinate { lat: f64, lon: f64 },
    Other(serde_json::Value),                      // forward-compat: unknown datatypes preserved,
                                                   // never a parse failure, still diffable via `raw`
}

pub enum StatementRank { Preferred, Normal, Deprecated }

impl Reference {
    /// P854 "reference URL" snaks. (This is #103's `extract_ref_url`, typed and plural.)
    pub fn urls(&self) -> impl Iterator<Item = &str>;
}
impl Entity {
    /// Select the statement a `StatementRef` names (by GUID, else the first for the property).
    /// This is exactly #103's `parse_statement` selection step.
    pub fn statement(&self, r: &StatementRef) -> Option<&Statement>;
}
```

### Reading (endpoint-agnostic)

```rust
/// Parse a Wikibase entity from EITHER a `Special:EntityData` `{entities:{id:{…}}}` doc
/// OR the bare entity object inside an Action-API `prop=revisions` slot. Endpoint-agnostic:
/// the entity JSON body is identical, only the wrapper differs.
pub fn parse_entity(id: &EntityId, body: &[u8]) -> Result<Entity, WikibaseParseError>;

/// Keyless entity read, optionally at a specific revision (Special:EntityData?revision=).
/// `revision: None` = current (what #103 uses); `Some(rev)` = the parent, for a diff.
pub fn build_entity_request(id: &EntityId, revision: Option<u64>) -> HttpRequest;

/// wbgetentities props=labels — resolve property/item ids to human-readable labels.
pub fn build_label_request(ids: &[&str], lang: Lang) -> HttpRequest;   // = #103's labels_url
pub fn parse_labels(body: &[u8]) -> Result<Labels, WikibaseParseError>;
pub struct Labels(BTreeMap<String, String>);
impl Labels { pub fn get(&self, id: &str) -> Option<&str>; }           // = #103's label_of
```

### Rendering (promotes #103's `render_value` + claim rendering)

```rust
/// Render one value to a display string; carries the item id when the value is an entity,
/// so the caller can look up its label. (This is #103's `render_value`, typed.)
pub struct ValueDisplay { pub text: String, pub item: Option<EntityId> }
pub fn render_value(value: &WikibaseValue) -> ValueDisplay;

/// Render a statement to a natural-language claim ("<subject> <property> <value>."),
/// resolving property/value ids via `labels`. This IS #103's `claim_rendered`.
pub fn render_statement_claim(subject: &Entity, stmt: &Statement, labels: &Labels) -> String;
```

### Diffing (ADR-0016)

```rust
pub enum ContentDiff { Text(StructuredDiff), Entity(EntityDiff) }   // routed on content_model

pub struct EntityDiff {
    pub labels:       Vec<TermChange>,
    pub descriptions: Vec<TermChange>,
    pub aliases:      Vec<AliasChange>,
    pub sitelinks:    Vec<SitelinkChange>,
    pub statements:   Vec<StatementChange>,
}

pub enum StatementChange {
    Added(Statement),
    Removed(Statement),
    Changed { before: Statement, after: Statement, parts: StatementChangeParts },
}
/// Which sub-parts of a statement moved — this is what powers "an edit touching only a
/// qualifier / rank / reference is never a no-op." Computed from `Statement.raw` equality.
pub struct StatementChangeParts { pub value: bool, pub qualifiers: bool, pub rank: bool, pub references: bool }

/// `old = None` = first revision (everything Added). Change detection uses `raw` equality,
/// so unknown datatypes still register as changes.
pub fn diff_entities(old: Option<&Entity>, new: &Entity) -> EntityDiff;
```

### Statement identity + write payload (ADR-0017)

```rust
/// Promoted verbatim from #103 (`sp42-mcp::StatementRef`); #103 re-exports it from here.
pub struct StatementRef { pub entity: EntityId, pub property: PropertyId, pub statement_id: Option<StatementId> }

/// The ADR-0017 propose/confirm payload for adding a referenced statement.
pub struct StatementProposal {
    pub entity: EntityId,
    pub base_revid: u64,           // drift baseline == Entity.last_revid at propose time
    pub statement: Statement,      // property + value + qualifiers + rank + the citation reference
    pub grounding: Grounding,      // ADR-0007: verbatim source passage + source hash for the fact
}
```

## How #103's `wikidata.rs` maps onto this (the convergence)

`verify_wikidata_statement` keeps its exact behavior; its internals become calls into
the shared model. Nothing in PRD-0010's DoD changes.

| #103 today (`sp42-mcp/src/wikidata.rs`) | Shared platform model |
|---|---|
| `get_json` + `Special:EntityData` URL | `build_entity_request(id, None)` + `parse_entity` |
| `parse_statement` → `ParsedStatement` | `parse_entity` + `Entity::statement(&StatementRef)` |
| `render_value` (Value poking) | `WikibaseValue` + `render_value` (typed) |
| `extract_ref_url` (first P854) | `Statement.references` → `Reference::urls()` |
| `label_of`, `labels_url` | `Labels::get`, `build_label_request` |
| `claim_rendered = format!(...)` | `render_statement_claim` |
| `StatementRef` (defined in `sp42-mcp`) | promoted here; `sp42-mcp` re-exports it |

Post-convergence, the verb body is: `build_entity_request` → `parse_entity` →
`Entity::statement` → `render_statement_claim` → `Reference::urls()` (P854) →
`verify_claim`. Same three fetches, same abstention on no-P854, same tests.

## What patrol / the write lane add (the extension delta #103 didn't need)

All additive; #103's verb simply ignores them:

- **Qualifiers, rank, sitelinks, aliases, descriptions, `Statement.raw`** — #103 only
  needed mainsnak value + P854; the diff needs the rest and the raw form for the
  no-op invariant.
- **`EntityDiff` / `ContentDiff` / `diff_entities`** — patrol only.
- **`build_entity_request(id, Some(rev))`** — parent-revision read for a diff; #103
  reads only `None` (current).
- **`StatementProposal` + `last_revid` drift baseline + `Grounding`** — write lane
  (ADR-0017) only.

## Open points to settle with #103

1. **`ContentDiff` home** — `sp42-platform` next to `diff_engine` (recommended, no
   cross-crate edge) vs. `sp42-wiki` with the entity model. Confirm against the dep
   graph.
2. **Fetch endpoint for the diff** — `Special:EntityData?revision=` (reuses #103's
   endpoint, two calls) vs. Action API `prop=revisions&revids=a|b` (one call, also
   yields `contentmodel` for routing). The parser is endpoint-agnostic either way;
   this is purely which builder patrol uses. Content-model *detection* for routing
   stays on the Action API regardless (ADR-0016 Decision 1).
3. **Merge order** — if #103 lands first, this is a refactor-in-place of
   `wikidata.rs` onto the promoted module; if PRD-0011 work lands first, #103 consumes
   the module from the start. Either way the verb's public contract (PRD-0010) is
   unchanged.
4. **`render_statement_claim` fidelity** — shared with PRD-0010 open question #1
   (string/item/time/quantity now, richer datatypes later). One renderer, one place to
   improve it.
