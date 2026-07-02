# Wikidata Read Model Implementation Plan — Phase 4: `EntityDiff` + `diff_entities`

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** A structured entity diff with the honesty invariant: any byte-level difference between two entity revisions surfaces as at least one classified change (ADR-0016 Decision 3).

**Architecture:** Pure diffing over Phase 1's `Entity`, sibling to `StructuredDiff` (`crates/sp42-platform/src/diff_engine.rs:118`) — a new type, not an extension. Statement change detection compares `Statement.raw` (exact, covers datatypes we don't richly model); sub-part classification (`StatementChangeParts`) compares the raw JSON's `mainsnak`/`qualifiers`/`rank`/`references` fields.

**Tech Stack:** Rust, serde_json equality. No new dependencies.

**Scope:** Phase 4 of 7.

**Codebase verified:** 2026-07-01. `proptest` is a dev-dependency in `sp42-citation` (used by `scoring_engine.rs` in platform too) — available if a property test is warranted for the honesty invariant; the plan below uses a targeted mutation test instead, which is cheaper and pins the exact guarantee.

---

### Task 1: Change types

**Files:**
- Create: `crates/sp42-platform/src/wikibase/diff.rs`
- Modify: `crates/sp42-platform/src/wikibase/mod.rs` (add `mod diff;` + `pub use diff::{diff_entities, AliasChange, EntityDiff, SitelinkChange, StatementChange, StatementChangeParts, TermChange};`)

**Step 1: Write the types** (types-only step; tests arrive with `diff_entities` in Task 2)

```rust
use serde::{Deserialize, Serialize};

use super::model::{Lang, Sitelink, Statement};

/// Structured diff of two Wikibase entity revisions (ADR-0016 Decision 3).
/// Sibling of `StructuredDiff` (diff_engine), selected by `ContentDiff` (Phase 6).
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
    pub fn is_empty(&self) -> bool { ... }
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SitelinkChange {
    pub site: String,
    pub before: Option<Sitelink>,
    pub after: Option<Sitelink>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StatementChange {
    Added(Statement),
    Removed(Statement),
    Changed { before: Statement, after: Statement, parts: StatementChangeParts },
}

/// Which sub-parts of a statement moved — powers "an edit touching only a
/// qualifier / rank / reference is never a no-op". Computed from raw-JSON equality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatementChangeParts {
    pub value: bool,
    pub qualifiers: bool,
    pub rank: bool,
    pub references: bool,
}
```

**Step 2: Verify compile** — `cargo check -p sp42-platform`.

**Step 3: Commit** — `feat(wikibase): EntityDiff change taxonomy (ADR-0016 D3)`

---

### Task 2: `diff_entities`

**Files:**
- Modify: `crates/sp42-platform/src/wikibase/diff.rs`

**Step 1: Write the failing tests**

