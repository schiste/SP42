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
    BlockKind, BlockRef, ParsoidBlock, WikiConfig, WikitextEditOutcome, WikitextEditRefusal,
    WikitextEditor, WikitextEditorError, WikitextNodeDescriptor, WikitextNodeKind,
    WikitextNodeLocator, WikitextPageRef, normalize_anchor_text,
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

    // Map Reference.id() -> source URLs, read structurally from cite templates
    // and bare ExtLinks inside each reference's contents.
    let mut ref_urls: HashMap<String, Vec<url::Url>> = HashMap::new();
    for reference in code.filter_references() {
        ref_urls.insert(reference.id(), urls_in_reference(&reference));
    }

    let mut blocks = Vec::new();
    let mut ordinal = 0usize;
    let mut heading_stack: Vec<(u32, String)> = Vec::new();
    walk(
        &code,
        &mut heading_stack,
        &ref_urls,
        &mut blocks,
        &mut ordinal,
    );
    Ok(blocks)
}

/// Recursive walker — track headings, emit blocks, don't recurse into a block once emitted.
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
            while headings.last().is_some_and(|(l, _)| *l >= level) {
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

/// Collect text and refs from a block, skipping ref markers' own text.
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

/// URL extraction from a reference's contents.
fn urls_in_reference(reference: &Reference) -> Vec<url::Url> {
    let contents = reference.contents();
    let mut out = Vec::new();

    // Cite-template params via data-mw.
    for span in contents.select("span[typeof~=\"mw:Transclusion\"]") {
        if let Some(element) = span.as_node().as_element()
            && let Some(data_mw) = element.attributes.borrow().get("data-mw")
        {
            push_template_urls(data_mw, &mut out);
        }
    }
    // Bare ExtLinks.
    for node in contents.descendants() {
        if let Some(extlink) = node.as_extlink()
            && let Ok(u) = url::Url::parse(&extlink.target())
        {
            out.push(u);
        }
    }
    out.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    out.dedup();
    out
}

/// Extract URLs from cite-template data-mw.
fn push_template_urls(data_mw: &str, out: &mut Vec<url::Url>) {
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
        for key in ["url", "archive-url", "archiveurl"] {
            if let Some(wt) = params
                .pointer(&format!("/{key}/wt"))
                .and_then(|v| v.as_str())
                && let Ok(u) = url::Url::parse(wt.trim())
            {
                out.push(u);
            }
        }
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
