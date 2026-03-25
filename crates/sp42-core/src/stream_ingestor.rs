//! `EventStreams` ingestion and filtering lives here.

use std::collections::BTreeSet;
use std::net::IpAddr;
use std::str::FromStr;

use serde::Deserialize;

use crate::errors::StreamIngestorError;
use crate::types::{EditEvent, EditorIdentity, WikiConfig};

const SUPPORTED_CHANGE_TYPES: [&str; 2] = ["edit", "new"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamIngestor {
    wiki_id: String,
    namespace_allowlist: BTreeSet<i32>,
    ignore_bots: bool,
    ignore_minor: bool,
    allowed_change_types: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamIngestorOptions {
    pub ignore_bots: bool,
    pub ignore_minor: bool,
    pub allowed_change_types: BTreeSet<String>,
}

impl StreamIngestor {
    #[must_use]
    pub fn from_config(config: &WikiConfig) -> Self {
        Self::with_options(
            config,
            StreamIngestorOptions {
                ignore_bots: true,
                ignore_minor: false,
                allowed_change_types: SUPPORTED_CHANGE_TYPES
                    .iter()
                    .map(|value| (*value).to_string())
                    .collect(),
            },
        )
    }

    #[must_use]
    pub fn with_options(config: &WikiConfig, options: StreamIngestorOptions) -> Self {
        Self {
            wiki_id: config.wiki_id.clone(),
            namespace_allowlist: config.namespace_allowlist.iter().copied().collect(),
            ignore_bots: options.ignore_bots,
            ignore_minor: options.ignore_minor,
            allowed_change_types: options.allowed_change_types,
        }
    }

    /// Parse and filter a single `recentchange` event payload.
    ///
    /// # Errors
    ///
    /// Returns [`StreamIngestorError`] when the payload is not valid JSON or is
    /// missing required fields for an actionable edit event.
    pub fn ingest(&self, payload: &str) -> Result<Option<EditEvent>, StreamIngestorError> {
        let raw: RecentChangeEvent = serde_json::from_str(payload)?;

        if raw.wiki != self.wiki_id {
            return Ok(None);
        }

        if !is_allowed_change_type(&self.allowed_change_types, &raw.change_type) {
            return Ok(None);
        }

        if self.ignore_bots && raw.bot {
            return Ok(None);
        }

        if self.ignore_minor && raw.minor {
            return Ok(None);
        }

        if !self.namespace_allowlist.is_empty()
            && !self.namespace_allowlist.contains(&raw.namespace)
        {
            return Ok(None);
        }

        let revision = raw
            .revision
            .ok_or_else(|| StreamIngestorError::InvalidPayload {
                message: "revision object is required".to_string(),
            })?;
        let rev_id = revision
            .new_rev_id
            .ok_or_else(|| StreamIngestorError::InvalidPayload {
                message: "revision.new is required".to_string(),
            })?;

        let byte_delta = raw.length.map_or(0, |length| {
            compute_byte_delta(length.new_length, length.old_length)
        });

        Ok(Some(EditEvent {
            wiki_id: raw.wiki,
            title: raw.title,
            namespace: raw.namespace,
            rev_id,
            old_rev_id: revision.old_rev_id,
            performer: classify_editor(&raw.user),
            timestamp_ms: raw.timestamp.to_ms()?,
            is_bot: raw.bot.into(),
            is_minor: raw.minor.into(),
            is_new_page: (raw.change_type == "new").into(),
            tags: raw.tags,
            comment: raw.comment.filter(|value| !value.is_empty()),
            byte_delta,
            is_patrolled: false.into(),
        }))
    }

    /// Parse and filter a newline-delimited batch of `recentchange` payloads.
    ///
    /// # Errors
    ///
    /// Returns [`StreamIngestorError`] when any non-empty line is not valid
    /// recentchange JSON.
    pub fn ingest_lines(&self, payloads: &str) -> Result<Vec<EditEvent>, StreamIngestorError> {
        let mut events = Vec::new();

        for line in payloads.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            if let Some(event) = self.ingest(trimmed)? {
                events.push(event);
            }
        }

        Ok(events)
    }
}

fn classify_editor(user: &str) -> EditorIdentity {
    if user.starts_with('~') {
        return EditorIdentity::Temporary {
            label: user.to_string(),
        };
    }

    if IpAddr::from_str(user).is_ok() {
        return EditorIdentity::Anonymous {
            label: user.to_string(),
        };
    }

    EditorIdentity::Registered {
        username: user.to_string(),
    }
}

#[derive(Debug, Deserialize)]
struct RecentChangeEvent {
    wiki: String,
    namespace: i32,
    title: String,
    user: String,
    timestamp: TimestampValue,
    #[serde(default)]
    bot: bool,
    #[serde(default)]
    minor: bool,
    #[serde(rename = "type")]
    change_type: String,
    comment: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    revision: Option<RevisionState>,
    length: Option<LengthState>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum TimestampValue {
    Numeric(i64),
    Text(String),
}

impl TimestampValue {
    fn to_ms(&self) -> Result<i64, StreamIngestorError> {
        match self {
            Self::Numeric(value) => Ok(normalize_unix_timestamp_ms(*value)),
            Self::Text(value) => parse_timestamp_text(value),
        }
    }
}

fn parse_timestamp_text(value: &str) -> Result<i64, StreamIngestorError> {
    if let Ok(value) = value.parse::<i64>() {
        return Ok(normalize_unix_timestamp_ms(value));
    }

    if let Some((seconds, nanos)) = parse_rfc3339_utc(value) {
        return Ok(seconds
            .saturating_mul(1_000)
            .saturating_add(i64::from(nanos / 1_000_000)));
    }

    Err(StreamIngestorError::InvalidPayload {
        message: format!("unsupported timestamp format: {value}"),
    })
}

fn parse_rfc3339_utc(value: &str) -> Option<(i64, u32)> {
    if value.len() < 20 || !value.ends_with('Z') {
        return None;
    }

    if !has_rfc3339_utc_layout(value) {
        return None;
    }

    let year = value[0..4].parse::<i32>().ok()?;
    let month = value[5..7].parse::<u32>().ok()?;
    let day = value[8..10].parse::<u32>().ok()?;
    let hour = value[11..13].parse::<u32>().ok()?;
    let minute = value[14..16].parse::<u32>().ok()?;
    let second = value[17..19].parse::<u32>().ok()?;

    if !is_valid_utc_date(year, month, day) || hour > 23 || minute > 59 || second > 59 {
        return None;
    }

    let nanos = parse_fractional_nanos(&value[19..value.len() - 1])?;

    let days = days_from_civil(year, month, day);
    let seconds = days
        .saturating_mul(86_400)
        .saturating_add(i64::from(hour) * 3_600)
        .saturating_add(i64::from(minute) * 60)
        .saturating_add(i64::from(second));

    Some((seconds, nanos))
}

fn is_allowed_change_type(allowed_change_types: &BTreeSet<String>, change_type: &str) -> bool {
    SUPPORTED_CHANGE_TYPES.contains(&change_type) && allowed_change_types.contains(change_type)
}

const fn normalize_unix_timestamp_ms(value: i64) -> i64 {
    if value > 10_000_000_000 {
        value
    } else {
        value.saturating_mul(1_000)
    }
}

fn compute_byte_delta(new_length: i32, old_length: Option<i32>) -> i32 {
    new_length.saturating_sub(old_length.unwrap_or(0))
}

fn has_rfc3339_utc_layout(value: &str) -> bool {
    matches!(value.as_bytes(), [y1, y2, y3, y4, b'-', m1, m2, b'-', d1, d2, b'T', h1, h2, b':', min1, min2, b':', s1, s2, rest @ .., b'Z']
        if y1.is_ascii_digit()
            && y2.is_ascii_digit()
            && y3.is_ascii_digit()
            && y4.is_ascii_digit()
            && m1.is_ascii_digit()
            && m2.is_ascii_digit()
            && d1.is_ascii_digit()
            && d2.is_ascii_digit()
            && h1.is_ascii_digit()
            && h2.is_ascii_digit()
            && min1.is_ascii_digit()
            && min2.is_ascii_digit()
            && s1.is_ascii_digit()
            && s2.is_ascii_digit()
            && (rest.is_empty() || (rest[0] == b'.' && rest[1..].iter().all(u8::is_ascii_digit))))
}

fn parse_fractional_nanos(segment: &str) -> Option<u32> {
    let Some(fractional) = segment.strip_prefix('.') else {
        return if segment.is_empty() { Some(0) } else { None };
    };

    if fractional.is_empty() || !fractional.chars().all(|digit| digit.is_ascii_digit()) {
        return None;
    }

    let digits = fractional.chars().take(9).collect::<Vec<_>>();
    let nanos = digits.into_iter().fold(0u32, |acc, digit| {
        acc * 10 + digit.to_digit(10).unwrap_or(0)
    });
    let scale = 9usize.saturating_sub(fractional.len().min(9));

    Some(nanos.saturating_mul(10u32.saturating_pow(u32::try_from(scale).unwrap_or(0))))
}

fn is_valid_utc_date(year: i32, month: u32, day: u32) -> bool {
    let Some(max_day) = days_in_month(year, month) else {
        return false;
    };

    (1..=max_day).contains(&day)
}

const fn days_in_month(year: i32, month: u32) -> Option<u32> {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => Some(31),
        4 | 6 | 9 | 11 => Some(30),
        2 => Some(if is_leap_year(year) { 29 } else { 28 }),
        _ => None,
    }
}

const fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let adjusted_year = year - i32::from(month <= 2);
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let year_of_era = adjusted_year - era * 400;
    let month_i32 = i32::try_from(month).unwrap_or(0);
    let day_i32 = i32::try_from(day).unwrap_or(0);
    let day_of_year =
        (153 * (month_i32 + if month_i32 > 2 { -3 } else { 9 }) + 2) / 5 + day_i32 - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;

    i64::from(era) * 146_097 + i64::from(day_of_era) - 719_468
}

