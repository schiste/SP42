# Wikidata Read Model Implementation Plan — Phase 6: `ContentDiff` routing + content-model capability axis

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** The routing sum type consumers `match` on (`ContentDiff`), and the content-model capability axis that gates wikitext-only signals off for entity content (ADR-0016 Decisions 4–5).

**Architecture:** `ContentDiff` lives in `sp42-platform` next to `diff_engine.rs` (design plan §Placement — so it can name both `StructuredDiff` and `EntityDiff` without a cross-crate edge). The capability axis is a standalone derivation keyed on `ContentModel` — a property of the content, not the wiki/account — living in `sp42-platform` (see Placement below), leaving `derive_wiki_capability_profile` untouched.

**Tech Stack:** Rust, serde.

**Scope:** Phase 6 of 7. **Consumer wiring (patrol queue, reporting renderer, app diff view) is deliberately NOT here** — it's the follow-on implementation plan for PRD-0011's patrol MVP; this phase delivers the platform contract they'll match on.

**Codebase verified:** 2026-07-01.
- `WikiCapabilityProfile` at `crates/sp42-wiki/src/capabilities.rs:21-26` composed of `WikiReadCapabilityProfile` (29-33), `WikiEditingCapabilityProfile` (36-39), `WikiModerationCapabilityProfile` (42-45); derivation fn at 48-108; exported via `crates/sp42-wiki/src/lib.rs:21-25`. That account-derived profile is untouched by this phase; the content axis lives in platform (see Placement below).

---

### Placement (verified, no investigation needed)

Dependency directions were verified against the worktree during plan review: `sp42-platform` depends only on `sp42-types`, and `sp42-wiki` depends on `sp42-core` — there is **no edge between `sp42-platform` and `sp42-wiki` in either direction**. The design plan's literal wording ("extends `sp42-wiki::capabilities`") would therefore force a new cross-crate edge; instead the axis lands in `crates/sp42-platform/src/wikibase/capability.rs`, needing only the local `ContentModel`. This is the more ADR-consistent placement anyway (ADR-0016 Decision 8 puts content-model types in platform), and it is a *separate axis* from `derive_wiki_capability_profile` by design (Decision 5) — shells/domains combine the two profiles at the call site. Note this divergence from the design plan's placement sentence in the commit message.

---

### Task 1: `ContentCapabilityProfile`

**Files:**
- Create: `crates/sp42-platform/src/wikibase/capability.rs`
- Modify: `crates/sp42-platform/src/wikibase/mod.rs` (add `mod capability;` + `pub use capability::{derive_content_capability_profile, ContentCapabilityProfile};`)

**Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ContentModel;

    #[test]
    fn wikitext_enables_all_wikitext_signals() {
        let profile = derive_content_capability_profile(&ContentModel::Wikitext);
        assert!(profile.media_reference_extraction);
        assert!(profile.talk_page_warning_parsing);
        assert!(profile.citation_extraction);
        assert!(profile.revertrisk_scoring);
        assert!(!profile.entity_diff);
    }

    #[test]
    fn entity_models_gate_wikitext_signals_off_and_enable_entity_diff() {
        for model in [ContentModel::WikibaseItem, ContentModel::WikibaseProperty] {
            let profile = derive_content_capability_profile(&model);
            assert!(!profile.media_reference_extraction);
            assert!(!profile.talk_page_warning_parsing);
            assert!(!profile.citation_extraction);
            assert!(!profile.revertrisk_scoring, "ADR-0016 D7: no LiftWing for entities");
            assert!(profile.entity_diff);
        }
    }

    #[test]
    fn unknown_models_degrade_to_text_with_no_entity_diff() {
        let profile = derive_content_capability_profile(&ContentModel::Other("Scribunto".into()));
        assert!(!profile.entity_diff);
        assert!(!profile.revertrisk_scoring); // trained on Wikipedia wikitext only
    }
}
```

**Step 2: Run to verify failure.**

**Step 3: Implement**

```rust
use crate::types::ContentModel;

/// Which content-model-specific features apply to a revision (ADR-0016 Decision 5).
/// A property of the *content*, not the account — a separate axis from
/// `derive_wiki_capability_profile` (sp42-wiki), which is untouched.
/// Gated features are NOT invoked (not invoked-and-discarded).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ContentCapabilityProfile {
    pub media_reference_extraction: bool,
    pub talk_page_warning_parsing: bool,
    pub citation_extraction: bool,
    /// LiftWing revertrisk — Wikipedia-wikitext-trained; skipped, never faked (D7).
    pub revertrisk_scoring: bool,
    pub entity_diff: bool,
}

