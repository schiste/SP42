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
use sp42_platform::{
    BlockKind, BlockRef, CitedSource, ParsoidBlock, WikiConfig, WikitextEditorError,
    WikitextPageRef,
};
use std::collections::HashMap;

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

/// Extract prose-bearing blocks from a Parsoid revision, in document order.
///
/// # Errors
///
/// Currently infallible (the `Result` mirrors the editing path and leaves room for a
/// future DOM-interpretation failure).
#[allow(clippy::unnecessary_wraps)]
pub fn blocks_from_revision(
    revision: &ImmutableWikicode,
) -> Result<Vec<ParsoidBlock>, WikitextEditorError> {
    let code = Wikicode::new(revision.html());

    // Map Reference.id() -> cited sources (primary URL + archive fallbacks),
    // read structurally from cite templates and bare ExtLinks inside each reference's contents.
    let mut ref_sources: HashMap<String, Vec<CitedSource>> = HashMap::new();
    for reference in code.filter_references() {
        ref_sources.insert(reference.id(), urls_in_reference(&reference));
    }

    let mut blocks = Vec::new();
    let mut ordinal = 0usize;
    walk(&code, &ref_sources, &mut blocks, &mut ordinal);
    Ok(blocks)
}

/// Recursive walker — emit blocks, don't recurse into a block once emitted.
fn walk(
    node: &impl WikinodeIterator,
    ref_sources: &HashMap<String, Vec<CitedSource>>,
    blocks: &mut Vec<ParsoidBlock>,
    ordinal: &mut usize,
) {
    for child in node.children() {
        if let Some(kind) = block_kind(&child) {
            blocks.push(build_block(&child, kind, *ordinal, ref_sources));
            *ordinal += 1;
            continue; // do not descend into an emitted block
        }
        walk(&child, ref_sources, blocks, ordinal);
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
    ref_sources: &HashMap<String, Vec<CitedSource>>,
) -> ParsoidBlock {
    let mut text = String::new();
    let mut refs = Vec::new();
    collect_block(node, &mut text, &mut refs, ref_sources);

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
    ref_sources: &HashMap<String, Vec<CitedSource>>,
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
                sources,
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
        collect_block(&child, text, refs, ref_sources);
    }
}

/// Cited source extraction from a reference's contents.
/// Returns one `CitedSource` per cite-template (primary url + archive fallbacks),
/// plus `CitedSource` entries for *literal* bare `ExtLink`s not already present in
/// templates. Preserves document order: template-derived sources first, then
/// bare-ExtLink sources.
fn urls_in_reference(reference: &Reference) -> Vec<CitedSource> {
    let contents = reference.contents();
    let mut sources = Vec::new();
    let mut seen_urls = std::collections::HashSet::new();

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
    for span in contents.select("[typeof~=\"mw:Transclusion\"]") {
        if let Some(element) = span.as_node().as_element() {
            let attributes = element.attributes.borrow();
            if let Some(data_mw) = attributes.get("data-mw") {
                push_template_sources(data_mw, &mut sources, &mut seen_urls);
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

    sources
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
    "chapter-url",
    "conference-url",
    "contribution-url",
    "article-url",
    "section-url",
    "entry-url",
    "map-url",
    "transcript-url",
];

/// Extract cited sources from a cite-template data-mw.
/// For each template part with a primary url param (`url` or a URL-valued alias),
/// builds one `CitedSource` with that url as primary and
/// `archive-url`/`archiveurl` as fallbacks.
/// Appends to sources and updates `seen_urls` with all URLs (primary + archives).
fn push_template_sources(
    data_mw: &str,
    sources: &mut Vec<CitedSource>,
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
        let mut seen_urls = std::collections::HashSet::new();
        super::push_template_sources(data_mw, &mut sources, &mut seen_urls);

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
        let mut seen_urls = std::collections::HashSet::new();
        super::push_template_sources(data_mw, &mut sources, &mut seen_urls);

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
        let mut seen_urls = std::collections::HashSet::new();
        super::push_template_sources(data_mw, &mut sources, &mut seen_urls);

        // Should extract nothing.
        assert!(
            sources.is_empty(),
            "should not extract source without primary URL"
        );
        assert!(
            seen_urls.is_empty(),
            "archive URL without primary should not be recorded"
        );
    }

    #[test]
    fn push_template_sources_uses_url_alias_when_no_top_level_url() {
        // A cite template carrying its source in `chapter-url=` (no top-level
        // `url=`) must still yield a citable source, not be dropped as non-URL.
        let data_mw = r#"{"parts":[{"template":{"target":{"wt":"cite book"},"params":{"chapter-url":{"wt":"https://example.org/chapter"},"title":{"wt":"A Book"}},"i":0}}]}"#;
        let mut sources = Vec::new();
        let mut seen_urls = std::collections::HashSet::new();
        super::push_template_sources(data_mw, &mut sources, &mut seen_urls);
        assert_eq!(sources.len(), 1, "chapter-url should produce one source");
        assert_eq!(sources[0].url.as_str(), "https://example.org/chapter");
    }

    #[test]
    fn push_template_sources_prefers_top_level_url_over_alias() {
        // When both `url=` and an alias are present, `url=` wins as primary.
        let data_mw = r#"{"parts":[{"template":{"target":{"wt":"cite book"},"params":{"url":{"wt":"https://example.org/main"},"chapter-url":{"wt":"https://example.org/chapter"}},"i":0}}]}"#;
        let mut sources = Vec::new();
        let mut seen_urls = std::collections::HashSet::new();
        super::push_template_sources(data_mw, &mut sources, &mut seen_urls);
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].url.as_str(), "https://example.org/main");
    }
}
