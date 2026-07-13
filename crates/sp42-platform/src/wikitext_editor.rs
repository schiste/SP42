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
    /// Reserved for future block kinds; never produced by current implementations.
    Other,
}

/// One cited source: a primary (live) URL plus archive fallbacks
/// (e.g. `archive-url=`, Wayback/wikiwix), consulted only when the
/// primary is unavailable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CitedSource {
    pub url: url::Url,
    pub archive_urls: Vec<url::Url>,
}

/// A positive book identifier read from a cite template's parameters
/// (PRD-0009 Layer 1, ADR-0024 Decision 1). Values are normalized and
/// shape-validated at construction — the constructors return `None` for a
/// value that fails validation, so catalog resolution is gated on genuinely
/// positive identifiers and never guesses.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", tag = "scheme", content = "value")]
pub enum BookIdentifier {
    /// ISBN-10 or ISBN-13: hyphen/space-free, checksum-validated.
    Isbn(String),
    /// OCLC control number: digits, `ocm`/`ocn`/`on` prefixes stripped.
    Oclc(String),
    /// Library of Congress Control Number, LC-normalized (lowercase,
    /// space-free, `/`-suffix dropped, hyphenated serial zero-padded to six).
    Lccn(String),
    /// Open Library **edition or work** id in canonical `OL…M`/`OL…W` form.
    /// An author id (`OL…A`) is not a book identifier and is rejected.
    Olid(String),
}

impl BookIdentifier {
    /// Normalize and checksum-validate an ISBN-10 or ISBN-13.
    #[must_use]
    pub fn isbn(raw: &str) -> Option<Self> {
        let compact: String = raw
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '-')
            .map(|c| c.to_ascii_uppercase())
            .collect();
        let bytes = compact.as_bytes();
        let valid = match bytes.len() {
            10 => {
                // Mod-11: positions weighted 10..1; last digit may be X (=10).
                bytes[..9].iter().all(u8::is_ascii_digit)
                    && (bytes[9].is_ascii_digit() || bytes[9] == b'X')
                    && bytes
                        .iter()
                        .enumerate()
                        .map(|(i, b)| {
                            let value = if *b == b'X' { 10 } else { u32::from(b - b'0') };
                            (10 - u32::try_from(i).unwrap_or(0)) * value
                        })
                        .sum::<u32>()
                        % 11
                        == 0
            }
            13 => {
                // EAN-13: alternating weights 1 and 3.
                bytes.iter().all(u8::is_ascii_digit)
                    && bytes
                        .iter()
                        .enumerate()
                        .map(|(i, b)| u32::from(b - b'0') * if i % 2 == 0 { 1 } else { 3 })
                        .sum::<u32>()
                        % 10
                        == 0
            }
            _ => false,
        };
        valid.then_some(Self::Isbn(compact))
    }

    /// Normalize an OCLC control number: trim, strip the `ocm`/`ocn`/`on`
    /// record prefixes, require all-digits.
    #[must_use]
    pub fn oclc(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        let lower = trimmed.to_ascii_lowercase();
        let digits = ["ocm", "ocn", "on"]
            .iter()
            .find_map(|prefix| lower.strip_prefix(prefix))
            .unwrap_or(&lower)
            .trim();
        (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
            .then(|| Self::Oclc(digits.to_string()))
    }

    /// Normalize an LCCN per the Library of Congress normalization rules:
    /// lowercase, remove spaces, drop everything from the first `/`, and
    /// zero-pad a hyphenated serial to six digits.
    #[must_use]
    pub fn lccn(raw: &str) -> Option<Self> {
        let mut value: String = raw
            .to_ascii_lowercase()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        if let Some(slash) = value.find('/') {
            value.truncate(slash);
        }
        if let Some((prefix, serial)) = value.split_once('-') {
            if serial.is_empty() || serial.len() > 6 || !serial.bytes().all(|b| b.is_ascii_digit())
            {
                return None;
            }
            value = format!("{prefix}{serial:0>6}");
        }
        (!value.is_empty() && value.bytes().all(|b| b.is_ascii_alphanumeric()))
            .then_some(Self::Lccn(value))
    }

    /// Canonicalize an Open Library id from a cite template's `ol=` param
    /// (which conventionally omits the `OL` prefix): accept `(OL)?\d+[MW]`
    /// case-insensitively, emit `OL…M`/`OL…W`. An author id (`…A`) yields
    /// `None` — author records are not book identifiers (PRD-0009).
    #[must_use]
    pub fn olid(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        if !trimmed.is_ascii() {
            return None;
        }
        let rest = if trimmed.len() >= 2 && trimmed[..2].eq_ignore_ascii_case("OL") {
            &trimmed[2..]
        } else {
            trimmed
        };
        let (digits, kind) = rest.split_at(rest.len().saturating_sub(1));
        let kind = kind.to_ascii_uppercase();
        ((kind == "M" || kind == "W")
            && !digits.is_empty()
            && digits.bytes().all(|b| b.is_ascii_digit()))
        .then(|| Self::Olid(format!("OL{digits}{kind}")))
    }
}

