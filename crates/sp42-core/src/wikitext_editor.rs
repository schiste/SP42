//! Node-anchored wikitext editing contract (ADR-0003).
//!
//! Content edits address a structural node — a `<ref>` citation or a template
//! transclusion — by document-order ordinal instead of a literal text span.
//! Every mutating operation re-grounds on the expected node text and refuses,
//! rather than guessing, when the target drifted or the ordinal is out of
//! range. Implementations re-serialize the full page losslessly so the result
//! can be saved with a `baserevid` guard.

use std::sync::Mutex;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::types::WikiConfig;

/// Structural node families addressable by a [`WikitextNodeLocator`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WikitextNodeKind {
    /// A `<ref>` citation node.
    Reference,
    /// A template transclusion (`{{...}}`).
    Template,
}

impl WikitextNodeKind {
    /// Stable wire label for the node kind.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Reference => "reference",
            Self::Template => "template",
        }
    }
}

/// Addresses one structural node within a specific page revision.
///
/// `expected_text` is the anti-drift anchor: mutating operations refuse when
/// it no longer matches the node's current anchor text after whitespace
/// normalization. Callers should obtain anchors from
/// [`WikitextEditor::enumerate_nodes`] and echo them back unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikitextNodeLocator {
    /// Node family the ordinal indexes into.
    pub kind: WikitextNodeKind,
    /// Zero-based position within `kind`, in document order.
    pub ordinal: usize,
    /// Anchor text the addressed node must still match.
    pub expected_text: String,
}

/// The page revision a node-anchored operation grounds on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikitextPageRef {
    /// Page title.
    pub title: String,
    /// Revision the caller reviewed; the edit grounds on exactly this
    /// revision and is saved with it as `baserevid`.
    pub rev_id: u64,
}

/// One structural node reported by [`WikitextEditor::enumerate_nodes`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikitextNodeDescriptor {
    /// Node family.
    pub kind: WikitextNodeKind,
    /// Zero-based document-order position within `kind`.
    pub ordinal: usize,
    /// Canonical anchor text to echo back as
    /// [`WikitextNodeLocator::expected_text`].
    pub anchor_text: String,
}

/// Kind of prose-bearing block a citation can appear in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Paragraph,
    ListItem,
    TableCell,
    Other,
}

/// One inline `<ref>` within a [`ParsoidBlock`], in document order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockRef {
    /// Byte offset into [`ParsoidBlock::text`] where the marker sat — the
    /// position of the punctuation it follows. Anchors claim↔ref association.
    pub offset: usize,
    /// Stable cite id of the inline marker, e.g. `"cite_ref-smith_3-0"`.
    pub ref_id: String,
    /// Source URL(s) read from the ref's structured `data-mw` cite-template
    /// params (`url=`, `archive-url=`) via the parsoid crate; for a bare-URL
    /// ref with no template, from the structured `ExtLink` node. Empty ⇒ a
    /// non-URL ref (book/ISBN) that the core records as skipped.
    pub source_urls: Vec<url::Url>,
    /// Rendered text of the marker (e.g. `"[3]"`), for provenance.
    pub ref_text: String,
    /// `true` when this is a reuse of a `<ref name="…">`.
    pub named: bool,
}

/// A single prose-bearing block emitted by the editor's one DOM pass.
/// Plain `Send` data: no DOM handles. Ref markers are removed from `text`
/// but their positions are preserved as [`BlockRef::offset`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsoidBlock {
    /// Visible text of the block with ref markers removed.
    pub text: String,
    /// Heading stack from page root to this block, outermost first
    /// (e.g. `["History", "Early life"]`).
    pub section_path: Vec<String>,
    /// Inline refs in this block, in document order.
    pub refs: Vec<BlockRef>,
    pub block_kind: BlockKind,
    /// Document-order index of the block within the page.
    pub block_ordinal: usize,
}

/// A structured edit that was refused without touching the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WikitextEditRefusal {
    /// The ordinal does not address any node of the requested kind.
    OrdinalOutOfRange {
        /// Ordinal the caller requested.
        requested: usize,
        /// Number of nodes of that kind present in the revision.
        available: usize,
    },
    /// The addressed node no longer matches the expected anchor text.
    NodeDrifted {
        /// Normalized anchor the caller expected.
        expected: String,
        /// Normalized anchor currently found at the ordinal.
        found: String,
    },
}

