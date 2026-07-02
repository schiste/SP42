# Wikidata Read Model Implementation Plan — Phase 5: `content_model` as a per-revision fact

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** Every `EditEvent` and revision-content read carries the revision's content model, serde-back-compatible (ADR-0016 Decision 1).

**Architecture:** A `ContentModel` type in `sp42-platform` types, an additive `#[serde(default)]` field on `EditEvent`, and population from `rvprop=contentmodel` where revision content is fetched. Existing streams/snapshots deserialize unchanged as `wikitext` (ADR-0009 discipline).

**Tech Stack:** Rust, serde.

**Scope:** Phase 5 of 7.

**Codebase verified:** 2026-07-01.
- `EditEvent` at `crates/sp42-platform/src/types.rs:86-102`; it already carries `#[serde(default)] is_patrolled: FlagState` — the same additive pattern applies.
- `content_model`/`contentmodel` appears nowhere in the workspace (confirmed by ADR-0016 and by grep).
- Revision content is fetched today in `crates/sp42-server/src/revision_artifacts.rs` (`fetch_revision_texts` around line 489, used by `fetch_revision_text_pair` at line 288, params built like `("rvprop", "ids|content")` — see the same pattern in `crates/sp42-platform/src/wiki_storage.rs:408-410`). The recentchanges ingest is `crates/sp42-live/src/recent_changes.rs` (`parse_recent_changes_response` line 194).

---

### Task 1: `ContentModel` type

**Files:**
- Modify: `crates/sp42-platform/src/types.rs` (add near `EditEvent`)
- Modify: `crates/sp42-platform/src/lib.rs` (re-export `ContentModel` alongside the existing `types` re-exports)

**Step 1: Write the failing tests** (in the existing `#[cfg(test)]` module of `types.rs`, or create one following house style)

```rust
    #[test]
    fn content_model_defaults_to_wikitext_and_routes_known_models() {
        assert_eq!(ContentModel::default(), ContentModel::Wikitext);
        assert_eq!(ContentModel::parse("wikitext"), ContentModel::Wikitext);
        assert_eq!(ContentModel::parse("wikibase-item"), ContentModel::WikibaseItem);
        assert_eq!(ContentModel::parse("wikibase-property"), ContentModel::WikibaseProperty);
        assert_eq!(
            ContentModel::parse("Scribunto"),
            ContentModel::Other("Scribunto".to_string())
        );
    }

    #[test]
    fn content_model_round_trips_the_wire_string() {
        // The #[serde(from/into String)] pair underwrites ADR-0009 back-compat:
        // unknown models must survive serialize→deserialize verbatim.
        for model in [ContentModel::Wikitext, ContentModel::WikibaseItem, ContentModel::Other("Scribunto".into())] {
            let json = serde_json::to_string(&model).unwrap();
            let back: ContentModel = serde_json::from_str(&json).unwrap();
            assert_eq!(back, model);
        }
        assert_eq!(serde_json::to_string(&ContentModel::Other("Scribunto".into())).unwrap(), r#""Scribunto""#);
    }

    #[test]
    fn edit_event_without_content_model_deserializes_as_wikitext() {
        // Serialize an event, strip the field, deserialize — ADR-0009 back-compat.
        let mut value = serde_json::to_value(sample_edit_event()).unwrap(); // no such helper exists yet — create it in this test module (or in the crate's `test_fixtures` module if one is already exported)
        value.as_object_mut().unwrap().remove("content_model");
        let event: EditEvent = serde_json::from_value(value).unwrap();
        assert_eq!(event.content_model, ContentModel::Wikitext);
    }
```

**Step 2: Run to verify failure** — `cargo test -p sp42-platform types` → compile error.

**Step 3: Implement**

```rust
/// A revision's content model (ADR-0016 Decision 1). Per-revision, never per-wiki:
/// wikidata.org carries wikitext (talk pages) and Wikipedias carry non-wikitext
/// (JSON tabs, Scribunto). All content-model routing keys on this value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum ContentModel {
    #[default]
    Wikitext,
    WikibaseItem,
    WikibaseProperty,
    /// Preserved verbatim; routes to the text path with a note (Decision 4).
    Other(String),
}
```

- `parse(&str) -> Self` maps `"wikitext"`, `"wikibase-item"`, `"wikibase-property"`, else `Other(s)`. Implement `From<String>` (calls `parse`) and `From<ContentModel> for String` (inverse, `Other` returns its payload) so serde round-trips the wire string exactly.
- Add to `EditEvent`:

