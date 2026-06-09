//! The anti-fabrication locator (ADR-0007 §5).
//!
//! [`locate_quote`] decides whether a candidate supporting passage is present
//! **verbatim** in a source SP42 actually fetched, returning the byte offset of the
//! match (or `None`). It is the independent re-check that lets a non-deterministic
//! model sit in a governed system: a `Supported` / `Partial` verdict is only ever
//! surfaced if its quote re-locates in the fetched bytes (ADR-0007 §5; the gate
//! itself lives in [`crate::citation::verify`]).
//!
//! Matching folds only *transcription/extraction artifacts*, nothing semantic (ADR-0007
//! §5, Alt (c); SP42#25 layer 1): Unicode NFC, whitespace-run collapse, curly→straight
//! quotes, dash unification (en/em/figure dash, minus → `-`), zero-width-char stripping,
//! and **case folding** (so a re-cased quote — a transcription artifact, not a fabrication
//! — still locates). A quote that elides the middle with an ellipsis is matched
//! fragment-by-fragment, in document order, within a bounded window (SP42#25 layer 2) —
//! still verbatim per fragment. A genuinely *reworded* quote still does not match, and a
//! fabricated span still does not match, so the anti-fabrication guarantee is preserved. An
//! empty or whitespace-only quote returns `None` (an empty string would otherwise "locate
//! everywhere").
//!
//! The returned offset is the byte offset into the *original* `source` where the
//! (normalized) match begins. The load-bearing output is the found/not-found
//! decision; SP42's article-side anchor is the use-site ordinal (ADR-0007 §2), not
//! this offset, so a byte-offset convention is sufficient.

use std::sync::LazyLock;

use regex::Regex;
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

/// Minimum normalized length (in chars) for a fragment to anchor a multi-fragment match;
/// shorter fragments can stitch spuriously, so they are dropped (SP42#25 layer 2).
const MIN_FRAGMENT_CHARS: usize = 8;
/// Maximum normalized-char span from the first fragment's start to the last fragment's end —
/// bounds a multi-fragment match to a local passage, never stitching across a document.
const MAX_FRAGMENT_SPAN_CHARS: usize = 1500;

/// Ellipsis delimiters a model uses to elide the middle of a quoted passage: ASCII `...`,
/// Unicode `…`, or a bracketed `[...]` / `[…]`. Linear-time (no backtracking).
static ELLIPSIS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\.\.\.|…|\[\s*…?\s*\.*\s*\]").expect("valid ellipsis regex"));

/// Locate `quote` verbatim within `source`, returning the byte offset of the match
/// start in the original `source`, or `None` if it is not present.
///
/// Folds transcription artifacts only — NFC, whitespace-run collapse, curly→straight
/// quotes, dash unification, zero-width stripping, and case (ADR-0007 §5; SP42#25 layer 1).
/// An empty/whitespace-only quote returns `None`.
#[must_use]
pub fn locate_quote(quote: &str, source: &str) -> Option<usize> {
    let trimmed = quote.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Fast path: the trimmed quote occurs verbatim in the raw source.
    if let Some(byte_index) = source.find(trimmed) {
        return Some(byte_index);
    }

    // Normalized path: absorb whitespace / quote-style / NFC differences.
    let normalized_quote = normalize_for_match(trimmed);
    if normalized_quote.is_empty() {
        return None;
    }
    let (normalized_source, offset_map) = normalize_with_map(source);
    if let Some(byte_index) = normalized_source.find(&normalized_quote) {
        let char_index = normalized_source[..byte_index].chars().count();
        return offset_map.get(char_index).copied();
    }

    // Multi-fragment fallback (SP42#25 layer 2): the model elided the middle of a passage
    // with an ellipsis. Each fragment must locate verbatim, in order, within a bounded window.
    locate_multi_fragment(trimmed, &normalized_source, &offset_map)
}

/// Locate an ellipsis-elided quote as ordered fragments: split on ellipsis, require each
/// substantial (≥ `MIN_FRAGMENT_CHARS`) fragment to occur verbatim in document order within
/// `MAX_FRAGMENT_SPAN_CHARS`, and return the original byte offset of the first fragment.
/// Returns `None` unless at least two substantial fragments locate in order and in window, so
/// a fabricated/reworded quote or out-of-order fragments never stitch a match (ADR-0007 §5).
fn locate_multi_fragment(
    quote: &str,
    normalized_source: &str,
    offset_map: &[usize],
) -> Option<usize> {
    let fragments: Vec<String> = ELLIPSIS
        .split(quote)
        .map(normalize_for_match)
        .filter(|fragment| fragment.chars().count() >= MIN_FRAGMENT_CHARS)
        .collect();
    if fragments.len() < 2 {
        return None;
    }

    let mut search_from = 0usize; // byte index into `normalized_source`
    let mut first_char: Option<usize> = None;
    let mut last_end_char = 0usize;
    for fragment in &fragments {
        let relative = normalized_source
            .get(search_from..)?
            .find(fragment.as_str())?;
        let byte_index = search_from + relative;
        let start_char = normalized_source[..byte_index].chars().count();
        first_char.get_or_insert(start_char);
        last_end_char = start_char + fragment.chars().count();
        search_from = byte_index + fragment.len();
    }

    let first = first_char?;
    if last_end_char.saturating_sub(first) > MAX_FRAGMENT_SPAN_CHARS {
        return None;
    }
    offset_map.get(first).copied()
}

