# Wikidata Read Model Implementation Plan — Phase 2: read builders/parsers

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** Pure request builders and response parsers for the three Wikibase reads: entity (optionally at a revision), labels, and the Action-API revision pair carrying `contentmodel`.

**Architecture:** Pure functions over `sp42_types::HttpRequest` following the Citoid precedent (`crates/sp42-citation/src/citation/citoid.rs:59-80`: `build_citoid_request(&str) -> HttpRequest`, `parse_citoid_response(&[u8]) -> Option<...>`) and the recentchanges precedent (`crates/sp42-live/src/recent_changes.rs:79-250`: builder returns `Result<HttpRequest, Error>`, parser takes the response). No I/O; shells inject the `HttpClient`. This phase depends on Phase 1's `parse_entity` only for tests that chain builder→fixture→parser.

**Tech Stack:** Rust, serde_json, `sp42_types::{HttpRequest, HttpMethod}` (verified at `crates/sp42-types/src/transport.rs:17-25`; re-exported from `sp42-platform/src/lib.rs`).

**Scope:** Phase 2 of 7.

**Codebase verified:** 2026-07-01. `HttpRequest { method, url: Url, headers: BTreeMap<String,String>, body: Vec<u8> }`. `StubHttpClient` exists at `crates/sp42-types/src/traits.rs:51-83` for any test that needs an executed round-trip, but builder/parser tests here don't need it — they are pure.

---

### Task 1: Fixtures — labels response and revision-pair response

**Files:**
- Create: `fixtures/wikibase/q42_labels.json`
- Create: `fixtures/wikibase/q42_revision_pair.json`

**Step 1: Create the labels fixture** (a `wbgetentities&props=labels` response)

```json
{
  "entities": {
    "P69": { "type": "property", "id": "P69", "labels": { "en": { "language": "en", "value": "educated at" } } },
    "Q691283": { "type": "item", "id": "Q691283", "labels": { "en": { "language": "en", "value": "St John's College" } } }
  },
  "success": 1
}
```

**Step 2: Create the revision-pair fixture** (an `action=query&prop=revisions&revids=a|b&rvslots=main&rvprop=ids|content|contentmodel&formatversion=2` response; the slot `content` is the entity JSON **as a string**)

```json
{
  "batchcomplete": true,
  "query": {
    "pages": [
      {
        "pageid": 138,
        "ns": 0,
        "title": "Q42",
        "revisions": [
          {
            "revid": 2000341,
            "parentid": 2000200,
            "slots": {
              "main": {
                "contentmodel": "wikibase-item",
                "contentformat": "application/json",
                "content": "{\"type\":\"item\",\"id\":\"Q42\",\"labels\":{\"en\":{\"language\":\"en\",\"value\":\"Douglas Adams\"}},\"descriptions\":{},\"aliases\":{},\"claims\":{},\"sitelinks\":{}}"
              }
            }
          },
          {
            "revid": 2000200,
            "parentid": 1999990,
            "slots": {
              "main": {
                "contentmodel": "wikibase-item",
                "contentformat": "application/json",
                "content": "{\"type\":\"item\",\"id\":\"Q42\",\"labels\":{\"en\":{\"language\":\"en\",\"value\":\"Douglas N. Adams\"}},\"descriptions\":{},\"aliases\":{},\"claims\":{},\"sitelinks\":{}}"
              }
            }
          }
        ]
      }
    ]
  }
}
```

**Step 3: Validate JSON, commit**

Run: `python3 -m json.tool fixtures/wikibase/q42_labels.json > /dev/null && python3 -m json.tool fixtures/wikibase/q42_revision_pair.json > /dev/null && echo OK`
Expected: `OK`

```bash
git add fixtures/wikibase/
git commit -m "test(wikibase): labels + revision-pair fixtures"
```

---

### Task 2: Entity + label request builders and label parser

**Files:**
- Create: `crates/sp42-platform/src/wikibase/read.rs`
- Modify: `crates/sp42-platform/src/wikibase/mod.rs` (add `mod read;` + `pub use read::{build_entity_request, build_label_request, parse_labels, Labels};`)

