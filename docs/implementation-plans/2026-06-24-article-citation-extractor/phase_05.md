# Article Citation Extractor Implementation Plan — Phase 5: Parsoid Block Extraction

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** Implement the editor's one DOM pass: Parsoid HTML → `Vec<ParsoidBlock>`, with ref markers stripped (offsets recorded), heading stacks captured, and source URLs read structurally.

**Architecture:** New `extract_blocks` method on the `WikitextEditor` trait, implemented on `ParsoidWikitextEditor` in `sp42-server`. The `!Send` kuchikiki DOM stays confined to this synchronous pass (no DOM value crosses `.await`), exactly like `enumerate_nodes`. This is the only component needing real-DOM fixtures.

**Tech Stack:** `parsoid` crate v0.10.1 (`Wikicode`, `Wikinode`, `filter_references`, `descendants`/`children`, `select`, `iter_sections`), `serde_json` for `data-mw`, `url::Url`.

**Scope:** Phase 5 of 6.

**Codebase verified:** 2026-06-24
- `WikitextEditor` is `#[async_trait]` with `async fn enumerate_nodes(&self, config: &WikiConfig, page: &WikitextPageRef, kind) -> Result<…, WikitextEditorError>` (`wikitext_editor.rs:275`). New method mirrors this.
- `ParsoidWikitextEditor` (zero-sized) impls the trait via `#[async_trait]` (`parsoid_editor.rs:292`); gets a `parsoid::Client` from `editor_client(config)` (`parsoid_editor.rs:34`); fetches via `client.get_revision(&page.title, page.rev_id).await` → `ImmutableWikicode`; builds DOM with `Wikicode::new(revision.html())` (`parsoid_editor.rs:98`).
- Existing structured-node helpers in `parsoid_editor.rs`: `template.params()`, `element.attributes.borrow().get("data-mw")` (lines 77, 165–186).

**External dependency findings (parsoid v0.10.1, gitlab.wikimedia.org/repos/mwbot-rs/mwbot):**
- `Wikicode`/`Wikinode` impl `WikinodeIterator`: `.descendants()`, `.children()`, `.select(&str)`, `.select_first(&str)`, `.text_contents()`. `Wikinode` derefs to kuchikiki `NodeRef`; `.as_node()` → `&NodeRef`, then `.as_element()` / `.as_text()`.
- `Wikinode` enum variants incl. `ReferenceLink`, `ExtLink`, `Heading`, `Section`, `Generic`. Type guards `.as_reference_link()`, `.as_extlink()`, `.as_heading()`.
- `ReferenceLink`: `.id() -> String`, `.name() -> Result<Option<String>>`, `.reference_id() -> Result<String>`, `.is_reused() -> Result<bool>`.
- `Reference` (from `code.filter_references()`): `.id() -> String`, `.contents() -> Wikinode`.
- `ExtLink`: `.target() -> String` (the href).
- `Heading`: `.level() -> u32`, `.text_contents()`.
- Cite-template params inside a reference: `reference.contents().select("span[typeof~=\"mw:Transclusion\"]")`, read `data-mw` JSON (`parts[].template.params.url.wt`, `archive-url.wt`).

---

## Task 1: Add `extract_blocks` to the trait (with a default stub) + a no-op default

**Files:**
- Modify: `crates/sp42-core/src/wikitext_editor.rs` (trait, ~line 275)

**Step 1: Add the method to the trait**

Inside the `#[async_trait] pub trait WikitextEditor` block, add — **with a default body**:

```rust
    /// Extract prose-bearing blocks (paragraphs, list items, table cells) in
    /// document order, with inline ref markers removed from text but their byte
    /// offsets, ids, and source URLs recorded. Read-only.
    ///
    /// Defaults to "no blocks": only the Parsoid production editor understands
    /// page structure, so non-Parsoid impls (scripted/test fakes) inherit the
    /// empty default and need no changes.
    async fn extract_blocks(
        &self,
        config: &WikiConfig,
        page: &WikitextPageRef,
    ) -> Result<Vec<ParsoidBlock>, WikitextEditorError> {
        let _ = (config, page);
        Ok(Vec::new())
    }
```