/// Fold a typographic quote or dash character to its ASCII equivalent; all other
/// characters pass through unchanged. These are transcription/extraction artifacts, not
/// semantic content (ADR-0007 §5; SP42#25 layer 1).
fn substitute(ch: char) -> char {
    match ch {
        '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' | '\u{2032}' => '\'',
        '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' | '\u{2033}' => '"',
        // Hyphen, non-breaking hyphen, figure dash, en/em dash, horizontal bar, minus.
        '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2015}'
        | '\u{2212}' => '-',
        other => other,
    }
}

/// Zero-width characters a model or extractor may insert mid-token; dropped before matching.
fn is_zero_width(ch: char) -> bool {
    matches!(ch, '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{FEFF}')
}

/// Normalize the quote side: NFC, then collapse whitespace runs to a single ASCII
/// space and substitute curly quotes; finally trim.
fn normalize_for_match(text: &str) -> String {
    let mut out = String::new();
    let mut prev_space = false;
    for ch in text.nfc() {
        if is_zero_width(ch) {
            continue;
        }
        if ch.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            for low in substitute(ch).to_lowercase() {
                out.push(low);
            }
            prev_space = false;
        }
    }
    out.trim().to_string()
}

/// Append the NFC form of a completed base+combining-marks unit to `text`, recording
/// the unit's original byte offset for every emitted character.
fn flush_unit(text: &mut String, map: &mut Vec<usize>, unit: &str, unit_start: usize) {
    if unit.is_empty() {
        return;
    }
    for ch in unit.nfc() {
        for low in ch.to_lowercase() {
            text.push(low);
            map.push(unit_start);
        }
    }
}

/// Normalize the source side, returning the normalized text alongside a per-character
/// map back to the original byte offset.
///
/// Each base character plus its trailing combining marks is NFC-composed as one unit
/// so a decomposed (NFD) source matches a precomposed (NFC) quote, while the map still
/// points each normalized character at the start of the original unit it came from.
fn normalize_with_map(source: &str) -> (String, Vec<usize>) {
    let mut text = String::new();
    let mut map: Vec<usize> = Vec::new();
    let mut prev_space = false;
    let mut unit = String::new();
    let mut unit_start = 0usize;
    let mut byte_offset = 0usize;

    for ch in source.chars() {
        let start = byte_offset;
        byte_offset += ch.len_utf8();

        if is_zero_width(ch) {
            continue;
        }

        if ch.is_whitespace() {
            flush_unit(&mut text, &mut map, &unit, unit_start);
            unit.clear();
            if !prev_space {
                text.push(' ');
                map.push(start);
                prev_space = true;
            }
            continue;
        }

        prev_space = false;
        let sub = substitute(ch);
        if !unit.is_empty() && is_combining_mark(ch) {
            unit.push(sub);
        } else {
            flush_unit(&mut text, &mut map, &unit, unit_start);
            unit.clear();
            unit.push(sub);
            unit_start = start;
        }
    }
    flush_unit(&mut text, &mut map, &unit, unit_start);

    (text, map)
}

#[cfg(test)]
mod tests {
    use super::locate_quote;
    use proptest::prelude::*;

    #[test]
    fn exact_substring_returns_its_offset() {
        let source = "He won the Nobel Prize in 1921.";
        let offset = locate_quote("won the Nobel Prize", source).expect("should locate");
        assert_eq!(offset, 3);
        assert!(source[offset..].starts_with("won the Nobel Prize"));
    }

    #[test]
    fn match_at_start_is_offset_zero() {
        assert_eq!(locate_quote("Acme Corp", "Acme Corp was founded"), Some(0));
    }

    #[test]
    fn absent_quote_returns_none() {
        // The must-reject case: a fabricated quote that is not in the source.
        assert_eq!(
            locate_quote("never appears here", "a completely different text"),
            None
        );
    }

    #[test]
    fn empty_or_whitespace_quote_returns_none() {
        assert_eq!(locate_quote("", "anything"), None);
        assert_eq!(locate_quote("   \n\t ", "anything"), None);
    }

    #[test]
    fn case_difference_now_matches_and_points_at_original() {
        // Re-casing is a transcription artifact, not a fabrication (SP42#25, layer 1):
        // a re-cased quote must locate, and the offset must point at the original-cased span.
        let source = "won the Nobel Prize";
        let offset = locate_quote("NOBEL PRIZE", source).expect("case-insensitive match");
        assert!(source[offset..].starts_with("Nobel Prize"));
    }

    #[test]
    fn mixed_case_quote_locates() {
        assert!(locate_quote("acme CORP", "Acme Corp was founded").is_some());
    }

