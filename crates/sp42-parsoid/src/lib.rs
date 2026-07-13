//! Read-only Parsoid page access: fetch a revision and decompose it into prose-bearing
//! [`ParsoidBlock`]s (ADR-0003 Decisions 5/6).
//!
//! This is the read half of SP42's Parsoid surface, factored out of `sp42-server`'s
//! `WikitextEditor` so both the server's page pipeline and the `sp42-mcp` citation surface
//! share one fetch+parse implementation. It is **host-only** (pulls the `parsoid` crate's
//! reqwest-backed client and a `!Send` kuchikiki DOM), so it cannot live in the wasm-facing
//! `sp42-core`. The DOM-editing half (node enumeration, anchored replace, selser round-trip)
//! stays in `sp42-server`.
//!
//! The cited-source extraction (cite-template `data-mw`, archive fallbacks, bare `ExtLinks`)
//! is structural and citation-correctness-sensitive; keeping it in one place avoids drift
//! between the server and MCP consumers.

use parsoid::prelude::*;
use parsoid::{Client, ImmutableWikicode};
use percent_encoding::percent_decode_str;
use sp42_platform::{
    BlockKind, BlockRef, BookIdentifier, BookSource, CitedSource, ParsoidBlock, WikiConfig,
    WikitextEditorError, WikitextPageRef,
};
use std::collections::HashMap;

/// Shortened-footnote template families whose refs cite a bibliography entry
/// by anchor (design sketch 2026-07-13). Cross-wiki constant on purpose —
/// the *anchor* is followed literally, so per-wiki id conventions (enwiki's
/// CITEREF prefix, frwiki's bare name+year) need no configuration; only the
/// template names would ever need a per-wiki override, and none does yet.
// `sfnm` is deliberately absent: one {{sfnm}} renders SEVERAL bibliography
// anchors from one template part, so neither one-link-per-part association
// nor 1..=4 positional reconstruction fits it — handling it wrongly would
// misattribute sources (Codex round 3, PR 153). Dedicated multi-source
// handling is a follow-up; until then sfnm refs ride the existing lanes.
const SHORT_CITE_TEMPLATES: &[&str] = &[
    "sfn",
    "sfnp",
    "harvsp",
    "harvnb",
    "harv",
    "harvtxt",
    "harvcoltxt",
];

/// Build a Parsoid REST client for `config`'s `parsoid_url`.
///
/// # Errors
///
/// [`WikitextEditorError::NotConfigured`] if the wiki has no `parsoid_url`, or
/// [`WikitextEditorError::Unavailable`] if the client cannot be constructed.
pub fn editor_client(config: &WikiConfig) -> Result<Client, WikitextEditorError> {
    let Some(parsoid_url) = config.parsoid_url.as_ref() else {
        return Err(WikitextEditorError::NotConfigured {
            wiki_id: config.wiki_id.clone(),
        });
    };
    Client::new(
        parsoid_url.as_str().trim_end_matches('/'),
        sp42_platform::branding::USER_AGENT,
    )
    .map_err(|error| WikitextEditorError::Unavailable {
        message: format!("failed to build Parsoid client: {error}"),
        retryable: false,
    })
}

/// Map a `parsoid` crate error onto the editor's error taxonomy.
#[must_use]
pub fn map_parsoid_error(error: parsoid::Error) -> WikitextEditorError {
    match error {
        parsoid::Error::PageDoesNotExist(title) => WikitextEditorError::MissingTarget {
            message: format!("page does not exist: {title}"),
        },
        parsoid::Error::HttpTooManyRequests { .. } => WikitextEditorError::Unavailable {
            message: "the Parsoid endpoint rate limited the request".to_string(),
            retryable: true,
        },
        parsoid::Error::Http(error) => WikitextEditorError::Unavailable {
            message: format!("Parsoid HTTP request failed: {error}"),
            retryable: true,
        },
        other => WikitextEditorError::Unavailable {
            message: format!("Parsoid request failed: {other}"),
            retryable: false,
        },
    }
}

/// Fetch a revision through Parsoid and decompose it into prose-bearing blocks.
///
/// The read-only half of the Parsoid surface: build the client, fetch the addressed
/// revision, and parse it to [`ParsoidBlock`]s — no DOM editing. Shared by
/// `sp42-server`'s `WikitextEditor::extract_blocks` production path and the `sp42-mcp`
/// page verb, so both decompose pages identically.
///
/// # Errors
///
/// [`WikitextEditorError::NotConfigured`] if the wiki has no `parsoid_url`, or a mapped
/// Parsoid fetch error (missing revision, rate limit, transport).
pub async fn fetch_page_blocks(
    config: &WikiConfig,
    page: &WikitextPageRef,
) -> Result<Vec<ParsoidBlock>, WikitextEditorError> {
    let client = editor_client(config)?;
    let revision = client
        .get_revision(&page.title, page.rev_id)
        .await
        .map_err(map_parsoid_error)?;
    blocks_from_revision(&revision)
}

/// Document-level index of bibliography-entry book sources, keyed by DOM id
/// (the `#CITEREF…`-style anchor targets of shortened footnotes; frwiki ids
/// carry no CITEREF prefix, so keys are stored verbatim).
type BiblioIndex = std::collections::HashMap<String, BookSource>;

/// Extract ISBN identifiers from descendant ISBN magiclink elements.
/// For each `a.mw-magiclink-isbn` found, extract the last path segment of the href
/// (which is the normalized ISBN digits), validate via `BookIdentifier::isbn`,
/// and collect all valid ISBNs into a vec.
pub(crate) fn magiclink_isbns(node: &impl WikinodeIterator) -> Vec<BookIdentifier> {
    let mut isbns = Vec::new();
    let selected = node.select("[class~=\"mw-magiclink-isbn\"]");
    for link in selected {
        if let Some(link_elem) = link.as_node().as_element()
            && let Some(href) = link_elem.attributes.borrow().get("href")
            && let Some(last_segment) = href.rsplit('/').next()
            && let Some(isbn) = BookIdentifier::isbn(last_segment)
        {
            isbns.push(isbn);
        }
    }
    isbns
}

/// Build a document-level bibliography index from transclusion elements.
/// Maps DOM `id` attributes to their corresponding `BookSource`s from either:
/// - Template params (data-mw `isbn`, `oclc`, `lccn`, `ol` params)
/// - Descendant ISBN magiclinks
pub(crate) fn biblio_index(code: &Wikicode) -> BiblioIndex {
    let mut index = BiblioIndex::new();
    let mut about_to_book: HashMap<String, BookSource> = HashMap::new();

    // First pass: walk all transclusion elements to extract book sources
    let selected = code.select("[typeof~=\"mw:Transclusion\"]");
    for elem in selected {
        // Try to extract book source from data-mw template params
        let mut book_source = None;
        if let Some(element) = elem.as_node().as_element()
            && let Some(data_mw) = element.attributes.borrow().get("data-mw")
            && let Ok(value) = serde_json::from_str::<serde_json::Value>(data_mw)
            && let Some(parts) = value.get("parts").and_then(|p| p.as_array())
        {
            for part in parts {
                if let Some(params) = part.pointer("/template/params")
                    && let Some(book) = template_book_source(params)
                {
                    book_source = Some(book);
                    break; // First matching template wins
                }
            }
        }

        // If no template book source, try magiclinks in this element
        if book_source.is_none() {
            let isbns = magiclink_isbns(&elem);
            if !isbns.is_empty() {
                book_source = Some(BookSource {
                    identifiers: isbns,
                    cited_page: None,
                });
            }
        }

        // Store the book source by its about attribute for later indexing
        if let Some(book) = book_source
            && let Some(element) = elem.as_node().as_element()
        {
            let attrs = element.attributes.borrow();
            // Shape 1: id on this element itself (frwiki shape)
            if let Some(id) = attrs.get("id")
                && !index.contains_key(id)
            {
                index.insert(id.to_string(), book.clone());
            }
            // Shape 2: about-indirection (enwiki shape) — store by about for second pass
            if let Some(about) = attrs.get("about") {
                about_to_book.insert(about.to_string(), book);
            }
        }
    }

    // Second pass: walk ALL elements looking for ids with matching about values
    // This handles enwiki's shape where id is on a different element than data-mw.
    // Assumption: one cite template output carries one id-bearing element per about;
    // multiple id descendants under one about all map to the same book by design
    // (first-writer-wins on id collisions via the !index.contains_key check).
    let all_elems = code.descendants();
    for node in all_elems {
        if let Some(element) = node.as_element() {
            let attrs = element.attributes.borrow();
            if let Some(id) = attrs.get("id")
                && let Some(about) = attrs.get("about")
                && let Some(book) = about_to_book.get(about)
                && !index.contains_key(id)
            {
                index.insert(id.to_string(), book.clone());
            }
        }
    }

    // Third pass: hand-written (template-less) bibliography items — an
    // id-bearing element whose descendants carry ISBN magiclinks but no
    // transclusion wrapper (dewiki-style manual bibliographies; Codex P2 on
    // PR 153). Template-derived entries above win; this only fills ids the
    // first two passes left empty.
    for node in code.descendants() {
        let Some(element) = node.as_element() else {
            continue;
        };
        let id = {
            let attrs = element.attributes.borrow();
            match attrs.get("id") {
                Some(id) if !index.contains_key(id) => id.to_string(),
                _ => continue,
            }
        };
        let isbns = magiclink_isbns(&node);
        if !isbns.is_empty() {
            index.insert(
                id,
                BookSource {
                    identifiers: isbns,
                    cited_page: None,
                },
            );
        }
    }

    index
}

