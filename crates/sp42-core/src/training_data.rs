//! Structured action logging and export types live here.

use serde::{Deserialize, Serialize};

use crate::errors::TrainingDataError;
use crate::types::Action;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrainingLabel {
    pub wiki_id: String,
    pub rev_id: u64,
    pub actor: String,
    pub action: Action,
    pub captured_at_ms: i64,
    pub note: Option<String>,
}

/// Encode a training label as a single JSON Lines record.
///
/// # Errors
///
/// Returns [`TrainingDataError`] when the record cannot be serialized.
pub fn encode_json_line(label: &TrainingLabel) -> Result<String, TrainingDataError> {
    let mut line = serde_json::to_string(label).map_err(TrainingDataError::from)?;
    line.push('\n');
    Ok(line)
}

/// Encode many training labels as newline-delimited JSON.
///
/// # Errors
///
/// Returns [`TrainingDataError`] when any label cannot be serialized.
pub fn encode_json_lines(labels: &[TrainingLabel]) -> Result<String, TrainingDataError> {
    let mut output = String::new();
    for label in labels {
        output.push_str(&encode_json_line(label)?);
    }

    Ok(output)
}

/// Encode training labels as a JSON array.
///
/// # Errors
///
/// Returns [`TrainingDataError`] when serialization fails.
pub fn encode_json(labels: &[TrainingLabel]) -> Result<String, TrainingDataError> {
    serde_json::to_string_pretty(labels).map_err(TrainingDataError::from)
}

/// Encode training labels as CSV with a stable header row.
#[must_use]
pub fn encode_csv(labels: &[TrainingLabel]) -> String {
    let mut rows = Vec::with_capacity(labels.len() + 1);
    rows.push("wiki_id,rev_id,actor,action,captured_at_ms,note".to_string());

    for label in labels {
        rows.push(format!(
            "{},{},{},{},{},{}",
            encode_csv_field(&label.wiki_id),
            label.rev_id,
            encode_csv_field(&label.actor),
            encode_csv_field(&format!("{:?}", label.action)),
            label.captured_at_ms,
            encode_csv_field(label.note.as_deref().unwrap_or(""))
        ));
    }

    rows.join("\n")
}

fn encode_csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{TrainingLabel, encode_csv, encode_json, encode_json_line, encode_json_lines};
    use crate::types::Action;

    fn sample_labels() -> Vec<TrainingLabel> {
        vec![
            TrainingLabel {
                wiki_id: "frwiki".to_string(),
                rev_id: 123_456,
                actor: "Example".to_string(),
                action: Action::Rollback,
                captured_at_ms: 1_710_000_000_000,
                note: Some("obvious spam".to_string()),
            },
            TrainingLabel {
                wiki_id: "frwiki".to_string(),
                rev_id: 123_457,
                actor: "Admin".to_string(),
                action: Action::Warn,
                captured_at_ms: 1_710_000_000_100,
                note: Some("warning, level 2".to_string()),
            },
        ]
    }

    #[test]
    fn encodes_json_lines_record() {
        let line = encode_json_line(&sample_labels()[0]).expect("training record should encode");

        assert!(line.ends_with('\n'));
        assert!(line.contains("\"rev_id\":123456"));
        assert!(line.contains("\"action\":\"Rollback\""));
    }

    #[test]
    fn encodes_json_lines_batch() {
        let output = encode_json_lines(&sample_labels()).expect("batch should encode");

        assert_eq!(output.lines().count(), 2);
        assert!(output.contains("\"action\":\"Warn\""));
    }

    #[test]
    fn encodes_json_array() {
        let output = encode_json(&sample_labels()).expect("json should encode");

        assert!(output.starts_with("[\n"));
        assert!(output.contains("\"rev_id\": 123456"));
        assert!(output.contains("\"actor\": \"Admin\""));
    }

    #[test]
    fn encodes_csv_output() {
        let output = encode_csv(&sample_labels());

        assert!(output.starts_with("wiki_id,rev_id,actor,action,captured_at_ms,note\n"));
        assert!(output.contains("frwiki,123456,Example,Rollback,1710000000000,obvious spam"));
        assert!(output.contains("\"warning, level 2\""));
    }
}