impl std::fmt::Display for BookIdentifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Isbn(v) => write!(f, "isbn:{v}"),
            Self::Oclc(v) => write!(f, "oclc:{v}"),
            Self::Lccn(v) => write!(f, "lccn:{v}"),
            Self::Olid(v) => write!(f, "olid:{v}"),
        }
    }
}

/// The book identifiers carried by one cite template (PRD-0009 Layer 1):
/// the positive identifiers that gate catalog resolution, plus the cited
/// page for the future search-inside pass (ADR-0024 Decision 4).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BookSource {
    /// Validated identifiers, in template order (`isbn`, `oclc`, `lccn`, `ol`).
    pub identifiers: Vec<BookIdentifier>,
    /// Verbatim `page=`/`pages=` value, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cited_page: Option<String>,
}

/// One inline `<ref>` within a [`ParsoidBlock`], in document order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockRef {
    /// Byte offset into [`ParsoidBlock::text`] where the marker sat — the
    /// position of the punctuation it follows. Anchors claim↔ref association.
    pub offset: usize,
    /// Stable cite id of the inline marker, e.g. `"cite_ref-smith_3-0"`.
    pub ref_id: String,
    /// Cited sources from this ref: primary URL + archive fallbacks.
    /// One cite-template ⇒ one cited source (url + its archive-url).
    /// A bare-URL `<ref>` ⇒ one cited source (url, no archives).
    /// A bundled ref with multiple cite templates ⇒ multiple cited sources.
    /// Empty ⇒ a non-URL ref (book/ISBN) that the core records as skipped.
    pub sources: Vec<CitedSource>,
    /// Book identifiers from this ref's cite templates (PRD-0009 Layer 1),
    /// one entry per template that carries a validated identifier. Extracted
    /// independently of `sources`: a template can yield both (url + isbn),
    /// either, or neither.
    pub book_sources: Vec<BookSource>,
    /// Rendered text of the marker (e.g. `"[3]"`), for provenance.
    pub ref_text: String,
    /// `true` when this is a reuse of a `<ref name="…">`.
    pub named: bool,
    /// `true` when the reference's whole rendered content is a single bare URL
    /// (no cite template) — i.e. a bare-URL-repair target. Classified from the
    /// ref's content (not the marker), so a finding can be routed to bare-URL
    /// repair only when its own ref is genuinely bare, rather than inferred from
    /// a source URL that another (bare) ref happens to share.
    pub is_bare_url_ref: bool,
}

