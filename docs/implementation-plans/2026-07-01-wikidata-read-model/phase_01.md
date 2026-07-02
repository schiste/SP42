# Wikidata Read Model Implementation Plan — Phase 1: `wikibase` core types + `parse_entity`

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** A typed, endpoint-agnostic Wikibase entity model in `sp42-platform` with a fixture-tested `parse_entity`.

**Architecture:** New `wikibase` module in `crates/sp42-platform`, sibling to `diff_engine.rs` (per design plan `docs/design-plans/2026-07-01-wikidata-read-model.md` and ADR-0016 Decision 8). Pure parsing over `serde_json::Value`; every statement retains its canonical raw JSON so diffing (Phase 4) can guarantee the never-a-no-op invariant. No HTTP in this phase.

**Tech Stack:** Rust 1.96 / edition 2024, serde_json, thiserror. Clippy pedantic is `deny` workspace-wide.

**Scope:** Phase 1 of 7 (derived from the convergence-sketch design plan).

**Codebase verified:** 2026-07-01. `crates/sp42-platform/src/lib.rs` registers modules as `pub mod name;` (lines 36–62) with selective re-exports below. No `EntityId`/`Lang`/`TermMap` types exist anywhere in the workspace — this module defines them. `scripts/check-layering.sh` registers crates, not modules — no change needed.

**Testing conventions (follow, do not reinvent):** inline `#[cfg(test)] mod tests` at the end of each source file (e.g. `crates/sp42-platform/src/article_inventory.rs`); JSON fixtures live in the workspace-root `fixtures/` directory loaded with `include_str!` (e.g. `crates/sp42-citation/src/bare_url_repair.rs` loading `fixtures/citoid/*.json`); run with `cargo test -p sp42-platform`, gate with `cargo ci-clippy`.

---

### Task 1: Fixture — sample EntityData document

**Files:**
- Create: `fixtures/wikibase/q42_entitydata.json`

**Step 1: Create the fixture**

A trimmed but structurally faithful `Special:EntityData/{id}.json` document. It must exercise: labels, descriptions, aliases, sitelinks, a statement with GUID + qualifier + rank + P854 reference (entity value), a time value, a quantity value, and an unknown datatype.

```json
{
  "entities": {
    "Q42": {
      "type": "item",
      "id": "Q42",
      "lastrevid": 2000341,
      "labels": {
        "en": { "language": "en", "value": "Douglas Adams" },
        "fr": { "language": "fr", "value": "Douglas Adams" }
      },
      "descriptions": {
        "en": { "language": "en", "value": "English author and humourist" }
      },
      "aliases": {
        "en": [
          { "language": "en", "value": "Douglas Noel Adams" },
          { "language": "en", "value": "DNA" }
        ]
      },
      "sitelinks": {
        "enwiki": { "site": "enwiki", "title": "Douglas Adams" }
      },
      "claims": {
        "P69": [
          {
            "id": "Q42$0E9C4724-C954-4698-84A7-5CE0D296A6F2",
            "mainsnak": {
              "snaktype": "value",
              "property": "P69",
              "datavalue": {
                "value": { "entity-type": "item", "numeric-id": 691283, "id": "Q691283" },
                "type": "wikibase-entityid"
              }
            },
            "type": "statement",
            "qualifiers": {
              "P580": [
                {
                  "snaktype": "value",
                  "property": "P580",
                  "datavalue": {
                    "value": { "time": "+1971-00-00T00:00:00Z", "precision": 9 },
                    "type": "time"
                  }
                }
              ]
            },
            "rank": "normal",
            "references": [
              {
                "snaks": {
                  "P854": [
                    {
                      "snaktype": "value",
                      "property": "P854",
                      "datavalue": { "value": "https://example.org/adams-bio", "type": "string" }
                    }
                  ]
                }
              }
            ]
          }
        ],
        "P569": [
          {
            "id": "Q42$D8404CDA-25E4-4334-AF13-A3290BCD9C0F",
            "mainsnak": {
              "snaktype": "value",
              "property": "P569",
              "datavalue": {
                "value": { "time": "+1952-03-11T00:00:00Z", "precision": 11 },
                "type": "time"
              }
            },
            "type": "statement",
            "rank": "normal",
            "references": []
          }
        ],
        "P2048": [
          {
            "id": "Q42$fake-quantity",
            "mainsnak": {
              "snaktype": "value",
              "property": "P2048",
              "datavalue": {
                "value": { "amount": "+1.96", "unit": "http://www.wikidata.org/entity/Q11573" },
                "type": "quantity"
              }
            },
            "type": "statement",
            "rank": "preferred",
            "references": []
          }
        ],
        "P9999": [
          {
            "id": "Q42$unknown-datatype",
            "mainsnak": {
              "snaktype": "value",
              "property": "P9999",
              "datavalue": {
                "value": { "some": "future-shape" },
                "type": "musical-notation"
              }
            },
            "type": "statement",
            "rank": "deprecated",
            "references": []
          }
        ],
        "P106": [
          {
            "id": "Q42$novalue-example",
            "mainsnak": { "snaktype": "novalue", "property": "P106" },
            "type": "statement",
            "rank": "normal",
            "references": []
          }
        ],
        "P1477": [
          {
            "id": "Q42$monolingual-example",
            "mainsnak": {
              "snaktype": "value",
              "property": "P1477",
              "datavalue": {
                "value": { "text": "Douglas Noel Adams", "language": "en" },
                "type": "monolingualtext"
              }
            },
            "type": "statement",
            "rank": "normal",
            "references": []
          }
        ],
        "P625": [
          {
            "id": "Q42$coordinate-example",
            "mainsnak": {
              "snaktype": "value",
              "property": "P625",
              "datavalue": {
                "value": { "latitude": 51.75194, "longitude": -0.33638 },
                "type": "globecoordinate"
              }
            },
            "type": "statement",
            "rank": "normal",
            "references": []
          }
        ],
        "P40": [
          {
            "id": "Q42$somevalue-example",
            "mainsnak": { "snaktype": "somevalue", "property": "P40" },
            "type": "statement",
            "rank": "normal",
            "references": []
          }
        ]
      }
    }
  }
}
```

