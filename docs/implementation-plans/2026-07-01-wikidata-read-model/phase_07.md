# Wikidata Read Model Implementation Plan — Phase 7: `StatementProposal` payload types

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** The ADR-0017 propose/confirm payload types, so the write lane and #103's future write verbs share one shape from day one. **Types only — no write machinery, no HTTP, no wbeditentity** (ADR-0016 non-goal; ADR-0017 status is Proposed).

**Architecture:** Serde-serializable types in the `wikibase` module referencing Phase 1's `Statement` and the grounding contract. The drift baseline is `Entity.last_revid` at propose time (already modeled in Phase 1).

**Tech Stack:** Rust, serde.

**Scope:** Phase 7 of 7.

**Codebase verified:** 2026-07-01. Check what grounding type exists before writing code: the design plan sketches `pub grounding: Grounding // ADR-0007: verbatim source passage + source hash`. Find the real type with:
```bash
grep -rn 'pub struct Grounding\|grounding' crates/sp42-citation/src/ crates/sp42-platform/src/ | head -20
```
If a reusable grounding type exists in a crate `sp42-platform` may depend on, use it. If it lives in `sp42-citation` (a **domain** crate — platform must NOT depend on it per ADR-0013), define a minimal structural equivalent here and note the convergence point in its rustdoc. **Do not add a platform→domain dependency; `./scripts/check-layering.sh` will catch it.**

---

### Task 1: `StatementProposal`

**Files:**
- Create: `crates/sp42-platform/src/wikibase/proposal.rs`
- Modify: `crates/sp42-platform/src/wikibase/mod.rs` (add `mod proposal;` + `pub use proposal::{StatementGrounding, StatementProposal};`)

**Step 1: Write the failing test** (round-trip — these types exist to be serialized across the propose/confirm boundary)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::wikibase::{parse_entity, EntityId, PropertyId};

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
```

**Step 2: Run to verify failure.**

**Step 3: Implement**

```rust
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
```

(Adjust `StatementGrounding` per the Task-1 investigation: if a platform-reachable grounding type exists, use it and delete this one from the plan.)

**Step 4: Run tests** — green.

**Step 5: Lint + commit**

```bash
git add crates/sp42-platform/src/wikibase
git commit -m "feat(wikibase): ADR-0017 StatementProposal payload types (propose-side only)"
```

---

### Task 2: module docs + design-plan status flip

**Files:**
- Modify: `crates/sp42-platform/src/wikibase/mod.rs` (module-level rustdoc: one paragraph inventorying the module per the design plan's section names — model / reading / rendering / diffing / identity+proposal — so `cargo doc` output mirrors the design doc; keep links to ADR paths as plain text, NOT intra-doc links to private items — that exact rustdoc failure is live on PR #103)
- Modify: `docs/design-plans/2026-07-01-wikidata-read-model.md` (change `**Status:** Sketch (pre-implementation)` to `**Status:** Implemented (sp42-platform::wikibase)`)

**Step 1: Make the edits. Step 2: Verify**

Run: `cargo ci-doc` — expected clean (this is the gate #103 tripped on).
Run: `cargo ci-test && cargo ci-clippy && ./scripts/check-layering.sh` — full green.

**Step 3: Commit**

```bash
git add crates/sp42-platform/src/wikibase docs/design-plans/2026-07-01-wikidata-read-model.md
git commit -m "docs(wikibase): module inventory; design plan marked implemented"
```

---

**Phase done when:** entire workspace `cargo ci-test`, `cargo ci-clippy`, `cargo ci-doc`, and `./scripts/check-layering.sh` all green. The shared read model is complete; the follow-on plans are (a) patrol MVP wiring (queue + ContentDiff consumption + app diff view, PRD-0011) and (b) the #103 convergence refactor (per the design plan's merge-order note, whichever lands second refactors onto this module).
