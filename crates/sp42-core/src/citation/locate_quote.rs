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

/// Minimum cleaned quote tokens before the fuzzy path may fire (SP42#25 layer 5): short
/// spans carry too little signal to fuzzy-match safely, so they must locate exactly.
const MIN_FUZZY_TOKENS: usize = 5;
/// Fuzzy similarity threshold as a ratio: `matched * DEN >= quote_tokens * NUM` (85%).
/// Integer arithmetic only — no float ever touches a verdict path (ADR-0006/0008).
const FUZZY_THRESHOLD_NUM: usize = 17;
const FUZZY_THRESHOLD_DEN: usize = 20;
/// Cap on candidate anchor windows examined — bounds worst-case work on a hostile source.
const MAX_FUZZY_WINDOWS: usize = 50;

/// A guarded fuzzy match (SP42#25 layer 5): the span is the SOURCE's own text (the code
/// extracts real fetched bytes — the model's mangled quote is never surfaced), with the
/// measured token counts that justified it (any ratio is derived at display, never stored).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuzzyLocate {
    /// Byte offset of the span start in the original source.
    pub offset: usize,
    /// The matched span, copied from the original source bytes.
    pub span: String,
    /// Quote tokens matched in order within the window (LCS).
    pub matched_tokens: u32,
    /// Total cleaned quote tokens.
    pub quote_tokens: u32,
}

/// A token of the normalized source: its cleaned text plus its char range, so a matched
/// window can be mapped back to original source bytes via the offset map.
struct SourceToken {
    cleaned: String,
    start_char: usize,
    end_char: usize,
}

/// Locate `quote` in `source` by bounded fuzzy match — the guarded last resort (SP42#25
/// layer 5), tried only after [`locate_quote`] fails. Guards, all of which must hold:
///
/// - the quote has at least [`MIN_FUZZY_TOKENS`] cleaned tokens (short spans: exact only);
/// - candidate windows are anchored on real shared tokens (a quote sharing no anchor with
///   the source examines zero windows — a fabricated quote cannot even be considered);
/// - quote tokens must match **in order** (longest common subsequence) at ≥ 85%;
/// - every load-bearing (digit-bearing) quote token occurs EXACTLY in the window — a wrong
///   date or number is a factual mismatch, never transcription noise.
///
/// The returned span is the source's own text; the model's wording is discarded.
#[must_use]
pub fn locate_quote_fuzzy(quote: &str, source: &str) -> Option<FuzzyLocate> {
    let normalized_quote = normalize_for_match(quote.trim());
    let quote_tokens: Vec<String> = normalized_quote
        .split(' ')
        .map(clean_token)
        .filter(|token| !token.is_empty())
        .collect();
    if quote_tokens.len() < MIN_FUZZY_TOKENS {
        return None;
    }

    let (normalized_source, offset_map) = normalize_with_map(source);
    let source_tokens = tokenize_source(&normalized_source);
    if source_tokens.is_empty() {
        return None;
    }

    let load_bearing: Vec<&String> = quote_tokens
        .iter()
        .filter(|token| token.chars().any(|ch| ch.is_ascii_digit()))
        .collect();
    let anchors = anchor_tokens(&quote_tokens);
    if anchors.is_empty() {
        return None;
    }

    // Examine a bounded window of source tokens around every anchor occurrence.
    let radius = quote_tokens.len() + 2;
    let mut best: Option<(usize, usize, usize)> = None; // (matched, first_token, last_token)
    let mut windows = 0usize;
    for (index, token) in source_tokens.iter().enumerate() {
        if !anchors.contains(&token.cleaned.as_str()) {
            continue;
        }
        windows += 1;
        if windows > MAX_FUZZY_WINDOWS {
            break;
        }
        let window_start = index.saturating_sub(radius);
        let window_end = (index + radius + 1).min(source_tokens.len());
        let window = &source_tokens[window_start..window_end];
        if let Some((matched, first, last)) = lcs_match(&quote_tokens, window) {
            let candidate = (matched, window_start + first, window_start + last);
            if best.is_none_or(|(best_matched, _, _)| matched > best_matched) {
                best = Some(candidate);
            }
        }
    }

    let (matched, first_token, last_token) = best?;
    if matched * FUZZY_THRESHOLD_DEN < quote_tokens.len() * FUZZY_THRESHOLD_NUM {
        return None;
    }
    // Load-bearing tokens must occur exactly inside the matched span itself.
    let span_tokens = &source_tokens[first_token..=last_token];
    if !load_bearing
        .iter()
        .all(|needed| span_tokens.iter().any(|token| token.cleaned == **needed))
    {
        return None;
    }

    let start_char = source_tokens[first_token].start_char;
    let end_char = source_tokens[last_token].end_char;
    let start_byte = offset_map.get(start_char).copied()?;
    let end_byte = offset_map.get(end_char).copied().unwrap_or(source.len());
    Some(FuzzyLocate {
        offset: start_byte,
        span: source[start_byte..end_byte].trim_end().to_string(),
        matched_tokens: u32::try_from(matched).unwrap_or(u32::MAX),
        quote_tokens: u32::try_from(quote_tokens.len()).unwrap_or(u32::MAX),
    })
}

