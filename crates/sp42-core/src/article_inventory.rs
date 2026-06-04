//! Article-level inventory extracted from current wikitext.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::media_diff::{MediaReference, extract_media_references};

const PREVIEW_LIMIT: usize = 140;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArticleReference {
    pub ordinal: usize,
    pub name: Option<String>,
    pub has_content: bool,
    pub citation_template_count: usize,
    #[serde(default)]
    pub bare_urls: Vec<String>,
    pub preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArticleInventory {
    pub wiki_id: String,
    pub title: String,
    pub byte_len: usize,
    pub section_count: usize,
    #[serde(default)]
    pub section_headings: Vec<String>,
    #[serde(default)]
    pub references: Vec<ArticleReference>,
    #[serde(default)]
    pub bare_urls: Vec<String>,
    #[serde(default)]
    pub citation_templates: Vec<String>,
    #[serde(default)]
    pub citation_needed_templates: Vec<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub interwiki_links: Vec<String>,
    #[serde(default)]
    pub templates: Vec<String>,
    #[serde(default)]
    pub media_references: Vec<MediaReference>,
    #[serde(default)]
    pub notes: Vec<String>,
}

impl ArticleInventory {
    #[must_use]
    pub fn reference_count(&self) -> usize {
        self.references.len()
    }

    #[must_use]
    pub fn media_count(&self) -> usize {
        self.media_references.len()
    }
}

#[must_use]
pub fn build_article_inventory(wiki_id: &str, title: &str, wikitext: &str) -> ArticleInventory {
    let references = extract_references(wikitext);
    let bare_urls = extract_bare_urls(wikitext);
    let templates = extract_template_names(wikitext);
    let citation_templates = templates
        .iter()
        .filter(|name| is_citation_template(name))
        .cloned()
        .collect::<Vec<_>>();
    let citation_needed_templates = templates
        .iter()
        .filter(|name| is_citation_needed_template(name))
        .cloned()
        .collect::<Vec<_>>();
    let (categories, interwiki_links) = extract_page_links(wikitext);
    let section_headings = extract_section_headings(wikitext);
    let media_references = extract_media_references(wikitext);

    ArticleInventory {
        wiki_id: wiki_id.to_string(),
        title: title.to_string(),
        byte_len: wikitext.len(),
        section_count: section_headings.len(),
        section_headings,
        references,
        bare_urls,
        citation_templates,
        citation_needed_templates,
        categories,
        interwiki_links,
        templates,
        media_references,
        notes: article_inventory_notes(wikitext),
    }
}

#[must_use]
pub fn article_inventory_notes(wikitext: &str) -> Vec<String> {
    let mut notes = Vec::new();
    if extract_references(wikitext).is_empty() {
        notes.push("No <ref> tags were detected in the current article text.".to_string());
    }
    if extract_media_references(wikitext).is_empty() {
        notes.push(
            "No explicit file, gallery, or template media references were detected.".to_string(),
        );
    }
    notes.push(
        "Inventory is wikitext-derived and does not yet validate external URLs, Wikidata claims, or Commons metadata."
            .to_string(),
    );
    notes
}

fn extract_references(wikitext: &str) -> Vec<ArticleReference> {
    let mut references = Vec::new();
    let mut cursor = 0usize;
    let mut ordinal = 1usize;

    while let Some(start) = find_ascii_case_insensitive(wikitext, "<ref", cursor) {
        let Some(open_end_rel) = wikitext[start..].find('>') else {
            break;
        };
        let open_end = start + open_end_rel + 1;
        let opening = &wikitext[start..open_end];
        let self_closing = opening.trim_end().ends_with("/>");
        let (content, next_cursor) = if self_closing {
            ("", open_end)
        } else if let Some(close_start) = find_ascii_case_insensitive(wikitext, "</ref>", open_end)
        {
            (
                &wikitext[open_end..close_start],
                close_start + "</ref>".len(),
            )
        } else {
            ("", open_end)
        };

        let citation_template_count = extract_template_names(content)
            .iter()
            .filter(|name| is_citation_template(name))
            .count();
        references.push(ArticleReference {
            ordinal,
            name: extract_attribute(opening, "name"),
            has_content: !content.trim().is_empty(),
            citation_template_count,
            bare_urls: extract_bare_urls(content),
            preview: preview_text(content),
        });
        ordinal = ordinal.saturating_add(1);
        cursor = next_cursor;
    }

    references
}

fn extract_page_links(wikitext: &str) -> (Vec<String>, Vec<String>) {
    let mut categories = BTreeSet::new();
    let mut interwiki_links = BTreeSet::new();
    let mut cursor = 0usize;

    while let Some(start_rel) = wikitext[cursor..].find("[[") {
        let start = cursor + start_rel + 2;
        let Some(end_rel) = wikitext[start..].find("]]") else {
            break;
        };
        let end = start + end_rel;
        let target = wikitext[start..end]
            .split('|')
            .next()
            .unwrap_or_default()
            .trim();
        let lowered = target.to_ascii_lowercase();
        if lowered.starts_with("category:") || lowered.starts_with("catégorie:") {
            categories.insert(target.to_string());
        } else if is_interwiki_target(&lowered) {
            interwiki_links.insert(target.to_string());
        }
        cursor = end + 2;
    }

    (
        categories.into_iter().collect(),
        interwiki_links.into_iter().collect(),
    )
}

fn is_interwiki_target(lowered: &str) -> bool {
    let Some((prefix, _)) = lowered.split_once(':') else {
        return false;
    };
    matches!(
        prefix,
        "en" | "fr"
            | "de"
            | "es"
            | "it"
            | "pt"
            | "nl"
            | "pl"
            | "ru"
            | "uk"
            | "wikidata"
            | "commons"
            | "wikisource"
            | "wikiquote"
            | "wikivoyage"
    )
}

fn extract_template_names(wikitext: &str) -> Vec<String> {
    let mut names = BTreeSet::new();
    let mut cursor = 0usize;

    while let Some(start_rel) = wikitext[cursor..].find("{{") {
        let start = cursor + start_rel + 2;
        let Some(end_rel) = wikitext[start..].find("}}") else {
            break;
        };
        let end = start + end_rel;
        let name = wikitext[start..end]
            .split('|')
            .next()
            .unwrap_or_default()
            .trim()
            .trim_start_matches('#')
            .trim();
        if !name.is_empty() {
            names.insert(name.to_string());
        }
        cursor = end + 2;
    }

    names.into_iter().collect()
}

fn extract_section_headings(wikitext: &str) -> Vec<String> {
    wikitext
        .lines()
        .filter_map(parse_section_heading)
        .collect::<Vec<_>>()
}

fn parse_section_heading(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let prefix = trimmed.chars().take_while(|ch| *ch == '=').count();
    let suffix = trimmed.chars().rev().take_while(|ch| *ch == '=').count();
    if prefix < 2 || prefix != suffix || trimmed.len() <= prefix + suffix {
        return None;
    }
    let heading = trimmed[prefix..trimmed.len() - suffix].trim();
    (!heading.is_empty()).then(|| heading.to_string())
}

fn extract_bare_urls(value: &str) -> Vec<String> {
    value
        .split_whitespace()
        .filter_map(|token| {
            let trimmed = token.trim_matches(|ch: char| {
                matches!(
                    ch,
                    '"' | '\'' | ')' | '(' | ']' | '[' | '<' | '>' | ',' | ';'
                )
            });
            (trimmed.starts_with("http://") || trimmed.starts_with("https://"))
                .then(|| trimmed.trim_end_matches('.').to_string())
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn extract_attribute(opening_tag: &str, name: &str) -> Option<String> {
    let lowered = opening_tag.to_ascii_lowercase();
    let key = format!("{name}=");
    let start = lowered.find(&key)? + key.len();
    let value = opening_tag[start..].trim_start();
    let quote = value.chars().next()?;
    if quote == '"' || quote == '\'' {
        let end = value[quote.len_utf8()..].find(quote)?;
        return Some(value[quote.len_utf8()..quote.len_utf8() + end].to_string());
    }
    value
        .split_whitespace()
        .next()
        .map(|entry| entry.trim_end_matches("/>").to_string())
        .filter(|entry| !entry.is_empty())
}

fn find_ascii_case_insensitive(haystack: &str, needle: &str, from: usize) -> Option<usize> {
    let lower_haystack = haystack[from..].to_ascii_lowercase();
    let lower_needle = needle.to_ascii_lowercase();
    lower_haystack.find(&lower_needle).map(|index| from + index)
}

fn preview_text(value: &str) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let preview = normalized.chars().take(PREVIEW_LIMIT).collect::<String>();
    if normalized.chars().count() > PREVIEW_LIMIT {
        format!("{preview}...")
    } else {
        preview
    }
}

fn is_citation_template(name: &str) -> bool {
    let lowered = name.to_ascii_lowercase();
    lowered.starts_with("cite ")
        || lowered.starts_with("cite_")
        || lowered == "citation"
        || lowered.starts_with("ouvrage")
        || lowered.starts_with("article")
        || lowered.starts_with("lien web")
}

fn is_citation_needed_template(name: &str) -> bool {
    let lowered = name.to_ascii_lowercase();
    matches!(
        lowered.as_str(),
        "citation needed"
            | "cn"
            | "refnec"
            | "référence nécessaire"
            | "reference necessary"
            | "references needed"
    )
}

#[cfg(test)]
mod tests {
    use super::build_article_inventory;

    #[test]
    fn inventories_article_references_and_categories() {
        let inventory = build_article_inventory(
            "frwiki",
            "Exemple",
            r#"Intro {{Infobox}} text.<ref name="a">{{Lien web|url=https://example.org|titre=Example}}</ref>
== Histoire ==
[[Catégorie:Exemple]]
[[en:Example]]
[[Fichier:Example.jpg|thumb|Caption]]
{{Référence nécessaire|Un fait|date=mai 2026}}
"#,
        );

        assert_eq!(inventory.reference_count(), 1);
        assert_eq!(inventory.references[0].name.as_deref(), Some("a"));
        assert_eq!(inventory.section_headings, vec!["Histoire"]);
        assert_eq!(inventory.categories, vec!["Catégorie:Exemple"]);
        assert_eq!(inventory.interwiki_links, vec!["en:Example"]);
        assert_eq!(inventory.media_count(), 1);
        assert_eq!(inventory.citation_needed_templates.len(), 1);
    }

    #[test]
    fn inventories_self_closing_named_references() {
        let inventory = build_article_inventory(
            "frwiki",
            "Exemple",
            r"Text<ref name=a /> and <ref name='b'>https://example.test/source</ref>",
        );

        assert_eq!(inventory.reference_count(), 2);
        assert_eq!(inventory.references[0].name.as_deref(), Some("a"));
        assert!(!inventory.references[0].has_content);
        assert_eq!(inventory.references[1].name.as_deref(), Some("b"));
        assert_eq!(inventory.references[1].bare_urls.len(), 1);
    }
}
