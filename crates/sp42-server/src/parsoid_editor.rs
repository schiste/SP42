//! Parsoid-REST-backed [`WikitextEditor`] (ADR-0003 Decisions 5/6).
//!
//! Per operation: fetch the addressed revision as Parsoid HTML, apply the
//! node edit to the DOM, and POST the edited HTML through the html→wikitext
//! transform so Parsoid re-serializes the page losslessly (selser). The
//! kuchikiki DOM behind [`parsoid::Wikicode`] is `!Send`, so every DOM
//! manipulation lives in a synchronous helper; only [`ImmutableWikicode`]
//! crosses `.await` points.
//!
//! Anchor contract (mirrors the documentation on
//! [`sp42_core::WikitextNodeDescriptor`]): template anchors are the
//! normalized reconstruction `{{name|param=value|…}}` from `data-mw`
//! (positional parameters appear as `1=value`); reference anchors are the
//! normalized plain-text content of the citation.

use async_trait::async_trait;
use parsoid::prelude::*;
use parsoid::{Client, ImmutableWikicode};
use sp42_core::{
    BlockKind, BlockRef, CitedSource, ParsoidBlock, WikiConfig, WikitextEditOutcome,
    WikitextEditRefusal, WikitextEditor, WikitextEditorError, WikitextNodeDescriptor,
    WikitextNodeKind, WikitextNodeLocator, WikitextPageRef, normalize_anchor_text,
};
use std::collections::HashMap;

/// Parsoid-REST production implementation of [`WikitextEditor`].
pub(crate) struct ParsoidWikitextEditor;

impl ParsoidWikitextEditor {
    pub(crate) const fn new() -> Self {
        Self
    }
}

fn editor_client(config: &WikiConfig) -> Result<Client, WikitextEditorError> {
    let Some(parsoid_url) = config.parsoid_url.as_ref() else {
        return Err(WikitextEditorError::NotConfigured {
            wiki_id: config.wiki_id.clone(),
        });
    };
    Client::new(
        parsoid_url.as_str().trim_end_matches('/'),
        sp42_core::branding::USER_AGENT,
    )
    .map_err(|error| WikitextEditorError::Unavailable {
        message: format!("failed to build Parsoid client: {error}"),
        retryable: false,
    })
}