Build small entities in-code via a helper that parses inline JSON through `parse_entity` (reuses the production parser rather than hand-assembling structs — the invariant is about what the parser produced):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::wikibase::{parse_entity, EntityId};

    fn entity(claims_json: &str, label: &str) -> crate::wikibase::Entity {
        let doc = format!(
            r#"{{"id":"Q1","type":"item","labels":{{"en":{{"language":"en","value":"{label}"}}}},
               "descriptions":{{}},"aliases":{{}},"claims":{claims_json},"sitelinks":{{}}}}"#
        );
        parse_entity(&EntityId::new("Q1"), doc.as_bytes()).unwrap()
    }

    const STMT: &str = r#"{"P569":[{"id":"Q1$a","mainsnak":{"snaktype":"value","property":"P569",
        "datavalue":{"value":"x","type":"string"}},"type":"statement","rank":"normal","references":[]}]}"#;

    #[test]
    fn identical_entities_diff_empty() {
        assert!(diff_entities(Some(&entity(STMT, "A")), &entity(STMT, "A")).is_empty());
    }

    #[test]
    fn missing_parent_yields_all_added_not_an_error() {
        let diff = diff_entities(None, &entity(STMT, "A"));
        assert_eq!(diff.labels.len(), 1);
        assert!(matches!(diff.statements[0], StatementChange::Added(_)));
    }

    #[test]
    fn label_change_is_classified() {
        let diff = diff_entities(Some(&entity(STMT, "A")), &entity(STMT, "B"));
        assert_eq!(diff.labels, vec![TermChange {
            lang: "en".into(), before: Some("A".into()), after: Some("B".into()),
        }]);
        assert!(diff.statements.is_empty());
    }

    #[test]
    fn rank_only_change_is_never_a_no_op() {
        let promoted = STMT.replace(r#""rank":"normal""#, r#""rank":"preferred""#);
        let diff = diff_entities(Some(&entity(STMT, "A")), &entity(&promoted, "A"));
        let StatementChange::Changed { parts, .. } = &diff.statements[0] else {
            panic!("expected Changed, got {:?}", diff.statements)
        };
        assert!(parts.rank);
        assert!(!parts.value && !parts.qualifiers && !parts.references);
    }

    #[test]
    fn qualifiers_only_change_is_never_a_no_op() {
        let with_qualifier = STMT.replace(
            r#""type":"statement","rank":"normal""#,
            r#""type":"statement","qualifiers":{"P580":[{"snaktype":"value","property":"P580",
                "datavalue":{"value":{"time":"+1971-00-00T00:00:00Z","precision":9},"type":"time"}}]},"rank":"normal""#,
        );
        let diff = diff_entities(Some(&entity(STMT, "A")), &entity(&with_qualifier, "A"));
        let StatementChange::Changed { parts, .. } = &diff.statements[0] else { panic!() };
        assert!(parts.qualifiers);
        assert!(!parts.value && !parts.rank && !parts.references);
    }

    #[test]
    fn raw_difference_outside_the_four_sub_parts_still_surfaces() {
        // The honesty catch-all: a field none of the four classifiers cover
        // (e.g. a statement-level "hash") must still register as a change.
        let with_extra = STMT.replace(
            r#""type":"statement""#,
            r#""type":"statement","hash":"deadbeef""#,
        );
        let diff = diff_entities(Some(&entity(STMT, "A")), &entity(&with_extra, "A"));
        let StatementChange::Changed { parts, .. } = &diff.statements[0] else {
            panic!("difference outside sub-parts must never be a no-op")
        };
        assert!(parts.value, "catch-all marks value so the change is visible");
    }

    #[test]
    fn reference_only_change_is_never_a_no_op() {
        let with_ref = STMT.replace(
            r#""references":[]"#,
            r#""references":[{"snaks":{"P854":[{"snaktype":"value","property":"P854",
                "datavalue":{"value":"https://e.org","type":"string"}}]}}]"#,
        );
        let diff = diff_entities(Some(&entity(STMT, "A")), &entity(&with_ref, "A"));
        let StatementChange::Changed { parts, .. } = &diff.statements[0] else { panic!() };
        assert!(parts.references && !parts.value);
    }

    #[test]
    fn statement_added_and_removed_by_guid() {
        let two = STMT.replace(
            r#"}]}"#,
            r#"},{"id":"Q1$b","mainsnak":{"snaktype":"value","property":"P569",
               "datavalue":{"value":"y","type":"string"}},"type":"statement","rank":"normal","references":[]}]}"#,
        );
        let diff = diff_entities(Some(&entity(STMT, "A")), &entity(&two, "A"));
        assert!(matches!(diff.statements.as_slice(), [StatementChange::Added(s)] if s.id.as_ref().unwrap().as_str() == "Q1$b"));
        let reverse = diff_entities(Some(&entity(&two, "A")), &entity(STMT, "A"));
        assert!(matches!(reverse.statements.as_slice(), [StatementChange::Removed(_)]));
    }

    #[test]
    fn unknown_datatype_change_still_registers() {
        // The honesty invariant for datatypes we model as Other: raw equality catches it.
        let odd = STMT.replace(r#""value":"x","type":"string""#, r#""value":{"z":1},"type":"musical-notation""#);
        let odd2 = odd.replace(r#"{"z":1}"#, r#"{"z":2}"#);
        let diff = diff_entities(Some(&entity(&odd, "A")), &entity(&odd2, "A"));
        assert!(matches!(&diff.statements[0], StatementChange::Changed { parts, .. } if parts.value));
    }
}
```

**Step 2: Run to verify failure** — `cargo test -p sp42-platform wikibase::diff` → compile error (`diff_entities` undefined).

**Step 3: Implement**

```rust
/// Diff two entity revisions. `old = None` = first revision (everything Added).
/// Change detection uses `Statement.raw` equality, so unknown datatypes still
/// register (the honesty invariant, ADR-0016 Decision 3).
#[must_use]
pub fn diff_entities(old: Option<&Entity>, new: &Entity) -> EntityDiff { ... }
```

Algorithm:
- **Terms** (labels, descriptions): union of language keys; emit `TermChange` where the values differ (including `None` sides).
- **Aliases:** union of language keys; emit `AliasChange` where the `Vec<String>` differs.
- **Sitelinks:** union of site keys; emit `SitelinkChange` where they differ.
- **Statements:** key by identity — GUID when present, else `(property, index)` position fallback:
  1. Build maps GUID→statement per side; statements without GUIDs pair positionally within their property (rare in practice; Wikibase assigns GUIDs).
  2. In `new` but not `old` → `Added`; in `old` but not `new` → `Removed`.
  3. Present in both with `raw` unequal → `Changed` with parts:
     `value: raw["mainsnak"] != raw["mainsnak"]`, `qualifiers: raw["qualifiers"] (or Null) differs`, `rank: raw["rank"] differs`, `references: raw["references"] (or Null) differs`. If `raw` differs but all four sub-parts are equal (e.g. a field outside the four), set all-false is a lie — instead set `value: true` as the catch-all so the change is visible. Document this in a comment: the invariant beats sub-part precision.
- Ordering: deterministic — iterate `BTreeMap`s in key order; statement changes ordered by property id then GUID.

**Step 4: Run tests** — all pass: `cargo test -p sp42-platform wikibase`.

**Step 5: Lint + commit**

```bash
git add crates/sp42-platform/src/wikibase
git commit -m "feat(wikibase): diff_entities with the never-a-no-op invariant"
```

---

**Phase done when:** all wikibase tests green (Phases 1–4), `cargo ci-clippy` clean. `EntityDiff` is fully usable standalone; routing arrives in Phase 6.
