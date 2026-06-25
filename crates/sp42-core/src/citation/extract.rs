//! Article-level claim↔ref extraction: turning the editor's `ParsoidBlock`
//! intermediate into per-use-site verification inputs. Pure, no DOM, no I/O.

use crate::citation::page::PageVerificationRequest;
use crate::citation::prompts::ClaimContext;
use crate::citation::segment::{Sentence, segment_sentences};
use crate::citation::verify::CitationVerificationRequest;
use crate::wikitext_editor::ParsoidBlock;

/// Maximum preceding in-block sentences carried as context.
const MAX_PRECEDING: usize = 3;

/// One citation use-site: a claim sentence, one source URL, and the context
/// passed alongside it. The unit the orchestrator fans the verifier over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CitationUseSite {
    /// Document-order index across the page.
    pub use_site_ordinal: u32,
    /// Document-order index of the block this use-site came from (provenance;
    /// used to attribute verify errors back to a block in the report).
    pub block_ordinal: usize,
    /// Claim + source URL + page identity for the verifier.
    pub request: CitationVerificationRequest,
    /// Article title + preceding sentences passed alongside the claim.
    pub context: ClaimContext,
    /// The originating ref's marker id, for provenance.
    pub ref_id: String,
    /// Archive/fallback URLs, tried only if the primary `source_url` is unavailable.
    pub archive_urls: Vec<url::Url>,
}

/// Why a ref produced no use-site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkippedReason {
    /// The ref carries no extractable URL (book/ISBN/offline source).
    NonUrlSource,
}

/// A ref that was intentionally not verified.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SkippedRef {
    pub ref_id: String,
    pub reason: SkippedReason,
    pub block_ordinal: usize,
}

/// A block (or ref) that could not be processed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BlockFailure {
    pub block_ordinal: usize,
    pub reason: String,
}

/// Result of extracting use-sites from a page's blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractOutcome {
    pub use_sites: Vec<CitationUseSite>,
    pub skipped: Vec<SkippedRef>,
    pub failures: Vec<BlockFailure>,
}

/// Extract every URL-bearing citation use-site from a page's blocks.
/// Non-URL refs are recorded in `skipped`; blocks that yield no usable claim
/// go to `failures`. Document order is preserved across the page.
#[must_use]
pub fn extract_use_sites(
    blocks: &[ParsoidBlock],
    page: &PageVerificationRequest,
) -> ExtractOutcome {
    let mut use_sites = Vec::new();
    let mut skipped = Vec::new();
    let mut failures = Vec::new();
    let mut ordinal: u32 = 0;

    for block in blocks {
        let sentences = segment_sentences(&block.text);
        for r in &block.refs {
            if r.sources.is_empty() {
                skipped.push(SkippedRef {
                    ref_id: r.ref_id.clone(),
                    reason: SkippedReason::NonUrlSource,
                    block_ordinal: block.block_ordinal,
                });
                continue;
            }

            // Find the sentence index this ref attaches to.
            let attached = attach_index(&sentences, r.offset);
            let (claim, preceding) = if let Some(idx) = attached {
                let claim = sentences[idx].text.clone();
                let lo = idx.saturating_sub(MAX_PRECEDING);
                let preceding: Vec<String> =
                    sentences[lo..idx].iter().map(|s| s.text.clone()).collect();
                (claim, preceding)
            } else {
                // Fallback: whole block text (fragmentary block).
                let claim = block.text.trim().to_string();
                (claim, Vec::new())
            };

            if claim.is_empty() {
                failures.push(BlockFailure {
                    block_ordinal: block.block_ordinal,
                    reason: format!("ref {} has no resolvable claim text", r.ref_id),
                });
                continue;
            }

            let context = ClaimContext {
                article_title: page.title.clone(),
                preceding_sentences: preceding,
            };

            for source in &r.sources {
                use_sites.push(CitationUseSite {
                    use_site_ordinal: ordinal,
                    block_ordinal: block.block_ordinal,
                    request: CitationVerificationRequest {
                        wiki_id: page.wiki_id.clone(),
                        rev_id: page.rev_id,
                        title: page.title.clone(),
                        claim: claim.clone(),
                        source_url: source.url.clone(),
                    },
                    context: context.clone(),
                    ref_id: r.ref_id.clone(),
                    archive_urls: source.archive_urls.clone(),
                });
                ordinal += 1;
            }
        }
    }

    ExtractOutcome {
        use_sites,
        skipped,
        failures,
    }
}