impl WikitextEditRefusal {
    /// Stable machine-readable refusal code.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::OrdinalOutOfRange { .. } => "node-out-of-range",
            Self::NodeDrifted { .. } => "node-drift",
        }
    }

    /// Human-readable refusal message.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::OrdinalOutOfRange {
                requested,
                available,
            } => format!(
                "node ordinal {requested} is out of range; the revision has {available} node(s) of that kind"
            ),
            Self::NodeDrifted { expected, found } => {
                format!("node anchor drifted; expected `{expected}` but found `{found}`")
            }
        }
    }
}

/// Outcome of a node-anchored edit operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WikitextEditOutcome {
    /// The edit applied; the full page was re-serialized losslessly.
    Applied {
        /// Complete new page wikitext, ready for a `baserevid`-guarded save.
        new_wikitext: String,
    },
    /// The edit was refused; the page was not touched.
    Refused(WikitextEditRefusal),
}

/// Hard failure while talking to or configuring the editing backend.
#[derive(Debug, Error)]
pub enum WikitextEditorError {
    /// The backend could not be reached or answered with an error.
    #[error("wikitext editor unavailable: {message}")]
    Unavailable {
        /// Backend failure detail.
        message: String,
        /// Whether retrying later may succeed.
        retryable: bool,
    },
    /// The page or revision does not exist on the backend.
    #[error("wikitext edit target missing: {message}")]
    MissingTarget {
        /// Backend lookup detail.
        message: String,
    },
    /// No editing backend is configured for the wiki.
    #[error("wikitext editing is not configured for wiki `{wiki_id}`")]
    NotConfigured {
        /// Wiki the request addressed.
        wiki_id: String,
    },
    /// The operation cannot be applied to the addressed node.
    #[error("unsupported wikitext edit: {message}")]
    Unsupported {
        /// Why the operation cannot be applied.
        message: String,
    },
}

/// Normalize anchor text for drift comparison: collapse every whitespace run
/// to a single space and trim both ends.
#[must_use]
pub fn normalize_anchor_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// One node the scripted editor pretends exists, in document order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptedWikitextNode {
    /// Node family.
    pub kind: WikitextNodeKind,
    /// Anchor text reported by enumeration and matched against
    /// `expected_text`.
    pub anchor_text: String,
}

/// One operation observed by [`ScriptedWikitextEditor`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptedEditorInvocation {
    /// `"replace_node"` or `"set_template_params"`.
    pub operation: String,
    /// Locator the caller supplied.
    pub locator: WikitextNodeLocator,
    /// Replacement wikitext, or the parameter list rendered as
    /// `key=value` pairs joined with `|`.
    pub payload: String,
}

/// Deterministic in-crate [`WikitextEditor`] double (Constitution Art. 1/2).
///
/// It enforces the real locator contract — ordinal range and anti-drift
/// anchor comparison — against a scripted node list, records every mutating
/// invocation, and answers successful edits with a scripted page wikitext.
#[derive(Debug, Default)]
pub struct ScriptedWikitextEditor {
    nodes: Vec<ScriptedWikitextNode>,
    serialized_wikitext: String,
    invocations: Mutex<Vec<ScriptedEditorInvocation>>,
}

impl ScriptedWikitextEditor {
    /// Build a double exposing `nodes` and answering successful edits with
    /// `serialized_wikitext`.
    #[must_use]
    pub fn new(nodes: Vec<ScriptedWikitextNode>, serialized_wikitext: String) -> Self {
        Self {
            nodes,
            serialized_wikitext,
            invocations: Mutex::new(Vec::new()),
        }
    }

    /// Mutating operations the double has observed so far, in order.
    ///
    /// # Panics
    ///
    /// Panics when the invocation log mutex is poisoned.
    #[must_use]
    pub fn invocations(&self) -> Vec<ScriptedEditorInvocation> {
        self.invocations
            .lock()
            .expect("scripted editor invocation log should not be poisoned")
            .clone()
    }

    fn check(&self, locator: &WikitextNodeLocator) -> Option<WikitextEditRefusal> {
        let anchors: Vec<&ScriptedWikitextNode> = self
            .nodes
            .iter()
            .filter(|node| node.kind == locator.kind)
            .collect();
        let Some(node) = anchors.get(locator.ordinal) else {
            return Some(WikitextEditRefusal::OrdinalOutOfRange {
                requested: locator.ordinal,
                available: anchors.len(),
            });
        };
        let expected = normalize_anchor_text(&locator.expected_text);
        let found = normalize_anchor_text(&node.anchor_text);
        // Empty expected_text provides no anti-drift guarantee; always refuse.
        if expected.is_empty() || expected != found {
            Some(WikitextEditRefusal::NodeDrifted { expected, found })
        } else {
            None
        }
    }

