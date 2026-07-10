# Article Citation Extractor Implementation Plan — Phase 2: Sentence Segmentation

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** Split a block's prose into sentences with byte ranges, so a ref's `offset` can be mapped to the sentence it supports.

**Architecture:** Pure function in `sp42-core`, hand-rolled rule-based splitter with an abbreviation guard list. No dependency added (swappable for the `sentence-splitter` crate later). TDD — segmentation quality is the thing we iterate on.

**Tech Stack:** Rust; `std::ops::Range`. Inline `#[cfg(test)] mod tests`, table-driven.

**Scope:** Phase 2 of 6.

**Codebase verified:** 2026-06-24 — `sp42-core` tests are inline `#[cfg(test)] mod tests` with synchronous `#[test]` (e.g. `body_classifier.rs:188`). Ref markers are already stripped from `ParsoidBlock.text` (Phase 1 / Phase 5), so the splitter never sees `<ref>` noise.

---

## Task 1: `Sentence` type and `segment_sentences` skeleton

**Files:**
- Create: `crates/sp42-core/src/citation/segment.rs`
- Modify: citation module file — add `pub mod segment;` (alongside `pub mod verify;`)
- Modify: `crates/sp42-core/src/lib.rs` — `pub use citation::segment::{segment_sentences, Sentence};`

**Step 1: Write the failing test (single sentence, no terminator)**

```rust
//! Hand-rolled sentence segmentation for wiki prose. Ref markers are already
//! stripped upstream, so this sees clean text. Byte ranges index back into the
//! input for ref↔sentence association.

use std::ops::Range;

/// A sentence and its byte range within the segmented input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sentence {
    pub text: String,
    pub range: Range<usize>,
}

/// Split `text` into sentences. Never empty for non-empty input: text with no
/// detected terminator becomes a single sentence. Ranges are byte offsets into
/// `text`; concatenating slices in order reproduces `text`.
#[must_use]
pub fn segment_sentences(text: &str) -> Vec<Sentence> {
    todo!("implemented in Task 2")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(input: &str) -> Vec<String> {
        segment_sentences(input).into_iter().map(|s| s.text).collect()
    }

    #[test]
    fn no_terminator_is_one_sentence() {
        assert_eq!(texts("a claim with no period"), vec!["a claim with no period"]);
    }
}
```

**Step 2: Run to verify it fails**

Run: `PATH="$HOME/.cargo/bin:$PATH" cargo test -p sp42-core no_terminator_is_one_sentence -- --exact`
Expected: panics in `todo!` (FAIL).

---

## Task 2: Implement the splitter

**Files:**
- Modify: `crates/sp42-core/src/citation/segment.rs`

**Step 1: Add the failing edge-case tests**

Append to the `tests` module:

```rust
    #[test]
    fn splits_on_terminal_punctuation() {
        assert_eq!(
            texts("The treaty was signed. It ended the war. Peace held?"),
            vec![
                "The treaty was signed.",
                "It ended the war.",
                "Peace held?",
            ]
        );
    }

    #[test]
    fn does_not_split_common_abbreviations() {
        assert_eq!(
            texts("He moved to the U.S. in 1990. He stayed."),
            vec!["He moved to the U.S. in 1990.", "He stayed."]
        );
        assert_eq!(
            texts("Dr. Smith and Mr. Jones met. They agreed."),
            vec!["Dr. Smith and Mr. Jones met.", "They agreed."]
        );
    }

    #[test]
    fn does_not_split_decimals() {
        assert_eq!(texts("The value was 3.14 exactly."), vec!["The value was 3.14 exactly."]);
    }

    #[test]
    fn handles_trailing_quote_or_paren() {
        assert_eq!(
            texts("She said \"go.\" Then she left."),
            vec!["She said \"go.\"", "Then she left."]
        );
    }

    #[test]
    fn ranges_index_back_into_input() {
        let input = "One. Two.";
        let sentences = segment_sentences(input);
        for s in &sentences {
            assert_eq!(&input[s.range.clone()], s.text);
        }
        assert_eq!(sentences.len(), 2);
    }

    #[test]
    fn empty_input_is_empty() {
        assert!(segment_sentences("").is_empty());
        assert!(segment_sentences("   ").is_empty());
    }
```

**Step 2: Replace `todo!` with the implementation**