#[derive(Debug, Deserialize)]
struct RevisionState {
    #[serde(rename = "new")]
    new_rev_id: Option<u64>,
    #[serde(rename = "old")]
    old_rev_id: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct LengthState {
    #[serde(rename = "new")]
    new_length: i32,
    #[serde(rename = "old")]
    old_length: Option<i32>,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use proptest::prelude::*;

    use super::{
        StreamIngestor, StreamIngestorOptions, normalize_unix_timestamp_ms, parse_timestamp_text,
    };
    use crate::config_parser::parse_wiki_config;
    use crate::types::EditorIdentity;

    const CONFIG: &str = include_str!("../../../configs/frwiki.yaml");
    const SAMPLE_EVENT: &str = include_str!("../../../fixtures/frwiki_recentchange_edit.json");
    const SAMPLE_BATCH: &str = include_str!("../../../fixtures/frwiki_recentchanges_batch.jsonl");

    #[test]
    fn ingests_supported_recentchange_event() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let ingestor = StreamIngestor::from_config(&config);

        let event = ingestor
            .ingest(SAMPLE_EVENT)
            .expect("event should parse")
            .expect("fixture should not be filtered");

        assert_eq!(event.wiki_id, "frwiki");
        assert_eq!(event.rev_id, 123_456);
        assert!(matches!(event.performer, EditorIdentity::Anonymous { .. }));
    }