**Step 2: Verify it is valid JSON**

Run: `python3 -m json.tool fixtures/wikibase/q42_entitydata.json > /dev/null && echo OK`
Expected: `OK`

**Step 3: Commit**

```bash
git add fixtures/wikibase/q42_entitydata.json
git commit -m "test(wikibase): EntityData fixture for the entity parser"
```

---

### Task 2: Core types

**Files:**
- Create: `crates/sp42-platform/src/wikibase/mod.rs`
- Create: `crates/sp42-platform/src/wikibase/model.rs`
- Modify: `crates/sp42-platform/src/lib.rs` (add `pub mod wikibase;` alphabetically among the existing `pub mod` declarations)

**Step 1: Create the module skeleton**

`crates/sp42-platform/src/wikibase/mod.rs`:

```rust
//! Shared Wikibase (Wikidata) read model — ADR-0016.
//!
//! Typed entity parse, render, and diff shared by the citation MCP verb
//! (PRD-0010), patrol's `EntityDiff`, and the statement write lane (ADR-0017).
//! Design: docs/design-plans/2026-07-01-wikidata-read-model.md.

mod model;

pub use model::{
    Entity, EntityId, Lang, PropertyId, Reference, Sitelink, Snak, Statement, StatementId,
    StatementRank, TermMap, WikibaseValue,
};
```

**Step 2: Write the types** (`model.rs`)