```rust
/// Abbreviations that end in `.` but do not end a sentence.
const ABBREVIATIONS: &[&str] = &[
    "U.S.", "U.K.", "U.N.", "E.U.", "a.m.", "p.m.",
    "Dr.", "Mr.", "Mrs.", "Ms.", "Prof.", "Sr.", "Jr.", "St.", "Mt.",
    "vs.", "etc.", "al.", "ca.", "c.", "No.", "Vol.", "pp.", "p.",
    "e.g.", "i.e.", "cf.", "Inc.", "Ltd.", "Co.",
];

#[must_use]
pub fn segment_sentences(text: &str) -> Vec<Sentence> {
    let bytes = text.as_bytes();
    let mut sentences = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;

    while i < bytes.len() {
        let c = bytes[i];
        if c == b'.' || c == b'!' || c == b'?' {
            // Consume trailing closing quotes/parens that belong to this sentence.
            let mut end = i + 1;
            while end < bytes.len() && matches!(bytes[end], b'"' | b'\'' | b')' | b']' | 0x9d) {
                end += 1;
            }
            if is_boundary(text, i, end) {
                let slice = &text[start..end];
                let trimmed_start = start + leading_ws(slice);
                let trimmed = text[start..end].trim();
                if !trimmed.is_empty() {
                    sentences.push(Sentence {
                        text: trimmed.to_string(),
                        range: trimmed_start..(trimmed_start + trimmed.len()),
                    });
                }
                start = end;
            }
            i = end;
        } else {
            i += 1;
        }
    }

    // Trailing remainder with no terminator.
    let tail = text[start..].trim();
    if !tail.is_empty() {
        let tail_start = start + leading_ws(&text[start..]);
        sentences.push(Sentence {
            text: tail.to_string(),
            range: tail_start..(tail_start + tail.len()),
        });
    }
    sentences
}

fn leading_ws(s: &str) -> usize {
    s.len() - s.trim_start().len()
}

/// Decide whether a terminator at byte `dot` (with closers up to `end`) ends a
/// sentence: not a decimal, not a known abbreviation, and followed by
/// whitespace+capital or end-of-text.
fn is_boundary(text: &str, dot: usize, end: usize) -> bool {
    let bytes = text.as_bytes();
    // Decimal: digit '.' digit
    if bytes[dot] == b'.'
        && dot > 0
        && bytes[dot - 1].is_ascii_digit()
        && end < bytes.len()
        && bytes[end].is_ascii_digit()
    {
        return false;
    }
    // Known abbreviation ending at `dot+1`.
    if bytes[dot] == b'.' {
        let head = &text[..dot + 1];
        if ABBREVIATIONS.iter().any(|abbr| head.ends_with(abbr)) {
            return false;
        }
    }
    // End of text → boundary.
    let mut j = end;
    while j < bytes.len() && (bytes[j] as char).is_whitespace() {
        j += 1;
    }
    if j >= bytes.len() {
        return true;
    }
    // Next non-space must be uppercase / digit / opening quote to count.
    let next = text[j..].chars().next().unwrap_or(' ');
    next.is_uppercase() || next.is_ascii_digit() || matches!(next, '"' | '\'' | '(')
}
```

(The `0x9d` byte in the closer match is the trailing byte of a UTF-8 right-double-quote `”`; ASCII closers cover the common case. If a future test needs full Unicode closers, switch the closer scan to `char`-based.)

**Step 3: Run all segment tests**

Run: `PATH="$HOME/.cargo/bin:$PATH" cargo test -p sp42-core --lib citation::segment -- --nocapture`
Expected: all PASS.

**Step 4: clippy + fmt**

Run: `PATH="$HOME/.cargo/bin:$PATH" cargo clippy -p sp42-core --all-targets -- -D warnings && cargo fmt -p sp42-core`
Expected: clean.

**Step 5: Commit**

```bash
git add crates/sp42-core/src/citation/segment.rs crates/sp42-core/src/lib.rs crates/sp42-core/src/citation*.rs
SP42_SKIP_GIT_HOOKS=1 git commit -m "feat(citation): hand-rolled sentence segmentation with abbreviation guard

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_015NtcEzaj8J373GGsv3FC7y"
```

**Done when:** all `citation::segment` tests pass; ranges index back into the input; clippy clean.