The default body is load-bearing: there are **three** in-tree impls — `ParsoidWikitextEditor` (overridden in Task 3), plus `ScriptedWikitextEditor` (`crates/sp42-core/src/wikitext_editor.rs`) and `FailingEditor` (`crates/sp42-server/src/citation_routes.rs` test module). The default lets the latter two compile untouched. (`#[async_trait]` supports default method bodies.)

**Step 2: Verify both crates build, including test targets**

Run: `PATH="$HOME/.cargo/bin:$PATH" cargo build -p sp42-core -p sp42-server --all-targets`
Expected: builds. `--all-targets` ensures the `FailingEditor` test fake in `sp42-server` compiles against the new trait method now, not in Phase 6.

**Step 3: Commit**

```bash
git add crates/sp42-core/src/wikitext_editor.rs
SP42_SKIP_GIT_HOOKS=1 git commit -m "feat(citation): add extract_blocks to WikitextEditor trait

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_015NtcEzaj8J373GGsv3FC7y"
```

---

## Task 2: Capture a committed Parsoid HTML fixture

**Files:**
- Create: `crates/sp42-server/tests/fixtures/parsoid_cats.html` (small, hand-trimmed real sample)

**Step 1: Fetch a real Parsoid HTML sample and trim it**

Run (network; pick a small stable article):

```bash
mkdir -p crates/sp42-server/tests/fixtures
curl -s "https://en.wikipedia.org/api/rest_v1/page/html/Cat" -o /tmp/cat.html
```

Open `/tmp/cat.html`, and copy into `crates/sp42-server/tests/fixtures/parsoid_cats.html` a trimmed excerpt that contains: a `<section data-mw-section-id>` with an `<h2>`/`<h3>` heading, at least one `<p>` whose text ends with a `<sup class="mw-ref" typeof="mw:Extension/ref">` marker backed by a `{{cite}}`-style reference (so `data-mw` carries `url=`), and at least one bare-URL ref, plus the trailing `<ol class="mw-references">` reference list those markers point into. Keep the `<html><head><base/></head><body>…` envelope intact so the parsoid crate parses it. Target < 200 lines.

**Step 2: Sanity-check it parses**

Add a throwaway test (delete after) or use the Task 3 test to confirm `Wikicode::new(html)` + `filter_references()` returns ≥ 1 reference. If the trimmed fixture loses the markers' linkage, re-trim with more of the reference list included.

**Step 3: Commit the fixture**

```bash
git add crates/sp42-server/tests/fixtures/parsoid_cats.html
SP42_SKIP_GIT_HOOKS=1 git commit -m "test(citation): add Parsoid HTML fixture for block extraction

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_015NtcEzaj8J373GGsv3FC7y"
```

---

## Task 3: Implement `extract_blocks` on `ParsoidWikitextEditor`

**Files:**
- Modify: `crates/sp42-server/src/parsoid_editor.rs`

**Step 1: Write the failing fixture test**

Add to `parsoid_editor.rs` (or a sibling test module). Drive the pure DOM→blocks routine directly (factor the DOM walk into a free function `blocks_from_revision(&ImmutableWikicode) -> Result<Vec<ParsoidBlock>, WikitextEditorError>` so it is testable without network — mirrors how `enumerate_revision` is a free fn):

```rust
#[cfg(test)]
mod extract_tests {
    use super::*;

    fn fixture() -> parsoid::ImmutableWikicode {
        let html = include_str!("../tests/fixtures/parsoid_cats.html");
        parsoid::ImmutableWikicode::new(html)
    }

    #[test]
    fn extracts_blocks_with_section_refs_and_urls() {
        let blocks = blocks_from_revision(&fixture()).expect("blocks");
        assert!(!blocks.is_empty(), "should find prose blocks");

        // At least one block has a heading stack.
        assert!(blocks.iter().any(|b| !b.section_path.is_empty()));

        // At least one ref with an extracted URL, and its offset lands within
        // (or at the end of) the cleaned block text.
        let with_url = blocks
            .iter()
            .flat_map(|b| b.refs.iter().map(move |r| (b, r)))
            .find(|(_, r)| !r.source_urls.is_empty())
            .expect("a ref with a URL");
        let (block, r) = with_url;
        assert!(r.offset <= block.text.len(), "offset within text bounds");

        // Markers are stripped: the cleaned text should not contain "[1]"-style
        // bracketed cue if the fixture used them (skip if not applicable).
    }
}
```

