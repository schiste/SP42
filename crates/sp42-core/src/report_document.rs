//! Shared structured report document helpers for text and markdown rendering.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportSection {
    pub name: String,
    pub available: bool,
    pub summary_lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportDocument {
    pub title: String,
    pub lead_lines: Vec<String>,
    pub sections: Vec<ReportSection>,
}

impl ReportDocument {
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            lead_lines: Vec::new(),
            sections: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_lead_lines(mut self, lead_lines: Vec<String>) -> Self {
        self.lead_lines = lead_lines;
        self
    }

    #[must_use]
    pub fn with_sections(mut self, sections: Vec<ReportSection>) -> Self {
        self.sections = sections;
        self
    }
}

#[must_use]
pub fn render_report_document_text(document: &ReportDocument) -> String {
    let mut lines = vec![document.title.clone()];

    lines.extend(document.lead_lines.iter().cloned());

    for section in &document.sections {
        lines.push(format!(
            "[{}] available={}",
            section.name, section.available
        ));
        if section.summary_lines.is_empty() {
            lines.push("  _Empty_".to_string());
        } else {
            lines.extend(section.summary_lines.iter().map(|line| format!("  {line}")));
        }
    }

    lines.join("\n")
}

#[must_use]
pub fn render_report_document_markdown(document: &ReportDocument) -> String {
    let mut sections = vec![format!("# {}", document.title)];

    if document.lead_lines.is_empty() {
        sections.push("_No summary_".to_string());
    } else {
        sections.push(
            document
                .lead_lines
                .iter()
                .map(|line| format!("- {line}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }

    for section in &document.sections {
        let body = if section.summary_lines.is_empty() {
            "_Empty_".to_string()
        } else {
            section
                .summary_lines
                .iter()
                .map(|line| format!("- {line}"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        sections.push(format!(
            "## {}\n\n- available: {}\n{body}",
            section.name, section.available
        ));
    }

    sections.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::{
        ReportDocument, ReportSection, render_report_document_markdown, render_report_document_text,
    };

    #[test]
    fn renders_text_documents_with_sections() {
        let document = ReportDocument::new("Example")
            .with_lead_lines(vec!["wiki=frwiki".to_string(), "queue_depth=2".to_string()])
            .with_sections(vec![
                ReportSection {
                    name: "Overview".to_string(),
                    available: true,
                    summary_lines: vec!["ready".to_string()],
                },
                ReportSection {
                    name: "Warnings".to_string(),
                    available: false,
                    summary_lines: vec![],
                },
            ]);

        let text = render_report_document_text(&document);

        assert!(text.contains("Example"));
        assert!(text.contains("wiki=frwiki"));
        assert!(text.contains("[Warnings] available=false"));
        assert!(text.contains("_Empty_"));
    }

    #[test]
    fn renders_markdown_documents_with_sections() {
        let document = ReportDocument::new("Example")
            .with_lead_lines(vec!["wiki=frwiki".to_string()])
            .with_sections(vec![ReportSection {
                name: "Overview".to_string(),
                available: true,
                summary_lines: vec!["ready".to_string()],
            }]);

        let markdown = render_report_document_markdown(&document);

        assert!(markdown.contains("# Example"));
        assert!(markdown.contains("- wiki=frwiki"));
        assert!(markdown.contains("## Overview"));
        assert!(markdown.contains("- available: true"));
    }
}
