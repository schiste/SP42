//! Structured diff generation lives here.

use serde::{Deserialize, Serialize};
use similar::{ChangeTag, TextDiff};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiffSegmentKind {
    Equal,
    Insert,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffSegment {
    pub kind: DiffSegmentKind,
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inline_highlights: Vec<InlineSpan>,
}

/// A word-level span within a diff segment, used to highlight the exact
/// changed words inside a large Insert or Delete block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InlineSpan {
    pub kind: DiffSegmentKind,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DiffStats {
    pub equal_segments: usize,
    pub insert_segments: usize,
    pub delete_segments: usize,
    pub equal_char_count: usize,
    pub inserted_char_count: usize,
    pub deleted_char_count: usize,
}

impl DiffStats {
    #[must_use]
    pub const fn has_changes(&self) -> bool {
        self.insert_segments > 0 || self.delete_segments > 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredDiff {
    pub segments: Vec<DiffSegment>,
    pub stats: DiffStats,
}

#[must_use]
pub fn diff_lines(before: &str, after: &str) -> StructuredDiff {
    let mut diff = collect_segments(TextDiff::from_lines(before, after).iter_all_changes());
    compute_inline_highlights(&mut diff.segments);
    diff
}

#[must_use]
pub fn diff_chars(before: &str, after: &str) -> StructuredDiff {
    collect_segments(TextDiff::from_chars(before, after).iter_all_changes())
}

/// For each adjacent Delete→Insert pair, compute word-level inline
/// highlights so the UI can show exactly which words changed instead of
/// painting the entire segment red/green.
fn compute_inline_highlights(segments: &mut [DiffSegment]) {
    let len = segments.len();
    let mut i = 0;
    while i + 1 < len {
        if segments[i].kind == DiffSegmentKind::Delete
            && segments[i + 1].kind == DiffSegmentKind::Insert
        {
            let word_diff =
                TextDiff::from_words(segments[i].text.as_str(), segments[i + 1].text.as_str());
            let mut del_spans = Vec::new();
            let mut ins_spans = Vec::new();
            for change in word_diff.iter_all_changes() {
                let span_text = change.to_string_lossy().to_string();
                match change.tag() {
                    ChangeTag::Equal => {
                        del_spans.push(InlineSpan {
                            kind: DiffSegmentKind::Equal,
                            text: span_text.clone(),
                        });
                        ins_spans.push(InlineSpan {
                            kind: DiffSegmentKind::Equal,
                            text: span_text,
                        });
                    }
                    ChangeTag::Delete => {
                        del_spans.push(InlineSpan {
                            kind: DiffSegmentKind::Delete,
                            text: span_text,
                        });
                    }
                    ChangeTag::Insert => {
                        ins_spans.push(InlineSpan {
                            kind: DiffSegmentKind::Insert,
                            text: span_text,
                        });
                    }
                }
            }
            segments[i].inline_highlights = del_spans;
            segments[i + 1].inline_highlights = ins_spans;
            i += 2;
        } else {
            i += 1;
        }
    }
}

fn collect_segments<'a>(
    changes: impl IntoIterator<Item = similar::Change<&'a str>>,
) -> StructuredDiff {
    let mut segments = Vec::new();
    let mut stats = DiffStats::default();

    for change in changes {
        let text = change.to_string();
        let character_count = text.chars().count();
        let kind = match change.tag() {
            ChangeTag::Equal => {
                stats.equal_segments += 1;
                stats.equal_char_count += character_count;
                DiffSegmentKind::Equal
            }
            ChangeTag::Insert => {
                stats.insert_segments += 1;
                stats.inserted_char_count += character_count;
                DiffSegmentKind::Insert
            }
            ChangeTag::Delete => {
                stats.delete_segments += 1;
                stats.deleted_char_count += character_count;
                DiffSegmentKind::Delete
            }
        };

        segments.push(DiffSegment {
            kind,
            text,
            inline_highlights: Vec::new(),
        });
    }

    StructuredDiff { segments, stats }
}

#[cfg(test)]
mod tests {
    use super::{DiffSegmentKind, diff_chars, diff_lines};

    #[test]
    fn line_diff_marks_insertions() {
        let diff = diff_lines("alpha\n", "alpha\nbeta\n");

        assert_eq!(diff.segments.len(), 2);
        assert_eq!(diff.segments[0].kind, DiffSegmentKind::Equal);
        assert_eq!(diff.segments[1].kind, DiffSegmentKind::Insert);
        assert!(diff.stats.has_changes());
        assert_eq!(diff.stats.insert_segments, 1);
        assert_eq!(diff.stats.equal_segments, 1);
    }

    #[test]
    fn char_diff_marks_deletions() {
        let diff = diff_chars("spam", "sam");

        assert!(
            diff.segments
                .iter()
                .any(|segment| segment.kind == DiffSegmentKind::Delete)
        );
        assert_eq!(diff.stats.delete_segments, 1);
        assert!(diff.stats.deleted_char_count >= 1);
    }

    #[test]
    fn inline_highlights_show_changed_words_in_adjacent_delete_insert() {
        let diff = diff_lines(
            "the quick brown fox\n",
            "the slow brown cat\n",
        );

        // Delete + Insert pair should both have inline highlights
        let delete_seg = diff
            .segments
            .iter()
            .find(|s| s.kind == DiffSegmentKind::Delete)
            .expect("should have a delete segment");
        let insert_seg = diff
            .segments
            .iter()
            .find(|s| s.kind == DiffSegmentKind::Insert)
            .expect("should have an insert segment");

        assert!(
            !delete_seg.inline_highlights.is_empty(),
            "delete segment should have inline highlights"
        );
        assert!(
            !insert_seg.inline_highlights.is_empty(),
            "insert segment should have inline highlights"
        );

        // The changed words should be marked, unchanged words should be Equal
        assert!(delete_seg
            .inline_highlights
            .iter()
            .any(|span| span.kind == DiffSegmentKind::Equal));
        assert!(delete_seg
            .inline_highlights
            .iter()
            .any(|span| span.kind == DiffSegmentKind::Delete));
    }
}
