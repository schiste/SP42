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

/// Abbreviations that end in `.` but do not end a sentence.
const ABBREVIATIONS: &[&str] = &[
    "U.S.", "U.K.", "U.N.", "E.U.", "U.", "K.", "N.", "a.m.", "p.m.", "Dr.", "Mr.", "Mrs.", "Ms.",
    "Prof.", "Sr.", "Jr.", "St.", "Mt.", "vs.", "etc.", "al.", "ca.", "c.", "No.", "Vol.", "pp.",
    "p.", "e.g.", "i.e.", "cf.", "Inc.", "Ltd.", "Co.",
];

/// Split `text` into sentences. Never empty for non-empty input: text with no
/// detected terminator becomes a single sentence. Ranges are byte offsets into
/// `text`; concatenating slices in order reproduces `text`.
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
            // Scan forward by chars (not bytes) to stay on char boundaries.
            let remainder = &text[end..];
            let mut char_offset = 0;
            for ch in remainder.chars() {
                if matches!(
                    ch,
                    '"' | '\'' | ')' | ']' | '\u{201C}' | '\u{201D}' | '\u{2018}' | '\u{2019}'
                ) {
                    char_offset += ch.len_utf8();
                } else {
                    break;
                }
            }
            end += char_offset;

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
    // Known abbreviation: check if this dot is part of a known abbreviation.
    if bytes[dot] == b'.' {
        // Look at text ending at dot+1 and see if any abbreviation matches at the end.
        let head = &text[..=dot];

        // Check abbreviations that end at this dot
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

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(input: &str) -> Vec<String> {
        segment_sentences(input)
            .into_iter()
            .map(|s| s.text)
            .collect()
    }

    #[test]
    fn no_terminator_is_one_sentence() {
        assert_eq!(
            texts("a claim with no period"),
            vec!["a claim with no period"]
        );
    }

    #[test]
    fn splits_on_terminal_punctuation() {
        assert_eq!(
            texts("The treaty was signed. It ended the war. Peace held?"),
            vec!["The treaty was signed.", "It ended the war.", "Peace held?",]
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
        assert_eq!(
            texts("The value was 3.14 exactly."),
            vec!["The value was 3.14 exactly."]
        );
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

    #[test]
    fn multibyte_accented_latin_after_terminator() {
        let input = "Café René opened. It closed.";
        let sentences = segment_sentences(input);

        // Verify the segmentation
        assert_eq!(sentences.len(), 2);
        assert_eq!(sentences[0].text, "Café René opened.");
        assert_eq!(sentences[1].text, "It closed.");

        // Verify byte ranges index back into input without panic
        for s in &sentences {
            assert_eq!(&input[s.range.clone()], s.text);
        }
    }

    #[test]
    fn multibyte_cyrillic_after_terminator() {
        let input = "Текст один. Текст два.";
        let sentences = segment_sentences(input);

        // Verify the segmentation
        assert_eq!(sentences.len(), 2);
        assert_eq!(sentences[0].text, "Текст один.");
        assert_eq!(sentences[1].text, "Текст два.");

        // Verify byte ranges index back into input without panic
        for s in &sentences {
            assert_eq!(&input[s.range.clone()], s.text);
        }
    }

    #[test]
    fn multibyte_curly_quotes_after_terminator() {
        let input = "She said \u{201C}go.\u{201D} Then she left.";
        let sentences = segment_sentences(input);

        // Verify the segmentation with curly quotes
        assert_eq!(sentences.len(), 2);
        assert_eq!(sentences[0].text, "She said \u{201C}go.\u{201D}");
        assert_eq!(sentences[1].text, "Then she left.");

        // Verify byte ranges index back into input without panic
        for s in &sentences {
            assert_eq!(&input[s.range.clone()], s.text);
        }
    }
}