    #[test]
    fn dash_variants_unify() {
        // en/em dash and minus sign all match an ASCII hyphen and vice-versa.
        assert!(locate_quote("2010-2020", "growth of 2010\u{2013}2020 was steep").is_some());
        assert!(locate_quote("cost\u{2014}benefit", "a cost-benefit analysis").is_some());
    }

    #[test]
    fn zero_width_chars_are_ignored() {
        // A zero-width space inside a source token must not block the match.
        let source = "the No\u{200B}bel Prize";
        assert!(locate_quote("Nobel Prize", source).is_some());
    }

    #[test]
    fn fabricated_quote_still_rejected_under_case_folding() {
        // Guardrail: case-folding must NOT let a fabricated span match.
        assert_eq!(
            locate_quote("entirely invented phrase", "a completely different text"),
            None
        );
    }

    #[test]
    fn leading_and_trailing_whitespace_on_quote_is_ignored() {
        assert_eq!(locate_quote("  Nobel Prize  ", "the Nobel Prize"), Some(4));
    }

    #[test]
    fn whitespace_runs_in_source_still_locate_and_point_at_original() {
        let source = "won the\n   Nobel    Prize today";
        let offset = locate_quote("won the Nobel Prize", source).expect("should locate");
        assert!(source[offset..].starts_with("won the"));
    }

    #[test]
    fn curly_quotes_in_source_match_straight_in_quote() {
        let source = "she said \u{201C}hello world\u{201D} loudly";
        let offset = locate_quote("\"hello world\"", source).expect("should locate");
        assert!(source[offset..].starts_with('\u{201C}'));
    }

    #[test]
    fn curly_apostrophe_in_source_matches_straight() {
        let source = "the company\u{2019}s founder";
        assert!(locate_quote("company's founder", source).is_some());
    }

    #[test]
    fn nfd_source_matches_nfc_quote() {
        // "café" with a decomposed é (e + combining acute) in the source.
        let source = "the cafe\u{301} on the corner";
        let quote = "caf\u{e9}"; // precomposed é
        assert!(locate_quote(quote, source).is_some());
    }

    #[test]
    fn nfc_source_matches_nfd_quote() {
        let source = "the caf\u{e9} on the corner"; // precomposed
        let quote = "cafe\u{301}"; // decomposed
        assert!(locate_quote(quote, source).is_some());
    }

    // --- multi-fragment / ellipsis (SP42#25 layer 2) ---

    #[test]
    fn multi_fragment_locates_in_order_and_points_at_the_first_fragment() {
        let source = "The scruffy park near the corner of Third Street has gotten new asphalt \
                      on the dog-walking paths and removed brush and old tires from the cove.";
        let quote = "The scruffy park near the corner ... removed brush and old tires";
        let offset = locate_quote(quote, source).expect("multi-fragment should locate");
        assert!(source[offset..].starts_with("The scruffy park"));
    }

    #[test]
    fn multi_fragment_unicode_ellipsis_locates() {
        let source = "Acme Corp was established in 1985 by investors, and its founder John Smith \
                      served as chief executive until 2001.";
        let quote = "Acme Corp was established in 1985 \u{2026} its founder John Smith";
        assert!(locate_quote(quote, source).is_some());
    }

    #[test]
    fn multi_fragment_out_of_order_does_not_match() {
        // Fragments present but in the WRONG order must not stitch a match.
        let source = "The scruffy park near the corner has removed brush and old tires.";
        let quote = "removed brush and old tires ... The scruffy park near the corner";
        assert_eq!(locate_quote(quote, source), None);
    }

    #[test]
    fn multi_fragment_with_a_fabricated_fragment_does_not_match() {
        let source = "The scruffy park near the corner has removed brush and old tires.";
        let quote = "The scruffy park near the corner ... an entirely invented closing span";
        assert_eq!(locate_quote(quote, source), None);
    }

    #[test]
    fn multi_fragment_trivial_fragments_do_not_match() {
        // Sub-threshold (<8 normalized chars) fragments cannot anchor a stitch.
        let source = "the quick brown fox jumped over the lazy dog in the yard";
        assert_eq!(locate_quote("the ... fox ... dog", source), None);
    }

    #[test]
    fn multi_fragment_respects_the_bounded_window() {
        // Two real fragments separated by far more than the bounded window must not stitch.
        let filler = "lorem ipsum dolor sit amet ".repeat(120); // ~3200 chars
        let source = format!("the scruffy park is here {filler} and old tires were removed");
        let quote = "the scruffy park is here ... and old tires were removed";
        assert_eq!(locate_quote(quote, &source), None);
    }

    proptest! {
        /// Anti-fabrication (the guardrail): a multi-fragment quote whose alphabet is
        /// DISJOINT from the source can never stitch a match, however the ellipsis splits it.
        #[test]
        fn fabricated_disjoint_alphabet_multi_fragment_never_locates(
            a in "[n-z ]{8,40}",
            b in "[n-z ]{8,40}",
            src in "[a-m ]{50,400}",
        ) {
            let quote = format!("{a} ... {b}");
            prop_assert_eq!(locate_quote(&quote, &src), None);
        }
    }
}