    fn record(&self, operation: &str, locator: &WikitextNodeLocator, payload: String) {
        self.invocations
            .lock()
            .expect("scripted editor invocation log should not be poisoned")
            .push(ScriptedEditorInvocation {
                operation: operation.to_string(),
                locator: locator.clone(),
                payload,
            });
    }

    fn outcome_for(&self, locator: &WikitextNodeLocator) -> WikitextEditOutcome {
        self.check(locator).map_or_else(
            || WikitextEditOutcome::Applied {
                new_wikitext: self.serialized_wikitext.clone(),
            },
            WikitextEditOutcome::Refused,
        )
    }
}

/// Node-anchored wikitext editing over one page revision (ADR-0003).
///
/// Implementations fetch the revision, locate the addressed node, verify the
/// anti-drift anchor, apply the edit, and re-serialize the whole page
/// losslessly. They never write to the wiki — callers save the returned
/// wikitext through the existing `baserevid`-guarded save path.
#[async_trait]
pub trait WikitextEditor: Send + Sync {
    /// Enumerate the nodes of `kind` in document order for the revision.
    ///
    /// # Errors
    ///
    /// Returns [`WikitextEditorError`] when the backend is unavailable, the
    /// revision is missing, or the wiki has no editing backend configured.
    async fn enumerate_nodes(
        &self,
        config: &WikiConfig,
        page: &WikitextPageRef,
        kind: WikitextNodeKind,
    ) -> Result<Vec<WikitextNodeDescriptor>, WikitextEditorError>;

    /// Replace the node addressed by `locator` with `replacement_wikitext`.
    ///
    /// For [`WikitextNodeKind::Reference`] the replacement applies to the
    /// citation content inside the `<ref>` (the tag and its attributes are
    /// preserved); for [`WikitextNodeKind::Template`] it replaces the whole
    /// transclusion.
    ///
    /// # Errors
    ///
    /// Returns [`WikitextEditorError`] on backend failure; drift and
    /// out-of-range conditions are reported as
    /// [`WikitextEditOutcome::Refused`], not as errors.
    async fn replace_node(
        &self,
        config: &WikiConfig,
        page: &WikitextPageRef,
        locator: &WikitextNodeLocator,
        replacement_wikitext: &str,
    ) -> Result<WikitextEditOutcome, WikitextEditorError>;

    /// Set parameters on the template addressed by `locator`, preserving the
    /// template's existing formatting style for untouched parameters.
    ///
    /// The locator must have kind [`WikitextNodeKind::Template`].
    ///
    /// # Errors
    ///
    /// Returns [`WikitextEditorError::Unsupported`] when the locator does not
    /// address a template, and other [`WikitextEditorError`] variants on
    /// backend failure. Drift and out-of-range conditions are reported as
    /// [`WikitextEditOutcome::Refused`].
    async fn set_template_params(
        &self,
        config: &WikiConfig,
        page: &WikitextPageRef,
        locator: &WikitextNodeLocator,
        params: &[(String, String)],
    ) -> Result<WikitextEditOutcome, WikitextEditorError>;

    /// Extract prose-bearing blocks (paragraphs, list items, table cells) in
    /// document order, with inline ref markers removed from text but their byte
    /// offsets, ids, and source URLs recorded. Read-only.
    ///
    /// Defaults to "no blocks": only the Parsoid production editor understands
    /// page structure, so non-Parsoid impls (scripted/test fakes) inherit the
    /// empty default and need no changes.
    ///
    /// # Errors
    ///
    /// Returns [`WikitextEditorError`] when the backend is unavailable, the
    /// revision is missing, or the wiki has no editing backend configured.
    async fn extract_blocks(
        &self,
        config: &WikiConfig,
        page: &WikitextPageRef,
    ) -> Result<Vec<ParsoidBlock>, WikitextEditorError> {
        let _ = (config, page);
        Ok(Vec::new())
    }
}