#[must_use]
pub fn derive_content_capability_profile(model: &ContentModel) -> ContentCapabilityProfile { ... }
```

Mapping: `Wikitext` → all wikitext signals true, `entity_diff: false`. `WikibaseItem | WikibaseProperty` → all wikitext signals false, `entity_diff: true`. `Other(_)` → all false (honest degradation: rendered as text, no signals claimed).

**Step 4: Run tests. Step 5: Commit** — `feat(wikibase): content-model capability axis (ADR-0016 D5/D7)`

---

### Task 2: `ContentDiff`

**Files:**
- Modify: `crates/sp42-platform/src/wikibase/diff.rs` (or a sibling `routing.rs` if diff.rs is crowded)
- Modify: `crates/sp42-platform/src/wikibase/mod.rs` (export `ContentDiff, route_content_diff`)

**Step 1: Write the failing tests**

```rust
    #[test]
    fn wikitext_routes_to_text_diff() {
        let diff = route_content_diff(
            &ContentModel::Wikitext, "old text", "new text",
            || diff_revision_texts_stub(), // see note below — use the real diff_engine entry point
        );
        assert!(matches!(diff, ContentDiff::Text(_)));
    }

    #[test]
    fn entity_models_route_to_entity_diff() { /* wikibase-item with two parseable entity bodies -> ContentDiff::Entity */ }

    #[test]
    fn unparseable_entity_body_degrades_to_text_with_note() { /* wikibase-item but garbage JSON -> ContentDiff::Text, note set */ }

    #[test]
    fn unknown_model_degrades_to_text_with_note() { /* Other("Scribunto") -> Text + note */ }
```

**Step 2: Run to verify failure.**

**Step 3: Implement**

First check how `StructuredDiff` is produced: `grep -n 'pub fn' crates/sp42-platform/src/diff_engine.rs` (the entry point that takes before/after text — verified `StructuredDiff` is at line 118; find its constructor function and call it, don't reimplement).

```rust
use crate::diff_engine::StructuredDiff;
use crate::types::ContentModel;

/// The diff a consumer receives, selected by the revision's content model
/// (ADR-0016 Decision 4). Wikitext is byte-for-byte the existing path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ContentDiff {
    Text {
        diff: StructuredDiff,
        /// Set when this is a degradation (unknown model, unparseable entity):
        /// honest fallback, not a silent lie (D4).
        note: Option<String>,
    },
    Entity(EntityDiff),
}

/// Route two revision bodies to the right diff. `entity_id` is required only for
/// the entity path (parse needs the id); pass the page title (Q-id) from patrol.
#[must_use]
pub fn route_content_diff(
    model: &ContentModel,
    entity_id: Option<&EntityId>,
    before: Option<&str>,
    after: &str,
) -> ContentDiff { ... }
```

Routing rules:
- `Wikitext` → `Text { diff: <existing diff_engine entry point>(before.unwrap_or(""), after), note: None }`.
- `WikibaseItem | WikibaseProperty` → `parse_entity` both sides (before `None` → first revision). Both parses OK → `Entity(diff_entities(...))`. Any parse fails, or `entity_id` is `None` → `Text` with `note: Some("entity revision could not be parsed; showing text diff")` (exact wording: keep it operator-facing, no jargon).
- `Other(m)` → `Text` with `note: Some(format!("content model {m} is not specially handled; showing text diff"))`.

Adjust the test signatures in Step 1 to this final signature (the sketch above predates it — write the tests directly against this signature).

**Step 4: Run tests** — green. **Step 5: Lint + commit**

```bash
git add crates/sp42-platform/src/wikibase
git commit -m "feat(wikibase): ContentDiff routing with honest text fallback (ADR-0016 D4)"
```

---

**Phase done when:** `cargo ci-test` green, `cargo ci-clippy` clean, `./scripts/check-layering.sh` passes. The platform contract for ADR-0016 is complete: a consumer holding a `ContentModel`, two bodies, and an id gets the right diff and the right feature gates without knowing about Wikibase.
