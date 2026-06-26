# Article Citation Extractor Implementation Plan — Phase 3: Claim↔Ref Association

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** Turn `Vec<ParsoidBlock>` into `ExtractOutcome` (use-sites + skipped + failures) by associating each ref with the sentence its `offset` falls in and building its `ClaimContext`.

**Architecture:** Pure `sp42-core` function, the heuristic heart. Unit-tested against hand-built `ParsoidBlock` fixtures — no DOM, no network.

**Tech Stack:** Rust. Reuses `segment_sentences` (Phase 2), `ParsoidBlock`/`BlockRef` (Phase 1), `ClaimContext` (`prompts.rs`), `CitationVerificationRequest` (`verify.rs`).

**Scope:** Phase 3 of 6.

**Codebase verified:** 2026-06-24 — `ClaimContext { article_title: String, section_title: Option<String>, preceding_sentences: Vec<String> }` (`prompts.rs:21`). `CitationVerificationRequest { wiki_id, rev_id, title, claim, source_url: Url }` (`verify.rs:54`). Both reused unchanged.

---

## Task 1: `extract_use_sites` — failing tests

**Files:**
- Modify: `crates/sp42-core/src/citation/extract.rs` (add the function + tests; types already exist from Phase 1)
- Modify: `crates/sp42-core/src/lib.rs` — add `extract_use_sites` to the `citation::extract` re-export.

**Step 1: Add the function signature (returns empty) so tests compile**

In `extract.rs`, add:

```rust
use crate::citation::segment::segment_sentences;
use crate::wikitext_editor::{BlockRef, ParsoidBlock};

/// Maximum preceding in-block sentences carried as context.
const MAX_PRECEDING: usize = 3;

/// Extract every URL-bearing citation use-site from a page's blocks.
/// Non-URL refs are recorded in `skipped`; blocks that yield no usable claim
/// go to `failures`. Document order is preserved across the page.
#[must_use]
pub fn extract_use_sites(blocks: &[ParsoidBlock], page: &PageVerificationRequest) -> ExtractOutcome {
    let _ = (blocks, page);
    ExtractOutcome { use_sites: Vec::new(), skipped: Vec::new(), failures: Vec::new() }
}
```

Add the import `use crate::citation::page::PageVerificationRequest;` if not already present (Phase 1 added it).

