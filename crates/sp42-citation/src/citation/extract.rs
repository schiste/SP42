//! Article-level claim↔ref extraction: turning the editor's `ParsoidBlock`
//! intermediate into per-use-site verification inputs. Pure, no DOM, no I/O.

use crate::citation::page::PageVerificationRequest;
use crate::citation::prompts::ClaimContext;
use crate::citation::segment::{Sentence, segment_sentences};
use crate::citation::verify::CitationVerificationRequest;
use crate::wikitext_editor::{BookIdentifier, BookSource, ParsoidBlock};

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
    /// `true` when the originating ref's whole content is a single bare URL — a
    /// bare-URL-repair target. Carried through so a finding can be routed to
    /// bare-URL repair only when its own ref is genuinely bare.
    pub is_bare_url_ref: bool,
}

/// Why a ref produced no use-site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkippedReason {
    /// The ref carries no extractable URL **and no book identifier** — there
    /// is nothing to fetch and nothing to resolve (offline/unidentified source).
    NonUrlSource,
    /// The ref carries no URL but does carry a validated book identifier
    /// (ISBN/OCLC/LCCN/OLID) that did **not** resolve to an Open Library
    /// record (catalog miss or failed lookup) — assigned by the page
    /// orchestrator after resolution (PRD-0009 Layer 1, ADR-0024). A resolved
    /// book ref becomes a finding instead (Layer 2).
    BookSource,
    /// The ref is a shortened footnote whose bibliography target could not
    /// be resolved to a book identifier (PRD-0009 Layer-1 amendment,
    /// 2026-07-13): the anchor matched nothing, or the matched entry carried
    /// no validated identifier. Never guessed — disclosed.
    UnresolvedShortCite,
}

/// A ref that was intentionally not verified.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SkippedRef {
    pub ref_id: String,
    pub reason: SkippedReason,
    pub block_ordinal: usize,
    /// The ref's book sources (validated identifiers + cited page, one per
    /// cite template) — populated with [`SkippedReason::BookSource`]. The page
    /// orchestrator resolves these against Open Library (PRD-0009 Layer 1)
    /// and the report shows *which* book the ref names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub book_sources: Vec<BookSource>,
}

impl SkippedRef {
    /// All validated identifiers across the ref's book sources, in template
    /// order — the flattened view the skipped-section renderer prints.
    #[must_use]
    pub fn book_identifiers(&self) -> Vec<&BookIdentifier> {
        self.book_sources
            .iter()
            .flat_map(|book| book.identifiers.iter())
            .collect()
    }
}

/// One book-citation use-site (PRD-0009): a claim sentence attached to a
/// url-less ref that carries validated book identifiers. The page
/// orchestrator resolves it against Open Library and — when an exact-edition
/// scan exists — grounds the claim in the scan's search-inside snippets
/// (Layer 2), so it is a claim-bearing unit exactly like [`CitationUseSite`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookUseSite {
    /// Document-order index across the page (shared with URL use-sites).
    pub use_site_ordinal: u32,
    pub block_ordinal: usize,
    /// The originating ref's marker id, for provenance.
    pub ref_id: String,
    /// The claim sentence the ref attaches to.
    pub claim: String,
    /// Article title + preceding sentences passed alongside the claim.
    pub context: ClaimContext,
    /// The book named by this use-site (identifiers + cited page).
    pub book: BookSource,
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
    /// Book-citation use-sites (PRD-0009): url-less refs with validated
    /// identifiers, carrying the claim they attach to. NOT in `skipped` —
    /// whether one ends as a finding or a refined skip is decided by the page
    /// orchestrator after Open Library resolution.
    pub book_use_sites: Vec<BookUseSite>,
    pub skipped: Vec<SkippedRef>,
    pub failures: Vec<BlockFailure>,
}