/// Extract prose-bearing blocks from a Parsoid revision, in document order.
///
/// # Errors
///
/// Currently infallible (the `Result` mirrors the editing path and leaves room for a
/// future DOM-interpretation failure).
#[allow(clippy::unnecessary_wraps)] // https://github.com/schiste/SP42/pull/103 mirrors editor errors.
pub fn blocks_from_revision(
    revision: &ImmutableWikicode,
) -> Result<Vec<ParsoidBlock>, WikitextEditorError> {
    let code = Wikicode::new(revision.html());

    // Build document-level bibliography index (for short-cite resolution).
    let index = biblio_index(&code);

    // Extract ref marker data-mw by body id for short-cite detection.
    // Each ref marker points to its reference list item via the body.id field.
    let mut ref_marker_data: HashMap<String, String> = HashMap::new();
    for marker in code.select("[typeof~=\"mw:Extension/ref\"]") {
        if let Some(element) = marker.as_node().as_element()
            && let Some(data_mw) = element.attributes.borrow().get("data-mw")
            && let Ok(value) = serde_json::from_str::<serde_json::Value>(data_mw)
            && let Some(body_id) = value.pointer("/body/id").and_then(|v| v.as_str())
        {
            ref_marker_data.insert(body_id.to_string(), data_mw.to_string());
        }
    }

    // Map Reference.id() -> cited sources (primary URL + archive fallbacks) and
    // book identifiers (PRD-0009), read structurally from cite templates and
    // bare ExtLinks inside each reference's contents.
    let mut ref_sources: HashMap<String, RefSources> = HashMap::new();
    // Parallel to `ref_sources`: whether each reference's whole rendered content is a single bare
    // URL (a bare-URL-repair target). Keyed the same way so `collect_block` can stamp it per marker.
    let mut ref_bare: HashMap<String, bool> = HashMap::new();
    for reference in code.filter_references() {
        ref_sources.insert(
            reference.id(),
            sources_in_reference(&reference, &index, &ref_marker_data),
        );
        ref_bare.insert(reference.id(), reference_is_bare_url(&reference));
    }

    let mut blocks = Vec::new();
    let mut ordinal = 0usize;
    walk(&code, &ref_sources, &ref_bare, &mut blocks, &mut ordinal);
    Ok(blocks)
}

/// `true` when a reference's whole rendered content is a single bare URL (no cite template) — the
/// same test `sp42-citation`'s `classify_bare_url` applies to a wikitext anchor, evaluated here on
/// the ref's rendered content so a finding's ref can be classified as a bare-URL-repair target.
fn reference_is_bare_url(reference: &Reference) -> bool {
    let text = reference.contents().text_contents();
    let trimmed = text.trim();
    (trimmed.starts_with("http://") || trimmed.starts_with("https://"))
        && !trimmed.chars().any(char::is_whitespace)
        && url::Url::parse(trimmed).is_ok()
}

/// Recursive walker — emit blocks, don't recurse into a block once emitted.
fn walk(
    node: &impl WikinodeIterator,
    ref_sources: &HashMap<String, RefSources>,
    ref_bare: &HashMap<String, bool>,
    blocks: &mut Vec<ParsoidBlock>,
    ordinal: &mut usize,
) {
    for child in node.children() {
        if let Some(kind) = block_kind(&child) {
            blocks.push(build_block(&child, kind, *ordinal, ref_sources, ref_bare));
            *ordinal += 1;
            continue; // do not descend into an emitted block
        }
        walk(&child, ref_sources, ref_bare, blocks, ordinal);
    }
}

/// Block detection by tag.
fn block_kind(node: &Wikinode) -> Option<BlockKind> {
    let element = node.as_node().as_element()?;
    match element.name.local.as_ref() {
        "p" => Some(BlockKind::Paragraph),
        "li" | "dd" => Some(BlockKind::ListItem),
        "td" | "th" | "caption" => Some(BlockKind::TableCell),
        _ => None,
    }
}

/// Build one block — ordered child traversal, skipping ref-marker internals and recording offsets.
fn build_block(
    node: &Wikinode,
    kind: BlockKind,
    ordinal: usize,
    ref_sources: &HashMap<String, RefSources>,
    ref_bare: &HashMap<String, bool>,
) -> ParsoidBlock {
    let mut text = String::new();
    let mut refs = Vec::new();
    collect_block(node, &mut text, &mut refs, ref_sources, ref_bare);

    // Adjust ref offsets from the untrimmed text to the trimmed text.
    // collect_block records offsets against the untrimmed accumulator,
    // but we store text.trim(), so offsets are too large by the leading-whitespace byte count.
    let lead = text.len() - text.trim_start().len();
    let trimmed = text.trim().to_string();
    for r in &mut refs {
        r.offset = r.offset.saturating_sub(lead).min(trimmed.len());
    }

    ParsoidBlock {
        text: trimmed,
        refs,
        block_kind: kind,
        block_ordinal: ordinal,
    }
}