fn map_parsoid_error(error: parsoid::Error) -> WikitextEditorError {
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

fn dom_interpretation_error(error: &parsoid::Error) -> WikitextEditorError {
    WikitextEditorError::Unavailable {
        message: format!("Parsoid HTML could not be interpreted: {error}"),
        retryable: false,
    }
}

fn template_anchor_text(template: &Template) -> String {
    let mut anchor = format!("{{{{{}", template.name_in_wikitext());
    for (name, value) in template.params() {
        anchor.push('|');
        anchor.push_str(&name);
        anchor.push('=');
        anchor.push_str(&value);
    }
    anchor.push_str("}}");
    normalize_anchor_text(&anchor)
}

fn reference_anchor_text(reference: &Reference) -> String {
    normalize_anchor_text(&reference.contents().text_contents())
}

/// Enumerate the nodes of `kind` in `revision`, in document order.
fn enumerate_revision(
    revision: &ImmutableWikicode,
    kind: WikitextNodeKind,
) -> Result<Vec<WikitextNodeDescriptor>, WikitextEditorError> {
    let code = Wikicode::new(revision.html());
    match kind {
        WikitextNodeKind::Template => Ok(code
            .filter_templates()
            .map_err(|error| dom_interpretation_error(&error))?
            .iter()
            .enumerate()
            .map(|(ordinal, template)| WikitextNodeDescriptor {
                kind,
                ordinal,
                anchor_text: template_anchor_text(template),
            })
            .collect()),
        WikitextNodeKind::Reference => Ok(code
            .filter_references()
            .iter()
            .enumerate()
            .map(|(ordinal, reference)| WikitextNodeDescriptor {
                kind,
                ordinal,
                anchor_text: reference_anchor_text(reference),
            })
            .collect()),
    }
}

/// A node edit to apply synchronously to a fetched revision.
enum EditRequest {
    /// Replace the addressed node with the body content of `fragment`
    /// (a Parsoid-parsed rendering of the replacement wikitext).
    Replace { fragment: ImmutableWikicode },
    /// Set parameters on the addressed template.
    SetTemplateParams { params: Vec<(String, String)> },
}

/// Result of applying an [`EditRequest`].
#[derive(Debug)]
enum AppliedEdit {
    /// The DOM edit applied; the edited document, metadata preserved.
    Edited(ImmutableWikicode),
    /// The locator contract refused the edit; nothing changed.
    Refused(WikitextEditRefusal),
}

fn refusal_if_drifted(expected_text: &str, found: String) -> Option<WikitextEditRefusal> {
    let expected = normalize_anchor_text(expected_text);
    // Empty expected_text provides no anti-drift guarantee; always refuse.
    if !expected.is_empty() && expected == found {
        None
    } else {
        Some(WikitextEditRefusal::NodeDrifted { expected, found })
    }
}

fn fragment_body_children(
    fragment: &ImmutableWikicode,
) -> Result<Vec<Wikinode>, WikitextEditorError> {
    let code = Wikicode::new(fragment.html());
    let body = code
        .select_first("body")
        .ok_or_else(|| WikitextEditorError::Unavailable {
            message: "Parsoid fragment has no body element".to_string(),
            retryable: false,
        })?;
    Ok(body.children().collect())
}

fn transclusion_part_count(template: &Template) -> Result<usize, WikitextEditorError> {
    let nodes = template.as_nodes();
    let Some(first) = nodes.first() else {
        return Ok(1);
    };
    let Some(element) = first.as_element() else {
        return Ok(1);
    };
    let attributes = element.attributes.borrow();
    let Some(data_mw) = attributes.get("data-mw") else {
        return Ok(1);
    };
    let value: serde_json::Value =
        serde_json::from_str(data_mw).map_err(|error| WikitextEditorError::Unavailable {
            message: format!("transclusion data-mw is not valid JSON: {error}"),
            retryable: false,
        })?;
    Ok(value
        .get("parts")
        .and_then(serde_json::Value::as_array)
        .map_or(1, Vec::len))
}

/// Apply `edit` to the node addressed by `locator` inside `revision`.
///
/// All DOM work happens synchronously here; the function consumes and
/// returns [`ImmutableWikicode`] so no `!Send` DOM value can leak across an
/// `.await` point in the callers.
fn apply_revision_edit(
    revision: ImmutableWikicode,
    locator: &WikitextNodeLocator,
    edit: &EditRequest,
) -> Result<AppliedEdit, WikitextEditorError> {
    let code = revision.into_mutable();
    match locator.kind {
        WikitextNodeKind::Template => {
            let templates = code
                .filter_templates()
                .map_err(|error| dom_interpretation_error(&error))?;
            let available = templates.len();
            let Some(target) = templates.get(locator.ordinal) else {
                return Ok(AppliedEdit::Refused(
                    WikitextEditRefusal::OrdinalOutOfRange {
                        requested: locator.ordinal,
                        available,
                    },
                ));
            };
            if let Some(refusal) =
                refusal_if_drifted(&locator.expected_text, template_anchor_text(target))
            {
                return Ok(AppliedEdit::Refused(refusal));
            }
            match edit {
                EditRequest::SetTemplateParams { params } => {
                    for (name, value) in params {
                        target.set_param(name, value).map_err(|error| {
                            WikitextEditorError::Unavailable {
                                message: format!(
                                    "failed to set template parameter `{name}`: {error}"
                                ),
                                retryable: false,
                            }
                        })?;
                    }
                }
                EditRequest::Replace { fragment } => {
                    if transclusion_part_count(target)? > 1 {
                        return Err(WikitextEditorError::Unsupported {
                            message: "the addressed transclusion carries multiple template parts; replacing it is not supported".to_string(),
                        });
                    }
                    let children = fragment_body_children(fragment)?;
                    let target_nodes = target.as_nodes();
                    let Some(first_node) = target_nodes.first() else {
                        return Err(WikitextEditorError::Unavailable {
                            message: "the addressed transclusion has no DOM nodes".to_string(),
                            retryable: false,
                        });
                    };
                    for child in children {
                        for node in child.as_nodes() {
                            first_node.insert_before(node);
                        }
                    }
                    target.detach();
                }
            }
        }
        WikitextNodeKind::Reference => {
            let references = code.filter_references();
            let available = references.len();
            let Some(target) = references.get(locator.ordinal) else {
                return Ok(AppliedEdit::Refused(
                    WikitextEditRefusal::OrdinalOutOfRange {
                        requested: locator.ordinal,
                        available,
                    },
                ));
            };
            if let Some(refusal) =
                refusal_if_drifted(&locator.expected_text, reference_anchor_text(target))
            {
                return Ok(AppliedEdit::Refused(refusal));
            }
            match edit {
                EditRequest::SetTemplateParams { .. } => {
                    return Err(WikitextEditorError::Unsupported {
                        message: "set_template_params requires a template locator".to_string(),
                    });
                }
                EditRequest::Replace { fragment } => {
                    let contents = target.contents();
                    let existing: Vec<Wikinode> = contents.children().collect();
                    for child in existing {
                        child.detach();
                    }
                    for child in fragment_body_children(fragment)? {
                        contents.append(&child);
                    }
                }
            }
        }
    }
    Ok(AppliedEdit::Edited(code.into_immutable()))
}

/// Extract prose-bearing blocks from a Parsoid revision, in document order.
#[allow(clippy::unnecessary_wraps)]
fn blocks_from_revision(
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

/// Extract cited sources from a cite-template data-mw.
/// For each template part with a primary `url` param, builds one `CitedSource`
/// with that url as primary and `archive-url`/`archiveurl` as fallbacks.
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

        // Extract primary url.
        let Some(primary_url) = params
            .pointer("/url/wt")
            .and_then(|v| v.as_str())
            .and_then(|wt| url::Url::parse(wt.trim()).ok())
        else {
            continue; // Skip parts with no primary url; an orphan archive-url is not a citable source.
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

#[async_trait]
impl WikitextEditor for ParsoidWikitextEditor {
    async fn enumerate_nodes(
        &self,
        config: &WikiConfig,
        page: &WikitextPageRef,
        kind: WikitextNodeKind,
    ) -> Result<Vec<WikitextNodeDescriptor>, WikitextEditorError> {
        let client = editor_client(config)?;
        let revision = client
            .get_revision(&page.title, page.rev_id)
            .await
            .map_err(map_parsoid_error)?;
        enumerate_revision(&revision, kind)
    }

    async fn replace_node(
        &self,
        config: &WikiConfig,
        page: &WikitextPageRef,
        locator: &WikitextNodeLocator,
        replacement_wikitext: &str,
    ) -> Result<WikitextEditOutcome, WikitextEditorError> {
        let client = editor_client(config)?;
        let revision = client
            .get_revision(&page.title, page.rev_id)
            .await
            .map_err(map_parsoid_error)?;
        let fragment = client
            .transform_to_html(replacement_wikitext)
            .await
            .map_err(map_parsoid_error)?;
        match apply_revision_edit(revision, locator, &EditRequest::Replace { fragment })? {
            AppliedEdit::Refused(refusal) => Ok(WikitextEditOutcome::Refused(refusal)),
            AppliedEdit::Edited(edited) => {
                let new_wikitext = client
                    .transform_to_wikitext(&edited)
                    .await
                    .map_err(map_parsoid_error)?;
                Ok(WikitextEditOutcome::Applied { new_wikitext })
            }
        }
    }

    async fn set_template_params(
        &self,
        config: &WikiConfig,
        page: &WikitextPageRef,
        locator: &WikitextNodeLocator,
        params: &[(String, String)],
    ) -> Result<WikitextEditOutcome, WikitextEditorError> {
        if locator.kind != WikitextNodeKind::Template {
            return Err(WikitextEditorError::Unsupported {
                message: "set_template_params requires a template locator".to_string(),
            });
        }
        let client = editor_client(config)?;
        let revision = client
            .get_revision(&page.title, page.rev_id)
            .await
            .map_err(map_parsoid_error)?;
        let edit = EditRequest::SetTemplateParams {
            params: params.to_vec(),
        };
        match apply_revision_edit(revision, locator, &edit)? {
            AppliedEdit::Refused(refusal) => Ok(WikitextEditOutcome::Refused(refusal)),
            AppliedEdit::Edited(edited) => {
                let new_wikitext = client
                    .transform_to_wikitext(&edited)
                    .await
                    .map_err(map_parsoid_error)?;
                Ok(WikitextEditOutcome::Applied { new_wikitext })
            }
        }
    }

    async fn extract_blocks(
        &self,
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
}

#[cfg(test)]
mod tests {
    use sp42_core::{
        WikitextEditRefusal, WikitextNodeKind, WikitextNodeLocator, normalize_anchor_text,
    };

    use super::{AppliedEdit, EditRequest, apply_revision_edit, enumerate_revision};

    pub(super) const FIXTURE_HTML: &str = r##"<!DOCTYPE html><html><body>
<p id="mwAa">The cosmic latte paragraph with a citation.<sup about="#cite_ref-1" class="mw-ref reference" id="cite_ref-1" rel="dc:references" typeof="mw:Extension/ref" data-mw='{"name":"ref","attrs":{},"body":{"id":"mw-reference-text-cite_note-1"}}'><a href="./Test#cite_note-1"><span class="mw-reflink-text">[1]</span></a></sup> More prose.<sup about="#cite_ref-2" class="mw-ref reference" id="cite_ref-2" rel="dc:references" typeof="mw:Extension/ref" data-mw='{"name":"ref","attrs":{},"body":{"id":"mw-reference-text-cite_note-2"}}'><a href="./Test#cite_note-2"><span class="mw-reflink-text">[2]</span></a></sup></p>
<p id="mwAb">Inline: <span about="#mwt1" typeof="mw:Transclusion" data-mw='{"parts":[{"template":{"target":{"wt":"lang","href":"./Template:Lang"},"params":{"1":{"wt":"fr"},"2":{"wt":"latte cosmique"}},"i":0}}]}'>latte cosmique</span></p>
<div class="mw-references-wrap" typeof="mw:Extension/references" about="#mwt9" data-mw='{"name":"references","attrs":{},"autoGenerated":true}'><ol class="mw-references references"><li about="#cite_note-1" id="cite_note-1"><a href="./Test#cite_ref-1" rel="mw:referencedBy"><span class="mw-linkback-text">&#8593; </span></a> <span id="mw-reference-text-cite_note-1" class="mw-reference-text"><span about="#mwt5" typeof="mw:Transclusion" data-mw='{"parts":[{"template":{"target":{"wt":"cite web","href":"./Template:Cite_web"},"params":{"url":{"wt":"https://example.org/a"},"title":{"wt":"Example A"}},"i":0}}]}'>Example A citation</span></span></li><li about="#cite_note-2" id="cite_note-2"><a href="./Test#cite_ref-2" rel="mw:referencedBy"><span class="mw-linkback-text">&#8593; </span></a> <span id="mw-reference-text-cite_note-2" class="mw-reference-text">Plain text citation B</span></li></ol></div>
</body></html>"##;

    const MULTI_PART_FIXTURE_HTML: &str = r##"<!DOCTYPE html><html><body>
<p><span about="#mwt1" typeof="mw:Transclusion" data-mw='{"parts":[{"template":{"target":{"wt":"lang","href":"./Template:Lang"},"params":{"1":{"wt":"fr"}},"i":0}},{"template":{"target":{"wt":"lang","href":"./Template:Lang"},"params":{"1":{"wt":"en"}},"i":1}}]}'>combined</span></p>
</body></html>"##;

    fn revision(html: &str) -> parsoid::ImmutableWikicode {
        parsoid::ImmutableWikicode::new(html)
    }

    fn template_locator(ordinal: usize, expected_text: &str) -> WikitextNodeLocator {
        WikitextNodeLocator {
            kind: WikitextNodeKind::Template,
            ordinal,
            expected_text: expected_text.to_string(),
        }
    }

    #[test]
    fn enumerates_templates_in_document_order() {
        let descriptors = enumerate_revision(&revision(FIXTURE_HTML), WikitextNodeKind::Template)
            .expect("template enumeration should succeed");
        assert_eq!(descriptors.len(), 2);
        assert_eq!(
            descriptors[0].anchor_text,
            normalize_anchor_text("{{lang|1=fr|2=latte cosmique}}")
        );
        assert_eq!(
            descriptors[1].anchor_text,
            normalize_anchor_text("{{cite web|url=https://example.org/a|title=Example A}}")
        );
    }

    #[test]
    fn enumerates_references_in_document_order() {
        let descriptors = enumerate_revision(&revision(FIXTURE_HTML), WikitextNodeKind::Reference)
            .expect("reference enumeration should succeed");
        assert_eq!(descriptors.len(), 2);
        assert_eq!(descriptors[0].anchor_text, "Example A citation");
        assert_eq!(descriptors[1].anchor_text, "Plain text citation B");
    }

    #[test]
    fn sets_template_params_when_anchor_matches() {
        let locator = template_locator(1, "{{cite web|url=https://example.org/a|title=Example A}}");
        let applied = apply_revision_edit(
            revision(FIXTURE_HTML),
            &locator,
            &EditRequest::SetTemplateParams {
                params: vec![("access-date".to_string(), "9 June 2026".to_string())],
            },
        )
        .expect("param edit should succeed");
        let AppliedEdit::Edited(edited) = applied else {
            panic!("matching anchor must edit");
        };
        let descriptors = enumerate_revision(&edited, WikitextNodeKind::Template)
            .expect("re-enumeration should succeed");
        assert!(
            descriptors[1]
                .anchor_text
                .contains("access-date=9 June 2026"),
            "edited template should carry the new parameter: {}",
            descriptors[1].anchor_text
        );
    }

    #[test]
    fn replaces_reference_content_when_anchor_matches() {
        let locator = WikitextNodeLocator {
            kind: WikitextNodeKind::Reference,
            ordinal: 1,
            expected_text: "Plain text citation B".to_string(),
        };
        let fragment = revision("<html><body>Replaced citation text</body></html>");
        let applied = apply_revision_edit(
            revision(FIXTURE_HTML),
            &locator,
            &EditRequest::Replace { fragment },
        )
        .expect("reference replacement should succeed");
        let AppliedEdit::Edited(edited) = applied else {
            panic!("matching anchor must edit");
        };
        let descriptors = enumerate_revision(&edited, WikitextNodeKind::Reference)
            .expect("re-enumeration should succeed");
        assert_eq!(descriptors[1].anchor_text, "Replaced citation text");
        assert_eq!(descriptors[0].anchor_text, "Example A citation");
    }

    #[test]
    fn replaces_template_node_when_anchor_matches() {
        let locator = template_locator(0, "{{lang|1=fr|2=latte cosmique}}");
        let fragment = revision(
            r##"<html><body><span about="#mwt7" typeof="mw:Transclusion" data-mw='{"parts":[{"template":{"target":{"wt":"lang-fr","href":"./Template:Lang-fr"},"params":{"1":{"wt":"café au lait"}},"i":0}}]}'>café au lait</span></body></html>"##,
        );
        let applied = apply_revision_edit(
            revision(FIXTURE_HTML),
            &locator,
            &EditRequest::Replace { fragment },
        )
        .expect("template replacement should succeed");
        let AppliedEdit::Edited(edited) = applied else {
            panic!("matching anchor must edit");
        };
        let descriptors = enumerate_revision(&edited, WikitextNodeKind::Template)
            .expect("re-enumeration should succeed");
        assert_eq!(descriptors.len(), 2);
        assert_eq!(
            descriptors[0].anchor_text,
            normalize_anchor_text("{{lang-fr|1=café au lait}}")
        );
    }

    #[test]
    fn refuses_out_of_range_ordinal() {
        let locator = template_locator(9, "{{lang|1=fr|2=latte cosmique}}");
        let applied = apply_revision_edit(
            revision(FIXTURE_HTML),
            &locator,
            &EditRequest::SetTemplateParams { params: Vec::new() },
        )
        .expect("range refusal is not an error");
        assert!(matches!(
            applied,
            AppliedEdit::Refused(WikitextEditRefusal::OrdinalOutOfRange {
                requested: 9,
                available: 2,
            })
        ));
    }

    #[test]
    fn refuses_drifted_anchor() {
        let locator = template_locator(0, "{{lang|1=de|2=etwas anderes}}");
        let applied = apply_revision_edit(
            revision(FIXTURE_HTML),
            &locator,
            &EditRequest::SetTemplateParams { params: Vec::new() },
        )
        .expect("drift refusal is not an error");
        let AppliedEdit::Refused(refusal) = applied else {
            panic!("drifted anchor must refuse");
        };
        assert_eq!(refusal.code(), "node-drift");
    }

    #[test]
    fn set_template_params_on_reference_kind_is_unsupported() {
        let locator = WikitextNodeLocator {
            kind: WikitextNodeKind::Reference,
            ordinal: 0,
            expected_text: "Example A citation".to_string(),
        };
        let error = apply_revision_edit(
            revision(FIXTURE_HTML),
            &locator,
            &EditRequest::SetTemplateParams { params: Vec::new() },
        )
        .expect_err("param edit on a reference locator must be unsupported");
        assert!(matches!(
            error,
            sp42_core::WikitextEditorError::Unsupported { .. }
        ));
    }

    #[test]
    fn refuses_replacing_multi_part_transclusions() {
        let descriptors = enumerate_revision(
            &revision(MULTI_PART_FIXTURE_HTML),
            WikitextNodeKind::Template,
        )
        .expect("multi-part enumeration should succeed");
        let locator = template_locator(0, &descriptors[0].anchor_text);
        let fragment = revision("<html><body>plain</body></html>");
        let error = apply_revision_edit(
            revision(MULTI_PART_FIXTURE_HTML),
            &locator,
            &EditRequest::Replace { fragment },
        )
        .expect_err("multi-part transclusion replacement must be unsupported");
        assert!(matches!(
            error,
            sp42_core::WikitextEditorError::Unsupported { .. }
        ));
    }

    #[test]
    fn maps_missing_page_error() {
        let mapped =
            super::map_parsoid_error(parsoid::Error::PageDoesNotExist("Missing page".to_string()));
        assert!(matches!(
            mapped,
            sp42_core::WikitextEditorError::MissingTarget { .. }
        ));
    }

    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use sp42_core::{WikitextEditOutcome, WikitextEditor, WikitextEditorError, WikitextPageRef};

    struct MockParsoid {
        base_url: String,
        revision_requests: Arc<AtomicUsize>,
        transform_bodies: Arc<std::sync::Mutex<Vec<String>>>,
    }

    async fn spawn_mock_parsoid(missing_revision: bool) -> MockParsoid {
        let revision_requests = Arc::new(AtomicUsize::new(0));
        let transform_bodies = Arc::new(std::sync::Mutex::new(Vec::new()));
        let revision_counter = revision_requests.clone();
        let recorded_bodies = transform_bodies.clone();
        let handler = move |request: axum::extract::Request| {
            let revision_counter = revision_counter.clone();
            let recorded_bodies = recorded_bodies.clone();
            async move {
                let path = request.uri().path().to_string();
                let body_bytes = axum::body::to_bytes(request.into_body(), usize::MAX)
                    .await
                    .expect("mock body should read");
                let body = String::from_utf8_lossy(&body_bytes).to_string();
                if path.contains("/transform/html/to/wikitext") {
                    recorded_bodies
                        .lock()
                        .expect("mock transform log should lock")
                        .push(body);
                    return axum::response::Response::builder()
                        .status(200)
                        .body(axum::body::Body::from("CANNED WIKITEXT OUTPUT"))
                        .expect("mock response should build");
                }
                if path.contains("/transform/wikitext/to/html") {
                    return axum::response::Response::builder()
                        .status(200)
                        .body(axum::body::Body::from(
                            "<html><body>Replaced citation text</body></html>",
                        ))
                        .expect("mock response should build");
                }
                if path.contains("/revision/") || path.contains("/page/") {
                    revision_counter.fetch_add(1, Ordering::SeqCst);
                    if missing_revision {
                        return axum::response::Response::builder()
                            .status(404)
                            .body(axum::body::Body::from("{}"))
                            .expect("mock response should build");
                    }
                    return axum::response::Response::builder()
                        .status(200)
                        .header("etag", "W/\"42/test-etag\"")
                        .body(axum::body::Body::from(FIXTURE_HTML))
                        .expect("mock response should build");
                }
                axum::response::Response::builder()
                    .status(404)
                    .body(axum::body::Body::from(format!("unmocked path: {path}")))
                    .expect("mock response should build")
            }
        };
        let app = axum::Router::new().fallback(handler);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("mock parsoid should bind");
        let addr = listener
            .local_addr()
            .expect("mock parsoid should expose addr");
        tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("mock parsoid should serve");
        });
        MockParsoid {
            base_url: format!("http://{addr}/w/rest.php"),
            revision_requests,
            transform_bodies,
        }
    }

    fn config_with_parsoid(base_url: &str) -> sp42_core::WikiConfig {
        let mut config = sp42_wiki::WikiRegistry::embedded_default()
            .expect("embedded registry should load")
            .default_config();
        config.parsoid_url = Some(base_url.parse().expect("mock base url should parse"));
        config
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn smoke_verdict_str(verdict: &sp42_core::CitationVerdict) -> &'static str {
        use sp42_core::{CitationVerdict, SupportLevel};
        match verdict {
            CitationVerdict::Judged(SupportLevel::Supported) => "SUPPORTED",
            CitationVerdict::Judged(SupportLevel::Partial) => "PARTIAL",
            CitationVerdict::Judged(SupportLevel::NotSupported) => "NOT_SUPPORTED",
            CitationVerdict::SourceUnavailable => "SOURCE_UNAVAILABLE",
        }
    }

    fn smoke_truncate(text: &str, limit: usize) -> String {
        if text.chars().count() <= limit {
            text.to_string()
        } else {
            let head: String = text.chars().take(limit).collect();
            format!("{head}…")
        }
    }

    /// Live smoke test of the whole pipeline against a real Parsoid wiki + a real
    /// model panel. Ignored by default; opt in with `--ignored` and set:
    ///   SP42_INFERENCE_URL, SP42_INFERENCE_TOKEN, SP42_INFERENCE_MODELS
    ///   SMOKE_TITLE (canonical page title), SMOKE_REV (rev id)
    ///   SMOKE_PARSOID_URL (default en.wikipedia /w/rest.php), SMOKE_WIKI (label)
    #[tokio::test]
    #[ignore = "live: needs network + SP42_INFERENCE_* credentials"]
    #[allow(
        clippy::too_many_lines,
        clippy::format_push_string,
        clippy::uninlined_format_args,
        clippy::doc_markdown
    )]
    async fn smoke_verify_page_live() {
        let title =
            std::env::var("SMOKE_TITLE").unwrap_or_else(|_| "Thirst (Nothomb novel)".to_string());
        let rev_id: u64 = std::env::var("SMOKE_REV")
            .ok()
            .and_then(|raw| raw.parse().ok())
            .unwrap_or(1_361_018_978);
        let parsoid_url = std::env::var("SMOKE_PARSOID_URL")
            .unwrap_or_else(|_| "https://en.wikipedia.org/w/rest.php".to_string());
        let wiki_id = std::env::var("SMOKE_WIKI").unwrap_or_else(|_| "enwiki".to_string());

        let config = config_with_parsoid(&parsoid_url);
        let editor = super::ParsoidWikitextEditor::new();
        let page_ref = WikitextPageRef {
            title: title.clone(),
            rev_id,
        };
        let blocks = editor
            .extract_blocks(&config, &page_ref)
            .await
            .expect("extract_blocks should succeed against live Parsoid");

        let page = sp42_core::PageVerificationRequest {
            wiki_id,
            title: title.clone(),
            rev_id,
        };
        let extract = sp42_core::extract_use_sites(&blocks, &page);
        let block_count = blocks.len();
        let use_site_count = extract.use_sites.len();

        let http = crate::runtime_adapters::PlainHttpClient::new()
            .expect("guarded source client should build");
        let model = sp42_inference::client_from_env()
            .expect("set SP42_INFERENCE_URL/TOKEN for the smoke test");
        let panel =
            sp42_inference::panel_from_env().expect("set SP42_INFERENCE_MODELS for the smoke test");
        let clock = sp42_types::SystemClock;

        let mut report = sp42_core::verify_page(
            &http,
            &model,
            &clock,
            &panel,
            &page,
            extract,
            sp42_core::VerifyOptions::default(),
            8,
        )
        .await;

        let stats = &report.stats;
        let mut md = String::new();
        md.push_str(&format!("# Citation verification — {title}\n\n"));
        md.push_str(&format!(
            "_revision `{rev_id}` · wiki `{}` · panel: {} · {} blocks → {use_site_count} use-sites_\n\n",
            report.wiki_id,
            panel
                .iter()
                .map(|m| m.model.clone())
                .collect::<Vec<_>>()
                .join(", "),
            block_count,
        ));
        md.push_str("## Summary\n\n");
        md.push_str("| metric | count |\n|---|---|\n");
        md.push_str(&format!("| refs seen | {} |\n", stats.refs_seen));
        md.push_str(&format!(
            "| use-sites verified | {} |\n",
            stats.use_sites_verified
        ));
        md.push_str(&format!("| ✅ supported | {} |\n", stats.supported));
        md.push_str(&format!("| ◐ partial | {} |\n", stats.partial));
        md.push_str(&format!("| ✗ not supported | {} |\n", stats.not_supported));
        md.push_str(&format!(
            "| — source unavailable | {} |\n",
            stats.source_unavailable
        ));
        md.push_str(&format!("| skipped (non-URL) | {} |\n", stats.skipped));
        md.push_str(&format!(
            "| extraction failures | {} |\n\n",
            stats.extraction_failures
        ));

        md.push_str("## Findings\n\n");
        report
            .findings
            .sort_by_key(|finding| finding.use_site_ordinal);
        for finding in &report.findings {
            let verdict = smoke_verdict_str(&finding.verdict);
            let badge = match verdict {
                "SUPPORTED" => "✅",
                "PARTIAL" => "◐",
                "NOT_SUPPORTED" => "✗",
                _ => "—",
            };
            let url = finding.provenance.url.to_string();
            let claim = &finding.claim;
            // `source_unavailable_reason` is `Some` only for SOURCE_UNAVAILABLE.
            let verdict_with_reason = match finding.source_unavailable_reason {
                Some(reason) => format!("{verdict} ({})", reason.as_str()),
                None => verdict.to_string(),
            };
            md.push_str(&format!(
                "### {badge} {verdict_with_reason} · ord {} · grounding `{:?}` · agree {}/{}\n\n",
                finding.use_site_ordinal,
                finding.grounding_status,
                finding.agreement.winner_votes,
                finding.agreement.panel_size,
            ));
            md.push_str(&format!("**Claim.** {claim}\n\n"));
            md.push_str(&format!("**Source.** <{url}>\n\n"));
            if let Some(passage) = &finding.passage {
                md.push_str(&format!("> {}\n\n", smoke_truncate(&passage.quote, 400)));
            }
        }

        if !report.skipped.is_empty() {
            md.push_str("## Skipped (non-URL sources)\n\n");
            for skipped in &report.skipped {
                md.push_str(&format!(
                    "- ref `{}` (block {}) — {:?}\n",
                    skipped.ref_id, skipped.block_ordinal, skipped.reason
                ));
            }
            md.push('\n');
        }
        if !report.extraction_failures.is_empty() {
            md.push_str("## Extraction failures\n\n");
            for failure in &report.extraction_failures {
                md.push_str(&format!(
                    "- block {} — {}\n",
                    failure.block_ordinal, failure.reason
                ));
            }
            md.push('\n');
        }

        let out =
            std::env::var("SMOKE_OUT").unwrap_or_else(|_| "/tmp/sp42-smoke-report.md".to_string());
        std::fs::write(&out, &md).expect("write markdown report");
        let json = serde_json::to_string_pretty(&report).expect("serialize report");
        std::fs::write(format!("{out}.json"), &json).expect("write json report");
        eprintln!("wrote {out} and {out}.json");
    }

    fn fixture_page() -> WikitextPageRef {
        WikitextPageRef {
            title: "Test".to_string(),
            rev_id: 42,
        }
    }

    #[tokio::test]
    async fn set_template_params_round_trips_through_parsoid() {
        let mock = spawn_mock_parsoid(false).await;
        let editor = super::ParsoidWikitextEditor::new();
        let config = config_with_parsoid(&mock.base_url);
        let locator = template_locator(1, "{{cite web|url=https://example.org/a|title=Example A}}");
        let outcome = editor
            .set_template_params(
                &config,
                &fixture_page(),
                &locator,
                &[("access-date".to_string(), "9 June 2026".to_string())],
            )
            .await
            .expect("parsoid round trip should succeed");
        assert_eq!(
            outcome,
            WikitextEditOutcome::Applied {
                new_wikitext: "CANNED WIKITEXT OUTPUT".to_string()
            }
        );
        assert_eq!(mock.revision_requests.load(Ordering::SeqCst), 1);
        let bodies = mock
            .transform_bodies
            .lock()
            .expect("mock transform log should lock");
        assert_eq!(bodies.len(), 1);
        assert!(
            bodies[0].contains("access-date"),
            "the posted HTML must carry the data-mw edit"
        );
    }

    #[tokio::test]
    async fn replace_reference_round_trips_through_parsoid() {
        let mock = spawn_mock_parsoid(false).await;
        let editor = super::ParsoidWikitextEditor::new();
        let config = config_with_parsoid(&mock.base_url);
        let locator = WikitextNodeLocator {
            kind: WikitextNodeKind::Reference,
            ordinal: 1,
            expected_text: "Plain text citation B".to_string(),
        };
        let outcome = editor
            .replace_node(&config, &fixture_page(), &locator, "Replaced citation text")
            .await
            .expect("parsoid round trip should succeed");
        assert_eq!(
            outcome,
            WikitextEditOutcome::Applied {
                new_wikitext: "CANNED WIKITEXT OUTPUT".to_string()
            }
        );
        let bodies = mock
            .transform_bodies
            .lock()
            .expect("mock transform log should lock");
        assert!(
            bodies[0].contains("Replaced citation text"),
            "the posted HTML must carry the replaced reference content"
        );
    }

    #[tokio::test]
    async fn refusal_skips_the_serialization_transform() {
        let mock = spawn_mock_parsoid(false).await;
        let editor = super::ParsoidWikitextEditor::new();
        let config = config_with_parsoid(&mock.base_url);
        let locator = template_locator(0, "{{drifted|anchor=text}}");
        let outcome = editor
            .set_template_params(&config, &fixture_page(), &locator, &[])
            .await
            .expect("refusal is not an error");
        assert!(matches!(outcome, WikitextEditOutcome::Refused(_)));
        assert!(
            mock.transform_bodies
                .lock()
                .expect("mock transform log should lock")
                .is_empty(),
            "a refused edit must not reach the serializer"
        );
    }

    #[tokio::test]
    async fn unconfigured_wiki_refuses_with_not_configured() {
        let editor = super::ParsoidWikitextEditor::new();
        let mut config = config_with_parsoid("http://127.0.0.1:1/w/rest.php");
        config.parsoid_url = None;
        let error = editor
            .enumerate_nodes(&config, &fixture_page(), WikitextNodeKind::Template)
            .await
            .expect_err("unconfigured wiki must error");
        assert!(matches!(error, WikitextEditorError::NotConfigured { .. }));
    }

    #[tokio::test]
    async fn missing_revision_maps_to_editor_error() {
        let mock = spawn_mock_parsoid(true).await;
        let editor = super::ParsoidWikitextEditor::new();
        let config = config_with_parsoid(&mock.base_url);
        let error = editor
            .enumerate_nodes(&config, &fixture_page(), WikitextNodeKind::Template)
            .await
            .expect_err("missing revision must error");
        assert!(matches!(
            error,
            WikitextEditorError::MissingTarget { .. } | WikitextEditorError::Unavailable { .. }
        ));
    }
}

#[cfg(test)]
mod extract_tests {
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
}