**Step 1: Write the failing tests** (inline module in `read.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::wikibase::EntityId;

    #[test]
    fn entity_request_targets_entitydata_current() {
        let req = build_entity_request(&EntityId::new("Q42"), None);
        assert_eq!(req.url.as_str(), "https://www.wikidata.org/wiki/Special:EntityData/Q42.json");
    }

    #[test]
    fn entity_request_pins_a_revision() {
        let req = build_entity_request(&EntityId::new("Q42"), Some(2_000_200));
        assert_eq!(
            req.url.as_str(),
            "https://www.wikidata.org/wiki/Special:EntityData/Q42.json?revision=2000200"
        );
    }

    #[test]
    fn label_request_batches_ids() {
        let req = build_label_request(&["P69", "Q691283"], "en");
        let url = req.url.as_str();
        assert!(url.starts_with("https://www.wikidata.org/w/api.php?"));
        for needle in ["action=wbgetentities", "ids=P69%7CQ691283", "props=labels", "languages=en", "format=json"] {
            assert!(url.contains(needle), "missing {needle} in {url}");
        }
    }

    #[test]
    fn parses_labels() {
        let labels = parse_labels(include_str!("../../../../fixtures/wikibase/q42_labels.json").as_bytes())
            .expect("parses");
        assert_eq!(labels.get("P69"), Some("educated at"));
        assert_eq!(labels.get("Q691283"), Some("St John's College"));
        assert_eq!(labels.get("Q1"), None);
    }
}
```

**Step 2: Run to verify failure**

Run: `cargo test -p sp42-platform wikibase::read`
Expected: compile error (functions not defined).

**Step 3: Implement**

```rust
use std::collections::BTreeMap;

use serde_json::Value;
use sp42_types::{HttpMethod, HttpRequest};
use url::Url;

use super::model::EntityId;
use super::parse::WikibaseParseError;

/// Keyless entity read, optionally pinned to a revision (`Special:EntityData?revision=`).
/// `revision: None` = current (what PR #103's verb uses); `Some(rev)` = e.g. the parent, for a diff.
#[must_use]
pub fn build_entity_request(id: &EntityId, revision: Option<u64>) -> HttpRequest { ... }

/// `wbgetentities&props=labels` — resolve property/item ids to display labels.
#[must_use]
pub fn build_label_request(ids: &[&str], lang: &str) -> HttpRequest { ... }

/// Labels keyed by entity/property id, in the requested language.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Labels(BTreeMap<String, String>);

impl Labels {
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&str> { self.0.get(id).map(String::as_str) }
}

pub fn parse_labels(body: &[u8]) -> Result<Labels, WikibaseParseError> { ... }
```

Implementation notes:
- Follow the Citoid builder shape: construct `Url::parse(...)` / `Url::parse_with_params`, `HttpMethod::Get`, empty headers/body. Match exactly how `build_citoid_request` constructs `HttpRequest` (same field initialization style).
- Base host: hardcode `www.wikidata.org` for now — this matches #103's shell behavior (its `entity_data_url`/`labels_url` also target wikidata.org). Add a `// TODO` is NOT allowed; instead add a doc sentence: test-wikidata support arrives with the patrol wiring, which passes a host — so give both builders a `host: &str` FIRST parameter… **Decision:** keep the design-plan signature (no host param) and target `www.wikidata.org`; the patrol phase can generalize when it has a `WikiConfig` in hand. Document this in the rustdoc ("Wikidata.org; test instances arrive with patrol wiring").
- `parse_labels`: walk `entities.{id}.labels.{lang}.value` for every entity in the response; first (only) language wins — store `id -> value`.

**Step 4: Run tests to verify pass**

Run: `cargo test -p sp42-platform wikibase`
Expected: Phase-1 tests + 4 new pass.

**Step 5: Commit**

```bash
git add crates/sp42-platform/src/wikibase
git commit -m "feat(wikibase): entity + label read builders and label parser"
```

---

### Task 3: Revision-pair builder/parser (`contentmodel`-aware)

**Files:**
- Modify: `crates/sp42-platform/src/wikibase/read.rs`
- Modify: `crates/sp42-platform/src/wikibase/mod.rs` (export `build_revision_pair_request, parse_revision_contents, RevisionContent`)

**Step 1: Write the failing tests**

```rust
    #[test]
    fn revision_pair_request_asks_for_ids_content_and_model() {
        let req = build_revision_pair_request("https://www.wikidata.org/w/api.php", &[2_000_200, 2_000_341]);
        let url = req.url.as_str();
        for needle in [
            "action=query", "prop=revisions", "revids=2000200%7C2000341",
            "rvslots=main", "rvprop=ids%7Ccontent%7Ccontentmodel", "formatversion=2",
        ] {
            assert!(url.contains(needle), "missing {needle} in {url}");
        }
    }

    #[test]
    fn parses_revision_contents_with_model() {
        let revs = parse_revision_contents(
            include_str!("../../../../fixtures/wikibase/q42_revision_pair.json").as_bytes(),
        )
        .expect("parses");
        assert_eq!(revs.len(), 2);
        let new = revs.iter().find(|r| r.revid == 2_000_341).unwrap();
        assert_eq!(new.content_model, "wikibase-item");
        // The slot content chains into Phase 1's endpoint-agnostic parser:
        let entity = crate::wikibase::parse_entity(&EntityId::new("Q42"), new.content.as_bytes()).unwrap();
        assert_eq!(entity.labels.get("en").map(String::as_str), Some("Douglas Adams"));
    }
```

**Step 2: Run to verify failure** — `cargo test -p sp42-platform wikibase::read` → compile error.

**Step 3: Implement**

```rust
/// One revision's main-slot content as returned by `prop=revisions`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionContent {
    pub revid: u64,
    pub parentid: Option<u64>,
    pub content_model: String,
    pub content: String,
}

/// Action-API read of specific revisions' main-slot content **and content model**
/// (ADR-0016 Decisions 1–2): one call returns both sides of a diff plus the
/// routing key. `api_endpoint` comes from the caller's `WikiConfig` (any wiki —
/// this read is not Wikidata-specific).
#[must_use]
pub fn build_revision_pair_request(api_endpoint: &str, revids: &[u64]) -> HttpRequest { ... }

pub fn parse_revision_contents(body: &[u8]) -> Result<Vec<RevisionContent>, WikibaseParseError> { ... }
```

Notes:
- Builder takes the endpoint (unlike Task 2's builders) because patrol calls this for *any* wiki — the caller has a `WikiConfig` with the API URL. Query params: `action=query, format=json, formatversion=2, prop=revisions, revids=<joined with |>, rvslots=main, rvprop=ids|content|contentmodel`.
- Parser: `query.pages[].revisions[]`, flattened across pages; missing slot/content → skip that revision (a deleted/suppressed revision has no content — returning fewer entries than asked is the honest shape; callers check).

**Step 4: Run tests** — expect all wikibase tests green.

**Step 5: Lint + commit**

Run: `cargo ci-clippy`

```bash
git add crates/sp42-platform/src/wikibase
git commit -m "feat(wikibase): contentmodel-aware revision-pair read (ADR-0016 D1/D2)"
```

---

**Phase done when:** `cargo test -p sp42-platform` green (Phases 1–2 tests), `cargo ci-clippy` clean. The chain fixture→builder→parser→`parse_entity` proves endpoint-agnosticism end to end without network (ADR-0009).