    #[test]
    fn filters_out_unrelated_wikis() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let ingestor = StreamIngestor::from_config(&config);
        let payload = SAMPLE_EVENT.replace("\"frwiki\"", "\"enwiki\"");

        let event = ingestor.ingest(&payload).expect("payload should parse");

        assert!(event.is_none());
    }

    #[test]
    fn filters_out_bot_edits() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let ingestor = StreamIngestor::from_config(&config);
        let payload = SAMPLE_EVENT.replace("\"bot\": false", "\"bot\": true");

        let event = ingestor.ingest(&payload).expect("payload should parse");

        assert!(event.is_none());
    }

    #[test]
    fn ingests_json_lines_batch() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let ingestor = StreamIngestor::from_config(&config);

        let events = ingestor
            .ingest_lines(SAMPLE_BATCH)
            .expect("json lines payload should parse");

        assert_eq!(events.len(), 4);
        assert_eq!(events[0].rev_id, 123_456);
    }

    #[test]
    fn accepts_timestamp_strings_and_millisecond_values() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let ingestor = StreamIngestor::from_config(&config);

        let string_payload = SAMPLE_EVENT.replace(
            "\"timestamp\": 1710000000",
            "\"timestamp\": \"2024-03-09T12:34:56Z\"",
        );
        let string_event = ingestor
            .ingest(&string_payload)
            .expect("payload should parse")
            .expect("fixture should not be filtered");

        assert_eq!(string_event.timestamp_ms, 1_709_987_696_000);

        let millis_payload =
            SAMPLE_EVENT.replace("\"timestamp\": 1710000000", "\"timestamp\": 1710000000000");
        let millis_event = ingestor
            .ingest(&millis_payload)
            .expect("payload should parse")
            .expect("fixture should not be filtered");

        assert_eq!(millis_event.timestamp_ms, 1_710_000_000_000);
    }

    #[test]
    fn can_ignore_minor_edits_with_explicit_options() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let ingestor = StreamIngestor::with_options(
            &config,
            StreamIngestorOptions {
                ignore_bots: true,
                ignore_minor: true,
                allowed_change_types: BTreeSet::from(["edit".to_string(), "new".to_string()]),
            },
        );
        let payload = SAMPLE_EVENT.replace("\"minor\": false", "\"minor\": true");

        let event = ingestor.ingest(&payload).expect("payload should parse");

        assert!(event.is_none());
    }

    #[test]
    fn classifies_temporary_and_ipv6_editors() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let ingestor = StreamIngestor::from_config(&config);

        let temporary = ingestor
            .ingest(&SAMPLE_EVENT.replace("\"user\": \"192.0.2.44\"", "\"user\": \"~2026-42\""))
            .expect("temporary payload should parse")
            .expect("temporary editor should not be filtered");
        let ipv6 = ingestor
            .ingest(&SAMPLE_EVENT.replace("\"user\": \"192.0.2.44\"", "\"user\": \"2001:db8::42\""))
            .expect("ipv6 payload should parse")
            .expect("ipv6 editor should not be filtered");

        assert!(matches!(
            temporary.performer,
            EditorIdentity::Temporary { .. }
        ));
        assert!(matches!(ipv6.performer, EditorIdentity::Anonymous { .. }));
    }

    #[test]
    fn classifies_registered_editors_and_saturates_byte_delta() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let ingestor = StreamIngestor::from_config(&config);
        let payload = SAMPLE_EVENT
            .replace("\"user\": \"192.0.2.44\"", "\"user\": \"ExampleUser\"")
            .replace("\"new\": 120", &format!("\"new\": {}", i32::MAX))
            .replace("\"old\": 80", &format!("\"old\": {}", i32::MIN));

        let event = ingestor
            .ingest(&payload)
            .expect("payload should parse")
            .expect("fixture should not be filtered");

        assert!(matches!(
            event.performer,
            EditorIdentity::Registered { ref username } if username == "ExampleUser"
        ));
        assert_eq!(event.byte_delta, i32::MAX);
    }

    #[test]
    fn honors_explicit_allowed_change_type_subset() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let ingestor = StreamIngestor::with_options(
            &config,
            StreamIngestorOptions {
                ignore_bots: false,
                ignore_minor: false,
                allowed_change_types: BTreeSet::from(["edit".to_string()]),
            },
        );

        let event = ingestor
            .ingest(&SAMPLE_EVENT.replace("\"type\": \"edit\"", "\"type\": \"new\""))
            .expect("payload should parse");

        assert!(event.is_none());
    }

    #[test]
    fn normalizes_empty_comment_and_missing_old_length() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let ingestor = StreamIngestor::from_config(&config);
        let payload = SAMPLE_EVENT
            .replace(
                "\"comment\": \"Ajout http://spam.example.test\"",
                "\"comment\": \"\"",
            )
            .replace("\"old\": 80", "\"old\": null");

        let event = ingestor
            .ingest(&payload)
            .expect("payload should parse")
            .expect("fixture should not be filtered");

        assert_eq!(event.comment, None);
        assert_eq!(event.byte_delta, 120);
    }

    #[test]
    fn rejects_impossible_calendar_dates_and_fractional_junk() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let ingestor = StreamIngestor::from_config(&config);

        let impossible_date = SAMPLE_EVENT.replace(
            "\"timestamp\": 1710000000",
            "\"timestamp\": \"2024-02-30T12:34:56Z\"",
        );
        let junk_fraction = SAMPLE_EVENT.replace(
            "\"timestamp\": 1710000000",
            "\"timestamp\": \"2024-03-09T12:34:56.AZ\"",
        );

        let impossible_error = ingestor
            .ingest(&impossible_date)
            .expect_err("invalid date should fail");
        let fraction_error = ingestor
            .ingest(&junk_fraction)
            .expect_err("invalid fraction should fail");

        assert!(
            impossible_error
                .to_string()
                .contains("unsupported timestamp format")
        );
        assert!(
            fraction_error
                .to_string()
                .contains("unsupported timestamp format")
        );
    }

    #[test]
    fn accepts_fractional_rfc3339_timestamps() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let ingestor = StreamIngestor::from_config(&config);
        let payload = SAMPLE_EVENT.replace(
            "\"timestamp\": 1710000000",
            "\"timestamp\": \"2024-03-09T12:34:56.789Z\"",
        );

        let event = ingestor
            .ingest(&payload)
            .expect("payload should parse")
            .expect("fixture should not be filtered");

        assert_eq!(event.timestamp_ms, 1_709_987_696_789);
    }

    proptest! {
        #[test]
        fn property_numeric_timestamp_normalization_matches_text_path(timestamp in 0i64..100_000_000_000i64) {
            let string_millis = parse_timestamp_text(&timestamp.to_string()).expect("timestamp text should parse");
            prop_assert_eq!(string_millis, normalize_unix_timestamp_ms(timestamp));
        }
    }
}
