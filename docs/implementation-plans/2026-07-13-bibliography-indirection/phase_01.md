# Bibliography Indirection Implementation Plan — Phase 1: parsoid mechanism

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** `sp42-parsoid` resolves shortened-footnote refs to bibliography book identifiers (lane A) and extracts ref-local ISBN magiclinks/`{{ISBN}}` transclusions (lane B), per `docs/design-plans/2026-07-13-bibliography-indirection.md` and the PRD-0009 Layer-1 amendment.

**Architecture:** All new code in `crates/sp42-parsoid/src/lib.rs`, upstream of the existing book lane: `BlockRef.book_sources` gains entries; downstream (extract → resolve → ground → report) is untouched in this phase. A document-level bibliography index is built once in `blocks_from_revision` (the whole DOM is already parsed there) and threaded into `sources_in_reference`.

**Tech Stack:** kuchikiki DOM via the `parsoid` crate (CSS `select`, `descendants()`, `attributes.borrow()` — all already used in this file); `serde_json` for data-mw; `percent-encoding` (workspace dep) for fragment decoding.

**Scope:** Phase 1 of 2.

**Codebase verified:** 2026-07-13 (codebase-investigator). Key facts:
- `blocks_from_revision` (`lib.rs:97-118`) parses the whole document once (`Wikicode::new(revision.html())`) then builds `ref_sources: HashMap<String, RefSources>` by iterating `code.filter_references()`, calling `sources_in_reference(&reference)` (`lib.rs:241-293`).
- `sources_in_reference` walks `reference.contents()`: transclusions via `contents.select("[typeof~=\"mw:Transclusion\"]")` reading `data-mw`, then `contents.descendants()` for ExtLinks. `push_template_sources` (`lib.rs:352-414`) parses data-mw `parts[].template.params`; `template_book_source` (`lib.rs:421-452`) maps `isbn/ISBN`, `oclc/OCLC`, `lccn/LCCN`, `ol/OL` params through the checksum-validating `BookIdentifier` constructors and reads `cited_page` from `page/p/pages/pp`.
- `BookSource { identifiers, cited_page }` and `BookIdentifier` (validated constructors) — `crates/sp42-platform/src/wikitext_editor.rs:96-242`. `BlockRef` carries `book_sources: Vec<BookSource>` and the additive-bool precedent `is_bare_url_ref`.
- Fixtures: `crates/sp42-parsoid/tests/fixtures/*.html` loaded with `include_str!` (house pattern: small trimmed fragments; see `parsoid_cats.html` + `fn fixture()` at `lib.rs:459`).
- No existing id-index or fragment-link following anywhere.
- Probe HTML for fixture trimming (raw, saved 2026-07-13): `/tmp/claude-1000/-var-home-louie-Projects-Volunteering-Consulting-SP42/71bc0d68-0068-4ff4-be8d-41dcf404f480/scratchpad/parsoid-probe/` (`enwiki_eurovision1973.html`, `frwiki_Château_de_Chambord.html`, `dewiki_tannenberg.html`).

**Verified DOM shapes the code must handle (from the live probes):**
- enwiki: ref `<sup>`'s data-mw: `{"parts":[{"template":{"target":{"wt":"sfn",…},"params":{"1":{"wt":"Roxburgh"},"2":{"wt":"2014"},"pp":{"wt":"113–116"}}}}]}`; ref body contains `<a href="./<Title>#CITEREFRoxburgh2014" class="mw-selflink-fragment">`; bibliography `<cite id="CITEREFRoxburgh2014" about="#mwt981">` where the `{{Cite book}}` data-mw (with `isbn` param) lives on the transclusion wrapper sharing that `about`.
- frwiki: anchor fragment has NO CITEREF prefix (`#Martin-Demézil1986`, percent-encoded in hrefs — decode before matching); `<span class="ouvrage" id="Martin-Demézil1986" data-mw='…{"target":{"wt":"Ouvrage"},"params":{…"isbn":{"wt":"…"}}}'>` — data-mw directly ON the id-bearing element.
- dewiki: `<a href="./Special:BookSources/3486575813" class="internal mw-magiclink mw-magiclink-isbn">ISBN 3-486-57581-3</a>` — normalized digits in the href's last segment.