/// Extract every claim-bearing citation use-site from a page's blocks: one
/// [`CitationUseSite`] per URL source and one [`BookUseSite`] per book source
/// on a url-less ref (PRD-0009). A ref with neither an extractable URL nor a
/// validated book identifier is recorded in `skipped`; refs that yield no
/// usable claim go to `failures`. Document order is preserved across the page
/// and ordinals are shared across both use-site kinds.
#[must_use]
pub fn extract_use_sites(
    blocks: &[ParsoidBlock],
    page: &PageVerificationRequest,
) -> ExtractOutcome {
    let mut use_sites = Vec::new();
    let mut book_use_sites = Vec::new();
    let mut skipped = Vec::new();
    let mut failures = Vec::new();
    let mut ordinal: u32 = 0;

    for block in blocks {
        let sentences = segment_sentences(&block.text);
        for r in &block.refs {
            // An unresolved short cite is disclosed as a skip even when the
            // same ref also resolved other sources (a partially-resolved
            // bundled ref): the resolved parts proceed below, and the missing
            // bibliography target still surfaces instead of vanishing
            // (Codex round 8, PR 153).
            if r.short_cite_unresolved {
                skipped.push(SkippedRef {
                    ref_id: r.ref_id.clone(),
                    reason: SkippedReason::UnresolvedShortCite,
                    block_ordinal: block.block_ordinal,
                    book_sources: Vec::new(),
                });
            }
            if r.sources.is_empty() && r.book_sources.is_empty() {
                if !r.short_cite_unresolved {
                    // Nothing fetchable and nothing to resolve.
                    skipped.push(SkippedRef {
                        ref_id: r.ref_id.clone(),
                        reason: SkippedReason::NonUrlSource,
                        block_ordinal: block.block_ordinal,
                        book_sources: Vec::new(),
                    });
                }
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

            // A ref with URL sources rides the URL verification path; its
            // book identifiers stay on the BlockRef for future enrichment.
            // Only a url-less ref becomes a book use-site (PRD-0009).
            if r.sources.is_empty() {
                for book in &r.book_sources {
                    book_use_sites.push(BookUseSite {
                        use_site_ordinal: ordinal,
                        block_ordinal: block.block_ordinal,
                        ref_id: r.ref_id.clone(),
                        claim: claim.clone(),
                        context: context.clone(),
                        book: book.clone(),
                    });
                    ordinal += 1;
                }
                continue;
            }

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
                    is_bare_url_ref: r.is_bare_url_ref,
                });
                ordinal += 1;
            }
        }
    }

    ExtractOutcome {
        use_sites,
        book_use_sites,
        skipped,
        failures,
    }
}