/// Index of the sentence a ref at byte `offset` attaches to: the sentence whose
/// range contains `offset.saturating_sub(1)` (the marker sits just past the
/// punctuation it follows). A ref at end-of-block attaches to the last sentence.
fn attach_index(sentences: &[Sentence], offset: usize) -> Option<usize> {
    if sentences.is_empty() {
        return None;
    }
    let probe = offset.saturating_sub(1);
    for (idx, s) in sentences.iter().enumerate() {
        if probe < s.range.end {
            return Some(idx);
        }
    }
    Some(sentences.len() - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::citation::page::PageVerificationRequest;
    use crate::wikitext_editor::{BlockKind, BlockRef, ParsoidBlock};
    use url::Url;

    fn page() -> PageVerificationRequest {
        PageVerificationRequest {
            wiki_id: "enwiki".into(),
            title: "Cats".into(),
            rev_id: 7,
        }
    }

    fn url(u: &str) -> Url {
        Url::parse(u).unwrap()
    }

    fn block(text: &str, refs: Vec<BlockRef>) -> ParsoidBlock {
        ParsoidBlock {
            text: text.into(),
            refs,
            block_kind: BlockKind::Paragraph,
            block_ordinal: 0,
        }
    }

    fn bref(offset: usize, urls: &[&str]) -> BlockRef {
        BlockRef {
            offset,
            ref_id: format!("ref-{offset}"),
            sources: urls
                .iter()
                .map(|u| crate::wikitext_editor::CitedSource {
                    url: url(u),
                    archive_urls: vec![],
                })
                .collect(),
            ref_text: "[1]".into(),
            named: false,
        }
    }

    fn bref_archived(offset: usize, primary: &str, archives: &[&str]) -> BlockRef {
        BlockRef {
            offset,
            ref_id: format!("ref-{offset}"),
            sources: vec![crate::wikitext_editor::CitedSource {
                url: url(primary),
                archive_urls: archives.iter().map(|u| url(u)).collect(),
            }],
            ref_text: "[1]".into(),
            named: false,
        }
    }

    #[test]
    fn ref_attaches_to_its_sentence() {
        // "Cats purr. Cats sleep a lot." — ref after the first period (offset 10).
        let b = block(
            "Cats purr. Cats sleep a lot.",
            vec![bref(10, &["https://a.test"])],
        );
        let out = extract_use_sites(&[b], &page());
        assert_eq!(out.use_sites.len(), 1);
        let us = &out.use_sites[0];
        assert_eq!(us.request.claim, "Cats purr.");
        assert_eq!(us.request.source_url, url("https://a.test"));
    }

    #[test]
    fn multiple_refs_after_one_sentence_share_the_claim() {
        let b = block(
            "Cats purr.",
            vec![bref(10, &["https://a.test"]), bref(10, &["https://b.test"])],
        );
        let out = extract_use_sites(&[b], &page());
        assert_eq!(out.use_sites.len(), 2);
        assert_eq!(out.use_sites[0].request.claim, "Cats purr.");
        assert_eq!(out.use_sites[1].request.claim, "Cats purr.");
        assert_eq!(out.use_sites[0].use_site_ordinal, 0);
        assert_eq!(out.use_sites[1].use_site_ordinal, 1);
    }

    #[test]
    fn preceding_sentences_become_context_capped_at_three() {
        let text = "A. B. C. D. E."; // five short sentences
        // ref after "E." (end of block).
        let b = block(text, vec![bref(text.len(), &["https://a.test"])]);
        let out = extract_use_sites(&[b], &page());
        let us = &out.use_sites[0];
        assert_eq!(us.request.claim, "E.");
        assert_eq!(us.context.preceding_sentences, vec!["B.", "C.", "D."]);
        assert_eq!(us.context.article_title, "Cats");
    }

    #[test]
    fn non_url_ref_is_skipped_not_verified() {
        let b = block("Cats purr.", vec![bref(10, &[])]);
        let out = extract_use_sites(&[b], &page());
        assert!(out.use_sites.is_empty());
        assert_eq!(out.skipped.len(), 1);
        assert_eq!(out.skipped[0].reason, SkippedReason::NonUrlSource);
    }

    #[test]
    fn fragmentary_block_falls_back_to_whole_text() {
        // A list-item style fragment with no sentence terminator.
        let mut b = block(
            "ISO 4217 currency code",
            vec![bref(22, &["https://a.test"])],
        );
        b.block_kind = BlockKind::ListItem;
        let out = extract_use_sites(&[b], &page());
        assert_eq!(out.use_sites.len(), 1);
        assert_eq!(out.use_sites[0].request.claim, "ISO 4217 currency code");
    }

    #[test]
    fn empty_block_with_ref_is_a_failure() {
        let b = block("   ", vec![bref(0, &["https://a.test"])]);
        let out = extract_use_sites(&[b], &page());
        assert!(out.use_sites.is_empty());
        assert_eq!(out.failures.len(), 1);
        assert_eq!(out.failures[0].block_ordinal, 0);
    }

    #[test]
    fn url_with_archive_yields_one_use_site_with_archive_list() {
        // A single ref with primary URL and archive fallback should produce
        // exactly ONE use-site (not two), with both URLs properly threaded.
        let primary = "https://example.org/article";
        let archive = "https://web.archive.org/web/20240101/example.org/article";
        let b = block("Cats purr.", vec![bref_archived(10, primary, &[archive])]);
        let out = extract_use_sites(&[b], &page());
        assert_eq!(out.use_sites.len(), 1, "should yield exactly one use-site");
        let us = &out.use_sites[0];
        assert_eq!(us.request.claim, "Cats purr.");
        assert_eq!(us.request.source_url, url(primary));
        assert_eq!(us.archive_urls.len(), 1);
        assert_eq!(us.archive_urls[0], url(archive));
    }

    #[test]
    fn url_with_multiple_archives_preserved() {
        // A single ref with primary URL and multiple archive fallbacks should
        // preserve all archives in order.
        let primary = "https://example.org/article";
        let archive1 = "https://web.archive.org/web/20240101/example.org/article";
        let archive2 = "https://archive.is/example.org/article";
        let b = block(
            "The fact is true.",
            vec![bref_archived(16, primary, &[archive1, archive2])],
        );
        let out = extract_use_sites(&[b], &page());
        assert_eq!(out.use_sites.len(), 1);
        let us = &out.use_sites[0];
        assert_eq!(us.archive_urls.len(), 2);
        assert_eq!(us.archive_urls[0], url(archive1));
        assert_eq!(us.archive_urls[1], url(archive2));
    }
}