**Step 2: Run to verify it fails**

Run: `PATH="$HOME/.cargo/bin:$PATH" cargo test -p sp42-server extracts_blocks_with_section_refs_and_urls -- --exact`
Expected: FAIL (`blocks_from_revision` not yet defined).

**Step 3: Implement the DOM walk**

Add the imports and helpers. The trait method just fetches and delegates:

```rust
use sp42_core::{BlockKind, BlockRef, ParsoidBlock};
use std::collections::HashMap;

// In `impl WikitextEditor for ParsoidWikitextEditor`:
async fn extract_blocks(
    &self,
    config: &WikiConfig,
    page: &WikitextPageRef,
) -> Result<Vec<ParsoidBlock>, WikitextEditorError> {
    let client = editor_client(config)?;
    let revision = client
        .get_revision(&page.title, page.rev_id)
        .await
        .map_err(map_parsoid_error)?;   // reuse the existing error mapper
    blocks_from_revision(&revision)
}
```

Free function (no `await`; DOM is local):

```rust
fn blocks_from_revision(
    revision: &parsoid::ImmutableWikicode,
) -> Result<Vec<ParsoidBlock>, WikitextEditorError> {
    let code = Wikicode::new(revision.html());

    // Map Reference.id() -> source URLs, read structurally from cite templates
    // and bare ExtLinks inside each reference's contents.
    let mut ref_urls: HashMap<String, Vec<url::Url>> = HashMap::new();
    for reference in code.filter_references() {
        ref_urls.insert(reference.id(), urls_in_reference(&reference));
    }

    let mut blocks = Vec::new();
    let mut ordinal = 0usize;
    let mut heading_stack: Vec<(u32, String)> = Vec::new();
    walk(&code, &mut heading_stack, &ref_urls, &mut blocks, &mut ordinal);
    Ok(blocks)
}
```

Recursive walker — track headings, emit blocks, don't recurse into a block once emitted:

```rust
fn walk(
    node: &impl WikinodeIterator,
    headings: &mut Vec<(u32, String)>,
    ref_urls: &HashMap<String, Vec<url::Url>>,
    blocks: &mut Vec<ParsoidBlock>,
    ordinal: &mut usize,
) {
    for child in node.children() {
        if let Some(heading) = child.as_heading() {
            let level = heading.level();
            while headings.last().map(|(l, _)| *l >= level).unwrap_or(false) {
                headings.pop();
            }
            headings.push((level, child.text_contents().trim().to_string()));
            continue;
        }
        if let Some(kind) = block_kind(&child) {
            let section_path = headings.iter().map(|(_, t)| t.clone()).collect();
            blocks.push(build_block(&child, kind, section_path, *ordinal, ref_urls));
            *ordinal += 1;
            continue; // do not descend into an emitted block
        }
        walk(&child, headings, ref_urls, blocks, ordinal);
    }
}
```

Block detection by tag:

```rust
fn block_kind(node: &Wikinode) -> Option<BlockKind> {
    let element = node.as_node().as_element()?;
    match element.name.local.as_ref() {
        "p" => Some(BlockKind::Paragraph),
        "li" | "dd" => Some(BlockKind::ListItem),
        "td" | "th" | "caption" => Some(BlockKind::TableCell),
        _ => None,
    }
}
```

Build one block — ordered child traversal, skipping ref-marker internals and recording offsets:

```rust
fn build_block(
    node: &Wikinode,
    kind: BlockKind,
    section_path: Vec<String>,
    ordinal: usize,
    ref_urls: &HashMap<String, Vec<url::Url>>,
) -> ParsoidBlock {
    let mut text = String::new();
    let mut refs = Vec::new();
    collect_block(node, &mut text, &mut refs, ref_urls);
    ParsoidBlock {
        text: text.trim().to_string(),
        section_path,
        refs,
        block_kind: kind,
        block_ordinal: ordinal,
    }
}

fn collect_block(
    node: &impl WikinodeIterator,
    text: &mut String,
    refs: &mut Vec<BlockRef>,
    ref_urls: &HashMap<String, Vec<url::Url>>,
) {
    for child in node.children() {
        if let Some(ref_link) = child.as_reference_link() {
            let reference_id = ref_link.reference_id().unwrap_or_default();
            let source_urls = ref_urls.get(&reference_id).cloned().unwrap_or_default();
            refs.push(BlockRef {
                offset: text.len(),
                ref_id: ref_link.id(),
                source_urls,
                ref_text: child.text_contents(),
                named: ref_link.name().ok().flatten().is_some(),
            });
            continue; // skip the marker's own text
        }
        // A text node: append its text.
        if let Some(text_ref) = child.as_node().as_text() {
            text.push_str(&text_ref.borrow());
            continue;
        }
        // Any other element: recurse so we keep inline formatting text and catch
        // nested ref markers in order.
        collect_block(&child, text, refs, ref_urls);
    }
}
```

URL extraction from a reference's contents:

```rust
fn urls_in_reference(reference: &parsoid::Reference) -> Vec<url::Url> {
    let contents = reference.contents();
    let mut out = Vec::new();

    // Cite-template params via data-mw.
    for span in contents.select("span[typeof~=\"mw:Transclusion\"]") {
        if let Some(element) = span.as_node().as_element() {
            if let Some(data_mw) = element.attributes.borrow().get("data-mw") {
                push_template_urls(data_mw, &mut out);
            }
        }
    }
    // Bare ExtLinks.
    for node in contents.descendants() {
        if let Some(extlink) = node.as_extlink() {
            if let Ok(u) = url::Url::parse(&extlink.target()) {
                out.push(u);
            }
        }
    }
    out.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    out.dedup();
    out
}

fn push_template_urls(data_mw: &str, out: &mut Vec<url::Url>) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(data_mw) else { return };
    let Some(parts) = value.get("parts").and_then(|p| p.as_array()) else { return };
    for part in parts {
        let Some(params) = part.pointer("/template/params") else { continue };
        for key in ["url", "archive-url", "archiveurl"] {
            if let Some(wt) = params.pointer(&format!("/{key}/wt")).and_then(|v| v.as_str()) {
                if let Ok(u) = url::Url::parse(wt.trim()) {
                    out.push(u);
                }
            }
        }
    }
}
```

Executor notes:
- Confirm the exact import path for `WikinodeIterator`, `Wikinode`, `Reference`, and the `as_*` guards against the parsoid crate's actual API (`cargo doc -p parsoid --open`, or `rg` in `~/.cargo/registry/src/*/parsoid-0.10.*/src`). The method names above are from the v0.10.1 source survey but verify before relying on them; adjust if e.g. `reference_id()` is named differently or returns a different shape.
- `map_parsoid_error` is illustrative — reuse whatever error mapper the existing trait methods use (e.g. the closure in `enumerate_nodes` that builds `WikitextEditorError::Unavailable`). If there's no named helper, inline the same `.map_err(|error| WikitextEditorError::Unavailable { … })` form used at `parsoid_editor.rs:300`.
- `ImmutableWikicode::new` is used in the existing test helper `revision()` (`parsoid_editor.rs:387`); reuse that for the fixture loader if present.

**Step 4: Run the fixture test**

Run: `PATH="$HOME/.cargo/bin:$PATH" cargo test -p sp42-server extracts_blocks_with_section_refs_and_urls -- --exact`
Expected: PASS. If it fails on a parsoid API mismatch, fix the method names per the doc survey and re-run.

**Step 5: clippy, fmt, commit**

```bash
PATH="$HOME/.cargo/bin:$PATH" cargo clippy -p sp42-server --all-targets -- -D warnings && cargo fmt -p sp42-server
git add crates/sp42-server/src/parsoid_editor.rs
SP42_SKIP_GIT_HOOKS=1 git commit -m "feat(citation): Parsoid DOM block extraction (extract_blocks)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_015NtcEzaj8J373GGsv3FC7y"
```

**Done when:** `extract_blocks` returns blocks with heading stacks, ref offsets within text bounds, markers stripped from text, and URLs read structurally from cite-template `data-mw` and bare ExtLinks; the fixture test passes; clippy clean. No DOM value crosses an `.await`.
