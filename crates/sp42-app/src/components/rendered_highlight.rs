use std::collections::HashSet;

use sp42_core::{DiffSegmentKind, InlineSpan};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RenderedHighlightPhrase {
    text: String,
    whole_word_only: bool,
}

pub(crate) struct RenderedHighlightSource<'a> {
    pub(crate) kind: DiffSegmentKind,
    pub(crate) text: &'a str,
    pub(crate) inline_highlights: &'a [InlineSpan],
}

pub(crate) fn collect_rendered_highlight_phrases<'a>(
    segments: impl IntoIterator<Item = RenderedHighlightSource<'a>>,
    target_kind: DiffSegmentKind,
) -> Vec<RenderedHighlightPhrase> {
    let mut phrases = Vec::new();
    let mut seen = HashSet::new();

    for segment in segments
        .into_iter()
        .filter(|segment| segment.kind == target_kind)
    {
        if !segment.inline_highlights.is_empty() {
            for span in segment
                .inline_highlights
                .iter()
                .filter(|span| span.kind == target_kind)
            {
                push_rendered_highlight_phrases(&mut phrases, &mut seen, &span.text, false);
            }
        } else {
            push_rendered_highlight_phrases(&mut phrases, &mut seen, segment.text, true);
        }
    }

    phrases.sort_by(|left, right| {
        right
            .text
            .len()
            .cmp(&left.text.len())
            .then_with(|| left.text.cmp(&right.text))
    });
    phrases.truncate(24);
    phrases
}

#[cfg(any(target_arch = "wasm32", test))]
pub(crate) fn find_rendered_highlight_matches(
    text: &str,
    phrases: &[RenderedHighlightPhrase],
) -> Vec<(usize, usize)> {
    let mut matches = Vec::new();
    let mut cursor = 0usize;

    while cursor < text.len() {
        let candidate = phrases
            .iter()
            .filter_map(|phrase| find_rendered_phrase_match(text, cursor, phrase))
            .min_by(|left, right| left.0.cmp(&right.0).then_with(|| right.1.cmp(&left.1)));

        let Some((start, len)) = candidate else {
            break;
        };
        let end = start + len;
        matches.push((start, end));
        cursor = end;
    }

    matches
}

fn push_rendered_highlight_phrases(
    phrases: &mut Vec<RenderedHighlightPhrase>,
    seen: &mut HashSet<String>,
    raw: &str,
    fallback_only: bool,
) {
    for line in split_highlight_text_lines(raw) {
        for phrase in build_rendered_highlight_candidates(&line, fallback_only) {
            let dedupe_key = format!("{}:{}", phrase.whole_word_only as u8, phrase.text);
            if seen.insert(dedupe_key) {
                phrases.push(phrase);
            }
        }
    }
}

fn split_highlight_text_lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }

    text.split_inclusive('\n')
        .map(ToString::to_string)
        .collect::<Vec<_>>()
}

fn normalize_rendered_highlight_phrase(raw: &str) -> Option<String> {
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = collapsed.trim();
    if trimmed.len() < 3 {
        return None;
    }
    if !trimmed.chars().any(|ch| ch.is_alphanumeric()) {
        return None;
    }
    let has_markup = ["[[", "]]", "{{", "}}", "|", "http://", "https://", "="]
        .iter()
        .any(|token| trimmed.contains(token));
    if has_markup {
        return None;
    }
    Some(trimmed.to_string())
}

fn build_rendered_highlight_candidates(
    raw: &str,
    fallback_only: bool,
) -> Vec<RenderedHighlightPhrase> {
    let Some(normalized) = normalize_rendered_highlight_phrase(raw) else {
        return Vec::new();
    };

    let tokens = extract_rendered_word_tokens(&normalized);
    if tokens.is_empty() {
        return Vec::new();
    }

    let mut candidates = Vec::new();

    if !fallback_only && tokens.len() <= 4 && normalized.len() <= 48 {
        candidates.push(RenderedHighlightPhrase {
            text: normalized.clone(),
            whole_word_only: true,
        });
    }

    if !fallback_only && tokens.len() > 1 {
        for window in (2..=3).rev() {
            if tokens.len() < window {
                continue;
            }
            for index in 0..=tokens.len() - window {
                let phrase = tokens[index..index + window].join(" ");
                if phrase.len() >= 5 {
                    candidates.push(RenderedHighlightPhrase {
                        text: phrase,
                        whole_word_only: true,
                    });
                }
            }
        }
    }

    for token in tokens {
        if token.chars().count() >= 3 {
            candidates.push(RenderedHighlightPhrase {
                text: token,
                whole_word_only: true,
            });
        }
    }

    candidates
}

fn extract_rendered_word_tokens(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        if ch.is_alphanumeric() {
            current.push(ch);
        } else if (ch == '\'' || ch == '’' || ch == '-' || ch == '_') && !current.is_empty() {
            current.push(ch);
        } else if !current.is_empty() {
            trim_token_suffix(&mut current);
            if current.chars().any(|ch| ch.is_alphanumeric()) {
                tokens.push(std::mem::take(&mut current));
            } else {
                current.clear();
            }
        }
    }

    if !current.is_empty() {
        trim_token_suffix(&mut current);
        if current.chars().any(|ch| ch.is_alphanumeric()) {
            tokens.push(current);
        }
    }

    tokens
}

