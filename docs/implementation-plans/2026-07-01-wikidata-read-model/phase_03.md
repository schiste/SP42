# Wikidata Read Model Implementation Plan — Phase 3: selection, references, rendering

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** The convergence surface PR #103's verb will call: statement selection by `StatementRef`, P854 reference-URL extraction, value rendering, and natural-language claim rendering.

**Architecture:** Methods/functions over Phase 1's types, matching the observed behavior of `#103`'s `sp42-mcp/src/wikidata.rs` (verified 2026-07-01 in the `citation-mcp-surface` worktree) so the later refactor-in-place is behavior-preserving. Two deliberate upgrades over #103, both anticipated by the design plan: `Reference::urls()` is **plural** (#103's `extract_ref_url` returns only the first P854), and `render_value` returns a typed `ValueDisplay` (#103 returns a bare tuple).

**Tech Stack:** Rust, no new dependencies.

**Scope:** Phase 3 of 7.

**Codebase verified:** 2026-07-01. #103 behaviors to preserve (from `#103`'s `wikidata.rs`):
- `parse_statement` selection: statement by GUID when `statement_id` is `Some`, else the **first** statement for the property.
- `render_value` dispatch: string → itself; `wikibase-entityid` → id string (caller resolves label); `monolingualtext` → text; `time` → the raw `+YYYY-…` time string; `quantity` → amount; anything else → a fallback string.
- `label_of`: English label lookup from a `wbgetentities` response (Phase 2's `Labels::get` covers this).

---

### Task 1: `Entity::statement` + `StatementRef`

**Files:**
- Create: `crates/sp42-platform/src/wikibase/select.rs`
- Modify: `crates/sp42-platform/src/wikibase/mod.rs` (add `mod select;` + `pub use select::StatementRef;`)

**Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::wikibase::{parse_entity, EntityId, PropertyId, StatementId};

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
        let miss = StatementRef { statement_id: Some(StatementId::new("Q42$nope")), ..r };
        assert!(entity.statement(&miss).is_none());
    }
}
```

**Step 2: Run to verify failure** — `cargo test -p sp42-platform wikibase::select` → compile error.

**Step 3: Implement**

```rust
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
```

**Step 4: Run tests** — green.

**Step 5: Commit**

```bash
git add crates/sp42-platform/src/wikibase
git commit -m "feat(wikibase): StatementRef selection (promotes #103's parse_statement)"
```

---

### Task 2: `Reference::urls()`

**Files:**
- Modify: `crates/sp42-platform/src/wikibase/select.rs` (or `model.rs` impl block — keep `impl Reference` next to `impl Entity` in `select.rs`)

**Step 1: Write the failing test**

```rust
    #[test]
    fn reference_urls_yields_p854_strings() {
        let entity = q42();
        let educated = &entity.statements[&PropertyId::new("P69")][0];
        let urls: Vec<&str> = educated.references[0].urls().collect();
        assert_eq!(urls, vec!["https://example.org/adams-bio"]);
    }

    #[test]
    fn reference_urls_empty_when_no_p854() {
        let entity = q42();
        let birth = &entity.statements[&PropertyId::new("P569")][0];
        assert!(birth.references.is_empty()); // fixture has none — abstention case stays reachable
    }