/// Index of the sentence a ref at byte `offset` attaches to: the sentence whose
/// range contains `offset.saturating_sub(1)` (the marker sits just past the
/// punctuation it follows). A ref at end-of-block attaches to the last sentence.
///
/// The probe can land in the inter-sentence whitespace gap when the marker follows a
/// space (`Sentence one. <ref>` records the offset *after* the space, which
/// `segment_sentences` excludes from the first sentence's range). In that case the ref
/// belongs to the *preceding* sentence — the one it trails — not the one it precedes.
fn attach_index(sentences: &[Sentence], offset: usize) -> Option<usize> {
    if sentences.is_empty() {
        return None;
    }
    let probe = offset.saturating_sub(1);
    for (idx, s) in sentences.iter().enumerate() {
        // Probe lands before this sentence starts → it's in the gap after the previous
        // sentence (or leading whitespace). Attach to the preceding sentence.
        if probe < s.range.start {
            return Some(idx.saturating_sub(1));
        }
        // Probe lands within this sentence's range.
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
            book_sources: vec![],
            ref_text: "[1]".into(),
            named: false,
            is_bare_url_ref: false,
            short_cite_unresolved: false,
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
            book_sources: vec![],
            ref_text: "[1]".into(),
            named: false,
            is_bare_url_ref: false,
            short_cite_unresolved: false,
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
    fn ref_after_whitespace_attaches_to_preceding_sentence() {
        // "Cats purr. Cats sleep." — a ref whose marker follows the inter-sentence space
        // (offset 11, just before the second sentence). It must attach to "Cats purr.",
        // the sentence it trails, not the following one.
        let b = block(
            "Cats purr. Cats sleep.",
            vec![bref(11, &["https://a.test"])],
        );
        let out = extract_use_sites(&[b], &page());
        assert_eq!(out.use_sites.len(), 1);
        assert_eq!(out.use_sites[0].request.claim, "Cats purr.");
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
        assert!(out.skipped[0].book_sources.is_empty());
    }

    #[test]
    fn book_identifier_ref_becomes_a_book_use_site_with_the_claim() {
        // A url-less ref carrying a validated ISBN is a claim-bearing unit:
        // it gets the same sentence attach as a URL ref, ready for Layer 2.
        let mut r = bref(10, &[]);
        r.book_sources = vec![crate::wikitext_editor::BookSource {
            identifiers: vec![BookIdentifier::Isbn("9780140328721".to_string())],
            cited_page: Some("42".to_string()),
        }];
        let b = block("Cats purr.", vec![r]);
        let out = extract_use_sites(&[b], &page());
        assert!(out.use_sites.is_empty());
        assert!(out.skipped.is_empty(), "book refs are use-sites, not skips");
        assert_eq!(out.book_use_sites.len(), 1);
        let site = &out.book_use_sites[0];
        assert_eq!(site.claim, "Cats purr.");
        assert_eq!(site.context.article_title, "Cats");
        assert_eq!(
            site.book.identifiers,
            vec![BookIdentifier::Isbn("9780140328721".to_string())]
        );
        assert_eq!(site.book.cited_page.as_deref(), Some("42"));
    }

    #[test]
    fn book_use_sites_share_the_ordinal_sequence_with_url_use_sites() {
        let mut book_ref = bref(10, &[]);
        book_ref.book_sources = vec![crate::wikitext_editor::BookSource {
            identifiers: vec![BookIdentifier::Isbn("9780140328721".to_string())],
            cited_page: None,
        }];
        let url_ref = bref(22, &["https://a.test"]);
        let b = block("Cats purr. Cats sleep.", vec![book_ref, url_ref]);
        let out = extract_use_sites(&[b], &page());
        assert_eq!(out.book_use_sites[0].use_site_ordinal, 0);
        assert_eq!(out.use_sites[0].use_site_ordinal, 1);
    }

    #[test]
    fn book_ref_with_empty_claim_is_a_failure_like_the_url_path() {
        let mut r = bref(0, &[]);
        r.book_sources = vec![crate::wikitext_editor::BookSource {
            identifiers: vec![BookIdentifier::Isbn("9780140328721".to_string())],
            cited_page: None,
        }];
        let b = block("   ", vec![r]);
        let out = extract_use_sites(&[b], &page());
        assert!(out.book_use_sites.is_empty());
        assert_eq!(out.failures.len(), 1);
    }

    #[test]
    fn url_bearing_ref_with_book_identifiers_still_verifies_as_url() {
        // url= wins: the ref produces a URL use-site; the book identifiers
        // ride along on the BlockRef without spawning a book use-site.
        let mut r = bref(10, &["https://a.test"]);
        r.book_sources = vec![crate::wikitext_editor::BookSource {
            identifiers: vec![BookIdentifier::Isbn("9780140328721".to_string())],
            cited_page: None,
        }];
        let b = block("Cats purr.", vec![r]);
        let out = extract_use_sites(&[b], &page());
        assert_eq!(out.use_sites.len(), 1);
        assert!(out.book_use_sites.is_empty());
        assert!(out.skipped.is_empty());
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

    #[test]
    fn unresolved_short_cite_ref_gets_the_refined_skip_reason() {
        let mut r = bref(10, &[]);
        r.short_cite_unresolved = true;
        let b = block("Cats purr.", vec![r]);
        let out = extract_use_sites(&[b], &page());
        assert_eq!(out.skipped.len(), 1);
        assert_eq!(out.skipped[0].reason, SkippedReason::UnresolvedShortCite);
    }

    #[test]
    fn resolved_short_cite_ref_is_a_book_use_site_not_a_skip() {
        // A fully-resolved short cite (flag false) is a book use-site with
        // no skip disclosure.
        let mut r = bref(10, &[]);
        r.book_sources = vec![crate::wikitext_editor::BookSource {
            identifiers: vec![BookIdentifier::Isbn("9780140328721".to_string())],
            cited_page: None,
        }];
        let b = block("Cats purr.", vec![r]);
        let out = extract_use_sites(&[b], &page());
        assert_eq!(out.book_use_sites.len(), 1);
        assert!(out.skipped.is_empty());
    }

    #[test]
    fn partially_resolved_bundled_ref_discloses_the_unresolved_part() {
        // Codex round 8 (PR 153): a ref that resolved one bundled short cite
        // but not another emits BOTH the book use-site and an
        // unresolved-short-cite skip — the missing target never vanishes.
        let mut r = bref(10, &[]);
        r.short_cite_unresolved = true;
        r.book_sources = vec![crate::wikitext_editor::BookSource {
            identifiers: vec![BookIdentifier::Isbn("9780140328721".to_string())],
            cited_page: None,
        }];
        let b = block("Cats purr.", vec![r]);
        let out = extract_use_sites(&[b], &page());
        assert_eq!(out.book_use_sites.len(), 1, "resolved part proceeds");
        assert_eq!(out.skipped.len(), 1, "unresolved part disclosed");
        assert_eq!(out.skipped[0].reason, SkippedReason::UnresolvedShortCite);
    }

    #[test]
    fn skip_reason_round_trips_serde() {
        let json = serde_json::to_string(&SkippedReason::UnresolvedShortCite).expect("serializes");
        assert_eq!(json, "\"unresolved_short_cite\"");
        assert_eq!(
            serde_json::from_str::<SkippedReason>(&json).expect("parses"),
            SkippedReason::UnresolvedShortCite
        );
    }
}