/// Collect text and refs from a block, skipping ref markers' own text.
fn collect_block(
    node: &impl WikinodeIterator,
    text: &mut String,
    refs: &mut Vec<BlockRef>,
    ref_sources: &HashMap<String, RefSources>,
    ref_bare: &HashMap<String, bool>,
) {
    for child in node.children() {
        if let Some(ref_link) = child.as_reference_link() {
            // Empty reference_id simply misses the ref_sources map, yielding empty
            // sources for a parse-failed ref, without aliasing refs.
            let reference_id = ref_link.reference_id().unwrap_or_default();
            let sources = ref_sources.get(&reference_id).cloned().unwrap_or_default();
            refs.push(BlockRef {
                offset: text.len(),
                ref_id: ref_link.id(),
                sources: sources.cited,
                book_sources: sources.books,
                ref_text: child.text_contents(),
                named: ref_link.name().ok().flatten().is_some(),
                is_bare_url_ref: ref_bare.get(&reference_id).copied().unwrap_or(false),
                short_cite_unresolved: sources.short_cite_unresolved,
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
        collect_block(&child, text, refs, ref_sources, ref_bare);
    }
}

/// Everything one reference cites, split by kind (PRD-0009 Layer 1).
#[derive(Debug, Default, Clone)]
struct RefSources {
    /// Fetchable sources: primary URL + archive fallbacks.
    cited: Vec<CitedSource>,
    /// Book identifiers from cite templates, independent of any URL.
    books: Vec<BookSource>,
    /// `true` when a short-cite template pointed to a bibliography entry
    /// that was not found in the document index.
    short_cite_unresolved: bool,
}

/// Extract the `cited_page` override from a short-cite template's params.
fn get_short_cite_page(part: &serde_json::Value) -> Option<String> {
    ["p", "pp", "page", "pages", "loc"]
        .iter()
        .find_map(|param_name| {
            part.pointer(&format!("/template/params/{param_name}/wt"))
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
}

/// Use an indexed book source for a short-cite resolution, applying page overrides.
fn resolve_book_from_index(
    part: &serde_json::Value,
    indexed_book: &BookSource,
    seen_identifiers: &std::collections::HashSet<String>,
) -> BookSource {
    // The short-cite's own page wins (per-use-site precision); when it names
    // none, keep the bibliography entry's page rather than dropping to None
    // (Codex P2 on PR 153).
    let cited_page = get_short_cite_page(part).or_else(|| indexed_book.cited_page.clone());

    // Filter out identifiers we've already seen in this ref
    let new_identifiers: Vec<_> = indexed_book
        .identifiers
        .iter()
        .filter(|ident| !seen_identifiers.contains(&ident.to_string()))
        .cloned()
        .collect();

    // Return a BookSource with identifiers and the overridden page
    let identifiers = if new_identifiers.is_empty() {
        indexed_book.identifiers.clone()
    } else {
        new_identifiers
    };

    BookSource {
        identifiers,
        cited_page,
    }
}

/// Resolve a short-cite template to a bibliography-indexed `BookSource`.
/// Returns the resolved `BookSource` with identifiers from the index and `cited_page`
/// from the short-cite params, or None if the anchor is not found in the index.
/// The percent-decoded fragments of every same-page link in a ref body, in
/// document order — consumed one per short-cite part, so bundled short
/// citations bind to their own targets instead of all taking the first link
/// (Codex round 3, PR 153).
fn fragment_keys_in_order(contents: &impl WikinodeIterator) -> std::collections::VecDeque<String> {
    let mut keys = std::collections::VecDeque::new();
    // Only same-page fragment links qualify as bibliography targets: Parsoid
    // marks them `mw-selflink-fragment` (verified on the en/fr probes). An
    // external URL that merely contains `#` (a cite-web `…/page#section`
    // rendered before the short cite) must not poison the association
    // (Codex round 4, PR 153).
    for elem in contents.select("a.mw-selflink-fragment[href*=\"#\"]") {
        if let Some(element) = elem.as_node().as_element()
            && let Some(href) = element.attributes.borrow().get("href")
            && let Some(frag) = href.split('#').nth(1)
        {
            keys.push_back(percent_decode_str(frag).decode_utf8_lossy().to_string());
        }
    }
    keys
}

/// The anchor fragments a short-cite part can legitimately render, in
/// preference order. An explicit `ref=` param fully determines the anchor
/// (and positional reconstruction must NOT be tried — binding
/// `CITEREF<author><year>` when the author wrote `ref=X` is a guessed
/// identifier, Codex round 2, PR 153); otherwise the anchor is the
/// positional 1..=4 concat, with and without the `CITEREF` prefix (enwiki
/// vs frwiki conventions).
fn short_cite_candidates(part: &serde_json::Value) -> Vec<String> {
    if let Some(ref_param) = part
        .pointer("/template/params/ref/wt")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        // The ref= value is the anchor, verbatim (Module:Footnotes); a
        // CITEREF-prefixed variant is accepted too for templates that prefix
        // it (Codex round 10, PR 153). Both are this part's own namespace —
        // positional reconstruction stays excluded.
        let normalized = ref_param.replace(' ', "_");
        return vec![format!("CITEREF{normalized}"), normalized];
    }
    let mut concat = String::new();
    // Up to four author names plus the year: five positional params
    // ({{sfn|Smith|Jones|Brown|Black|1994}} → CITEREFSmithJonesBrownBlack1994,
    // Codex round 7, PR 153).
    for i in 1..=5 {
        if let Some(param) = part
            .pointer(&format!("/template/params/{i}/wt"))
            .and_then(|v| v.as_str())
        {
            concat.push_str(param.trim());
        }
    }
    if concat.is_empty() {
        return Vec::new();
    }
    // MediaWiki normalizes spaces to underscores in anchor ids, on both the
    // rendered fragment href and the bibliography element id
    // ({{sfn|Museum of Modern Art|2024}} → #CITEREFMuseum_of_Modern_Art2024;
    // Codex round 9, PR 153).
    let concat = concat.replace(' ', "_");
    vec![format!("CITEREF{concat}"), concat]
}

/// Resolve one short-cite part against the ref body's same-page links and
/// the bibliography index, by **expected-anchor matching** (Codex rounds
/// 2/3/5/6, PR 153): the part binds only a link whose fragment equals one of
/// its own candidate anchors — so bundled parts each find their own link,
/// unrelated section links can never match, and a matched link whose target
/// is not book-indexed stays an authoritative miss. With no matching link,
/// the candidates are tried directly against the index (safe: the candidate
/// IS this part's own anchor, never a different entry's).
fn resolve_short_cite(
    part: &serde_json::Value,
    fragment_keys: &mut std::collections::VecDeque<String>,
    index: &BiblioIndex,
    seen_identifiers: &std::collections::HashSet<String>,
) -> Option<BookSource> {
    let candidates = short_cite_candidates(part);
    if candidates.is_empty() {
        return None;
    }
    if let Some(position) = fragment_keys
        .iter()
        .position(|frag| candidates.iter().any(|c| c == frag))
    {
        let frag = fragment_keys
            .remove(position)
            .expect("position from iter is in bounds");
        // The part's own rendered link: resolve it or fail — never fall
        // through to a different key.
        return index
            .get(&frag)
            .map(|book| resolve_book_from_index(part, book, seen_identifiers));
    }
    // No link rendered for this part (plain-text body): the candidates are
    // the part's own anchor, so direct index lookup is not a guess.
    candidates
        .iter()
        .find_map(|key| index.get(key))
        .map(|book| resolve_book_from_index(part, book, seen_identifiers))
}

/// Process {{ISBN}} templates in a reference transclusion.
/// Extracts ISBNs from positional params and adds them as `BookSource`s,
/// deduping against already-collected identifiers.
fn process_isbn_template(
    data_mw: &str,
    books: &mut Vec<BookSource>,
    seen_identifiers: &mut std::collections::HashSet<String>,
) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(data_mw) else {
        return;
    };
    let Some(parts) = value.get("parts").and_then(|p| p.as_array()) else {
        return;
    };

    for part in parts {
        let Some(target_wt) = part.pointer("/template/target/wt").and_then(|v| v.as_str()) else {
            continue;
        };
        if !target_wt.trim().eq_ignore_ascii_case("isbn") {
            continue;
        }

        let Some(params) = part.pointer("/template/params") else {
            continue;
        };
        for i in 1..=10 {
            if let Some(isbn_val) = params.pointer(&format!("/{i}/wt")).and_then(|v| v.as_str())
                && let Some(isbn) = BookIdentifier::isbn(isbn_val.trim())
                && !seen_identifiers.contains(&isbn.to_string())
            {
                books.push(BookSource {
                    identifiers: vec![isbn.clone()],
                    cited_page: None,
                });
                seen_identifiers.insert(isbn.to_string());
            }
        }
    }
}

/// Process short-cite marker data from a reference.
/// Resolves sfn/harvsp/etc templates to bibliography-indexed book sources.
/// Cited source extraction from a reference's contents.
/// Returns one `CitedSource` per cite-template (primary url + archive fallbacks),
/// plus `CitedSource` entries for *literal* bare `ExtLink`s not already present in
/// templates. Preserves document order: template-derived sources first, then
/// bare-ExtLink sources. Alongside, each cite template carrying a validated book
/// identifier (`isbn`/`oclc`/`lccn`/`ol`) yields one `BookSource`, whether or not
/// it also carries a URL (ADR-0024 Decision 1). Short-cite templates (sfn/harvsp/etc)
/// resolve to bibliography-indexed book sources via fragment matching.
#[allow(clippy::too_many_lines)] // https://github.com/schiste/SP42/blob/main/docs/design-plans/2026-07-13-bibliography-indirection.md sources_in_reference spans both lanes by design
fn sources_in_reference(
    reference: &Reference,
    index: &BiblioIndex,
    ref_marker_data: &HashMap<String, String>,
) -> RefSources {
    let contents = reference.contents();
    let mut sources = Vec::new();
    let mut books = Vec::new();
    let mut seen_urls = std::collections::HashSet::new();
    let mut seen_identifiers: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut short_cite_unresolved = false;
    // Same-page links in the ref body, in order: each short-cite part binds
    // the link matching its own expected anchor (expected-anchor matching).
    let mut fragment_keys = fragment_keys_in_order(&contents);

    // First, check if this reference has a short-cite marker (sfn/harvsp/etc).
    // The ref marker's data-mw is keyed by the reference body id.
    // Reference.id() returns the full body id (e.g., "mw-reference-text-cite_note-FOOTNOTERoxburgh2014113–116-4").
    let body_id = reference.id();
    if let Some(marker_data) = ref_marker_data.get(&body_id)
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(marker_data)
        && let Some(parts) = value.get("parts").and_then(|p| p.as_array())
    {
        for part in parts {
            if let Some(target_wt) = part.pointer("/template/target/wt").and_then(|v| v.as_str()) {
                let template_name = target_wt.trim().to_ascii_lowercase();
                if SHORT_CITE_TEMPLATES.contains(&template_name.as_str()) {
                    // This is a short-cite template - try to resolve it
                    if let Some(resolved_book) =
                        resolve_short_cite(part, &mut fragment_keys, index, &seen_identifiers)
                    {
                        books.push(resolved_book.clone());
                        // Track identifiers we've added
                        for ident in &resolved_book.identifiers {
                            seen_identifiers.insert(ident.to_string());
                        }
                    } else {
                        // Resolution failed - flag this ref as unresolved
                        short_cite_unresolved = true;
                    }
                }
            }
        }
    }

    // Cite-template params via data-mw. The transclusion `data-mw` is carried on
    // whichever element starts the transclusion — in real Parsoid output for a
    // citation that is a `<link typeof="mw:Extension/templatestyles mw:Transclusion">`,
    // not a `<span>` — so match any element bearing the `mw:Transclusion` typeof.
    //
    // We also record each transclusion's `about` id: Parsoid groups every DOM node a
    // template rendered under one shared `about`. The cite template's *rendered*
    // links (DOI/PMID/PMC resolvers it derives from `doi=`/`pmid=` params) live in
    // that group but are not the `url=` param, so the bare-ExtLink pass below must
    // exclude them — they are template output, not literal bare URLs in the ref.
    let mut transclusion_abouts = std::collections::HashSet::new();
    let transclusions = contents.select("[typeof~=\"mw:Transclusion\"]");
    for span in transclusions {
        if let Some(element) = span.as_node().as_element() {
            let attributes = element.attributes.borrow();
            if let Some(data_mw) = attributes.get("data-mw") {
                // Short-cite templates rendered in the ref body (harvsp/etc):
                // resolve each part against its own consumed fragment link.
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(data_mw)
                    && let Some(parts) = value.get("parts").and_then(|p| p.as_array())
                {
                    for part in parts {
                        if let Some(target_wt) =
                            part.pointer("/template/target/wt").and_then(|v| v.as_str())
                        {
                            let template_name = target_wt.trim().to_ascii_lowercase();
                            if SHORT_CITE_TEMPLATES.contains(&template_name.as_str()) {
                                if let Some(resolved_book) = resolve_short_cite(
                                    part,
                                    &mut fragment_keys,
                                    index,
                                    &seen_identifiers,
                                ) {
                                    books.push(resolved_book.clone());
                                    // Track identifiers we've added
                                    for ident in &resolved_book.identifiers {
                                        seen_identifiers.insert(ident.to_string());
                                    }
                                } else {
                                    // Resolution failed - flag this ref as unresolved
                                    short_cite_unresolved = true;
                                }
                            }
                        }
                    }
                }

                // Check if this is an {{ISBN|...}} template
                process_isbn_template(data_mw, &mut books, &mut seen_identifiers);

                // Regular cite templates in the same data-mw still collect:
                // a parts array can mix a short cite with a normal cite/URL
                // template, and short-cite parts are harmless here (they carry
                // no url/identifier params), so nothing is suppressed
                // (Codex round 3, PR 153).
                push_template_sources(data_mw, &mut sources, &mut books, &mut seen_urls);

                // Track book identifiers we've collected so far for deduplication
                for book in &books {
                    for ident in &book.identifiers {
                        seen_identifiers.insert(ident.to_string());
                    }
                }
            }
            if let Some(about) = attributes.get("about") {
                transclusion_abouts.insert(about.to_string());
            }
        }
    }

    // Bare ExtLinks: a *literal* URL typed into the ref, i.e. one outside every
    // transclusion group. Add only if not already present (either as primary or
    // archive) from the template pass above.
    for node in contents.descendants() {
        if let Some(extlink) = node.as_extlink()
            && !in_transclusion(&node, &transclusion_abouts)
            && let Ok(u) = url::Url::parse(&extlink.target())
        {
            let url_str = u.to_string();
            if !seen_urls.contains(url_str.as_str()) {
                seen_urls.insert(url_str.clone());
                sources.push(CitedSource {
                    url: u,
                    archive_urls: vec![],
                });
            }
        }
    }

    // Extract ISBN magiclinks from the reference contents
    let magiclink_isbns_in_ref = magiclink_isbns(&contents);
    for isbn in magiclink_isbns_in_ref {
        // Only add if not already collected
        if !seen_identifiers.contains(&isbn.to_string()) {
            books.push(BookSource {
                identifiers: vec![isbn.clone()],
                cited_page: None,
            });
            seen_identifiers.insert(isbn.to_string());
        }
    }

    // Deduplicate identical BookSources within this ref, preserving document
    // order (bundled short cites must keep their in-body sequence; a sort
    // here previously reordered them — Codex round 3, PR 153).
    let mut seen_books = std::collections::HashSet::new();
    books.retain(|book| seen_books.insert(format!("{:?}|{:?}", book.identifiers, book.cited_page)));

    RefSources {
        cited: sources,
        books,
        short_cite_unresolved,
    }
}

/// True if `node` (or any ancestor) belongs to a transclusion group, i.e. carries
/// an `about` id that a `mw:Transclusion` element in the same reference also bears.
/// Such a node is rendered template output, not a literal bare URL.
fn in_transclusion(
    node: &impl WikinodeIterator,
    transclusion_abouts: &std::collections::HashSet<String>,
) -> bool {
    if transclusion_abouts.is_empty() {
        return false;
    }
    node.inclusive_ancestors().any(|ancestor| {
        ancestor.as_node().as_element().is_some_and(|element| {
            element
                .attributes
                .borrow()
                .get("about")
                .is_some_and(|about| transclusion_abouts.contains(about))
        })
    })
}

/// CS1/CS2 template parameters that carry a fetchable source URL, in priority
/// order. `url` is the canonical one; the rest are chapter/section-level aliases
/// used when a ref points at one part of a larger work and omits a top-level
/// `url=`. The first present alias becomes the citation's primary source.
const URL_PARAM_ALIASES: &[&str] = &[
    "url",
    // Each alias in both CS1 spellings, hyphenated first: the no-hyphen forms
    // (`chapterurl=`, …) are accepted CS1 aliases still common in article
    // wikitext, and a ref using one must not be dropped as a non-URL source
    // (SP42#62). Pairs stay adjacent so the priority order is by level, not
    // by spelling.
    "chapter-url",
    "chapterurl",
    "conference-url",
    "conferenceurl",
    "contribution-url",
    "contributionurl",
    "article-url",
    "articleurl",
    "section-url",
    "sectionurl",
    "entry-url",
    "entryurl",
    "map-url",
    "mapurl",
    "transcript-url",
    "transcripturl",
];

/// Extract cited sources from a cite-template data-mw.
/// For each template part with a primary url param (`url` or a URL-valued alias),
/// builds one `CitedSource` with that url as primary and
/// `archive-url`/`archiveurl` as fallbacks.
/// Appends to sources and updates `seen_urls` with all URLs (primary + archives).
/// Independently, each part carrying a validated book identifier appends one
/// `BookSource` to `books` — a template with `isbn=` but no `url=` still counts.
fn push_template_sources(
    data_mw: &str,
    sources: &mut Vec<CitedSource>,
    books: &mut Vec<BookSource>,
    seen_urls: &mut std::collections::HashSet<String>,
) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(data_mw) else {
        return;
    };
    let Some(parts) = value.get("parts").and_then(|p| p.as_array()) else {
        return;
    };
    for part in parts {
        let Some(params) = part.pointer("/template/params") else {
            continue;
        };

        // Book identifiers are extracted before (and regardless of) the URL
        // gate below: a URL-less {{cite book}} is exactly the case PRD-0009
        // exists for, and a template with both url= and isbn= yields both.
        if let Some(book) = template_book_source(params) {
            books.push(book);
        }

        // Extract the primary url. CS1/CS2 cite templates carry the fetchable
        // source in `url=` or, for chapter/section-level refs, in a URL-valued
        // alias (`chapter-url=`, `conference-url=`, …). Check `url=` first, then
        // the aliases in order, so an online citation without a top-level `url=`
        // is still verified rather than dropped as a non-URL source.
        let Some(primary_url) = URL_PARAM_ALIASES.iter().find_map(|key| {
            params
                .pointer(&format!("/{key}/wt"))
                .and_then(|v| v.as_str())
                .and_then(|wt| url::Url::parse(wt.trim()).ok())
        }) else {
            continue; // No url in any known alias; an orphan archive-url is not a citable source.
        };

        let primary_str = primary_url.to_string();
        seen_urls.insert(primary_str.clone());

        // Extract archive URLs (in order: archive-url first, then archiveurl).
        let mut archive_urls = Vec::new();
        for key in ["archive-url", "archiveurl"] {
            if let Some(wt) = params
                .pointer(&format!("/{key}/wt"))
                .and_then(|v| v.as_str())
                && let Ok(u) = url::Url::parse(wt.trim())
            {
                let archive_str = u.to_string();
                if !seen_urls.contains(&archive_str) {
                    seen_urls.insert(archive_str);
                    archive_urls.push(u);
                }
            }
        }

        sources.push(CitedSource {
            url: primary_url,
            archive_urls,
        });
    }
}

/// Read a template part's book identifiers (`isbn`/`oclc`/`lccn`/`ol`, with
/// their uppercase aliases) and cited page (`page`/`p`/`pages`/`pp`), each
/// validated/normalized by the `BookIdentifier` constructors. `None` when no
/// parameter yields a valid identifier — an invalid ISBN is "no identifier",
/// never a guess (ADR-0024 Decision 1).
fn template_book_source(params: &serde_json::Value) -> Option<BookSource> {
    type Constructor = fn(&str) -> Option<BookIdentifier>;

    let param = |keys: &[&str]| {
        keys.iter().find_map(|key| {
            params
                .pointer(&format!("/{key}/wt"))
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
        })
    };

    let schemes: [(&[&str], Constructor); 4] = [
        (&["isbn", "ISBN"], BookIdentifier::isbn),
        (&["oclc", "OCLC"], BookIdentifier::oclc),
        (&["lccn", "LCCN"], BookIdentifier::lccn),
        (&["ol", "OL"], BookIdentifier::olid),
    ];
    let identifiers: Vec<BookIdentifier> = schemes
        .iter()
        .filter_map(|(keys, constructor)| param(keys).and_then(constructor))
        .collect();
    if identifiers.is_empty() {
        return None;
    }

    Some(BookSource {
        identifiers,
        cited_page: param(&["page", "p", "pages", "pp"]).map(ToString::to_string),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> ImmutableWikicode {
        let html = include_str!("../tests/fixtures/parsoid_cats.html");
        ImmutableWikicode::new(html)
    }

    #[test]
    fn extracts_blocks_with_section_refs_and_urls() {
        let blocks = blocks_from_revision(&fixture()).expect("blocks");
        assert!(!blocks.is_empty(), "should find prose blocks");

        // The Etymology prose block, found by its content.
        let etymology_block = blocks
            .iter()
            .find(|b| b.text.contains("Felis catus"))
            .expect("should find Etymology block");

        // The Etymology block contains the fixture text (ends with
        // "Felis catus" and "differ") minus the bracketed ref markers
        assert!(
            etymology_block.text.contains("Felis catus"),
            "block text should contain 'Felis catus'"
        );
        assert!(
            etymology_block.text.contains("differ"),
            "block text should contain 'differ'"
        );
        // Verify markers [1] and [2] are NOT in the text (they're stripped)
        assert!(
            !etymology_block.text.contains("[1]"),
            "marker [1] should be stripped from text"
        );
        assert!(
            !etymology_block.text.contains("[2]"),
            "marker [2] should be stripped from text"
        );

        // The Etymology block has exactly 2 refs (from the fixture)
        assert_eq!(
            etymology_block.refs.len(),
            2,
            "Etymology block should have 2 refs"
        );

        // First ref: cite-template with primary URL https://example.com/cat-origins
        // and archive-url https://web.archive.org/web/20240101/example.com/cat-origins
        // (grouped in one CitedSource, not two separate sources)
        let cite_ref = &etymology_block.refs[0];
        assert_eq!(cite_ref.ref_id, "cite_ref-ety_1-0");
        assert_eq!(
            cite_ref.sources.len(),
            1,
            "cite-template should yield exactly one CitedSource (primary + archives grouped)"
        );
        assert_eq!(
            cite_ref.sources[0].url.as_str(),
            "https://example.com/cat-origins",
            "primary URL should be extracted from template"
        );
        assert_eq!(
            cite_ref.sources[0].archive_urls.len(),
            1,
            "cite-template should have one archive URL grouped with primary"
        );
        assert_eq!(
            cite_ref.sources[0].archive_urls[0].as_str(),
            "https://web.archive.org/web/20240101/example.com/cat-origins",
            "archive-url param should be grouped as archive fallback"
        );
        // Offset should be at the position of "Felis catus" (roughly where [1] was)
        assert!(
            cite_ref.offset > 0 && cite_ref.offset < etymology_block.text.len(),
            "cite-template ref offset should be inside text"
        );

        // Second ref: bare ExtLink with an external URL https://www.etymonline.com/word/cat
        // (the rendered live-url in the citation should NOT create a duplicate CitedSource
        // because it was already seen in the template extraction)
        let extlink_ref = &etymology_block.refs[1];
        assert_eq!(extlink_ref.ref_id, "cite_ref-orig_2-0");
        assert_eq!(extlink_ref.sources.len(), 1);
        assert_eq!(
            extlink_ref.sources[0].url.as_str(),
            "https://www.etymonline.com/word/cat",
            "bare ExtLink URL should be extracted"
        );
        assert_eq!(
            extlink_ref.sources[0].archive_urls.len(),
            0,
            "bare ExtLink should have no archive URLs"
        );
        assert!(
            extlink_ref.offset > 0 && extlink_ref.offset <= etymology_block.text.len(),
            "bare-URL ref offset should be inside text bounds"
        );

        // Bareness: the cite-template ref is NOT a bare-URL-repair target; the ref whose whole
        // rendered content is a single URL IS. This is the signal that lets the browser route
        // "Fix citation" to bare-URL repair only when a finding's own ref is genuinely bare.
        assert!(
            !cite_ref.is_bare_url_ref,
            "a cite-template ref is not a bare-URL-repair target"
        );
        assert!(
            extlink_ref.is_bare_url_ref,
            "a ref whose whole content is a single bare URL is a bare-URL-repair target"
        );
    }

    #[test]
    fn cite_template_resolver_links_are_not_extra_sources() {
        // A {{cite journal}} renders resolver links (DOI/PMID/PMC) that are NOT the
        // `url=` param. Parsoid groups the whole rendered citation under the
        // transclusion's shared `about` id. Those links must not be counted as
        // additional bare-URL sources — only a *literal* bare URL in the ref (one
        // outside any transclusion group) is a separate source. Otherwise a single
        // citation inflates use-site stats and model calls by verifying doi.org as a
        // phantom second source.
        let html = "<!DOCTYPE html>\n\
<html prefix=\"dc: http://purl.org/dc/terms/ mw: http://mediawiki.org/rdf/\">\n\
<head><meta charset=\"utf-8\"/></head>\n\
<body class=\"mw-parser-output\">\n\
<section data-mw-section-id=\"1\">\n\
<h2>Findings</h2>\n\
<p>A study reports a result.<sup about=\"#mwt1\" class=\"mw-ref reference\" id=\"cite_ref-j_1-0\" rel=\"dc:references\" typeof=\"mw:Extension/ref\" data-mw='{\"name\":\"ref\",\"attrs\":{\"name\":\"j\"},\"body\":{\"id\":\"mw-reference-text-cite_note-j-1\"}}'><a href=\"#cite_note-j-1\"><span class=\"mw-reflink-text\">[1]</span></a></sup></p>\n\
</section>\n\
<div class=\"mw-references-wrap\" typeof=\"mw:Extension/references\" about=\"#mwt-refs\" data-mw='{\"name\":\"references\",\"attrs\":{},\"autoGenerated\":true}'>\n\
<ol class=\"mw-references references\">\n\
<li about=\"#cite_note-j-1\" id=\"cite_note-j-1\"><a href=\"#cite_ref-j_1-0\" rel=\"mw:referencedBy\"><span class=\"mw-linkback-text\">↑</span></a> <span id=\"mw-reference-text-cite_note-j-1\" class=\"mw-reference-text\"><link rel=\"mw-deduplicated-inline-style\" href=\"mw-data:TemplateStyles:r1\" about=\"#mwt4\" typeof=\"mw:Extension/templatestyles mw:Transclusion\" data-mw='{\"parts\":[{\"template\":{\"target\":{\"wt\":\"cite journal\",\"href\":\"./Template:Cite_journal\"},\"params\":{\"title\":{\"wt\":\"Some Article\"},\"url\":{\"wt\":\"https://journal.example.org/article\"},\"doi\":{\"wt\":\"10.1234/abc\"}},\"i\":0}}]}'/><cite about=\"#mwt4\" class=\"citation journal\">&quot;Some Article&quot;. <a rel=\"mw:ExtLink nofollow\" href=\"https://journal.example.org/article\" class=\"external text\">journal.example.org</a>. <a rel=\"mw:ExtLink nofollow\" href=\"https://doi.org/10.1234/abc\" class=\"external text\">10.1234/abc</a>.</cite></span></li>\n\
</ol>\n\
</div>\n\
</body>\n\
</html>";

        let revision = ImmutableWikicode::new(html);
        let blocks = blocks_from_revision(&revision).expect("blocks");
        let block = blocks
            .iter()
            .find(|b| b.text.contains("A study reports"))
            .expect("should find the findings block");
        assert_eq!(block.refs.len(), 1, "block should have one ref");
        let r = &block.refs[0];

        let urls: Vec<&str> = r.sources.iter().map(|s| s.url.as_str()).collect();
        assert!(
            !urls.iter().any(|u| u.contains("doi.org")),
            "the template-rendered DOI resolver link must not become a source (got {urls:?})"
        );
        assert_eq!(
            r.sources.len(),
            1,
            "cite template yields exactly one source — its url= param, not the rendered resolver links (got {urls:?})"
        );
        assert_eq!(
            r.sources[0].url.as_str(),
            "https://journal.example.org/article"
        );
        assert!(
            !r.is_bare_url_ref,
            "a cite-template ref (even one rendering bare-looking resolver links) is not bare"
        );
    }

    #[test]
    fn offsets_adjusted_for_leading_whitespace() {
        // Regression test: ensure ref offsets are correct relative to trimmed text,
        // not untrimmed text with leading whitespace.
        //
        // The old buggy code recorded offsets against the untrimmed accumulator,
        // then trimmed the text, leaving offsets pointing into the wrong positions
        // (or past the end). The fix adjusts offsets by the leading-whitespace byte count.
        //
        // This test uses a synthetic Parsoid HTML with a paragraph that has LEADING
        // WHITESPACE and a mid-block reference (placed after the first sentence).
        // The trimmed text loses the leading spaces, so offsets MUST be adjusted or
        // they will point to the wrong sentence — this exposes the bug.

        // Synthetic Parsoid HTML: section + paragraph with leading spaces + mid-block ref.
        // The paragraph text is: "  Cats purr.<ref/> Cats sleep." (two leading spaces)
        // After trimming: "Cats purr. Cats sleep." (spaces removed)
        let synthetic_html = "<!DOCTYPE html>\n\
<html prefix=\"dc: http://purl.org/dc/terms/ mw: http://mediawiki.org/rdf/\">\n\
<head>\n\
<meta charset=\"utf-8\"/>\n\
<base href=\"//en.wikipedia.org/wiki/\"/>\n\
<title>Test</title>\n\
</head>\n\
<body class=\"mw-parser-output\">\n\
<section data-mw-section-id=\"1\">\n\
<h2>Test Section</h2>\n\
<p>  Cats purr.<sup about=\"#mwt1\" class=\"mw-ref reference\" id=\"cite_ref-test-1\" rel=\"dc:references\" typeof=\"mw:Extension/ref\" data-mw='{\"name\":\"ref\",\"attrs\":{},\"body\":{\"id\":\"mw-reference-text-cite_note-test-1\"}}'><a href=\"#cite_note-test-1\"><span class=\"mw-reflink-text\">[1]</span></a></sup> Cats sleep.</p>\n\
</section>\n\
<div class=\"mw-references-wrap\" typeof=\"mw:Extension/references\" about=\"#mwt-refs\" data-mw='{\"name\":\"references\",\"attrs\":{},\"autoGenerated\":true}'>\n\
<ol class=\"mw-references references\">\n\
<li about=\"#cite_note-test-1\" id=\"cite_note-test-1\"><a href=\"#cite_ref-test-1\" rel=\"mw:referencedBy\"><span class=\"mw-linkback-text\">↑</span></a> <span id=\"mw-reference-text-cite_note-test-1\" class=\"mw-reference-text\"><a rel=\"mw:ExtLink\" href=\"https://example.com/test\">[Test]</a></span></li>\n\
</ol>\n\
</div>\n\
</body>\n\
</html>";

        let revision = ImmutableWikicode::new(synthetic_html);
        let blocks = blocks_from_revision(&revision).expect("blocks");

        // We expect at least one block with content (the paragraph with the ref).
        // The references section may also generate a block, so we find the paragraph block.
        assert!(!blocks.is_empty(), "should have at least one block");
        let block = blocks
            .iter()
            .find(|b| b.text.contains("Cats purr"))
            .expect("should find paragraph block containing 'Cats purr'");

        // The block text should be trimmed (no leading spaces).
        assert!(
            !block.text.starts_with(' '),
            "block text should be trimmed (no leading spaces)"
        );
        assert_eq!(block.text, "Cats purr. Cats sleep.");

        // There should be exactly one ref.
        assert_eq!(block.refs.len(), 1, "block should have one ref");
        let r = &block.refs[0];

        // The ref's offset should be valid and should index into the trimmed text.
        assert!(
            r.offset <= block.text.len(),
            "ref offset should not exceed text length"
        );

        // Most importantly: the offset should point to a position at or before "Cats sleep",
        // i.e., somewhere in or just after "Cats purr. ".
        // "Cats purr." is 10 chars, so the offset should be <= 10.
        // The ref marker came AFTER "Cats purr.", so offset should be 10 (the position
        // where the second "Cats" starts).
        assert!(
            r.offset <= 10,
            "ref offset should point into or just after first sentence (expected <= 10, got {r_offset})",
            r_offset = r.offset
        );

        // Verify that indexing into the text at r.offset works correctly.
        let text_before_ref = &block.text[..r.offset];
        assert!(
            text_before_ref.ends_with("Cats purr.") || text_before_ref.ends_with("purr."),
            "text before ref should end with purr period (got: {text_before_ref})"
        );

        // On the buggy code (offsets against untrimmed, then trim), the offset would have
        // been ~12 (the position in the untrimmed string including the 2 leading spaces),
        // which after trim would incorrectly point into "Cats sleep" or past the text.
        // This assertion would fail on that buggy code, making the test red.
    }

    #[test]
    fn push_template_sources_extracts_archive_url() {
        // Test that push_template_sources correctly extracts primary URL and archive-url param
        // from a cite template data-mw structure.
        let data_mw = r#"{"parts":[{"template":{"target":{"wt":"cite web","href":"./Template:Cite_web"},"params":{"url":{"wt":"https://example.org/article"},"archive-url":{"wt":"https://web.archive.org/web/20240101/example.org/article"},"title":{"wt":"Example Article"}},"i":0}}]}"#;

        let mut sources = Vec::new();
        let mut books = Vec::new();
        let mut seen_urls = std::collections::HashSet::new();
        super::push_template_sources(data_mw, &mut sources, &mut books, &mut seen_urls);

        // Should extract exactly one CitedSource with primary URL and one archive URL.
        assert_eq!(
            sources.len(),
            1,
            "should extract one source from cite template"
        );
        assert_eq!(
            sources[0].url.as_str(),
            "https://example.org/article",
            "primary URL should be extracted"
        );
        assert_eq!(
            sources[0].archive_urls.len(),
            1,
            "should have one archive URL"
        );
        assert_eq!(
            sources[0].archive_urls[0].as_str(),
            "https://web.archive.org/web/20240101/example.org/article",
            "archive-url param should be extracted"
        );

        // Both URLs should be in seen_urls for dedup purposes.
        assert!(seen_urls.contains("https://example.org/article"));
        assert!(seen_urls.contains("https://web.archive.org/web/20240101/example.org/article"));
    }

    #[test]
    fn push_template_sources_handles_multiple_archives() {
        // Test that both archive-url and archiveurl params are collected (in order).
        let data_mw = r#"{"parts":[{"template":{"target":{"wt":"cite web","href":"./Template:Cite_web"},"params":{"url":{"wt":"https://example.org"},"archive-url":{"wt":"https://web.archive.org/web/20240101/example.org"},"archiveurl":{"wt":"https://archive.is/example.org"},"title":{"wt":"Example"}},"i":0}}]}"#;

        let mut sources = Vec::new();
        let mut books = Vec::new();
        let mut seen_urls = std::collections::HashSet::new();
        super::push_template_sources(data_mw, &mut sources, &mut books, &mut seen_urls);

        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].archive_urls.len(), 2);
        // archive-url comes first, archiveurl second.
        assert_eq!(
            sources[0].archive_urls[0].as_str(),
            "https://web.archive.org/web/20240101/example.org"
        );
        assert_eq!(
            sources[0].archive_urls[1].as_str(),
            "https://archive.is/example.org"
        );
    }

    #[test]
    fn push_template_sources_skips_part_without_primary_url() {
        // Test that parts without a primary url param are skipped entirely,
        // including any orphan archive-url (an archive without a primary is not a citable source).
        let data_mw = r#"{"parts":[{"template":{"target":{"wt":"cite","href":"./Template:Cite"},"params":{"archive-url":{"wt":"https://archive.org/web/example"},"title":{"wt":"No URL"}},"i":0}}]}"#;

        let mut sources = Vec::new();
        let mut books = Vec::new();
        let mut seen_urls = std::collections::HashSet::new();
        super::push_template_sources(data_mw, &mut sources, &mut books, &mut seen_urls);

        // Should extract nothing.
        assert!(
            sources.is_empty(),
            "should not extract source without primary URL"
        );
        assert!(
            seen_urls.is_empty(),
            "archive URL without primary should not be recorded"
        );
        assert!(books.is_empty(), "no identifier params, no book source");
    }

    #[test]
    fn push_template_sources_uses_url_alias_when_no_top_level_url() {
        // A cite template carrying its source in `chapter-url=` (no top-level
        // `url=`) must still yield a citable source, not be dropped as non-URL.
        let data_mw = r#"{"parts":[{"template":{"target":{"wt":"cite book"},"params":{"chapter-url":{"wt":"https://example.org/chapter"},"title":{"wt":"A Book"}},"i":0}}]}"#;
        let mut sources = Vec::new();
        let mut books = Vec::new();
        let mut seen_urls = std::collections::HashSet::new();
        super::push_template_sources(data_mw, &mut sources, &mut books, &mut seen_urls);
        assert_eq!(sources.len(), 1, "chapter-url should produce one source");
        assert_eq!(sources[0].url.as_str(), "https://example.org/chapter");
    }

    #[test]
    fn push_template_sources_accepts_no_hyphen_url_alias() {
        // The no-hyphen CS1 spelling (`chapterurl=`) is an accepted alias and
        // must extract just like `chapter-url=` (SP42#62).
        let data_mw = r#"{"parts":[{"template":{"target":{"wt":"cite book"},"params":{"chapterurl":{"wt":"https://example.org/chapter"},"title":{"wt":"A Book"}},"i":0}}]}"#;
        let mut sources = Vec::new();
        let mut books = Vec::new();
        let mut seen_urls = std::collections::HashSet::new();
        super::push_template_sources(data_mw, &mut sources, &mut books, &mut seen_urls);
        assert_eq!(sources.len(), 1, "chapterurl should produce one source");
        assert_eq!(sources[0].url.as_str(), "https://example.org/chapter");
    }

    #[test]
    fn push_template_sources_prefers_top_level_url_over_alias() {
        // When both `url=` and an alias are present, `url=` wins as primary.
        let data_mw = r#"{"parts":[{"template":{"target":{"wt":"cite book"},"params":{"url":{"wt":"https://example.org/main"},"chapter-url":{"wt":"https://example.org/chapter"}},"i":0}}]}"#;
        let mut sources = Vec::new();
        let mut books = Vec::new();
        let mut seen_urls = std::collections::HashSet::new();
        super::push_template_sources(data_mw, &mut sources, &mut books, &mut seen_urls);
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].url.as_str(), "https://example.org/main");
    }

    #[test]
    fn url_less_cite_book_yields_a_book_source_not_a_cited_source() {
        // The PRD-0009 case: {{cite book |isbn=… |page=…}} with no url= must
        // yield a validated book identifier instead of nothing.
        let data_mw = r#"{"parts":[{"template":{"target":{"wt":"cite book","href":"./Template:Cite_book"},"params":{"isbn":{"wt":"978-0-14-032872-1"},"title":{"wt":"Matilda"},"page":{"wt":"42"}},"i":0}}]}"#;
        let mut sources = Vec::new();
        let mut books = Vec::new();
        let mut seen_urls = std::collections::HashSet::new();
        super::push_template_sources(data_mw, &mut sources, &mut books, &mut seen_urls);

        assert!(sources.is_empty(), "no url param, no cited source");
        assert_eq!(books.len(), 1, "isbn param should yield one book source");
        assert_eq!(
            books[0].identifiers,
            vec![BookIdentifier::Isbn("9780140328721".to_string())]
        );
        assert_eq!(books[0].cited_page.as_deref(), Some("42"));
    }

    #[test]
    fn cite_book_with_url_and_isbn_yields_both_kinds() {
        let data_mw = r#"{"parts":[{"template":{"target":{"wt":"cite book"},"params":{"url":{"wt":"https://example.org/book"},"isbn":{"wt":"9780140328721"},"oclc":{"wt":"ocm12345678"}},"i":0}}]}"#;
        let mut sources = Vec::new();
        let mut books = Vec::new();
        let mut seen_urls = std::collections::HashSet::new();
        super::push_template_sources(data_mw, &mut sources, &mut books, &mut seen_urls);

        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].url.as_str(), "https://example.org/book");
        assert_eq!(books.len(), 1);
        assert_eq!(
            books[0].identifiers,
            vec![
                BookIdentifier::Isbn("9780140328721".to_string()),
                BookIdentifier::Oclc("12345678".to_string()),
            ]
        );
        assert_eq!(books[0].cited_page, None);
    }

    #[test]
    fn invalid_identifier_values_yield_no_book_source() {
        // A garbled ISBN (bad checksum) and an author OLID are "no identifier",
        // never sent upstream (ADR-0024 Decision 1).
        let data_mw = r#"{"parts":[{"template":{"target":{"wt":"cite book"},"params":{"isbn":{"wt":"978-0-14-032872-2"},"ol":{"wt":"23919A"},"title":{"wt":"Garbled"}},"i":0}}]}"#;
        let mut sources = Vec::new();
        let mut books = Vec::new();
        let mut seen_urls = std::collections::HashSet::new();
        super::push_template_sources(data_mw, &mut sources, &mut books, &mut seen_urls);
        assert!(sources.is_empty());
        assert!(
            books.is_empty(),
            "invalid values must not become identifiers"
        );
    }

    #[test]
    fn blocks_from_revision_carries_book_sources_onto_the_ref() {
        // End-to-end through the DOM pass: a reference whose contents hold a
        // url-less cite book lands on the BlockRef as a book source with empty
        // cited sources (the extract layer then refines the skip reason).
        let html = "<html><body>\
<section data-mw-section-id=\"1\">\n\
<p>Cats purr.<sup about=\"#mwt1\" class=\"mw-ref reference\" id=\"cite_ref-book_1-0\" rel=\"dc:references\" typeof=\"mw:Extension/ref\" data-mw='{\"name\":\"ref\",\"attrs\":{\"name\":\"book\"},\"body\":{\"id\":\"mw-reference-text-cite_note-book-1\"}}'><a href=\"#cite_note-book-1\"><span class=\"mw-reflink-text\">[1]</span></a></sup></p>\n\
<div class=\"mw-references-wrap\" typeof=\"mw:Extension/references\" about=\"#mwt-refs\" data-mw='{\"name\":\"references\",\"attrs\":{},\"autoGenerated\":true}'>\n\
<ol class=\"mw-references references\"><li about=\"#cite_note-book-1\" id=\"cite_note-book-1\"><span rel=\"mw:referencedBy\"><a href=\"#cite_ref-book_1-0\">↑</a></span> <span id=\"mw-reference-text-cite_note-book-1\" class=\"mw-reference-text\"><span about=\"#mwt5\" typeof=\"mw:Transclusion\" data-mw='{\"parts\":[{\"template\":{\"target\":{\"wt\":\"cite book\",\"href\":\"./Template:Cite_book\"},\"params\":{\"isbn\":{\"wt\":\"978-0-14-032872-1\"},\"title\":{\"wt\":\"Matilda\"},\"page\":{\"wt\":\"42\"}},\"i\":0}}]}'>Dahl, Roald. Matilda. p. 42.</span></span></li></ol>\n\
</div>\n\
</section></body></html>";
        let revision = ImmutableWikicode::new(html);
        let blocks = blocks_from_revision(&revision).expect("blocks");
        let block = blocks
            .iter()
            .find(|b| !b.refs.is_empty())
            .expect("a block with the ref");
        let r = &block.refs[0];
        assert!(
            r.sources.is_empty(),
            "url-less cite book has no cited source"
        );
        assert_eq!(r.book_sources.len(), 1);
        assert_eq!(
            r.book_sources[0].identifiers,
            vec![BookIdentifier::Isbn("9780140328721".to_string())]
        );
        assert_eq!(r.book_sources[0].cited_page.as_deref(), Some("42"));
    }

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
        // and the id has no CITEREF prefix. The fixture uses a literal UTF-8 accented id.
        let html = include_str!("../tests/fixtures/parsoid_harvsp_frwiki.html");
        let code = Wikicode::new(html);
        let index = biblio_index(&code);
        assert!(index.contains_key("Martin-Demézil1986"));
    }

    #[test]
    fn biblio_index_ignores_idless_and_bookless_elements() {
        let html = include_str!("../tests/fixtures/parsoid_cats.html");
        let code = Wikicode::new(html);
        assert!(biblio_index(&code).is_empty());
    }

    /// Helper: load a fixture by name and return parsed blocks.
    fn blocks_from_fixture(name: &str) -> Vec<ParsoidBlock> {
        let html = match name {
            "parsoid_sfn_enwiki.html" => include_str!("../tests/fixtures/parsoid_sfn_enwiki.html"),
            "parsoid_harvsp_frwiki.html" => {
                include_str!("../tests/fixtures/parsoid_harvsp_frwiki.html")
            }
            "parsoid_magiclink_dewiki.html" => {
                include_str!("../tests/fixtures/parsoid_magiclink_dewiki.html")
            }
            _ => panic!("unknown fixture: {name}"),
        };
        let revision = ImmutableWikicode::new(html);
        blocks_from_revision(&revision).expect("blocks_from_revision")
    }

    #[test]
    fn sfn_ref_resolves_to_the_bibliography_book_source_with_its_own_pages() {
        let blocks = blocks_from_fixture("parsoid_sfn_enwiki.html");
        let block = blocks.first().expect("at least one block");
        // Find the sfn ref by ref_id containing "Roxburgh"
        let r = block
            .refs
            .iter()
            .find(|ref_item| ref_item.ref_id.contains("Roxburgh2014"))
            .expect("should find Roxburgh2014 ref");

        assert_eq!(
            r.book_sources.len(),
            1,
            "sfn should resolve to bibliography entry"
        );
        assert_eq!(
            r.book_sources[0].identifiers,
            vec![BookIdentifier::isbn("978-1-84583-093-9").expect("valid")]
        );
        // The page range comes from the sfn's own pp param, NOT the cite book's.
        assert_eq!(r.book_sources[0].cited_page.as_deref(), Some("113–116"));
    }

    #[test]
    fn harvsp_ref_resolves_a_non_ascii_prefixless_fragment() {
        let blocks = blocks_from_fixture("parsoid_harvsp_frwiki.html");
        let block = blocks.first().expect("at least one block");
        // The frwiki fixture should have a harvsp ref with non-ASCII (literal UTF-8 accented) anchor
        let r = block.refs.first().expect("should find a ref");

        assert_eq!(
            r.book_sources.len(),
            1,
            "harvsp should resolve to bibliography entry via literal UTF-8 fragment"
        );
        assert_eq!(
            r.book_sources[0].identifiers[0],
            BookIdentifier::isbn("978-2-85822-660-3").expect("valid")
        );
    }

    #[test]
    fn unresolvable_short_cite_yields_no_book_source_and_flags_the_ref() {
        let blocks = blocks_from_fixture("parsoid_sfn_enwiki.html");
        let block = blocks.first().expect("at least one block");
        // Find the sfn ref that points to CITEREFNowhere2020 (unresolved)
        let r = block
            .refs
            .iter()
            .find(|ref_item| ref_item.ref_id.contains("Nowhere2020"))
            .expect("should find Nowhere2020 ref");

        assert!(r.book_sources.is_empty(), "never a guessed identifier");
        assert!(r.short_cite_unresolved, "should flag unresolved short-cite");
    }

    #[test]
    fn direct_cite_book_extraction_is_unchanged() {
        let blocks = blocks_from_fixture("parsoid_sfn_enwiki.html");
        let block = blocks.first().expect("at least one block");
        // Find the direct cite-book ref (not a short-cite, should extract directly)
        let r = block
            .refs
            .iter()
            .find(|ref_item| ref_item.ref_id.contains("direct"))
            .expect("should find direct cite-book ref");

        assert_eq!(r.book_sources.len(), 1, "direct lane regression");
        assert!(
            !r.short_cite_unresolved,
            "direct cites should not be flagged"
        );
    }

    #[test]
    fn ref_local_isbn_magiclinks_become_book_sources() {
        let blocks = blocks_from_fixture("parsoid_magiclink_dewiki.html");
        let r = blocks
            .first()
            .expect("at least one block")
            .refs
            .first()
            .expect("at least one ref");
        assert_eq!(
            r.book_sources.len(),
            3,
            "all three magiclinks, checksum-valid"
        );
        assert!(
            r.book_sources.iter().all(|b| b.cited_page.is_none()),
            "MVP: no free-text page parse"
        );
    }

    #[test]
    fn short_cite_resolves_a_template_less_magiclink_bibliography_item() {
        // Codex P2 (PR 153): a hand-written bibliography li with an id and a
        // magiclink ISBN but no transclusion wrapper must still be indexed,
        // so a short cite targeting it resolves instead of flagging.
        let blocks = blocks_from_fixture("parsoid_magiclink_dewiki.html");
        let block = blocks
            .iter()
            .find(|b| b.text.contains("erweitert"))
            .expect("anchored-ref block");
        let r = block.refs.first().expect("the harvnb ref");
        assert!(!r.short_cite_unresolved, "must resolve, not flag");
        assert_eq!(r.book_sources.len(), 1);
        assert_eq!(
            r.book_sources[0].identifiers,
            vec![BookIdentifier::isbn("978-0-306-40615-7").expect("valid isbn")]
        );
    }

    #[test]
    fn short_cite_without_a_page_keeps_the_bibliography_entry_page() {
        // Codex P2 (PR 153): the short-cite page wins when present, but its
        // absence must not erase the bibliography template's own page.
        let indexed = BookSource {
            identifiers: vec![BookIdentifier::isbn("978-0-306-40615-7").expect("valid")],
            cited_page: Some("42".to_string()),
        };
        let no_page_part: serde_json::Value = serde_json::json!({
            "template": {"target": {"wt": "sfn"}, "params": {"1": {"wt": "Doe"}, "2": {"wt": "2001"}}}
        });
        let resolved =
            resolve_book_from_index(&no_page_part, &indexed, &std::collections::HashSet::new());
        assert_eq!(resolved.cited_page.as_deref(), Some("42"), "preserved");

        let paged_part: serde_json::Value = serde_json::json!({
            "template": {"target": {"wt": "sfn"}, "params": {"1": {"wt": "Doe"}, "2": {"wt": "2001"}, "pp": {"wt": "7–9"}}}
        });
        let resolved =
            resolve_book_from_index(&paged_part, &indexed, &std::collections::HashSet::new());
        assert_eq!(resolved.cited_page.as_deref(), Some("7–9"), "override wins");
    }

    #[test]
    fn isbn_template_transclusion_in_a_ref_becomes_a_book_source() {
        // {{ISBN|978-…}} renders as a transclusion whose target.wt is "ISBN";
        // add a ref carrying one to the enwiki fixture (enwiki dropped magic
        // links in 2017 — the template is its replacement).
        let blocks = blocks_from_fixture("parsoid_sfn_enwiki.html");
        let block = blocks.first().expect("at least one block");
        // Find the ISBN-template ref (add it to the fixture in this task)
        let r = block
            .refs
            .iter()
            .find(|ref_item| ref_item.ref_id.contains("isbn-template"))
            .expect("should find ISBN-template ref");
        assert_eq!(r.book_sources.len(), 1);
    }

    #[test]
    fn bundled_short_cites_bind_to_their_own_targets_in_order() {
        // Codex round 3 (PR 153): a ref bundling two {{sfn}} parts must
        // resolve each against its own body link, in order — not bind both
        // to the first link or dedupe the second away.
        let blocks = blocks_from_fixture("parsoid_sfn_enwiki.html");
        let block = blocks.first().expect("prose block");
        let r = block
            .refs
            .iter()
            .find(|r| r.ref_id.contains("bundled"))
            .expect("bundled ref");
        assert!(!r.short_cite_unresolved);
        assert_eq!(r.book_sources.len(), 2, "one source per short cite");
        assert_eq!(
            r.book_sources[0].identifiers,
            vec![BookIdentifier::isbn("978-1-84583-093-9").expect("valid")]
        );
        assert_eq!(r.book_sources[0].cited_page.as_deref(), Some("12"));
        assert_eq!(
            r.book_sources[1].identifiers,
            vec![BookIdentifier::isbn("978-0-306-40615-7").expect("valid")]
        );
        assert_eq!(r.book_sources[1].cited_page.as_deref(), Some("99"));
    }

    #[test]
    fn mixed_transclusion_keeps_the_non_short_parts() {
        // Codex round 3 (PR 153): one data-mw whose parts mix a short cite
        // and a normal cite template must collect BOTH — the short cite via
        // the bibliography, the direct template via its own params.
        let blocks = blocks_from_fixture("parsoid_harvsp_frwiki.html");
        let block = blocks.first().expect("prose block");
        let r = block
            .refs
            .iter()
            .find(|r| r.ref_id.contains("mixed"))
            .expect("mixed ref");
        assert!(!r.short_cite_unresolved);
        assert_eq!(r.book_sources.len(), 2, "indexed + direct");
        assert_eq!(
            r.book_sources[0].identifiers,
            vec![BookIdentifier::isbn("978-2-85822-660-3").expect("valid")],
            "harvsp resolves the indexed Ouvrage"
        );
        assert_eq!(r.book_sources[0].cited_page.as_deref(), Some("77"));
        assert_eq!(
            r.book_sources[1].identifiers,
            vec![BookIdentifier::isbn("2-85822-000-X").expect("valid")],
            "direct Ouvrage part still collected"
        );
    }

    #[test]
    fn bundled_first_target_missing_does_not_steal_the_second() {
        // Codex round 6 (PR 153): part one's target is absent from the
        // bibliography; part two's resolves. Expected-anchor matching must
        // leave part one unresolved (its own link is an authoritative miss)
        // and bind part two to its own entry with its own page — never shift
        // the later hit onto the earlier template.
        let blocks = blocks_from_fixture("parsoid_sfn_enwiki.html");
        let block = blocks.first().expect("prose block");
        let r = block
            .refs
            .iter()
            .find(|r| r.ref_id.contains("halfbundled"))
            .expect("half-bundled ref");
        assert!(r.short_cite_unresolved, "part one is an honest miss");
        assert_eq!(r.book_sources.len(), 1, "only part two resolves");
        assert_eq!(
            r.book_sources[0].identifiers,
            vec![BookIdentifier::isbn("978-0-306-40615-7").expect("valid")]
        );
        assert_eq!(r.book_sources[0].cited_page.as_deref(), Some("33"));
    }

    #[test]
    fn multi_word_authors_normalize_spaces_to_underscores() {
        // Codex round 9 (PR 153): anchor ids underscore-normalize spaces.
        let part: serde_json::Value = serde_json::json!({
            "template": {"target": {"wt": "sfn"}, "params": {
                "1": {"wt": "Museum of Modern Art"}, "2": {"wt": "2024"}
            }}
        });
        assert_eq!(
            short_cite_candidates(&part),
            vec![
                "CITEREFMuseum_of_Modern_Art2024".to_string(),
                "Museum_of_Modern_Art2024".to_string()
            ]
        );
    }

    #[test]
    fn four_author_short_cites_reconstruct_with_the_year() {
        // Codex round 7 (PR 153): the year is the FIFTH positional param on
        // four-author short cites.
        let part: serde_json::Value = serde_json::json!({
            "template": {"target": {"wt": "sfn"}, "params": {
                "1": {"wt": "Smith"}, "2": {"wt": "Jones"},
                "3": {"wt": "Brown"}, "4": {"wt": "Black"}, "5": {"wt": "1994"}
            }}
        });
        assert_eq!(
            short_cite_candidates(&part),
            vec![
                "CITEREFSmithJonesBrownBlack1994".to_string(),
                "SmithJonesBrownBlack1994".to_string()
            ]
        );
    }

    #[test]
    fn explicit_fragment_miss_never_falls_back_to_reconstruction() {
        // Codex round 2 (PR 153): a body link to an absent anchor (custom
        // ref= / disambiguated keys) must stay unresolved, even when the
        // template params would reconstruct to a DIFFERENT existing entry
        // (Roxburgh/2014 here) — an explicit link is authoritative; anything
        // else is a guessed identifier.
        let blocks = blocks_from_fixture("parsoid_sfn_enwiki.html");
        let block = blocks.first().expect("prose block");
        let r = block
            .refs
            .iter()
            .find(|r| r.ref_id.contains("custom-anchor"))
            .expect("custom-anchor ref");
        assert!(r.book_sources.is_empty(), "never a guessed identifier");
        assert!(r.short_cite_unresolved, "explicit miss flags as unresolved");
    }

    #[test]
    fn sfn_without_a_body_link_resolves_via_the_reconstructed_key() {
        // Test that an sfn template without a fragment href in its body (plain text)
        // can still resolve via the reconstructed CITEREF key from params.
        // The fixture's fourth ref has params 1=Roxburgh 2=2014 which reconstructs
        // to CITEREFRoxburgh2014, matching the existing bibliography entry.
        let blocks = blocks_from_fixture("parsoid_sfn_enwiki.html");
        let block = blocks.first().expect("at least one block");
        // Find the fallback ref (no body link, reconstructed key resolution)
        let r = block
            .refs
            .iter()
            .find(|ref_item| ref_item.ref_id.contains("fallback-roxburgh"))
            .expect("should find fallback-roxburgh ref");

        assert_eq!(
            r.book_sources.len(),
            1,
            "sfn without body link should resolve via reconstructed CITEREF key"
        );
        assert_eq!(
            r.book_sources[0].identifiers[0],
            BookIdentifier::isbn("978-1-84583-093-9").expect("valid"),
            "should resolve to the Roxburgh 2014 bibliography entry"
        );
    }
}
