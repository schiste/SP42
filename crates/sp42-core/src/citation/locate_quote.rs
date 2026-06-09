//! The anti-fabrication locator (ADR-0007 §5).
//!
//! [`locate_quote`] decides whether a candidate supporting passage is present
//! **verbatim** in a source SP42 actually fetched, returning the byte offset of the
//! match (or `None`). It is the independent re-check that lets a non-deterministic
//! model sit in a governed system: a `Supported` / `Partial` verdict is only ever
//! surfaced if its quote re-locates in the fetched bytes (ADR-0007 §5; the gate
//! itself lives in [`crate::citation::verify`]).
//!
//! Matching is **case-sensitive** with only *conservative* normalization — Unicode
//! NFC, whitespace-run collapse, and curly→straight quote substitution — enough to
//! absorb HTML→text extraction artifacts and **nothing semantic** (ADR-0007 §5,
//! Alt (c)). A reworded or re-cased "quote" therefore does not match. An empty or
//! whitespace-only quote returns `None` (an empty string would otherwise "locate
//! everywhere").
//!
//! The returned offset is the byte offset into the *original* `source` where the
//! (normalized) match begins. The load-bearing output is the found/not-found
//! decision; SP42's article-side anchor is the use-site ordinal (ADR-0007 §2), not
//! this offset, so a byte-offset convention is sufficient.

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

/// Locate `quote` verbatim within `source`, returning the byte offset of the match
/// start in the original `source`, or `None` if it is not present.
///
/// Case-sensitive; normalizes only NFC + whitespace-run collapse + curly→straight
/// quotes (ADR-0007 §5). An empty/whitespace-only quote returns `None`.
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
    let byte_index = normalized_source.find(&normalized_quote)?;
    let char_index = normalized_source[..byte_index].chars().count();
    offset_map.get(char_index).copied()
}

/// Map a typographic quote character to its straight ASCII equivalent; all other
/// characters pass through unchanged (ADR-0007 §5; the exact wikiharness table).
fn substitute(ch: char) -> char {
    match ch {
        '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' | '\u{2032}' => '\'',
        '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' | '\u{2033}' => '"',
        other => other,
    }
}

/// Normalize the quote side: NFC, then collapse whitespace runs to a single ASCII
/// space and substitute curly quotes; finally trim.
fn normalize_for_match(text: &str) -> String {
    let mut out = String::new();
    let mut prev_space = false;
    for ch in text.nfc() {
        if ch.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(substitute(ch));
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
        text.push(ch);
        map.push(unit_start);
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
    fn case_difference_does_not_match() {
        // Deliberate: a model that re-cases a quote gets no free pass.
        assert_eq!(locate_quote("NOBEL PRIZE", "won the Nobel Prize"), None);
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
}
