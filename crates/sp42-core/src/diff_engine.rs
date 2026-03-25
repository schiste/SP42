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
    collect_segments(TextDiff::from_lines(before, after).iter_all_changes())
}

#[must_use]
pub fn diff_chars(before: &str, after: &str) -> StructuredDiff {
    collect_segments(TextDiff::from_chars(before, after).iter_all_changes())
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

        segments.push(DiffSegment { kind, text });
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
}