/// A single prose-bearing block emitted by the editor's one DOM pass.
/// Plain `Send` data: no DOM handles. Ref markers are removed from `text`
/// but their positions are preserved as [`BlockRef::offset`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsoidBlock {
    /// Visible text of the block with ref markers removed.
    pub text: String,
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
        BookIdentifier, ScriptedWikitextEditor, ScriptedWikitextNode, WikitextEditOutcome,
        WikitextEditRefusal, WikitextEditor, WikitextEditorError, WikitextNodeKind,
        WikitextNodeLocator, WikitextPageRef, normalize_anchor_text,
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

    #[test]
    fn isbn_normalizes_and_validates_both_lengths() {
        // Hyphens/spaces stripped, checksum verified (ISBN-13, EAN mod 10).
        assert_eq!(
            BookIdentifier::isbn("978-0-14-032872-1"),
            Some(BookIdentifier::Isbn("9780140328721".to_string()))
        );
        // ISBN-10 with an X check digit (mod 11), lowercase x accepted.
        assert_eq!(
            BookIdentifier::isbn("0-8044-2957-x"),
            Some(BookIdentifier::Isbn("080442957X".to_string()))
        );
        // Bad checksum, wrong length, and garbage all yield no identifier.
        assert_eq!(BookIdentifier::isbn("978-0-14-032872-2"), None);
        assert_eq!(BookIdentifier::isbn("12345"), None);
        assert_eq!(BookIdentifier::isbn("not-an-isbn"), None);
    }

    #[test]
    fn oclc_strips_record_prefixes_and_requires_digits() {
        assert_eq!(
            BookIdentifier::oclc(" ocm12345678 "),
            Some(BookIdentifier::Oclc("12345678".to_string()))
        );
        assert_eq!(
            BookIdentifier::oclc("902731744"),
            Some(BookIdentifier::Oclc("902731744".to_string()))
        );
        assert_eq!(BookIdentifier::oclc("ocm"), None);
        assert_eq!(BookIdentifier::oclc("12a45"), None);
    }

    #[test]
    fn lccn_applies_library_of_congress_normalization() {
        // Spaces removed, lowercased, /-suffix dropped, serial zero-padded.
        assert_eq!(
            BookIdentifier::lccn("n 78-890351"),
            Some(BookIdentifier::Lccn("n78890351".to_string()))
        );
        assert_eq!(
            BookIdentifier::lccn("85-2 "),
            Some(BookIdentifier::Lccn("85000002".to_string()))
        );
        assert_eq!(
            BookIdentifier::lccn("2001-000002/AC/r932"),
            Some(BookIdentifier::Lccn("2001000002".to_string()))
        );
        assert_eq!(BookIdentifier::lccn(""), None);
        assert_eq!(BookIdentifier::lccn("85-abc"), None);
    }

    #[test]
    fn olid_canonicalizes_editions_and_works_but_rejects_authors() {
        // The cite `ol=` param conventionally omits the OL prefix.
        assert_eq!(
            BookIdentifier::olid("7030731M"),
            Some(BookIdentifier::Olid("OL7030731M".to_string()))
        );
        assert_eq!(
            BookIdentifier::olid("ol45804W"),
            Some(BookIdentifier::Olid("OL45804W".to_string()))
        );
        // Author records are not book identifiers (PRD-0009).
        assert_eq!(BookIdentifier::olid("OL23919A"), None);
        assert_eq!(BookIdentifier::olid("M"), None);
        assert_eq!(BookIdentifier::olid("12345"), None);
    }

    #[test]
    fn book_identifier_serializes_with_scheme_tag_and_displays_compactly() {
        let id = BookIdentifier::Isbn("9780140328721".to_string());
        let json = serde_json::to_string(&id).expect("identifier should serialize");
        assert_eq!(json, r#"{"scheme":"isbn","value":"9780140328721"}"#);
        assert_eq!(id.to_string(), "isbn:9780140328721");
        let parsed: BookIdentifier =
            serde_json::from_str(&json).expect("identifier should deserialize");
        assert_eq!(parsed, id);
    }
}
