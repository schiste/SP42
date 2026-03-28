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
        notes.push("No explicit media references changed in the selected revision.".to_string());
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
            vec!["No explicit media references changed in the selected revision.".to_string()]
        );
    }
}