/// Strip leading/trailing punctuation from a normalized token, so `"study,"` and
/// `"(1985)"` compare as `"study"` / `"1985"`. Interior punctuation (hyphens, apostrophes)
/// is kept — it is part of the word.
fn clean_token(token: &str) -> String {
    token
        .trim_matches(|ch: char| !ch.is_alphanumeric())
        .to_string()
}

/// Tokenize the normalized source into cleaned tokens with their char ranges.
fn tokenize_source(normalized_source: &str) -> Vec<SourceToken> {
    let mut tokens = Vec::new();
    let mut start: Option<usize> = None;
    let mut current = String::new();
    for (char_index, ch) in normalized_source.chars().enumerate() {
        if ch == ' ' {
            if let Some(start_char) = start.take() {
                tokens.push(SourceToken {
                    cleaned: clean_token(&current),
                    start_char,
                    end_char: char_index,
                });
                current.clear();
            }
        } else {
            if start.is_none() {
                start = Some(char_index);
            }
            current.push(ch);
        }
    }
    if let Some(start_char) = start {
        let end_char = normalized_source.chars().count();
        tokens.push(SourceToken {
            cleaned: clean_token(&current),
            start_char,
            end_char,
        });
    }
    tokens.retain(|token| !token.cleaned.is_empty());
    tokens
}

/// Anchor tokens for candidate-window generation: every digit-bearing token plus the
/// three longest tokens of at least five chars. A window is only ever opened where one of
/// these occurs verbatim in the source.
fn anchor_tokens(quote_tokens: &[String]) -> Vec<&str> {
    let mut anchors: Vec<&str> = quote_tokens
        .iter()
        .filter(|token| token.chars().count() >= 5 || token.chars().any(|ch| ch.is_ascii_digit()))
        .map(String::as_str)
        .collect();
    anchors.sort_by_key(|token| std::cmp::Reverse(token.chars().count()));
    anchors.truncate(3);
    anchors
}