fn trim_token_suffix(token: &mut String) {
    while token.chars().last().is_some_and(|ch| !ch.is_alphanumeric()) {
        token.pop();
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn find_rendered_phrase_match(
    text: &str,
    cursor: usize,
    phrase: &RenderedHighlightPhrase,
) -> Option<(usize, usize)> {
    let mut search_from = cursor;
    while search_from < text.len() {
        let offset = text[search_from..].find(&phrase.text)?;
        let start = search_from + offset;
        let end = start + phrase.text.len();
        if !phrase.whole_word_only || is_whole_word_match(text, start, end) {
            return Some((start, phrase.text.len()));
        }
        search_from = end;
    }
    None
}

#[cfg(any(target_arch = "wasm32", test))]
fn is_whole_word_match(text: &str, start: usize, end: usize) -> bool {
    let prev_ok = text[..start]
        .chars()
        .next_back()
        .is_none_or(|ch| !ch.is_alphanumeric());
    let next_ok = text[end..]
        .chars()
        .next()
        .is_none_or(|ch| !ch.is_alphanumeric());
    prev_ok && next_ok
}

#[cfg(test)]
mod tests {
    use sp42_core::{DiffSegmentKind, InlineSpan};

    use super::{
        RenderedHighlightPhrase, RenderedHighlightSource, build_rendered_highlight_candidates,
        collect_rendered_highlight_phrases, extract_rendered_word_tokens,
        find_rendered_highlight_matches, normalize_rendered_highlight_phrase,
    };

    #[test]
    fn normalize_rendered_highlight_phrase_filters_markup_noise() {
        assert_eq!(
            normalize_rendered_highlight_phrase("  Added text here  "),
            Some("Added text here".to_string())
        );
        assert_eq!(normalize_rendered_highlight_phrase("{{Infobox}}"), None);
        assert_eq!(
            normalize_rendered_highlight_phrase("[[File:Example.jpg]]"),
            None
        );
        assert_eq!(normalize_rendered_highlight_phrase("  "), None);
    }

    #[test]
    fn rendered_highlight_matches_prefer_longer_phrase_at_same_position() {
        let matches = find_rendered_highlight_matches(
            "Added a major city landmark",
            &[
                RenderedHighlightPhrase {
                    text: "Added".to_string(),
                    whole_word_only: true,
                },
                RenderedHighlightPhrase {
                    text: "Added a major".to_string(),
                    whole_word_only: true,
                },
            ],
        );

        assert_eq!(matches, vec![(0, "Added a major".len())]);
    }

    #[test]
    fn rendered_highlight_matches_respect_word_boundaries() {
        let matches = find_rendered_highlight_matches(
            "Capitales parisiennes",
            &[RenderedHighlightPhrase {
                text: "pari".to_string(),
                whole_word_only: true,
            }],
        );

        assert!(matches.is_empty());
    }

    #[test]
    fn extract_rendered_word_tokens_keeps_short_meaningful_units() {
        assert_eq!(
            extract_rendered_word_tokens("Jean-Pierre d'Arc 2024"),
            vec![
                "Jean-Pierre".to_string(),
                "d'Arc".to_string(),
                "2024".to_string()
            ]
        );
    }

    #[test]
    fn build_rendered_highlight_candidates_avoids_long_sentence_fallbacks() {
        let candidates = build_rendered_highlight_candidates(
            "This is a long changed sentence with many words in it",
            true,
        );
        let texts = candidates
            .into_iter()
            .map(|candidate| candidate.text)
            .collect::<Vec<_>>();

        assert!(
            !texts.contains(&"This is a long changed sentence with many words in it".to_string())
        );
        assert!(texts.contains(&"long".to_string()));
        assert!(texts.contains(&"changed".to_string()));
        assert!(texts.contains(&"sentence".to_string()));
    }

    #[test]
    fn collect_rendered_highlight_phrases_uses_inline_target_spans() {
        let inline_highlights = vec![
            InlineSpan {
                kind: DiffSegmentKind::Insert,
                text: "major city landmark".to_string(),
            },
            InlineSpan {
                kind: DiffSegmentKind::Delete,
                text: "ignored removal".to_string(),
            },
        ];
        let sources = vec![RenderedHighlightSource {
            kind: DiffSegmentKind::Insert,
            text: "entire changed sentence should not be used",
            inline_highlights: &inline_highlights,
        }];

        let phrases = collect_rendered_highlight_phrases(sources, DiffSegmentKind::Insert);
        let texts = phrases
            .into_iter()
            .map(|phrase| phrase.text)
            .collect::<Vec<_>>();

        assert!(texts.contains(&"major city landmark".to_string()));
        assert!(texts.contains(&"city landmark".to_string()));
        assert!(!texts.contains(&"ignored removal".to_string()));
        assert!(!texts.contains(&"entire changed sentence should not be used".to_string()));
    }
}