```rust
    #[serde(default)]
    pub content_model: ContentModel,
```

- Fix every `EditEvent { ... }` literal that now misses the field: add `content_model: ContentModel::default()` (or the real value where the constructor has one). There are ~28 construction sites spread across `sp42-live`, `sp42-patrol`, `sp42-reporting`, and platform tests — do not enumerate by grep alone; let `cargo ci-build` (all targets) list them exhaustively and fix all. Do not paper over with `..Default::default()`.

**Step 4: Run** `cargo ci-build` (all targets — surfaces every construction site), then `cargo test -p sp42-platform`.
Expected: compile errors at each `EditEvent` literal, then green after fixing.

**Step 5: Commit**

```bash
git add -A crates/
git commit -m "feat(platform): per-revision ContentModel on EditEvent (ADR-0016 D1)"
```

---

### Task 2: recentchanges ingest populates `content_model` when present

**Files:**
- Modify: `crates/sp42-live/src/recent_changes.rs` (`parse_recent_changes_response`, line ~194, and `build_recent_changes_request` line ~79)

**Step 1: Check what the API offers.** The recentchanges list API does NOT return content models per change (no `rcprop` for it). **Do not invent a fetch.** The honest behavior: recentchanges-sourced `EditEvent`s keep `ContentModel::default()` (wikitext) **unless** the event's wiki + namespace imply otherwise is *also* a guess — so leave default and document:

In `parse_recent_changes_response`, construct the new field as `content_model: ContentModel::default()` with this comment:

```rust
            // The recentchanges list carries no content model; the authoritative
            // per-revision value arrives with the revision-content read
            // (rvprop=contentmodel, ADR-0016 D1) before any content-model routing.
            content_model: ContentModel::default(),
```

**Step 2: Write the test** asserting a parsed fixture event has `ContentModel::Wikitext` (pins the default, guards the comment):

```rust
    #[test]
    fn recentchanges_events_default_to_wikitext_content_model() { ... } // parse fixtures/frwiki_recentchange_edit.json via the existing test path
```

**Step 3: Run tests** — `cargo test -p sp42-live`. **Step 4: Commit** — `feat(live): recentchanges events carry the default content model`

---

### Task 3: revision-content read carries `contentmodel`

**Files:**
- Modify: `crates/sp42-server/src/revision_artifacts.rs` (`fetch_revision_texts` ~line 489, `fetch_revision_text_pair` line 288)

**Step 1: Extend the fetch.** In `fetch_revision_texts`'s query params change `("rvprop", "ids|content")` to `("rvprop", "ids|content|contentmodel")`, and return the model alongside the text: change the return map value from `String` to a small struct:

```rust
pub(crate) struct RevisionSlotContent {
    pub(crate) text: String,
    pub(crate) content_model: ContentModel,
}
```

`fetch_revision_text_pair` returns `Option<(RevisionSlotContent, RevisionSlotContent)>`; its call sites (find them with `grep -n 'fetch_revision_text_pair' crates/sp42-server/src/`) destructure `.text` for now — behavior unchanged for wikitext. Parse `contentmodel` from the slot with `ContentModel::parse`, defaulting to wikitext when absent.

Import note: `sp42-server` does not depend on `sp42-platform` directly — it reaches platform types through `sp42-core`'s re-exports (`revision_artifacts.rs` already does `use sp42_core::{…}`). Import as `sp42_core::ContentModel`; Task 1's platform re-export makes this resolve.

**Step 2: Verify** — `cargo test -p sp42-server` (the existing server tests cover the artifact path with stubbed responses; extend the stub JSON in `crates/sp42-server/src/tests.rs` with `"contentmodel": "wikitext"` where revision slots appear, and add one assertion that a `wikibase-item` slot parses to `ContentModel::WikibaseItem`).

**Step 3: Run the full workspace** — `cargo ci-test`. Expected: green.

**Step 4: Commit**

```bash
git add crates/sp42-server
git commit -m "feat(server): revision-content read carries contentmodel (ADR-0016 D1)"
```

---

**Phase done when:** `cargo ci-test` green workspace-wide, `cargo ci-clippy` clean, and a serialized pre-change `EditEvent` JSON (no `content_model` key) still deserializes (Task 1 test proves it).
