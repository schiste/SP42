//! Structured diff generation lives here.

use serde::{Deserialize, Serialize};
use similar::{ChangeTag, TextDiff};

use crate::types::ScoringSignalParameters;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DiffMode {
    #[default]
    Lines,
    Chars,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffLineSpan {
    pub start_line: usize,
    pub line_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiffSegmentKind {
    Equal,
    Insert,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffSegment {
    pub kind: DiffSegmentKind,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<DiffLineSpan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<DiffLineSpan>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inline_highlights: Vec<InlineSpan>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiffHunkKind {
    Modification,
    Addition,
    Removal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiffMoveRole {
    Source,
    Target,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiffMarker {
    References,
    Category,
    Interwiki,
    Template,
    Media,
    Heading,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DiffSectionContext {
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffHunk {
    pub kind: DiffHunkKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<DiffLineSpan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<DiffLineSpan>,
    #[serde(default)]
    pub section: DiffSectionContext,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub markers: Vec<DiffMarker>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub move_group: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub move_role: Option<DiffMoveRole>,
    #[serde(default)]
    pub segments: Vec<DiffSegment>,
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
    #[serde(default)]
    pub mode: DiffMode,
    pub segments: Vec<DiffSegment>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hunks: Vec<DiffHunk>,
    pub stats: DiffStats,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RenderedHunkSide {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub section_label: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub html: String,
    #[serde(default)]
    pub missing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RenderedHunkPreview {
    pub hunk_index: usize,
    #[serde(default)]
    pub before: RenderedHunkSide,
    #[serde(default)]
    pub after: RenderedHunkSide,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DiffAdditiveOnlyHints {
    pub link_addition_only: bool,
    pub reference_addition_only: bool,
    pub category_addition_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DiffRiskHints {
    pub interwiki_addition_only: bool,
    pub mass_blanking_detected: bool,
    pub inserted_profanity_detected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DiffNoiseHints {
    pub repeated_character_noise_detected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DiffScoringHints {
    pub additive_only: DiffAdditiveOnlyHints,
    pub risk: DiffRiskHints,
    pub noise: DiffNoiseHints,
}

impl DiffScoringHints {
    #[must_use]
    pub const fn link_addition_only(&self) -> bool {
        self.additive_only.link_addition_only
    }

    #[must_use]
    pub const fn reference_addition_only(&self) -> bool {
        self.additive_only.reference_addition_only
    }

    #[must_use]
    pub const fn category_addition_only(&self) -> bool {
        self.additive_only.category_addition_only
    }

    #[must_use]
    pub const fn interwiki_addition_only(&self) -> bool {
        self.risk.interwiki_addition_only
    }

    #[must_use]
    pub const fn mass_blanking_detected(&self) -> bool {
        self.risk.mass_blanking_detected
    }

    #[must_use]
    pub const fn inserted_profanity_detected(&self) -> bool {
        self.risk.inserted_profanity_detected
    }

    #[must_use]
    pub const fn repeated_character_noise_detected(&self) -> bool {
        self.noise.repeated_character_noise_detected
    }
}

#[must_use]
pub fn detect_link_addition_only(diff: &StructuredDiff) -> Option<String> {
    if diff.stats.insert_segments == 0 {
        return None;
    }

    let before = before_text(diff);
    let after = after_text(diff);
    let inserted_wrapper_chars = after
        .chars()
        .filter(|ch| matches!(ch, '[' | ']'))
        .count()
        .saturating_sub(before.chars().filter(|ch| matches!(ch, '[' | ']')).count());
    let stripped_after = after.replace("[[", "").replace("]]", "");

    (inserted_wrapper_chars >= 4 && stripped_after == before).then(|| {
        format!(
            "inserted only wikilink wrapper characters ({inserted_wrapper_chars} bracket chars)"
        )
    })
}

#[must_use]
pub fn analyze_diff_for_scoring(
    diff: &StructuredDiff,
    parameters: &ScoringSignalParameters,
) -> DiffScoringHints {
    let before = before_text(diff);
    let after = after_text(diff);

    DiffScoringHints {
        additive_only: DiffAdditiveOnlyHints {
            link_addition_only: detect_link_addition_only(diff).is_some(),
            reference_addition_only: detect_reference_addition_only(&before, &after),
            category_addition_only: detect_category_addition_only(&before, &after),
        },
        risk: DiffRiskHints {
            interwiki_addition_only: detect_interwiki_addition_only(&before, &after),
            mass_blanking_detected: detect_mass_blanking(diff, parameters),
            inserted_profanity_detected: contains_any_marker(&after, &parameters.profanity_markers)
                && !contains_any_marker(&before, &parameters.profanity_markers),
        },
        noise: DiffNoiseHints {
            repeated_character_noise_detected: has_repeated_character_run(
                &after,
                parameters.repeated_character_run_threshold,
            ) && !has_repeated_character_run(
                &before,
                parameters.repeated_character_run_threshold,
            ),
        },
    }
}

#[must_use]
pub fn diff_lines(before: &str, after: &str) -> StructuredDiff {
    let mut diff = collect_segments(
        DiffMode::Lines,
        TextDiff::from_lines(before, after).iter_all_changes(),
    );
    compute_inline_highlights(&mut diff.segments);
    diff.hunks = build_diff_hunks(&diff.segments, before, after, 3);
    diff
}

#[must_use]
pub fn diff_chars(before: &str, after: &str) -> StructuredDiff {
    collect_segments(
        DiffMode::Chars,
        TextDiff::from_chars(before, after).iter_all_changes(),
    )
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
    mode: DiffMode,
    changes: impl IntoIterator<Item = similar::Change<&'a str>>,
) -> StructuredDiff {
    let mut segments = Vec::new();
    let mut stats = DiffStats::default();
    let mut before_line = 1usize;
    let mut after_line = 1usize;

    for change in changes {
        let text = change.to_string_lossy().to_string();
        let character_count = text.chars().count();
        let line_count = match mode {
            DiffMode::Lines => count_text_lines(&text),
            DiffMode::Chars => 0,
        };
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
        let (before, after) = match mode {
            DiffMode::Lines => match kind {
                DiffSegmentKind::Equal => {
                    let before_span = line_span(before_line, line_count);
                    let after_span = line_span(after_line, line_count);
                    before_line += line_count;
                    after_line += line_count;
                    (before_span, after_span)
                }
                DiffSegmentKind::Delete => {
                    let before_span = line_span(before_line, line_count);
                    before_line += line_count;
                    (before_span, None)
                }
                DiffSegmentKind::Insert => {
                    let after_span = line_span(after_line, line_count);
                    after_line += line_count;
                    (None, after_span)
                }
            },
            DiffMode::Chars => (None, None),
        };

        segments.push(DiffSegment {
            kind,
            text,
            before,
            after,
            inline_highlights: Vec::new(),
        });
    }

    StructuredDiff {
        mode,
        segments,
        hunks: Vec::new(),
        stats,
    }
}

fn build_diff_hunks(
    segments: &[DiffSegment],
    before_text: &str,
    after_text: &str,
    context_lines: usize,
) -> Vec<DiffHunk> {
    let changed_ranges = collect_changed_ranges(segments, context_lines);
    if changed_ranges.is_empty() {
        return Vec::new();
    }

    let before_sections = build_section_index(before_text);
    let after_sections = build_section_index(after_text);

    let mut hunks = changed_ranges
        .into_iter()
        .map(|(start, end)| {
            let hunk_segments = segments[start..end].to_vec();
            let changed_before = merge_line_spans(
                hunk_segments
                    .iter()
                    .filter(|segment| segment.kind != DiffSegmentKind::Insert)
                    .filter(|segment| segment.kind != DiffSegmentKind::Equal)
                    .filter_map(|segment| segment.before.as_ref()),
            );
            let changed_after = merge_line_spans(
                hunk_segments
                    .iter()
                    .filter(|segment| segment.kind != DiffSegmentKind::Delete)
                    .filter(|segment| segment.kind != DiffSegmentKind::Equal)
                    .filter_map(|segment| segment.after.as_ref()),
            );
            let before = changed_before.clone().or_else(|| {
                merge_line_spans(
                    hunk_segments
                        .iter()
                        .filter_map(|segment| segment.before.as_ref()),
                )
            });
            let after = changed_after.clone().or_else(|| {
                merge_line_spans(
                    hunk_segments
                        .iter()
                        .filter_map(|segment| segment.after.as_ref()),
                )
            });
            let kind = classify_hunk_kind(&hunk_segments);
            let markers = collect_hunk_markers(&hunk_segments);
            let notes = build_hunk_notes(kind, &markers, before.as_ref(), after.as_ref());

            DiffHunk {
                kind,
                before: before.clone(),
                after: after.clone(),
                section: DiffSectionContext {
                    before: resolve_section_label(
                        changed_before.as_ref().or(before.as_ref()),
                        &before_sections,
                    ),
                    after: resolve_section_label(
                        changed_after.as_ref().or(after.as_ref()),
                        &after_sections,
                    ),
                },
                markers,
                notes,
                move_group: None,
                move_role: None,
                segments: hunk_segments,
            }
        })
        .collect::<Vec<_>>();

    annotate_moves(&mut hunks);
    hunks
}

fn collect_changed_ranges(segments: &[DiffSegment], context_lines: usize) -> Vec<(usize, usize)> {
    let len = segments.len();
    if len == 0 {
        return Vec::new();
    }

    let mut visible = vec![false; len];
    for (index, segment) in segments.iter().enumerate() {
        if segment.kind == DiffSegmentKind::Equal {
            continue;
        }
        let start = index.saturating_sub(context_lines);
        let end = (index + context_lines + 1).min(len);
        for slot in &mut visible[start..end] {
            *slot = true;
        }
    }

    let mut ranges = Vec::new();
    let mut cursor = 0usize;
    while cursor < len {
        if !visible[cursor] {
            cursor += 1;
            continue;
        }
        let start = cursor;
        while cursor < len && visible[cursor] {
            cursor += 1;
        }
        ranges.push((start, cursor));
    }
    ranges
}

fn merge_line_spans<'a>(spans: impl Iterator<Item = &'a DiffLineSpan>) -> Option<DiffLineSpan> {
    let mut iter = spans.peekable();
    let first = *iter.peek()?;
    let mut start_line = first.start_line;
    let mut end_line = first.start_line + first.line_count.saturating_sub(1);

    for span in iter {
        start_line = start_line.min(span.start_line);
        end_line = end_line.max(span.start_line + span.line_count.saturating_sub(1));
    }

    Some(DiffLineSpan {
        start_line,
        line_count: end_line.saturating_sub(start_line) + 1,
    })
}

fn classify_hunk_kind(segments: &[DiffSegment]) -> DiffHunkKind {
    let has_insert = segments
        .iter()
        .any(|segment| segment.kind == DiffSegmentKind::Insert);
    let has_delete = segments
        .iter()
        .any(|segment| segment.kind == DiffSegmentKind::Delete);

    match (has_insert, has_delete) {
        (true, false) => DiffHunkKind::Addition,
        (false, true) => DiffHunkKind::Removal,
        (true, true) | (false, false) => DiffHunkKind::Modification,
    }
}

fn build_section_index(text: &str) -> Vec<(usize, String)> {
    text.lines()
        .enumerate()
        .filter_map(|(index, line)| parse_section_heading(line).map(|heading| (index + 1, heading)))
        .collect()
}

fn parse_section_heading(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.len() < 4 {
        return None;
    }
    let prefix = trimmed.chars().take_while(|ch| *ch == '=').count();
    let suffix = trimmed.chars().rev().take_while(|ch| *ch == '=').count();
    if prefix < 2 || prefix != suffix {
        return None;
    }
    let heading = trimmed[prefix..trimmed.len() - suffix].trim();
    (!heading.is_empty()).then(|| heading.to_string())
}

fn resolve_section_label(
    span: Option<&DiffLineSpan>,
    sections: &[(usize, String)],
) -> Option<String> {
    let line = span?.start_line;
    sections
        .iter()
        .take_while(|(section_line, _)| *section_line <= line)
        .last()
        .map(|(_, label)| label.clone())
        .or_else(|| Some("Lead".to_string()))
}

fn collect_hunk_markers(segments: &[DiffSegment]) -> Vec<DiffMarker> {
    let inserted = segments
        .iter()
        .filter(|segment| segment.kind == DiffSegmentKind::Insert)
        .map(|segment| segment.text.as_str())
        .collect::<String>();
    let deleted = segments
        .iter()
        .filter(|segment| segment.kind == DiffSegmentKind::Delete)
        .map(|segment| segment.text.as_str())
        .collect::<String>();
    let combined = format!("{inserted}\n{deleted}");
    let lowered = combined.to_ascii_lowercase();
    let mut markers = Vec::new();

    if lowered.contains("<ref") || lowered.contains("{{cite") || lowered.contains("{{citation") {
        markers.push(DiffMarker::References);
    }
    if lowered.contains("[[category:") || lowered.contains("[[catégorie:") {
        markers.push(DiffMarker::Category);
    }
    if lowered.contains("[[file:")
        || lowered.contains("[[image:")
        || lowered.contains("[[fichier:")
        || lowered.contains("<gallery")
    {
        markers.push(DiffMarker::Media);
    }
    if lowered.contains("{{") {
        markers.push(DiffMarker::Template);
    }
    if combined
        .lines()
        .any(|line| parse_section_heading(line).is_some())
    {
        markers.push(DiffMarker::Heading);
    }
    if contains_interwiki_markup(&lowered) {
        markers.push(DiffMarker::Interwiki);
    }

    markers
}

fn contains_interwiki_markup(lowered: &str) -> bool {
    lowered
        .split("[[")
        .skip(1)
        .filter_map(|part| part.split("]]").next())
        .any(|candidate| is_interwiki_link(&format!("[[{candidate}]]")))
}

fn build_hunk_notes(
    kind: DiffHunkKind,
    markers: &[DiffMarker],
    before: Option<&DiffLineSpan>,
    after: Option<&DiffLineSpan>,
) -> Vec<String> {
    let mut notes = Vec::new();
    match kind {
        DiffHunkKind::Modification => notes.push("content changed in place".to_string()),
        DiffHunkKind::Addition => {
            if let Some(after) = after {
                notes.push(format!("new content around line {}", after.start_line));
            }
        }
        DiffHunkKind::Removal => {
            if let Some(before) = before {
                notes.push(format!("content removed around line {}", before.start_line));
            }
        }
    }

    if !markers.is_empty() {
        let marker_summary = markers
            .iter()
            .map(|marker| diff_marker_label(*marker))
            .collect::<Vec<_>>()
            .join(", ");
        notes.push(format!("semantic cues: {marker_summary}"));
    }
    notes
}

fn annotate_moves(hunks: &mut [DiffHunk]) {
    let mut next_group = 1usize;
    let mut consumed = vec![false; hunks.len()];

    for hunk in hunks.iter_mut() {
        if hunk.kind != DiffHunkKind::Modification || hunk.move_group.is_some() {
            continue;
        }
        let removed = normalized_hunk_side_text(hunk, DiffSegmentKind::Delete);
        let inserted = normalized_hunk_side_text(hunk, DiffSegmentKind::Insert);
        let sections_differ = hunk.section.before != hunk.section.after;
        if let (Some(removed), Some(inserted)) = (removed, inserted)
            && sections_differ
            && is_probable_move_match(&removed, &inserted)
        {
            hunk.move_group = Some(next_group);
            hunk.notes
                .push("probable moved block with in-place edits".to_string());
            next_group += 1;
        }
    }

    for source_index in 0..hunks.len() {
        if consumed[source_index] || hunks[source_index].kind != DiffHunkKind::Removal {
            continue;
        }
        let Some(source_signature) = normalized_change_text(&hunks[source_index]) else {
            continue;
        };
        for target_index in 0..hunks.len() {
            if source_index == target_index
                || consumed[target_index]
                || hunks[target_index].kind != DiffHunkKind::Addition
            {
                continue;
            }
            let Some(target_signature) = normalized_change_text(&hunks[target_index]) else {
                continue;
            };
            if !is_probable_move_match(&source_signature, &target_signature) {
                continue;
            }

            hunks[source_index].move_group = Some(next_group);
            hunks[source_index].move_role = Some(DiffMoveRole::Source);
            hunks[source_index]
                .notes
                .push("probable moved block".to_string());
            hunks[target_index].move_group = Some(next_group);
            hunks[target_index].move_role = Some(DiffMoveRole::Target);
            hunks[target_index]
                .notes
                .push("probable moved block".to_string());
            consumed[source_index] = true;
            consumed[target_index] = true;
            next_group += 1;
            break;
        }
    }
}

fn normalized_change_text(hunk: &DiffHunk) -> Option<String> {
    let relevant_kind = match hunk.kind {
        DiffHunkKind::Addition => DiffSegmentKind::Insert,
        DiffHunkKind::Removal => DiffSegmentKind::Delete,
        DiffHunkKind::Modification => return None,
    };

    let normalized_lines = hunk
        .segments
        .iter()
        .filter(|segment| segment.kind == relevant_kind)
        .flat_map(|segment| segment.text.lines())
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();

    let line_count = normalized_lines.len();
    let joined = normalized_lines.join("\n");
    if line_count < 2 && joined.chars().count() < 80 {
        return None;
    }
    Some(joined)
}

fn normalized_hunk_side_text(hunk: &DiffHunk, relevant_kind: DiffSegmentKind) -> Option<String> {
    let normalized_lines = hunk
        .segments
        .iter()
        .filter(|segment| segment.kind == relevant_kind)
        .flat_map(|segment| segment.text.lines())
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();

    let joined = normalized_lines.join("\n");
    (!joined.is_empty()).then_some(joined)
}

fn is_probable_move_match(left: &str, right: &str) -> bool {
    let left_words = normalized_word_bag(left);
    let right_words = normalized_word_bag(right);
    if left_words.is_empty() || right_words.is_empty() {
        return false;
    }

    let shared = left_words
        .iter()
        .filter(|word| right_words.contains(*word))
        .count();
    let total = left_words.len() + right_words.len() - shared;
    if total == 0 {
        return true;
    }

    shared.saturating_mul(10) >= total.saturating_mul(6)
}

fn normalized_word_bag(text: &str) -> Vec<String> {
    text.split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

fn diff_marker_label(marker: DiffMarker) -> &'static str {
    match marker {
        DiffMarker::References => "references",
        DiffMarker::Category => "categories",
        DiffMarker::Interwiki => "interwiki",
        DiffMarker::Template => "templates",
        DiffMarker::Media => "media",
        DiffMarker::Heading => "section headings",
    }
}

fn line_span(start_line: usize, line_count: usize) -> Option<DiffLineSpan> {
    (line_count > 0).then_some(DiffLineSpan {
        start_line,
        line_count,
    })
}

fn count_text_lines(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }

    let newline_count = text.chars().filter(|ch| *ch == '\n').count();
    if text.ends_with('\n') {
        newline_count
    } else {
        newline_count + 1
    }
}

fn before_text(diff: &StructuredDiff) -> String {
    let text = diff
        .segments
        .iter()
        .filter(|segment| segment.kind != DiffSegmentKind::Insert)
        .map(|segment| segment.text.as_str())
        .collect::<String>();
    normalize_reconstructed_text(diff.mode, text)
}

fn after_text(diff: &StructuredDiff) -> String {
    let text = diff
        .segments
        .iter()
        .filter(|segment| segment.kind != DiffSegmentKind::Delete)
        .map(|segment| segment.text.as_str())
        .collect::<String>();
    normalize_reconstructed_text(diff.mode, text)
}

fn normalize_reconstructed_text(mode: DiffMode, text: String) -> String {
    match mode {
        DiffMode::Lines => text,
        DiffMode::Chars => text.replace('\n', ""),
    }
}

fn detect_reference_addition_only(before_text: &str, after_text: &str) -> bool {
    if after_text.trim().is_empty() || before_text == after_text {
        return false;
    }

    remove_reference_constructs(after_text) == before_text
}

fn detect_category_addition_only(before_text: &str, after_text: &str) -> bool {
    if after_text.trim().is_empty() || before_text == after_text {
        return false;
    }
    remove_link_family(after_text, is_category_link) == before_text
}

fn detect_interwiki_addition_only(before_text: &str, after_text: &str) -> bool {
    if after_text.trim().is_empty() || before_text == after_text {
        return false;
    }
    remove_link_family(after_text, is_interwiki_link) == before_text
}

fn is_category_link(candidate: &str) -> bool {
    let lowered = candidate.trim().to_ascii_lowercase();
    lowered.starts_with("[[category:") || lowered.starts_with("[[catégorie:")
}

fn is_interwiki_link(candidate: &str) -> bool {
    let lowered = candidate.trim().to_ascii_lowercase();
    if !lowered.starts_with("[[") || !lowered.ends_with("]]") {
        return false;
    }
    let inner = &lowered[2..lowered.len() - 2];
    let Some((prefix, _target)) = inner.split_once(':') else {
        return false;
    };
    if prefix.is_empty() || prefix.contains(' ') {
        return false;
    }

    let excluded = [
        "category",
        "catégorie",
        "file",
        "fichier",
        "image",
        "template",
        "modèle",
        "user",
        "utilisateur",
        "portal",
        "help",
        "aide",
        "draft",
        "media",
        "special",
        "module",
        "project",
        "wikipedia",
    ];
    if excluded.contains(&prefix) {
        return false;
    }

    prefix
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch == '-')
}

fn detect_mass_blanking(diff: &StructuredDiff, parameters: &ScoringSignalParameters) -> bool {
    let deleted = i32::try_from(diff.stats.deleted_char_count).unwrap_or(i32::MAX);
    let inserted = i32::try_from(diff.stats.inserted_char_count).unwrap_or(i32::MAX);
    deleted >= parameters.massive_blanking_threshold.abs()
        && inserted <= 64
        && deleted >= inserted.saturating_mul(4)
}

fn contains_any_marker(text: &str, markers: &[String]) -> bool {
    if text.trim().is_empty() {
        return false;
    }
    let lowered = text.to_ascii_lowercase();
    markers.iter().any(|marker| lowered.contains(marker))
}

fn has_repeated_character_run(value: &str, threshold: u8) -> bool {
    max_repeated_character_run(value) >= threshold
}

fn max_repeated_character_run(value: &str) -> u8 {
    let mut last = '\0';
    let mut run = 0u8;
    let mut max_run = 0u8;
    for ch in value.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() && ch == last {
            run = run.saturating_add(1);
        } else {
            last = ch;
            run = 1;
        }
        max_run = max_run.max(run);
    }
    max_run
}

fn consume_ref_tag(value: &str) -> Option<&str> {
    let lowered = value.to_ascii_lowercase();
    if !lowered.starts_with("<ref") {
        return None;
    }
    let end = lowered.find("</ref>")?;
    Some(&value[end + "</ref>".len()..])
}

fn consume_reference_template(value: &str) -> Option<&str> {
    let lowered = value.to_ascii_lowercase();
    if !(lowered.starts_with("{{cite")
        || lowered.starts_with("{{citation")
        || lowered.starts_with("{{sfn"))
    {
        return None;
    }

    let mut depth = 0usize;
    let mut iter = value.char_indices().peekable();
    while let Some((idx, ch)) = iter.next() {
        if ch == '{' && iter.peek().is_some_and(|(_, next)| *next == '{') {
            depth += 1;
            let _ = iter.next();
            continue;
        }
        if ch == '}' && iter.peek().is_some_and(|(_, next)| *next == '}') {
            depth = depth.saturating_sub(1);
            let _ = iter.next();
            if depth == 0 {
                return Some(&value[idx + 2..]);
            }
        }
    }
    None
}

fn remove_reference_constructs(value: &str) -> String {
    let mut remaining = value;
    let mut output = String::new();

    while !remaining.is_empty() {
        if let Some(next) = consume_ref_tag(remaining) {
            remaining = next;
            continue;
        }
        if let Some(next) = consume_reference_template(remaining) {
            remaining = next;
            continue;
        }

        let Some(ch) = remaining.chars().next() else {
            break;
        };
        output.push(ch);
        remaining = &remaining[ch.len_utf8()..];
    }

    output
}

fn remove_link_family(value: &str, predicate: fn(&str) -> bool) -> String {
    let mut remaining = value;
    let mut output = String::new();

    while !remaining.is_empty() {
        if let Some(next) = consume_link_family(remaining, predicate) {
            remaining = next;
            continue;
        }

        let Some(ch) = remaining.chars().next() else {
            break;
        };
        output.push(ch);
        remaining = &remaining[ch.len_utf8()..];
    }

    output
}

fn consume_link_family(value: &str, predicate: fn(&str) -> bool) -> Option<&str> {
    if !value.starts_with("[[") {
        return None;
    }
    let end = value.find("]]")?;
    let candidate = &value[..end + 2];
    predicate(candidate).then_some(&value[end + 2..])
}

#[cfg(test)]
mod tests {
    use super::{
        DiffHunkKind, DiffMarker, DiffMode, DiffSegmentKind, analyze_diff_for_scoring,
        detect_link_addition_only, diff_chars, diff_lines,
    };
    use crate::types::ScoringSignalParameters;

    #[test]
    fn line_diff_marks_insertions() {
        let diff = diff_lines("alpha\n", "alpha\nbeta\n");

        assert_eq!(diff.mode, DiffMode::Lines);
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

        assert_eq!(diff.mode, DiffMode::Chars);
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
        let diff = diff_lines("the quick brown fox\n", "the slow brown cat\n");

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
        assert!(
            delete_seg
                .inline_highlights
                .iter()
                .any(|span| span.kind == DiffSegmentKind::Equal)
        );
        assert!(
            delete_seg
                .inline_highlights
                .iter()
                .any(|span| span.kind == DiffSegmentKind::Delete)
        );
    }

    #[test]
    fn line_diff_tracks_before_and_after_line_spans() {
        let diff = diff_lines("alpha\nbeta\n", "alpha\ngamma\n");

        assert_eq!(
            diff.segments[0].before.as_ref().map(|span| span.start_line),
            Some(1)
        );
        assert_eq!(
            diff.segments[0].after.as_ref().map(|span| span.start_line),
            Some(1)
        );

        let delete_seg = diff
            .segments
            .iter()
            .find(|segment| segment.kind == DiffSegmentKind::Delete)
            .expect("delete segment should exist");
        assert_eq!(
            delete_seg.before.as_ref().map(|span| span.start_line),
            Some(2)
        );
        assert_eq!(
            delete_seg.before.as_ref().map(|span| span.line_count),
            Some(1)
        );
        assert!(delete_seg.after.is_none());

        let insert_seg = diff
            .segments
            .iter()
            .find(|segment| segment.kind == DiffSegmentKind::Insert)
            .expect("insert segment should exist");
        assert!(insert_seg.before.is_none());
        assert_eq!(
            insert_seg.after.as_ref().map(|span| span.start_line),
            Some(2)
        );
        assert_eq!(
            insert_seg.after.as_ref().map(|span| span.line_count),
            Some(1)
        );
    }

    #[test]
    fn char_diff_does_not_claim_line_ranges() {
        let diff = diff_chars("abc", "adc");

        assert!(diff.segments.iter().all(|segment| segment.before.is_none()));
        assert!(diff.segments.iter().all(|segment| segment.after.is_none()));
        assert!(diff.hunks.is_empty());
    }

    #[test]
    fn line_diff_builds_section_aware_hunks() {
        let before = "Lead intro\n== History ==\nOld line\n== References ==\n<ref>One</ref>\n";
        let after = "Lead intro\n== History ==\nNew line\n== References ==\n<ref>One</ref>\n";
        let diff = diff_lines(before, after);

        assert_eq!(diff.hunks.len(), 1);
        let hunk = &diff.hunks[0];
        assert_eq!(hunk.section.before.as_deref(), Some("History"));
        assert_eq!(hunk.section.after.as_deref(), Some("History"));
        assert_eq!(hunk.kind, DiffHunkKind::Modification);
    }

    #[test]
    fn line_diff_marks_reference_hunks() {
        let diff = diff_lines("Lead\n", "Lead\n<ref>Source</ref>\n");

        assert_eq!(diff.hunks.len(), 1);
        assert!(diff.hunks[0].markers.contains(&DiffMarker::References));
    }

    #[test]
    fn line_diff_detects_probable_moved_blocks() {
        let before = "Lead\nIntro\nParagraph two moved.\nSpacer A\nSpacer B\nSpacer C\nSpacer D\n== Later ==\nTail\n";
        let after = "Lead\nIntro\nSpacer A\nSpacer B\nSpacer C\nSpacer D\n== Later ==\nParagraph two moved and expanded.\nTail\n";
        let diff = diff_lines(before, after);

        let move_hunks = diff
            .hunks
            .iter()
            .filter(|hunk| hunk.move_group.is_some())
            .count();

        assert_eq!(move_hunks, 1);
        assert!(
            diff.hunks
                .iter()
                .any(|hunk| hunk.notes.iter().any(|note| note.contains("moved block")))
        );
    }

    #[test]
    fn detects_pure_link_wrapper_addition() {
        let diff = diff_chars("Paris", "[[Paris]]");

        assert!(detect_link_addition_only(&diff).is_some());
    }

    #[test]
    fn rejects_link_addition_when_other_text_is_inserted() {
        let diff = diff_chars("Paris", "[[Paris]] touristique");

        assert!(detect_link_addition_only(&diff).is_none());
    }

    #[test]
    fn rejects_link_addition_when_content_is_deleted() {
        let diff = diff_chars("Paris ville", "[[Paris]]");

        assert!(detect_link_addition_only(&diff).is_none());
    }

    #[test]
    fn scoring_hints_detect_reference_addition() {
        let diff = diff_chars("Paris", "Paris<ref>source</ref>");
        let hints = analyze_diff_for_scoring(&diff, &ScoringSignalParameters::default());

        assert!(hints.reference_addition_only());
    }

    #[test]
    fn scoring_hints_detect_category_addition() {
        let diff = diff_chars("Paris", "Paris\n[[Catégorie:Capitales]]");
        let hints = analyze_diff_for_scoring(&diff, &ScoringSignalParameters::default());

        assert!(hints.category_addition_only());
    }

    #[test]
    fn scoring_hints_detect_interwiki_addition() {
        let diff = diff_chars("Paris", "Paris\n[[en:Paris]]");
        let hints = analyze_diff_for_scoring(&diff, &ScoringSignalParameters::default());

        assert!(hints.interwiki_addition_only());
    }

    #[test]
    fn scoring_hints_detect_mass_blanking() {
        let before = format!("{}\n", "A".repeat(1_200));
        let diff = diff_chars(&before, "A");
        let hints = analyze_diff_for_scoring(&diff, &ScoringSignalParameters::default());

        assert!(hints.mass_blanking_detected());
    }

    #[test]
    fn scoring_hints_detect_inserted_profanity() {
        let diff = diff_chars("Paris", "Paris putain");
        let hints = analyze_diff_for_scoring(&diff, &ScoringSignalParameters::default());

        assert!(hints.inserted_profanity_detected());
    }

    #[test]
    fn scoring_hints_detect_repeated_character_noise() {
        let diff = diff_chars("Paris", "Paris aaaaaaa");
        let hints = analyze_diff_for_scoring(&diff, &ScoringSignalParameters::default());

        assert!(hints.repeated_character_noise_detected());
    }
}
