# Bare-URL Repair MVP Implementation Plan — Phase 2: Per-wiki enablement config

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** `WikiTemplates` gains an optional `bare_url_citation` field. Presence (e.g. `"cite web"`) both enables bare-URL repair for that wiki and names the template to render; absence disables the feature. The testwiki fixture enables it; `configs/frwiki.yaml` stays untouched (absent ⇒ disabled).

**Architecture:** One field is both gate and map. **Verified divergence from the design plan:** the design called `citation_needed` "presence-gated", but in reality it is a `String` with a serde default (always present). The *new* field is therefore the first genuinely presence-gated template knob: `Option<String>` with `#[serde(default)]`, `None` ⇒ disabled. There are **no exhaustive `WikiTemplates { ... }` struct literals** anywhere in the workspace (all construction goes through YAML deserialization or `WikiRegistry::default_config()` clones), so only the struct's own `Default` impl needs updating.

**Tech Stack:** serde YAML config parsing in `crates/sp42-wiki/src/config.rs`; struct in `crates/sp42-core/src/types.rs`.

**Scope:** Phase 2 of 7 from `docs/design-plans/2026-06-09-bare-url-repair-mvp.md`. No dependency on Phase 1.

**Codebase verified:** 2026-06-09, branch `louie/bare-url-repair` @ `2ed57b3`.

---

**Working directory for every command:** `/var/home/louie/Projects/Volunteering-Consulting/SP42/.worktrees/bare-url-repair`

### Task 1: Add the `bare_url_citation` field (test-driven)

**Files:**
- Test: `crates/sp42-wiki/src/config.rs` (add tests to the existing `#[cfg(test)] mod tests`)
- Modify: `crates/sp42-core/src/types.rs:420-438` (`WikiTemplates` struct + `Default` impl)
- Modify: `fixtures/testwiki.yaml`

**Step 1: Write the failing tests**

In `crates/sp42-wiki/src/config.rs`, inside the existing `mod tests`, add (model: the existing `parsoid_url_defaults_to_none_when_absent` test at lines 204–219):

```rust
    #[test]
    fn bare_url_citation_parses_when_present() {
        let yaml = r#"
wiki_id: frwiki
display_name: French Wikipedia
api_url: https://fr.wikipedia.org/w/api.php
eventstreams_url: https://stream.wikimedia.org/v2/stream/recentchange
oauth_authorize_url: https://meta.wikimedia.org/w/rest.php/oauth2/authorize
oauth_token_url: https://meta.wikimedia.org/w/rest.php/oauth2/access_token
liftwing_url:
coordination_url:
namespace_allowlist: [0]
scoring_policy_ref: active/frwiki-vandalism
templates:
  citation_needed: "Citation needed"
  bare_url_citation: "cite web"
"#;
        let config = parse_wiki_config(yaml).expect("config with bare_url_citation should parse");
        assert_eq!(config.templates.bare_url_citation.as_deref(), Some("cite web"));
    }

    #[test]
    fn bare_url_citation_defaults_to_none_when_absent() {
        let source = include_str!("../../../configs/frwiki.yaml");
        let config = parse_wiki_config(source).expect("fixture should parse");
        assert_eq!(config.templates.bare_url_citation, None);
    }

    #[test]
    fn testwiki_fixture_enables_bare_url_citation() {
        let source = include_str!("../../../fixtures/testwiki.yaml");
        let config = parse_wiki_config(source).expect("testwiki fixture should parse");
        assert_eq!(config.templates.bare_url_citation.as_deref(), Some("cite web"));
    }
```

**Step 2: Run the tests to verify they fail**

```bash
cargo test -p sp42-wiki bare_url_citation
```

Expected: **compile error** — `no field bare_url_citation on type WikiTemplates`. (Type-driven change: the compile failure is the RED step.)

**Step 3: Add the field**

In `crates/sp42-core/src/types.rs`, the struct currently reads (lines 420–438):

```rust
/// Per-wiki template names for tagging actions. Each value is the short
/// template name without braces (e.g. `"refnec"` → `{{refnec}}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikiTemplates {
    #[serde(default = "default_citation_needed")]
    pub citation_needed: String,
}

impl Default for WikiTemplates {
    fn default() -> Self {
        Self {
            citation_needed: default_citation_needed(),
        }
    }
}
```

Change it to:

```rust
/// Per-wiki template names for tagging actions. Each value is the short
/// template name without braces (e.g. `"refnec"` → `{{refnec}}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikiTemplates {
    #[serde(default = "default_citation_needed")]
    pub citation_needed: String,
    /// Citation template rendered by bare-URL repair (PRD-0008), for example
    /// `"cite web"`. Presence enables the feature for this wiki; when `None`,
    /// the bare-URL routes refuse with `bare-url-repair-not-enabled`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bare_url_citation: Option<String>,
}

impl Default for WikiTemplates {
    fn default() -> Self {
        Self {
            citation_needed: default_citation_needed(),
            bare_url_citation: None,
        }
    }
}
```

**Step 4: Enable testwiki**

In `fixtures/testwiki.yaml`, the templates section currently reads:

```yaml
templates:
  citation_needed: "Citation needed"
```

Change to:

```yaml
templates:
  citation_needed: "Citation needed"
  bare_url_citation: "cite web"
```

**Do not touch `configs/frwiki.yaml`** — its absence of the key is the production-disabled state the second test asserts.

**Step 5: Run the tests to verify they pass**

```bash
cargo test -p sp42-wiki bare_url_citation
```

Expected: 3 tests pass.

**Step 6: Workspace check**

```bash
cargo test -p sp42-core -p sp42-wiki
cargo clippy -p sp42-core -p sp42-wiki --all-targets --all-features -- -D warnings
cargo check -p sp42-server -p sp42-cli
```

Expected: all green — no struct-literal construction sites exist outside the `Default` impl, so nothing else needs updating.

**Step 7: Commit**

```bash
git add crates/sp42-core/src/types.rs crates/sp42-wiki/src/config.rs fixtures/testwiki.yaml
git commit -m "feat: add bare_url_citation per-wiki template gate"
```

### Task 2: Document the knob

**Files:**
- Modify: `docs/platform/RUNTIME_CONFIGURATION.md`

**Step 1: Add the documentation bullet**

In `docs/platform/RUNTIME_CONFIGURATION.md`, under the `SP42_WIKI_CONFIG_DIR` bullet, the existing sub-bullet reads:

```markdown
  - Per-wiki YAML configs may set `parsoid_url` (the wiki's core REST endpoint,
    for example `https://fr.wikipedia.org/w/rest.php`). It enables node-anchored
    content edits (ADR-0003); when unset, those actions refuse with
    `editor-not-configured`.
```

Immediately after it (same indentation level), add:

```markdown
  - Per-wiki YAML configs may set `templates.bare_url_citation` (for example
    `"cite web"`). Its presence enables bare-URL reference repair (PRD-0008)
    and names the citation template SP42 renders; when unset, the
    `/dev/citation/bare-url-*` routes refuse with `bare-url-repair-not-enabled`.
```

**Step 2: Verify doc consistency**

```bash
bash scripts/check-doc-consistency.sh
```

Expected: exits 0 (the script checks for required existing lines; additions are safe).

**Step 3: Commit**

```bash
git add docs/platform/RUNTIME_CONFIGURATION.md
git commit -m "docs: document the bare_url_citation config knob"
```