Follow the design plan's type sketch exactly, with these verified adaptations:
- `EntityId`, `PropertyId`, `StatementId` are newtypes over `String` (nothing pre-exists; #103 used bare `String` — newtypes prevent id-kind mixups across the three consumers). Derive `Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize`, add `impl Display` and `fn new(impl Into<String>)` + `as_str()`.
- `pub type Lang = String;` and `pub type TermMap = BTreeMap<Lang, String>;` (plain aliases — term keys are opaque language codes).
- Serde derives on all model types (`Serialize, Deserialize`) — snapshots must round-trip (ADR-0009 discipline; `Statement.raw`/`Reference.raw` are `serde_json::Value`, which serializes natively).

```rust
use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

// ... newtypes as above ...

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sitelink {
    pub site: String,
    pub title: String,
}

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reference {
    pub snaks: Vec<Snak>,
    pub raw: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Snak {
    Value { property: PropertyId, value: WikibaseValue },
    SomeValue { property: PropertyId },
    NoValue { property: PropertyId },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WikibaseValue {
    String(String),
    EntityId(EntityId),
    Monolingual { lang: Lang, text: String },
    Time { time: String, precision: u8 },
    Quantity { amount: String, unit: Option<EntityId> },
    GlobeCoordinate { lat: f64, lon: f64 },
    /// Forward-compat: unknown datatypes preserved, never a parse failure,
    /// still diffable via `Statement.raw`.
    Other(serde_json::Value),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StatementRank {
    Preferred,
    Normal,
    Deprecated,
}
```

Note: `Entity`/`Statement` etc. derive `PartialEq` but not `Eq` (`WikibaseValue::GlobeCoordinate` holds `f64`; `serde_json::Value` is also only `PartialEq`).

**Step 3: Verify it compiles**

Run: `cargo check -p sp42-platform`
Expected: clean. (No tests yet — types only.)

**Step 4: Commit**

```bash
git add crates/sp42-platform/src/wikibase crates/sp42-platform/src/lib.rs
git commit -m "feat(wikibase): typed Wikibase entity model (ADR-0016)"
```

---

### Task 3: `parse_entity`

**Files:**
- Create: `crates/sp42-platform/src/wikibase/parse.rs`
- Modify: `crates/sp42-platform/src/wikibase/mod.rs` (add `mod parse;` + `pub use parse::{parse_entity, WikibaseParseError};`)

**Step 1: Write the failing tests first** (inline `#[cfg(test)]` in `parse.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const ENTITYDATA: &str = include_str!("../../../../fixtures/wikibase/q42_entitydata.json");

    fn q42() -> Entity {
        parse_entity(&EntityId::new("Q42"), ENTITYDATA.as_bytes()).expect("parses")
    }

    #[test]
    fn parses_terms_aliases_and_sitelinks() {
        let entity = q42();
        assert_eq!(entity.id, EntityId::new("Q42"));
        assert_eq!(entity.last_revid, Some(2_000_341));
        assert_eq!(entity.labels.get("en").map(String::as_str), Some("Douglas Adams"));
        assert_eq!(entity.aliases["en"], vec!["Douglas Noel Adams", "DNA"]);
        assert_eq!(entity.sitelinks["enwiki"].title, "Douglas Adams");
    }

    #[test]
    fn parses_statement_depth_qualifiers_rank_and_references() {
        let entity = q42();
        let educated = &entity.statements[&PropertyId::new("P69")][0];
        assert!(matches!(
            &educated.value,
            Snak::Value { value: WikibaseValue::EntityId(id), .. } if id.as_str() == "Q691283"
        ));
        assert_eq!(educated.qualifiers.len(), 1);
        assert_eq!(educated.rank, StatementRank::Normal);
        assert_eq!(educated.references.len(), 1);
        assert!(!educated.raw.is_null());
    }

    #[test]
    fn unknown_datatype_parses_as_other_never_fails() {
        let entity = q42();
        let unknown = &entity.statements[&PropertyId::new("P9999")][0];
        assert!(matches!(&unknown.value, Snak::Value { value: WikibaseValue::Other(_), .. }));
        assert_eq!(unknown.rank, StatementRank::Deprecated);
    }

    #[test]
    fn novalue_and_somevalue_snaks_parse() {
        let entity = q42();
        assert!(matches!(
            &entity.statements[&PropertyId::new("P106")][0].value,
            Snak::NoValue { .. }
        ));
        assert!(matches!(
            &entity.statements[&PropertyId::new("P40")][0].value,
            Snak::SomeValue { .. }
        ));
    }

    #[test]
    fn monolingual_and_coordinate_values_parse() {
        let entity = q42();
        assert!(matches!(
            &entity.statements[&PropertyId::new("P1477")][0].value,
            Snak::Value { value: WikibaseValue::Monolingual { lang, text }, .. }
                if lang == "en" && text == "Douglas Noel Adams"
        ));
        assert!(matches!(
            &entity.statements[&PropertyId::new("P625")][0].value,
            Snak::Value { value: WikibaseValue::GlobeCoordinate { .. }, .. }
        ));
    }

    #[test]
    fn parses_bare_entity_object_endpoint_agnostic() {
        // The Action-API revision slot carries the bare entity object, no
        // {"entities": {...}} wrapper. Same body, different wrapper (design plan §Reading).
        let doc: serde_json::Value = serde_json::from_str(ENTITYDATA).unwrap();
        let bare = serde_json::to_vec(&doc["entities"]["Q42"]).unwrap();
        let entity = parse_entity(&EntityId::new("Q42"), &bare).expect("parses bare form");
        assert_eq!(entity.labels.get("en").map(String::as_str), Some("Douglas Adams"));
    }

    #[test]
    fn missing_entity_is_an_error() {
        let err = parse_entity(&EntityId::new("Q1"), ENTITYDATA.as_bytes()).unwrap_err();
        assert!(matches!(err, WikibaseParseError::EntityNotFound { .. }));
    }
}
```

**Step 2: Run to verify they fail**

Run: `cargo test -p sp42-platform wikibase`
Expected: compile error (`parse_entity` not defined).

**Step 3: Implement `parse_entity`**

```rust
use serde_json::Value;
use thiserror::Error;

use super::model::*;

#[derive(Debug, Error)]
pub enum WikibaseParseError {
    #[error("entity payload is not valid JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("entity {id} not found in payload")]
    EntityNotFound { id: EntityId },
    #[error("entity payload for {id} has no recognizable shape")]
    UnrecognizedShape { id: EntityId },
}

/// Parse a Wikibase entity from EITHER a `Special:EntityData` `{"entities":{id:{…}}}`
/// document OR the bare entity object found inside an Action-API revision slot.
/// Endpoint-agnostic: the entity JSON body is identical, only the wrapper differs.
pub fn parse_entity(id: &EntityId, body: &[u8]) -> Result<Entity, WikibaseParseError> { ... }
```

Implementation notes (error-tolerant, per house style and the design's "unknown never fails"):
- Root resolution: if the doc has `entities`, take `entities[id]` (missing → `EntityNotFound`); else if it has an `"id"` field, treat the doc itself as the bare entity (mismatched id → `EntityNotFound`); else `UnrecognizedShape`.
- `last_revid` from `lastrevid` (u64, optional).
- `labels`/`descriptions`: objects of `{lang: {language, value}}` → `TermMap`. `aliases`: `{lang: [{value}...]}`.
- `sitelinks`: `{site: {site, title}}`.
- `claims` → statements: for each property, each statement object → `Statement`:
  - `raw` = the whole statement `Value`, cloned, **before** any field extraction.
  - `mainsnak` → `parse_snak`: `snaktype` of `novalue`/`somevalue` → those variants; `value` → `datavalue.type` dispatch: `string`→`String`, `wikibase-entityid`→`EntityId` (use `value.id`; fall back to `value["numeric-id"]` prefixed `Q`), `monolingualtext`→`Monolingual`, `time`→`Time` (string + precision u8), `quantity`→`Quantity` (`unit` of `"1"` or missing → `None`, else last path segment of the unit URI as `EntityId`), `globecoordinate`→`GlobeCoordinate` (from `value.latitude`/`value.longitude`), anything else → `Other(datavalue.value.clone())`.
  - `qualifiers`: object `{prop: [snak...]}` flattened in property order → `Vec<Snak>`.
  - `rank`: `"preferred"`/`"deprecated"` else `Normal`.
  - `references`: array → `Reference { snaks: flattened snaks from .snaks, raw: whole reference Value }`.
- Malformed individual statements: skip nothing silently — a statement object that fails snak extraction still lands with `value: Snak::Value { .. WikibaseValue::Other(mainsnak.clone()) }`... **No.** Keep it simpler and honest: extraction helpers return `Option`; a statement whose mainsnak is missing entirely becomes `WikibaseValue::Other` of the statement's `mainsnak` field (or `Value::Null`), so it still exists and still diffs via `raw`. Nothing is dropped.

**Step 4: Run tests to verify they pass**

Run: `cargo test -p sp42-platform wikibase`
Expected: 7 passed.

**Step 5: Lint and commit**

Run: `cargo ci-clippy` (pedantic is deny — expect naming/`must_use`/doc nits; fix them).

```bash
git add crates/sp42-platform/src/wikibase
git commit -m "feat(wikibase): endpoint-agnostic entity parser with raw retention"
```

---

**Phase done when:** `cargo test -p sp42-platform` green including the 7 new tests, `cargo ci-clippy` clean, `./scripts/check-layering.sh` still passes (no registration change expected).