**Step 2: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::citation::page::PageVerificationRequest;
    use crate::wikitext_editor::{BlockKind, BlockRef, ParsoidBlock};
    use url::Url;

    fn page() -> PageVerificationRequest {
        PageVerificationRequest { wiki_id: "enwiki".into(), title: "Cats".into(), rev_id: 7 }
    }

    fn url(u: &str) -> Url {
        Url::parse(u).unwrap()
    }

    fn block(text: &str, section: &[&str], refs: Vec<BlockRef>) -> ParsoidBlock {
        ParsoidBlock {
            text: text.into(),
            section_path: section.iter().map(|s| (*s).to_string()).collect(),
            refs,
            block_kind: BlockKind::Paragraph,
            block_ordinal: 0,
        }
    }

    fn bref(offset: usize, urls: &[&str]) -> BlockRef {
        BlockRef {
            offset,
            ref_id: format!("ref-{offset}"),
            source_urls: urls.iter().map(|u| url(u)).collect(),
            ref_text: "[1]".into(),
            named: false,
        }
    }

    #[test]
    fn ref_attaches_to_its_sentence() {
        // "Cats purr. Cats sleep a lot." — ref after the first period (offset 10).
        let b = block("Cats purr. Cats sleep a lot.", &["Behaviour"], vec![bref(10, &["https://a.test"])]);
        let out = extract_use_sites(&[b], &page());
        assert_eq!(out.use_sites.len(), 1);
        let us = &out.use_sites[0];
        assert_eq!(us.request.claim, "Cats purr.");
        assert_eq!(us.context.section_title.as_deref(), Some("Behaviour"));
        assert_eq!(us.request.source_url, url("https://a.test"));
    }

    #[test]
    fn multiple_refs_after_one_sentence_share_the_claim() {
        let b = block("Cats purr.", &["S"], vec![bref(10, &["https://a.test"]), bref(10, &["https://b.test"])]);
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
        let b = block(text, &["Sec"], vec![bref(text.len(), &["https://a.test"])]);
        let out = extract_use_sites(&[b], &page());
        let us = &out.use_sites[0];
        assert_eq!(us.request.claim, "E.");
        assert_eq!(us.context.preceding_sentences, vec!["B.", "C.", "D."]);
        assert_eq!(us.context.article_title, "Cats");
    }

    #[test]
    fn non_url_ref_is_skipped_not_verified() {
        let b = block("Cats purr.", &["S"], vec![bref(10, &[])]);
        let out = extract_use_sites(&[b], &page());
        assert!(out.use_sites.is_empty());
        assert_eq!(out.skipped.len(), 1);
        assert_eq!(out.skipped[0].reason, SkippedReason::NonUrlSource);
    }

    #[test]
    fn fragmentary_block_falls_back_to_whole_text() {
        // A list-item style fragment with no sentence terminator.
        let mut b = block("ISO 4217 currency code", &["Codes"], vec![bref(22, &["https://a.test"])]);
        b.block_kind = BlockKind::ListItem;
        let out = extract_use_sites(&[b], &page());
        assert_eq!(out.use_sites.len(), 1);
        assert_eq!(out.use_sites[0].request.claim, "ISO 4217 currency code");
    }

    #[test]
    fn empty_block_with_ref_is_a_failure() {
        let b = block("   ", &["S"], vec![bref(0, &["https://a.test"])]);
        let out = extract_use_sites(&[b], &page());
        assert!(out.use_sites.is_empty());
        assert_eq!(out.failures.len(), 1);
        assert_eq!(out.failures[0].block_ordinal, 0);
    }
}
```

**Step 3: Run to verify they fail**

Run: `PATH="$HOME/.cargo/bin:$PATH" cargo test -p sp42-core --lib citation::extract`
Expected: assertion failures (function returns empty).

---

## Task 2: Implement `extract_use_sites`

**Files:**
- Modify: `crates/sp42-core/src/citation/extract.rs`

**Step 1: Replace the stub body**

```rust
#[must_use]
pub fn extract_use_sites(blocks: &[ParsoidBlock], page: &PageVerificationRequest) -> ExtractOutcome {
    let mut use_sites = Vec::new();
    let mut skipped = Vec::new();
    let mut failures = Vec::new();
    let mut ordinal: u32 = 0;

    for block in blocks {
        let sentences = segment_sentences(&block.text);
        for r in &block.refs {
            if r.source_urls.is_empty() {
                skipped.push(SkippedRef {
                    ref_id: r.ref_id.clone(),
                    reason: SkippedReason::NonUrlSource,
                    block_ordinal: block.block_ordinal,
                });
                continue;
            }

            // Find the sentence index this ref attaches to.
            let attached = attach_index(&sentences, r.offset);
            let (claim, preceding) = match attached {
                Some(idx) => {
                    let claim = sentences[idx].text.clone();
                    let lo = idx.saturating_sub(MAX_PRECEDING);
                    let preceding: Vec<String> =
                        sentences[lo..idx].iter().map(|s| s.text.clone()).collect();
                    (claim, preceding)
                }
                None => {
                    // Fallback: whole block text (fragmentary block).
                    let claim = block.text.trim().to_string();
                    (claim, Vec::new())
                }
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
                section_title: block.section_path.last().cloned(),
                preceding_sentences: preceding,
            };

            for source_url in &r.source_urls {
                use_sites.push(CitationUseSite {
                    use_site_ordinal: ordinal,
                    block_ordinal: block.block_ordinal,
                    request: CitationVerificationRequest {
                        wiki_id: page.wiki_id.clone(),
                        rev_id: page.rev_id,
                        title: page.title.clone(),
                        claim: claim.clone(),
                        source_url: source_url.clone(),
                    },
                    context: context.clone(),
                    ref_id: r.ref_id.clone(),
                });
                ordinal += 1;
            }
        }
    }

    ExtractOutcome { use_sites, skipped, failures }
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
```

Add imports at the top of the impl section if missing:

```rust
use crate::citation::segment::{segment_sentences, Sentence};
```

**Step 2: Run the tests**

Run: `PATH="$HOME/.cargo/bin:$PATH" cargo test -p sp42-core --lib citation::extract`
Expected: all PASS.

**Step 3: Re-export the function**

In `lib.rs`, extend the `citation::extract` re-export to include `extract_use_sites`.

**Step 4: clippy + fmt + commit**

```bash
PATH="$HOME/.cargo/bin:$PATH" cargo clippy -p sp42-core --all-targets -- -D warnings && cargo fmt -p sp42-core
git add crates/sp42-core/src/citation/extract.rs crates/sp42-core/src/lib.rs
SP42_SKIP_GIT_HOOKS=1 git commit -m "feat(citation): claim-to-ref association into use-sites

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_015NtcEzaj8J373GGsv3FC7y"
```

**Done when:** all `citation::extract` tests pass — end-of-sentence, multi-ref-per-sentence, ≤3 preceding context, non-URL skip, fragment fallback, empty-block failure; clippy clean.
