# Node-Anchored Wikitext Editing (ADR-0003) — Phase 3: Node-Locator Contract Extension

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** `SessionActionExecutionRequest` gains an optional `node_locator` field (node-kind + document-order ordinal + expected node text) alongside the literal `selected_text` (ADR-0003 Decision 3). This is the protected contract/schema change the ADR records.

**Architecture:** The locator type is `sp42_core::WikitextNodeLocator` from Phase 2 — one authoritative definition (ADR-0004 DRY rule), reused as the wire type. The field is `#[serde(default)] Option<…>`, so every existing client payload (and stored fixture) deserializes unchanged.

**Tech Stack:** serde; no new dependencies.

**Scope:** Phase 3 of 5 from ADR-0003. Requires Phase 2 (the locator type).

**Codebase verified:** 2026-06-09.

---

## Context for the implementer

- `SessionActionExecutionRequest` lives at `crates/sp42-core/src/action_contracts.rs:86-101` (NOT in `types.rs` — the ADR predates the crate split). Wire format notes: `SessionActionKind` serializes with default variant names (`"Rollback"`, `"InlineEdit"`, …); the new locator's `kind` uses kebab-case (`"reference"`/`"template"`) per Phase 2.
- "Protected contract" has no snapshot/schema machinery in this repo — the protection is (a) this ADR as the record, (b) `#[serde(default)]` read-compatibility, (c) the serialization tests in `action_contracts.rs:145-197`.
- Construction sites that must gain `node_locator: None` (struct literals are exhaustive — `cargo check --workspace` will list any site this plan misses; fix all it reports):
  1. `crates/sp42-core/src/review_workbench.rs:121-171` — three literals in `build_session_action_execution_requests` (Rollback/Patrol/Undo).
  2. `crates/sp42-app/src/pages/patrol/action_controller.rs:187-209` — `build_action_request`.
  3. `crates/sp42-core/src/action_contracts.rs:154-165` — the existing serialization test literal.
- The server deserializes the payload in `post_execute_action` (`crates/sp42-server/src/action_routes.rs:50-54`) via `Json<SessionActionExecutionRequest>` — no change needed there in this phase (route behavior changes in Phase 5).

---

## Task 1: Add the field and update all construction sites

**Files:**
- Modify: `crates/sp42-core/src/action_contracts.rs`
- Modify: `crates/sp42-core/src/review_workbench.rs:121-171`
- Modify: `crates/sp42-app/src/pages/patrol/action_controller.rs:187-209`

**Step 1: Write the failing tests**

In the `#[cfg(test)] mod tests` of `crates/sp42-core/src/action_contracts.rs`, extend the `use super::{…}` import with nothing new (the locator types come from the crate root) and add below the existing tests:

```rust
    #[test]
    fn session_action_request_round_trips_node_locator() {
        use crate::wikitext_editor::{WikitextNodeKind, WikitextNodeLocator};

        let request = SessionActionExecutionRequest {
            wiki_id: "frwiki".to_string(),
            kind: SessionActionKind::InlineEdit,
            rev_id: 99,
            title: Some("Exemple".to_string()),
            target_user: None,
            undo_after_rev_id: None,
            summary: None,
            selected_text: None,
            batch_rev_ids: None,
            replacement_text: Some(
                "{{cite web|url=https://example.org|title=Exemple}}".to_string(),
            ),
            node_locator: Some(WikitextNodeLocator {
                kind: WikitextNodeKind::Template,
                ordinal: 2,
                expected_text: "{{cite web|url=https://old.example.org|title=Exemple}}"
                    .to_string(),
            }),
        };

        let json = serde_json::to_string(&request).expect("request should serialize");
        assert!(json.contains("\"node_locator\":{"));
        assert!(json.contains("\"kind\":\"template\""));
        assert!(json.contains("\"ordinal\":2"));

        let parsed: SessionActionExecutionRequest =
            serde_json::from_str(&json).expect("request should deserialize");
        assert_eq!(parsed, request);
    }

    #[test]
    fn session_action_request_deserializes_payload_without_node_locator() {
        let json = r#"{"wiki_id":"frwiki","kind":"Rollback","rev_id":5,"title":null,"target_user":null,"undo_after_rev_id":null,"summary":null}"#;
        let parsed: SessionActionExecutionRequest =
            serde_json::from_str(json).expect("legacy payload should deserialize");
        assert_eq!(parsed.node_locator, None);
        assert_eq!(parsed.selected_text, None);
        assert_eq!(parsed.kind, SessionActionKind::Rollback);
    }
```

**Step 2: Run the tests to verify they fail**

Run: `cargo test -p sp42-core action_contracts`
Expected: compile error — no field `node_locator`.

**Step 3: Add the field**

In `crates/sp42-core/src/action_contracts.rs`:

1. Add the import near the existing ones (after `use crate::types::FlagState;`):

```rust
use crate::wikitext_editor::WikitextNodeLocator;
```

2. Append the field to `SessionActionExecutionRequest` (after `replacement_text`):

```rust
    /// Optional node-anchored target (ADR-0003): when present, content-edit
    /// actions ground on this structural node instead of the literal
    /// `selected_text` span.
    #[serde(default)]
    pub node_locator: Option<WikitextNodeLocator>,
```

**Step 4: Update every construction site**

Run `cargo check --workspace 2>&1 | grep -A2 "missing field"` and add `node_locator: None,` to each reported literal. Known sites:

1. `crates/sp42-core/src/review_workbench.rs` — all three literals in `build_session_action_execution_requests` (after each `replacement_text: None,`).
2. `crates/sp42-core/src/action_contracts.rs` — the literal inside `session_action_contract_serializes_without_token_material` (line ~154).
3. `crates/sp42-app/src/pages/patrol/action_controller.rs` — `build_action_request` (after `replacement_text: None,`).

**Step 5: Run the tests to verify they pass**

Run: `cargo check --workspace` — expect clean.
Run: `cargo test -p sp42-core` — expect all green, including the two new tests.
Run: `cargo build -p sp42-app --target wasm32-unknown-unknown` — the frontend compiles with the extended contract (it constructs the struct; this is the build the CI pipeline also does).

**Step 6: Commit**

```bash
git add crates/sp42-core/src/action_contracts.rs crates/sp42-core/src/review_workbench.rs crates/sp42-app/src/pages/patrol/action_controller.rs
git commit -m "feat(contracts): add optional node_locator to SessionActionExecutionRequest (ADR-0003 D3)"
```

---

## Phase verification

Run: `cargo test -p sp42-core -p sp42-server -p sp42-app && cargo clippy --workspace --all-targets -- -D warnings`
Expected: green. (`sp42-cli` and `sp42-devtools` construct requests only through `build_session_action_execution_requests`, so they pick the change up transitively — the workspace check proves it.)

**Done when:** the contract round-trips `node_locator`, legacy payloads without the field still deserialize, and the whole workspace (including the wasm frontend build) compiles.