---

## Task 1: Trimmed fixtures from the probe HTML (infrastructure)

**Files:**
- Create: `crates/sp42-parsoid/tests/fixtures/parsoid_sfn_enwiki.html`
- Create: `crates/sp42-parsoid/tests/fixtures/parsoid_harvsp_frwiki.html`
- Create: `crates/sp42-parsoid/tests/fixtures/parsoid_magiclink_dewiki.html`

**Step 1: Create the fixtures.** Trim from the probe files (paths in the header) — each fixture is a minimal well-formed Parsoid fragment (< ~80 lines) in the `parsoid_cats.html` style (wrap in `<html><body>…</body></html>` matching that fixture's framing — read it first and mirror exactly):

- `parsoid_sfn_enwiki.html`: one paragraph with an `{{sfn}}` ref (`<sup typeof="mw:Extension/ref" data-mw='…sfn…"params":{"1":{"wt":"Roxburgh"},"2":{"wt":"2014"},"pp":{"wt":"113–116"}}…'>`), the references list containing that footnote's `<li>` whose body carries `<a href="./Page#CITEREFRoxburgh2014" class="mw-selflink-fragment">Roxburgh 2014</a>, pp. 113–116.`, and a Bibliography `<li>` containing the transclusion wrapper (with `about="#mwt981"` and the `{{Cite book}}` data-mw incl. `"isbn":{"wt":"978-1-84583-093-9"}`) wrapping `<cite id="CITEREFRoxburgh2014" about="#mwt981">…</cite>`. ALSO include a second `{{sfn}}` ref targeting `#CITEREFNowhere2020` with no matching bibliography element (the unresolved case), and a third ref that is a plain `{{cite book}}` inline (regression: direct extraction must be unaffected).
- `parsoid_harvsp_frwiki.html`: one `{{harvsp}}` ref whose body link is `<a href="./Page#Martin-Dem%C3%A9zil1986">` (keep the percent-encoding — that is the decode test) plus `<span class="ouvrage" id="Martin-Demézil1986" data-mw='…"target":{"wt":"Ouvrage"}…"isbn":{"wt":"978-2-85822-660-3"}…'>` (data-mw on the id element itself, no about-indirection).
- `parsoid_magiclink_dewiki.html`: one ref whose body is free text containing `<a href="./Special:BookSources/3486575813" class="internal mw-magiclink mw-magiclink-isbn">ISBN 3-486-57581-3</a>` — BUT check the probe file for the actual dewiki href form first (`grep -o 'href="[^"]*BookSources[^"]*"' dewiki_tannenberg.html | head -3` and `grep -o 'href="[^"]*ISBN[^"]*"' …`); use exactly what Parsoid emits (the probe report showed `./Special:BookSources/3486575813` — verify and use that). Include a second magiclink with an ISBN-13.

Copy attribute spellings (typeof, rel, class, about) verbatim from the probe files — invented attribute shapes are how fixture tests lie.

**Step 2: Verify** — each file parses: `ImmutableWikicode::new(include_str!(...))` in a scratch test, or just proceed (Task 2's failing tests exercise them).

**Step 3: Commit** — `test(parsoid): en/fr/de bibliography-indirection fixtures from live probes`

---

## Task 2: Bibliography index

**Files:**
- Modify: `crates/sp42-parsoid/src/lib.rs`

A document-level `BiblioIndex`: DOM `id` → `BookSource`, built once in `blocks_from_revision`.

**Step 1: Write the failing tests** (in the existing test module, mirroring `fn fixture()`):

```rust
#[test]
fn biblio_index_maps_citeref_ids_to_book_sources() {
    let html = include_str!("../tests/fixtures/parsoid_sfn_enwiki.html");
    let code = Wikicode::new(html);
    let index = biblio_index(&code);
    let source = index.get("CITEREFRoxburgh2014").expect("indexed");
    assert_eq!(
        source.identifiers,
        vec![BookIdentifier::isbn("978-1-84583-093-9").expect("valid isbn")]
    );
}

#[test]
fn biblio_index_reads_data_mw_on_the_id_element_itself() {
    // frwiki: {{Ouvrage}} puts data-mw directly on the id-bearing span,
    // and the id has no CITEREF prefix.
    let html = include_str!("../tests/fixtures/parsoid_harvsp_frwiki.html");
    let code = Wikicode::new(html);
    let index = biblio_index(&code);
    assert!(index.get("Martin-Demézil1986").is_some());
}

#[test]
fn biblio_index_ignores_idless_and_bookless_elements() {
    let html = include_str!("../tests/fixtures/parsoid_cats.html");
    let code = Wikicode::new(html);
    assert!(biblio_index(&code).is_empty());
}
```

**Step 2: Run to verify failure** — `cargo test -p sp42-parsoid` → compile error.

**Step 3: Implement.**

```rust
/// Document-level index of bibliography-entry book sources, keyed by DOM id
/// (the `#CITEREF…`-style anchor targets of shortened footnotes; frwiki ids
/// carry no CITEREF prefix, so keys are stored verbatim).
type BiblioIndex = std::collections::HashMap<String, BookSource>;

fn biblio_index(code: &Wikicode) -> BiblioIndex { … }
```

Algorithm (two association shapes, verified against the probes):
1. Walk every transclusion element (`code.select("[typeof~=\"mw:Transclusion\"]")`). For each with a `data-mw` whose parts yield a `BookSource` (reuse `template_book_source` over each part's params — factor the small parts-iteration loop out of `push_template_sources` if needed rather than duplicating):
   a. If the element itself has an `id`, index it (frwiki shape).
   b. Else, note its `about` value; after the pass, walk all elements carrying an `id` AND an `about` seen in step (a)'s map and index them (enwiki shape: `<cite id=… about=#mwtN>` inside the wrapper's output). Implementation note: a single `descendants()` pass collecting `(id, about)` pairs plus the about→BookSource map is O(n) and avoids re-querying.
2. Magiclink-bearing id elements: any element with an `id` whose descendants include `a.mw-magiclink-isbn` links (see Task 4's `magiclink_isbns` helper — implement that helper in THIS task since both need it) gets a `BookSource { identifiers: <those isbns>, cited_page: None }` — only when step 1 gave that id nothing (template params win).
3. Skip `id`s that already collide: first writer wins, matching MediaWiki's duplicate-anchor behavior.

`magiclink_isbns(node) -> Vec<BookIdentifier>` helper: for descendant elements with class containing `mw-magiclink-isbn`, take the href's final path segment (`…/BookSources/<digits>`), strip, and validate via `BookIdentifier::isbn` (checksummed — invalid digits drop silently).

**Step 4: Run tests** — pass. **Step 5: Commit** — `feat(parsoid): document-level bibliography book-source index`

---

## Task 3: Lane A — short-cite detection and link-following

**Files:**
- Modify: `crates/sp42-parsoid/src/lib.rs`

**Step 1: Write the failing tests:**

```rust
#[test]
fn sfn_ref_resolves_to_the_bibliography_book_source_with_its_own_pages() {
    let blocks = blocks_from_fixture("parsoid_sfn_enwiki.html");
    let r = /* the sfn ref's BlockRef — locate by ref id as existing tests do */;
    assert_eq!(r.book_sources.len(), 1);
    assert_eq!(
        r.book_sources[0].identifiers,
        vec![BookIdentifier::isbn("978-1-84583-093-9").expect("valid")]
    );
    // The page range comes from the sfn's own pp param, NOT the cite book's.
    assert_eq!(r.book_sources[0].cited_page.as_deref(), Some("113–116"));
}

#[test]
fn harvsp_ref_resolves_a_percent_encoded_prefixless_fragment() {
    let blocks = blocks_from_fixture("parsoid_harvsp_frwiki.html");
    let r = /* the harvsp ref */;
    assert_eq!(r.book_sources.len(), 1);
    assert_eq!(r.book_sources[0].identifiers[0], BookIdentifier::isbn("978-2-85822-660-3").expect("valid"));
}

#[test]
fn unresolvable_short_cite_yields_no_book_source_and_flags_the_ref() {
    let blocks = blocks_from_fixture("parsoid_sfn_enwiki.html");
    let r = /* the CITEREFNowhere2020 ref */;
    assert!(r.book_sources.is_empty(), "never a guessed identifier");
    assert!(r.short_cite_unresolved);
}

#[test]
fn direct_cite_book_extraction_is_unchanged() {
    let blocks = blocks_from_fixture("parsoid_sfn_enwiki.html");
    let r = /* the inline cite-book ref */;
    assert_eq!(r.book_sources.len(), 1, "direct lane regression");
    assert!(!r.short_cite_unresolved);
}
```

**Step 2: Run to verify failure.**

**Step 3: Implement.**

- First: `crates/sp42-parsoid/Cargo.toml` gains `percent-encoding.workspace = true` (it is a workspace dep at root `Cargo.toml:43` but NOT yet wired into this crate — without this the fragment decoding below does not compile).
- Author the shared test helper this task's tests use (it does not exist yet):
```rust
fn blocks_from_fixture(name: &str) -> Vec<ParsoidBlock> { /* include_str! by match on name, ImmutableWikicode::new, blocks_from_revision().expect */ }
```
  and locate refs the way existing tests do (find block by text `contains`, then match `refs[i].ref_id` — see `lib.rs:465-505`).
- Short-cite family constant (module-level, documented as the config-override seam):
```rust
/// Shortened-footnote template families whose refs cite a bibliography entry
/// by anchor (design sketch 2026-07-13). Cross-wiki constant on purpose —
/// the *anchor* is followed literally, so per-wiki id conventions (enwiki's
/// CITEREF prefix, frwiki's bare name+year) need no configuration; only the
/// template names would ever need a per-wiki override, and none does yet.
const SHORT_CITE_TEMPLATES: &[&str] = &["sfn", "sfnp", "sfnm", "harvsp", "harvnb", "harv", "harvtxt", "harvcoltxt"];
```
- In `sources_in_reference` (signature gains `index: &BiblioIndex`; update the `blocks_from_revision` call site to build the index first and pass it): while iterating transclusion data-mw parts, detect a part whose `target.wt` (trimmed, first letter case-folded — Parsoid preserves `sfn` vs `Sfn` as authored) is in `SHORT_CITE_TEMPLATES`. For such a part:
  1. Resolution key, in order: (a) the fragment of the first descendant `<a>` in the ref contents whose `href` contains `#` — take the substring after `#`, percent-decode it (`percent_encoding::percent_decode_str(...).decode_utf8_lossy()`); (b) reconstructed keys from the part's positional params `1..=4` concatenated in order plus nothing else (year is itself a positional param), tried as `CITEREF<concat>` then bare `<concat>`.
  2. On index hit: push a `BookSource` cloning the entry's `identifiers` but with `cited_page` OVERRIDDEN by the short-cite's own `p`/`pp`/`page`/`pages`/`loc` param when present (fall back to the bibliography entry's page, which is usually `None`).
  3. On miss: set a new `short_cite_unresolved: bool` on the ref's collected sources (see below), never inventing an identifier.
- `BlockRef` gains `#[serde(default)] pub short_cite_unresolved: bool` (`crates/sp42-platform/src/wikitext_editor.rs`, next to `is_bare_url_ref` — additive, mirrors that precedent; note `#[serde(default)]` helps deserialization only, every struct literal still needs the field). `RefSources` (the internal accumulator) carries it through. **~15 literal initializer sites** (verified): `sp42-citation/src/citation/extract.rs:278,296`; `sp42-citation/src/citation/page.rs:1101,1113,1143,1152,1216,1557,1634,1726,1787,1855`; `sp42-mcp/src/page.rs:268,296`; `sp42-parsoid/src/lib.rs:203`. The compiler enumerates any drift in those line numbers.
- Dedupe: a book source resolved via lane A whose identifier set duplicates one already collected (e.g. the same sfn used twice) is still pushed once per ref occurrence — refs are distinct use-sites; but within ONE ref, dedupe identical `BookSource`s.

**Step 4: Run** `cargo test -p sp42-parsoid` and `cargo test -p sp42-platform` — pass. **Step 5: Commit** — `feat(parsoid): resolve shortened-footnote refs to bibliography book sources`

---

## Task 4: Lane B — ref-local magiclinks and {{ISBN}} transclusions

**Files:**
- Modify: `crates/sp42-parsoid/src/lib.rs`

**Step 1: Failing tests:**

```rust
#[test]
fn ref_local_isbn_magiclinks_become_book_sources() {
    let blocks = blocks_from_fixture("parsoid_magiclink_dewiki.html");
    let r = /* the free-text ref */;
    assert_eq!(r.book_sources.len(), 2, "both magiclinks, checksum-valid");
    assert!(r.book_sources.iter().all(|b| b.cited_page.is_none()), "MVP: no free-text page parse");
}

#[test]
fn isbn_template_transclusion_in_a_ref_becomes_a_book_source() {
    // {{ISBN|978-…}} renders as a transclusion whose target.wt is "ISBN";
    // add a ref carrying one to the enwiki fixture (enwiki dropped magic
    // links in 2017 — the template is its replacement).
    let blocks = blocks_from_fixture("parsoid_sfn_enwiki.html");
    let r = /* the isbn-template ref (add it to the fixture in this task) */;
    assert_eq!(r.book_sources.len(), 1);
}
```

**Step 2: Run to verify failure.**

**Step 3: Implement in `sources_in_reference`:**
- After template processing: `magiclink_isbns(&contents)` (Task 2's helper) → each valid ISBN becomes `BookSource { identifiers: vec![isbn], cited_page: None }`, deduped against identifiers already collected for this ref (a `{{cite book}}` that ALSO renders a magiclink must not double-count — check against every identifier gathered so far).
- In the transclusion loop: a part whose `target.wt` trims/case-folds to `isbn` takes its positional params (`1`, `2`, …) through `BookIdentifier::isbn`, same dedupe.
- Fixture update: add the `{{ISBN}}`-carrying ref to `parsoid_sfn_enwiki.html` (copy a real `{{ISBN}}` transclusion's data-mw shape from any enwiki Parsoid output — fetch one fragment if unsure rather than inventing: `curl -s "https://en.wikipedia.org/api/rest_v1/page/html/ISBN" | grep -o '…'` or reuse the probe HTML if it contains one).

**Step 4: Run tests** — pass. **Step 5: Commit** — `feat(parsoid): ref-local ISBN magiclink and {{ISBN}}-template book sources`

---

## Task 5: Crate gate

```sh
cargo test -p sp42-parsoid -p sp42-platform -p sp42-citation -p sp42-mcp
cargo clippy -p sp42-parsoid -p sp42-platform --all-targets -- -D warnings
cargo fmt
```
Expected: green; the pre-existing book-lane tests unchanged (the design's regression gate). Commit fallout only (`chore(parsoid): phase-1 gate fixes`) or nothing.
