//! Structured media diff extraction from raw wiki revision text.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaDiffKind {
    Added,
    Removed,
    Changed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaReference {
    pub file_name: String,
    pub display_title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub usage_signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaDiffEntry {
    pub kind: MediaDiffKind,
    pub file_name: String,
    pub display_title: String,
    pub before_occurrences: usize,
    pub after_occurrences: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub before_signatures: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub after_signatures: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_url: Option<Url>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_url: Option<Url>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MediaDiffReport {
    #[serde(default)]
    pub entries: Vec<MediaDiffEntry>,
    #[serde(default)]
    pub notes: Vec<String>,
}

impl MediaDiffReport {
    #[must_use]
    pub fn has_changes(&self) -> bool {
        !self.entries.is_empty()
    }
}

#[derive(Debug, Clone, Default)]
struct MediaAggregate {
    display_title: String,
    occurrences: usize,
    signatures: BTreeSet<String>,
}

#[must_use]
pub fn extract_media_references(wikitext: &str) -> Vec<MediaReference> {
    let mut references = extract_wikilink_media_references(wikitext);
    references.extend(extract_gallery_media_references(wikitext));
    references.extend(extract_template_media_references(wikitext));
    references
}

#[must_use]
pub fn build_media_diff(before: &str, after: &str) -> MediaDiffReport {
    let before_map = aggregate_media_references(extract_media_references(before));
    let after_map = aggregate_media_references(extract_media_references(after));
    let file_names = before_map
        .keys()
        .chain(after_map.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut entries = Vec::new();

    for file_name in file_names.iter().cloned() {
        let before_aggregate = before_map.get(&file_name);
        let after_aggregate = after_map.get(&file_name);
        let kind = match (before_aggregate, after_aggregate) {
            (None, Some(_)) => Some(MediaDiffKind::Added),
            (Some(_), None) => Some(MediaDiffKind::Removed),
            (Some(before), Some(after))
                if before.occurrences != after.occurrences
                    || before.signatures != after.signatures =>
            {
                Some(MediaDiffKind::Changed)
            }
            _ => None,
        };
        let Some(kind) = kind else {
            continue;
        };

        let display_title = after_aggregate
            .map(|entry| entry.display_title.clone())
            .or_else(|| before_aggregate.map(|entry| entry.display_title.clone()))
            .unwrap_or_else(|| file_name.clone());

        entries.push(MediaDiffEntry {
            kind,
            file_name,
            display_title,
            before_occurrences: before_aggregate.map_or(0, |entry| entry.occurrences),
            after_occurrences: after_aggregate.map_or(0, |entry| entry.occurrences),
            before_signatures: before_aggregate
                .map_or_else(Vec::new, |entry| entry.signatures.iter().cloned().collect()),
            after_signatures: after_aggregate
                .map_or_else(Vec::new, |entry| entry.signatures.iter().cloned().collect()),
            preview_url: None,
            page_url: None,
        });
    }

    entries.sort_by(|left, right| {
        media_diff_kind_rank(left.kind)
            .cmp(&media_diff_kind_rank(right.kind))
            .then_with(|| left.display_title.cmp(&right.display_title))
    });

    let mut notes = Vec::new();
    if entries.is_empty() {
        notes.push("No media references changed in the selected revision.".to_string());
    }

    MediaDiffReport { entries, notes }
}

fn media_diff_kind_rank(kind: MediaDiffKind) -> u8 {
    match kind {
        MediaDiffKind::Added => 0,
        MediaDiffKind::Removed => 1,
        MediaDiffKind::Changed => 2,
    }
}

fn aggregate_media_references(references: Vec<MediaReference>) -> BTreeMap<String, MediaAggregate> {
    let mut aggregated = BTreeMap::new();
    for reference in references {
        let entry = aggregated
            .entry(reference.file_name.clone())
            .or_insert_with(|| MediaAggregate {
                display_title: reference.display_title.clone(),
                occurrences: 0,
                signatures: BTreeSet::new(),
            });
        entry.occurrences += 1;
        if !reference.usage_signature.is_empty() {
            entry.signatures.insert(reference.usage_signature);
        }
    }
    aggregated
}

fn extract_wikilink_media_references(wikitext: &str) -> Vec<MediaReference> {
    let mut references = Vec::new();
    let mut cursor = 0usize;

    while let Some(start_rel) = wikitext[cursor..].find("[[") {
        let start = cursor + start_rel + 2;
        let Some(end_rel) = wikitext[start..].find("]]") else {
            break;
        };
        let end = start + end_rel;
        if let Some(reference) = parse_wikilink_media_reference(&wikitext[start..end]) {
            references.push(reference);
        }
        cursor = end + 2;
    }

    references
}

fn parse_wikilink_media_reference(link_body: &str) -> Option<MediaReference> {
    let mut parts = link_body.split('|');
    let target = parts.next()?;
    let file_name = normalize_media_target(target)?;
    let usage_signature = normalize_usage_signature(parts.collect::<Vec<_>>());
    Some(MediaReference {
        display_title: display_title_from_file_name(&file_name),
        file_name,
        usage_signature,
    })
}

fn extract_gallery_media_references(wikitext: &str) -> Vec<MediaReference> {
    let mut references = Vec::new();
    let lowercase = wikitext.to_ascii_lowercase();
    let mut cursor = 0usize;

    while let Some(start_rel) = lowercase[cursor..].find("<gallery") {
        let start = cursor + start_rel;
        let Some(open_end_rel) = lowercase[start..].find('>') else {
            break;
        };
        let content_start = start + open_end_rel + 1;
        let Some(close_rel) = lowercase[content_start..].find("</gallery>") else {
            break;
        };
        let content_end = content_start + close_rel;
        let content = &wikitext[content_start..content_end];
        for line in content.lines() {
            if let Some(reference) = parse_gallery_media_reference(line) {
                references.push(reference);
            }
        }
        cursor = content_end + "</gallery>".len();
    }

    references
}

fn parse_gallery_media_reference(line: &str) -> Option<MediaReference> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    let mut parts = trimmed.split('|');
    let target = parts.next()?;
    let file_name = normalize_media_target(target)?;
    let usage_signature = normalize_usage_signature(parts.collect::<Vec<_>>());
    Some(MediaReference {
        display_title: display_title_from_file_name(&file_name),
        file_name,
        usage_signature,
    })
}

fn extract_template_media_references(wikitext: &str) -> Vec<MediaReference> {
    let mut references = Vec::new();

    for body in extract_template_bodies(wikitext) {
        let parts = split_top_level_segments(body, '|');
        let Some(template_name) = parts.first().map(|part| normalize_template_name(part)) else {
            continue;
        };
        if template_name.is_empty() {
            continue;
        }

        for parameter in parts.iter().skip(1) {
            let Some((raw_name, raw_value)) = split_top_level_once(parameter, '=') else {
                continue;
            };
            let parameter_name = normalize_template_parameter_name(raw_name);
            if !is_media_parameter_name(&parameter_name) {
                continue;
            }
            let Some(file_name) = normalize_template_media_value(raw_value) else {
                continue;
            };
            references.push(MediaReference {
                display_title: display_title_from_file_name(&file_name),
                file_name,
                usage_signature: format!("template:{template_name} | {parameter_name}"),
            });
        }
    }

    references
}

fn extract_template_bodies(wikitext: &str) -> Vec<&str> {
    let bytes = wikitext.as_bytes();
    let mut bodies = Vec::new();
    let mut index = 0usize;
    let mut depth = 0usize;
    let mut start = None;

    while index + 1 < bytes.len() {
        if bytes[index] == b'{' && bytes[index + 1] == b'{' {
            if depth == 0 {
                start = Some(index + 2);
            }
            depth += 1;
            index += 2;
            continue;
        }
        if bytes[index] == b'}' && bytes[index + 1] == b'}' {
            if depth == 0 {
                index += 2;
                continue;
            }
            depth -= 1;
            if depth == 0
                && let Some(body_start) = start.take()
            {
                bodies.push(&wikitext[body_start..index]);
            }
            index += 2;
            continue;
        }
        index += 1;
    }

    bodies
}

fn split_top_level_segments(text: &str, delimiter: char) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut segment_start = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let bytes = text.as_bytes();
    let delimiter = delimiter as u8;
    let mut index = 0usize;

    while index < bytes.len() {
        match bytes[index] {
            b'{' if index + 1 < bytes.len() && bytes[index + 1] == b'{' => {
                brace_depth += 1;
                index += 2;
            }
            b'}' if index + 1 < bytes.len() && bytes[index + 1] == b'}' => {
                brace_depth = brace_depth.saturating_sub(1);
                index += 2;
            }
            b'[' if index + 1 < bytes.len() && bytes[index + 1] == b'[' => {
                bracket_depth += 1;
                index += 2;
            }
            b']' if index + 1 < bytes.len() && bytes[index + 1] == b']' => {
                bracket_depth = bracket_depth.saturating_sub(1);
                index += 2;
            }
            byte if byte == delimiter && brace_depth == 0 && bracket_depth == 0 => {
                segments.push(&text[segment_start..index]);
                segment_start = index + 1;
                index += 1;
            }
            _ => {
                index += 1;
            }
        }
    }

    segments.push(&text[segment_start..]);
    segments
}

fn split_top_level_once(text: &str, delimiter: char) -> Option<(&str, &str)> {
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let bytes = text.as_bytes();
    let delimiter = delimiter as u8;
    let mut index = 0usize;

    while index < bytes.len() {
        match bytes[index] {
            b'{' if index + 1 < bytes.len() && bytes[index + 1] == b'{' => {
                brace_depth += 1;
                index += 2;
            }
            b'}' if index + 1 < bytes.len() && bytes[index + 1] == b'}' => {
                brace_depth = brace_depth.saturating_sub(1);
                index += 2;
            }
            b'[' if index + 1 < bytes.len() && bytes[index + 1] == b'[' => {
                bracket_depth += 1;
                index += 2;
            }
            b']' if index + 1 < bytes.len() && bytes[index + 1] == b']' => {
                bracket_depth = bracket_depth.saturating_sub(1);
                index += 2;
            }
            byte if byte == delimiter && brace_depth == 0 && bracket_depth == 0 => {
                return Some((&text[..index], &text[index + 1..]));
            }
            _ => {
                index += 1;
            }
        }
    }

    None
}

fn normalize_template_name(raw_name: &str) -> String {
    raw_name
        .trim()
        .replace('_', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_template_parameter_name(raw_name: &str) -> String {
    raw_name
        .trim()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .flat_map(char::to_lowercase)
        .collect()
}

fn is_media_parameter_name(parameter_name: &str) -> bool {
    let normalized = parameter_name.trim_end_matches(|character: char| character.is_ascii_digit());
    if normalized.is_empty() {
        return false;
    }

    if matches!(
        normalized,
        "image"
            | "imagename"
            | "imagefile"
            | "photo"
            | "logo"
            | "seal"
            | "sealimage"
            | "signature"
            | "map"
            | "mapimage"
            | "pushpinmap"
            | "pushpinmapimage"
            | "flag"
            | "flagimage"
            | "coatofarms"
            | "blason"
            | "drapeau"
            | "carte"
            | "screenshot"
            | "cover"
            | "patch"
            | "crest"
            | "emblem"
            | "insignia"
            | "symbol"
    ) {
        return true;
    }

    normalized.contains("image")
        && !matches!(
            normalized,
            "imagesize"
                | "imagecaption"
                | "imagelegend"
                | "imagealt"
                | "imageupright"
                | "imagewidth"
                | "imageheight"
        )
}

fn normalize_template_media_value(raw_value: &str) -> Option<String> {
    let stripped_comments = strip_html_comments(raw_value);
    let first_line = stripped_comments
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim();
    if first_line.is_empty()
        || first_line.contains("://")
        || first_line.contains("[[")
        || first_line.contains("{{")
    {
        return None;
    }

    let candidate = first_line
        .split('|')
        .next()
        .unwrap_or("")
        .split('<')
        .next()
        .unwrap_or("")
        .trim()
        .trim_start_matches(':')
        .trim();
    if candidate.is_empty() {
        return None;
    }

    if let Some(file_name) = normalize_media_target(candidate) {
        return Some(file_name);
    }
    if !looks_like_media_file_name(candidate) {
        return None;
    }

    Some(format!("File:{}", candidate.replace('_', " ")))
}

fn strip_html_comments(text: &str) -> String {
    let mut stripped = String::with_capacity(text.len());
    let mut remaining = text;

    while let Some(comment_start) = remaining.find("<!--") {
        stripped.push_str(&remaining[..comment_start]);
        let after_start = &remaining[comment_start + 4..];
        let Some(comment_end) = after_start.find("-->") else {
            return stripped;
        };
        remaining = &after_start[comment_end + 3..];
    }

    stripped.push_str(remaining);
    stripped
}

fn looks_like_media_file_name(candidate: &str) -> bool {
    let candidate = candidate.trim();
    if candidate.is_empty()
        || candidate.contains('\n')
        || candidate.contains('{')
        || candidate.contains('}')
        || candidate.contains('[')
        || candidate.contains(']')
        || candidate.contains('/')
    {
        return false;
    }

    let Some((base_name, extension)) = candidate.rsplit_once('.') else {
        return false;
    };
    if base_name.trim().is_empty() {
        return false;
    }

    matches!(
        extension.to_ascii_lowercase().as_str(),
        "jpg"
            | "jpeg"
            | "png"
            | "gif"
            | "svg"
            | "webp"
            | "tif"
            | "tiff"
            | "pdf"
            | "djvu"
            | "ogg"
            | "oga"
            | "ogv"
            | "webm"
    )
}

fn normalize_media_target(target: &str) -> Option<String> {
    let trimmed = target.trim().trim_start_matches(':').trim();
    let (namespace, title) = trimmed.split_once(':')?;
    let namespace = namespace.trim().to_ascii_lowercase();
    if !matches!(namespace.as_str(), "file" | "image" | "fichier") {
        return None;
    }
    let title = title.trim().trim_matches(char::from(0));
    if title.is_empty() {
        return None;
    }
    Some(format!("File:{}", title.replace('_', " ")))
}

fn normalize_usage_signature(parts: Vec<&str>) -> String {
    parts
        .into_iter()
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" | ")
}

fn display_title_from_file_name(file_name: &str) -> String {
    file_name
        .strip_prefix("File:")
        .unwrap_or(file_name)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::{MediaDiffKind, build_media_diff, extract_media_references};

    #[test]
    fn extracts_wikilink_file_references_with_aliases() {
        let references = extract_media_references(
            "[[File:Example.jpg|thumb|Caption]] and [[Fichier:Autre.png|220px]]",
        );

        assert_eq!(references.len(), 2);
        assert_eq!(references[0].file_name, "File:Example.jpg");
        assert_eq!(references[0].usage_signature, "thumb | Caption");
        assert_eq!(references[1].file_name, "File:Autre.png");
    }

    #[test]
    fn extracts_gallery_entries() {
        let references = extract_media_references(
            "<gallery>\nFile:One.jpg|First\nImage:Two.png|Second\n</gallery>",
        );

        assert_eq!(references.len(), 2);
        assert_eq!(references[0].file_name, "File:One.jpg");
        assert_eq!(references[1].file_name, "File:Two.png");
    }

    #[test]
    fn extracts_template_image_parameters() {
        let references = extract_media_references(
            "{{Infobox person|image=Example.jpg|image_size=220px|caption=Example.jpg}}",
        );

        assert_eq!(references.len(), 1);
        assert_eq!(references[0].file_name, "File:Example.jpg");
        assert_eq!(
            references[0].usage_signature,
            "template:Infobox person | image"
        );
    }

    #[test]
    fn extracts_french_infobox_media_parameters() {
        let references = extract_media_references(
            "{{Infobox Commune de France|blason=Blason.svg|drapeau=Flag of Paris.svg|carte=Paris map.png}}",
        );

        assert_eq!(references.len(), 3);
        assert_eq!(references[0].file_name, "File:Blason.svg");
        assert_eq!(references[1].file_name, "File:Flag of Paris.svg");
        assert_eq!(references[2].file_name, "File:Paris map.png");
    }

    #[test]
    fn ignores_non_media_template_parameters() {
        let references = extract_media_references(
            "{{Infobox person|name=Example.jpg|caption=Portrait.jpg|image_size=200px|birth_place=Paris}}",
        );

        assert!(references.is_empty());
    }

    #[test]
    fn detects_media_changes_in_infobox_parameters() {
        let report = build_media_diff(
            "{{Infobox person|image=Old portrait.jpg|logo=File:Legacy.svg}}",
            "{{Infobox person|image=New portrait.jpg|logo=File:Legacy.svg}}",
        );

        assert_eq!(report.entries.len(), 2);
        assert_eq!(report.entries[0].kind, MediaDiffKind::Added);
        assert_eq!(report.entries[0].file_name, "File:New portrait.jpg");
        assert_eq!(report.entries[1].kind, MediaDiffKind::Removed);
        assert_eq!(report.entries[1].file_name, "File:Old portrait.jpg");
    }

    #[test]
    fn builds_added_removed_and_changed_media_entries() {
        let report = build_media_diff(
            "[[File:Removed.jpg|thumb]] [[File:Changed.jpg|thumb|Old]]",
            "[[File:Added.jpg|thumb]] [[File:Changed.jpg|thumb|New]] [[File:Changed.jpg|thumb|New]]",
        );

        assert_eq!(report.entries.len(), 3);
        assert_eq!(report.entries[0].kind, MediaDiffKind::Added);
        assert_eq!(report.entries[0].file_name, "File:Added.jpg");
        assert_eq!(report.entries[1].kind, MediaDiffKind::Removed);
        assert_eq!(report.entries[1].file_name, "File:Removed.jpg");
        assert_eq!(report.entries[2].kind, MediaDiffKind::Changed);
        assert_eq!(report.entries[2].file_name, "File:Changed.jpg");
        assert_eq!(report.entries[2].before_occurrences, 1);
        assert_eq!(report.entries[2].after_occurrences, 2);
    }

    #[test]
    fn emits_note_when_no_media_references_change() {
        let report = build_media_diff("[[File:Same.jpg|thumb]]", "[[File:Same.jpg|thumb]]");

        assert!(report.entries.is_empty());
        assert_eq!(
            report.notes,
            vec!["No media references changed in the selected revision.".to_string()]
        );
    }
}