```

**Step 2: Run to verify failure.**

**Step 3: Implement**

```rust
impl Reference {
    /// P854 ("reference URL") snaks. This is #103's `extract_ref_url`, typed and
    /// plural — #103 takes `.next()` for its first-URL behavior.
    pub fn urls(&self) -> impl Iterator<Item = &str> {
        self.snaks.iter().filter_map(|snak| match snak {
            Snak::Value { property, value: WikibaseValue::String(url) }
                if property.as_str() == "P854" => Some(url.as_str()),
            _ => None,
        })
    }
}
```

**Step 4: Run tests. Step 5: Commit** — `feat(wikibase): typed plural P854 reference URLs`

---

### Task 3: `render_value` + `render_statement_claim`

**Files:**
- Create: `crates/sp42-platform/src/wikibase/render.rs`
- Modify: `crates/sp42-platform/src/wikibase/mod.rs` (add `mod render;` + `pub use render::{render_statement_claim, render_value, ValueDisplay};`)

**Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::wikibase::{parse_entity, parse_labels, EntityId, PropertyId, WikibaseValue};

    const ENTITYDATA: &str = include_str!("../../../../fixtures/wikibase/q42_entitydata.json");
    const LABELS: &str = include_str!("../../../../fixtures/wikibase/q42_labels.json");

    #[test]
    fn renders_scalar_values() {
        assert_eq!(render_value(&WikibaseValue::String("x".into())).text, "x");
        let time = render_value(&WikibaseValue::Time { time: "+1952-03-11T00:00:00Z".into(), precision: 11 });
        assert_eq!(time.text, "+1952-03-11T00:00:00Z");
        let qty = render_value(&WikibaseValue::Quantity { amount: "+1.96".into(), unit: None });
        assert_eq!(qty.text, "+1.96");
    }

    #[test]
    fn renders_monolingual_and_coordinate_values() {
        let mono = render_value(&WikibaseValue::Monolingual { lang: "en".into(), text: "Douglas Noel Adams".into() });
        assert_eq!(mono.text, "Douglas Noel Adams");
        assert_eq!(mono.item, None);
        let coord = render_value(&WikibaseValue::GlobeCoordinate { lat: 51.75194, lon: -0.33638 });
        assert_eq!(coord.text, "51.75194, -0.33638");
    }

    #[test]
    fn somevalue_and_novalue_render_in_claims() {
        let entity = parse_entity(&EntityId::new("Q42"), ENTITYDATA.as_bytes()).unwrap();
        let labels = crate::wikibase::Labels::default();
        let somevalue = &entity.statements[&PropertyId::new("P40")][0];
        assert_eq!(render_statement_claim(&entity, somevalue, &labels), "Douglas Adams P40 unknown value.");
        let novalue = &entity.statements[&PropertyId::new("P106")][0];
        assert_eq!(render_statement_claim(&entity, novalue, &labels), "Douglas Adams P106 no value.");
    }

    #[test]
    fn entity_values_carry_the_item_for_label_lookup() {
        let display = render_value(&WikibaseValue::EntityId(EntityId::new("Q691283")));
        assert_eq!(display.text, "Q691283");
        assert_eq!(display.item, Some(EntityId::new("Q691283")));
    }

    #[test]
    fn claim_renders_subject_property_value_with_labels() {
        let entity = parse_entity(&EntityId::new("Q42"), ENTITYDATA.as_bytes()).unwrap();
        let labels = parse_labels(LABELS.as_bytes()).unwrap();
        let stmt = &entity.statements[&PropertyId::new("P69")][0];
        assert_eq!(
            render_statement_claim(&entity, stmt, &labels),
            "Douglas Adams educated at St John's College."
        );
    }

    #[test]
    fn claim_falls_back_to_ids_when_labels_missing() {
        let entity = parse_entity(&EntityId::new("Q42"), ENTITYDATA.as_bytes()).unwrap();
        let labels = crate::wikibase::Labels::default();
        let stmt = &entity.statements[&PropertyId::new("P69")][0];
        assert_eq!(render_statement_claim(&entity, stmt, &labels), "Douglas Adams P69 Q691283.");
    }
}
```

**Step 2: Run to verify failure.**

**Step 3: Implement**

```rust
use super::model::{Entity, EntityId, Snak, Statement, WikibaseValue};
use super::read::Labels;

/// One rendered value; carries the item id when the value is an entity so the
/// caller can resolve its label. (#103's `render_value` tuple, typed.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueDisplay {
    pub text: String,
    pub item: Option<EntityId>,
}

#[must_use]
pub fn render_value(value: &WikibaseValue) -> ValueDisplay { ... }

/// Render a statement to a natural-language claim
/// ("<subject-label> <property-label> <value>."), resolving labels via `labels`
/// with id fallback. This IS #103's `claim_rendered`.
#[must_use]
pub fn render_statement_claim(subject: &Entity, stmt: &Statement, labels: &Labels) -> String { ... }
```

Behavior (match #103, per the verified mapping):
- `render_value`: `String`→text; `EntityId`→(id text, `item: Some(id)`); `Monolingual`→text; `Time`→raw time string; `Quantity`→amount; `GlobeCoordinate`→`"{lat}, {lon}"`; `Other(v)`→compact JSON (`v.to_string()`). Only `EntityId` sets `item`.
- `render_statement_claim`: subject label = entity's `"en"` label, falling back to the entity id; property label = `labels.get(property)` falling back to the property id; value text = `render_value`, and when `item` is `Some`, `labels.get(item)` replaces the id text when available. Snak variants `SomeValue`→`"unknown value"`, `NoValue`→`"no value"`. Ends with `"."`.
- English-first label policy is #103's current behavior; keep it (language plumbing is a PRD-0010 open question, out of scope).

**Step 4: Run tests** — all green: `cargo test -p sp42-platform wikibase`.

**Step 5: Lint + commit**

Run: `cargo ci-clippy`

```bash
git add crates/sp42-platform/src/wikibase
git commit -m "feat(wikibase): value + claim rendering (promotes #103's renderer)"
```

---

**Phase done when:** all wikibase tests green, clippy clean. The module now covers every row of the design plan's #103 mapping table except `get_json` (a shell concern — stays in `sp42-mcp`).