#[async_trait]
impl WikitextEditor for ScriptedWikitextEditor {
    async fn enumerate_nodes(
        &self,
        _config: &WikiConfig,
        _page: &WikitextPageRef,
        kind: WikitextNodeKind,
    ) -> Result<Vec<WikitextNodeDescriptor>, WikitextEditorError> {
        Ok(self
            .nodes
            .iter()
            .filter(|node| node.kind == kind)
            .enumerate()
            .map(|(ordinal, node)| WikitextNodeDescriptor {
                kind,
                ordinal,
                anchor_text: node.anchor_text.clone(),
            })
            .collect())
    }

    async fn replace_node(
        &self,
        _config: &WikiConfig,
        _page: &WikitextPageRef,
        locator: &WikitextNodeLocator,
        replacement_wikitext: &str,
    ) -> Result<WikitextEditOutcome, WikitextEditorError> {
        self.record("replace_node", locator, replacement_wikitext.to_string());
        Ok(self.outcome_for(locator))
    }

    async fn set_template_params(
        &self,
        _config: &WikiConfig,
        _page: &WikitextPageRef,
        locator: &WikitextNodeLocator,
        params: &[(String, String)],
    ) -> Result<WikitextEditOutcome, WikitextEditorError> {
        if locator.kind != WikitextNodeKind::Template {
            return Err(WikitextEditorError::Unsupported {
                message: "set_template_params requires a template locator".to_string(),
            });
        }
        let payload = params
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join("|");
        self.record("set_template_params", locator, payload);
        Ok(self.outcome_for(locator))
    }
}

#[cfg(test)]
mod tests {
    use futures::executor::block_on;

    use super::{
        ScriptedWikitextEditor, ScriptedWikitextNode, WikitextEditOutcome, WikitextEditRefusal,
        WikitextEditor, WikitextEditorError, WikitextNodeKind, WikitextNodeLocator,
        WikitextPageRef, normalize_anchor_text,
    };
    use crate::test_fixtures::fixture_wiki_config;

    fn scripted_nodes() -> Vec<ScriptedWikitextNode> {
        vec![
            ScriptedWikitextNode {
                kind: WikitextNodeKind::Template,
                anchor_text: "{{cite web|url=https://example.org/a|title=Example A}}".to_string(),
            },
            ScriptedWikitextNode {
                kind: WikitextNodeKind::Reference,
                anchor_text: "Example A citation".to_string(),
            },
            ScriptedWikitextNode {
                kind: WikitextNodeKind::Template,
                anchor_text: "{{lang|fr|latte cosmique}}".to_string(),
            },
        ]
    }

    fn page() -> WikitextPageRef {
        WikitextPageRef {
            title: "Exemple".to_string(),
            rev_id: 42,
        }
    }

    #[test]
    fn normalizes_anchor_whitespace() {
        assert_eq!(
            normalize_anchor_text("  {{cite \n web|url=x}}  "),
            "{{cite web|url=x}}"
        );
        assert_eq!(normalize_anchor_text(""), "");
    }

    #[test]
    fn enumerates_nodes_per_kind_in_order() {
        let editor = ScriptedWikitextEditor::new(scripted_nodes(), "WIKITEXT".to_string());
        let config = fixture_wiki_config();
        let templates =
            block_on(editor.enumerate_nodes(&config, &page(), WikitextNodeKind::Template))
                .expect("enumeration should succeed");
        assert_eq!(templates.len(), 2);
        assert_eq!(templates[0].ordinal, 0);
        assert_eq!(templates[1].anchor_text, "{{lang|fr|latte cosmique}}");
        let references =
            block_on(editor.enumerate_nodes(&config, &page(), WikitextNodeKind::Reference))
                .expect("enumeration should succeed");
        assert_eq!(references.len(), 1);
    }

    #[test]
    fn replaces_node_when_anchor_matches() {
        let editor = ScriptedWikitextEditor::new(scripted_nodes(), "NEW PAGE TEXT".to_string());
        let config = fixture_wiki_config();
        let locator = WikitextNodeLocator {
            kind: WikitextNodeKind::Template,
            ordinal: 1,
            expected_text: "{{lang|fr|latte   cosmique}}".to_string(),
        };
        let outcome = block_on(editor.replace_node(&config, &page(), &locator, "{{lang-fr|café}}"))
            .expect("scripted replace should succeed");
        assert_eq!(
            outcome,
            WikitextEditOutcome::Applied {
                new_wikitext: "NEW PAGE TEXT".to_string()
            }
        );
        let invocations = editor.invocations();
        assert_eq!(invocations.len(), 1);
        assert_eq!(invocations[0].operation, "replace_node");
        assert_eq!(invocations[0].payload, "{{lang-fr|café}}");
    }