/// Longest common subsequence of `quote_tokens` within `window`, returning the matched
/// count plus the window indices of the first and last matched tokens, or `None` when
/// nothing matches. Order is enforced by construction — shuffled tokens do not count.
fn lcs_match(quote_tokens: &[String], window: &[SourceToken]) -> Option<(usize, usize, usize)> {
    let rows = quote_tokens.len();
    let cols = window.len();
    let mut table = vec![0usize; (rows + 1) * (cols + 1)];
    let at = |row: usize, col: usize| row * (cols + 1) + col;
    for row in 1..=rows {
        for col in 1..=cols {
            table[at(row, col)] = if quote_tokens[row - 1] == window[col - 1].cleaned {
                table[at(row - 1, col - 1)] + 1
            } else {
                table[at(row - 1, col)].max(table[at(row, col - 1)])
            };
        }
    }
    let matched = table[at(rows, cols)];
    if matched == 0 {
        return None;
    }
    // Backtrack for the window range that the match actually spans.
    let (mut row, mut col) = (rows, cols);
    let mut first: Option<usize> = None;
    let mut last: Option<usize> = None;
    while row > 0 && col > 0 {
        if quote_tokens[row - 1] == window[col - 1].cleaned
            && table[at(row, col)] == table[at(row - 1, col - 1)] + 1
        {
            first = Some(col - 1);
            last = last.or(Some(col - 1));
            row -= 1;
            col -= 1;
        } else if table[at(row - 1, col)] >= table[at(row, col - 1)] {
            row -= 1;
        } else {
            col -= 1;
        }
    }
    Some((matched, first?, last?))
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
    use super::{locate_quote, locate_quote_fuzzy};
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

    // --- bounded fuzzy locate (SP42#25 layer 5) ---

    #[test]
    fn fuzzy_locates_a_quote_with_one_reworded_token_and_returns_the_source_span() {
        let source = "In 1985 the Acme Corporation was established in Springfield by a group \
                      of local investors led by John Smith.";
        // The model wrote "founded" where the source says "established": exact locate fails,
        // fuzzy recovers — and the returned span is the SOURCE's own text, not the model's.
        let quote = "the Acme Corporation was founded in Springfield by a group of local investors";
        assert_eq!(locate_quote(quote, source), None);
        let hit = locate_quote_fuzzy(quote, source).expect("fuzzy should locate");
        assert!(source[hit.offset..].starts_with("the Acme Corporation"));
        assert!(hit.span.contains("established in Springfield"));
        assert!(!hit.span.contains("founded"));
    }

    #[test]
    fn fuzzy_rejects_a_mismatched_load_bearing_number() {
        // The quote's year does not appear in the source: dates/numbers are load-bearing
        // and must match EXACTLY — a high token overlap cannot paper over a wrong number.
        let source = "In 1985 the Acme Corporation was established in Springfield by a group \
                      of local investors led by John Smith.";
        let quote = "In 1958 the Acme Corporation was established in Springfield by a group \
                     of local investors";
        assert_eq!(locate_quote_fuzzy(quote, source), None);
    }

    #[test]
    fn fuzzy_rejects_a_fabricated_quote() {
        let source = "The committee reviewed the annual budget and approved the proposal.";
        let quote = "the museum acquired seventeen paintings from the private collection downtown";
        assert_eq!(locate_quote_fuzzy(quote, source), None);
    }

    #[test]
    fn fuzzy_rejects_short_quotes_entirely() {
        // Below the minimum token count fuzzy never fires — short spans must locate exactly.
        let source = "The bridge opened to traffic in August.";
        assert_eq!(locate_quote_fuzzy("bridge opened to cars", source), None);
    }

    #[test]
    fn fuzzy_rejects_low_similarity_even_with_shared_anchors() {
        // Shares "Springfield"/"investors" anchors but most tokens differ: below threshold.
        let source = "In 1985 the Acme Corporation was established in Springfield by a group \
                      of local investors led by John Smith.";
        let quote = "several wealthy investors from Springfield reportedly demanded immediate \
                     control over every major decision";
        assert_eq!(locate_quote_fuzzy(quote, source), None);
    }

    #[test]
    fn fuzzy_tolerates_minor_punctuation_and_inflection_drift() {
        let source = "The study, published in March 2019, found that 62 percent of participants \
                      reported improved sleep quality after eight weeks.";
        let quote = "The study published in March 2019 found that 62 percent of participants \
                     reported improved sleep";
        // Exact locate already handles pure punctuation/whitespace; clip one word so the
        // exact path genuinely fails and the fuzzy path is exercised end to end.
        let quote = quote.replace("reported improved", "noted improved");
        assert_eq!(locate_quote(&quote, source), None);
        let hit = locate_quote_fuzzy(&quote, source).expect("fuzzy should locate");
        assert!(hit.span.contains("62 percent"));
    }

    #[test]
    fn fuzzy_counts_are_measured_not_derived() {
        // No float anywhere (ADR-0006/0008 discipline): the outcome carries measured
        // token counts; any ratio is derived at display time.
        let source = "In 1985 the Acme Corporation was established in Springfield by a group \
                      of local investors led by John Smith.";
        let quote = "the Acme Corporation was founded in Springfield by a group of local investors";
        let hit = locate_quote_fuzzy(quote, source).expect("fuzzy should locate");
        assert!(hit.quote_tokens >= hit.matched_tokens);
        assert!(hit.matched_tokens > 0);
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

        /// Layer 5 guardrail: a fuzzy match still requires REAL shared tokens — a quote
        /// whose alphabet is disjoint from the source can never fuzzy-locate.
        #[test]
        fn fabricated_disjoint_alphabet_never_fuzzy_locates(
            quote in "[n-z]{4,9}( [n-z]{4,9}){5,12}",
            src in "[a-m ]{50,400}",
        ) {
            prop_assert_eq!(locate_quote_fuzzy(&quote, &src), None);
        }
    }
}