    #[test]
    fn refuses_out_of_range_ordinal() {
        let editor = ScriptedWikitextEditor::new(scripted_nodes(), String::new());
        let config = fixture_wiki_config();
        let locator = WikitextNodeLocator {
            kind: WikitextNodeKind::Reference,
            ordinal: 5,
            expected_text: "anything".to_string(),
        };
        let outcome = block_on(editor.replace_node(&config, &page(), &locator, "x"))
            .expect("scripted replace should succeed");
        assert_eq!(
            outcome,
            WikitextEditOutcome::Refused(WikitextEditRefusal::OrdinalOutOfRange {
                requested: 5,
                available: 1,
            })
        );
    }

    #[test]
    fn refuses_drifted_anchor() {
        let editor = ScriptedWikitextEditor::new(scripted_nodes(), String::new());
        let config = fixture_wiki_config();
        let locator = WikitextNodeLocator {
            kind: WikitextNodeKind::Template,
            ordinal: 0,
            expected_text: "{{cite web|url=https://example.org/CHANGED|title=Example A}}"
                .to_string(),
        };
        let outcome = block_on(editor.replace_node(&config, &page(), &locator, "x"))
            .expect("scripted replace should succeed");
        let WikitextEditOutcome::Refused(refusal) = outcome else {
            panic!("drifted anchor must refuse");
        };
        assert_eq!(refusal.code(), "node-drift");
        assert!(refusal.message().contains("drifted"));
    }

    #[test]
    fn empty_expected_text_always_refuses() {
        let editor = ScriptedWikitextEditor::new(
            vec![ScriptedWikitextNode {
                kind: WikitextNodeKind::Reference,
                anchor_text: String::new(),
            }],
            String::new(),
        );
        let config = fixture_wiki_config();
        let locator = WikitextNodeLocator {
            kind: WikitextNodeKind::Reference,
            ordinal: 0,
            expected_text: String::new(),
        };
        let outcome = block_on(editor.replace_node(&config, &page(), &locator, "x"))
            .expect("scripted replace should succeed");
        let WikitextEditOutcome::Refused(refusal) = outcome else {
            panic!("empty anchor must always refuse");
        };
        assert_eq!(refusal.code(), "node-drift");
    }

    #[test]
    fn set_template_params_requires_template_kind() {
        let editor = ScriptedWikitextEditor::new(scripted_nodes(), String::new());
        let config = fixture_wiki_config();
        let locator = WikitextNodeLocator {
            kind: WikitextNodeKind::Reference,
            ordinal: 0,
            expected_text: "Example A citation".to_string(),
        };
        let error = block_on(editor.set_template_params(
            &config,
            &page(),
            &locator,
            &[("access-date".to_string(), "9 June 2026".to_string())],
        ))
        .expect_err("reference locator must be unsupported");
        assert!(matches!(error, WikitextEditorError::Unsupported { .. }));
    }

    #[test]
    fn set_template_params_applies_and_records_payload() {
        let editor = ScriptedWikitextEditor::new(scripted_nodes(), "PARAMS DONE".to_string());
        let config = fixture_wiki_config();
        let locator = WikitextNodeLocator {
            kind: WikitextNodeKind::Template,
            ordinal: 0,
            expected_text: "{{cite web|url=https://example.org/a|title=Example A}}".to_string(),
        };
        let outcome = block_on(editor.set_template_params(
            &config,
            &page(),
            &locator,
            &[("access-date".to_string(), "9 June 2026".to_string())],
        ))
        .expect("scripted set_template_params should succeed");
        assert_eq!(
            outcome,
            WikitextEditOutcome::Applied {
                new_wikitext: "PARAMS DONE".to_string()
            }
        );
        assert_eq!(editor.invocations()[0].payload, "access-date=9 June 2026");
    }

    #[test]
    fn node_locator_serializes_with_kebab_case_kind() {
        let locator = WikitextNodeLocator {
            kind: WikitextNodeKind::Reference,
            ordinal: 3,
            expected_text: "anchor".to_string(),
        };
        let json = serde_json::to_string(&locator).expect("locator should serialize");
        assert_eq!(
            json,
            r#"{"kind":"reference","ordinal":3,"expected_text":"anchor"}"#
        );
        let parsed: WikitextNodeLocator =
            serde_json::from_str(&json).expect("locator should deserialize");
        assert_eq!(parsed, locator);
    }
}
