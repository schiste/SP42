//! `MediaWiki` recentchanges backlog polling support.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::errors::RecentChangesError;
use crate::traits::HttpClient;
use crate::types::{EditEvent, EditorIdentity, HttpMethod, HttpRequest, HttpResponse, WikiConfig};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentChangesQuery {
    pub limit: u16,
    pub rccontinue: Option<String>,
    pub include_bots: bool,
    pub unpatrolled_only: bool,
    pub include_minor: bool,
    pub namespace_override: Option<Vec<i32>>,
}

impl RecentChangesQuery {
    #[must_use]
    pub const fn initial(limit: u16, include_bots: bool) -> Self {
        Self {
            limit,
            rccontinue: None,
            include_bots,
            unpatrolled_only: false,
            include_minor: true,
            namespace_override: None,
        }
    }

    #[must_use]
    pub fn with_continue(mut self, rccontinue: Option<String>) -> Self {
        self.rccontinue = rccontinue;
        self
    }

    #[must_use]
    pub fn is_initial_poll(&self) -> bool {
        self.rccontinue.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentChangesBatch {
    pub events: Vec<EditEvent>,
    pub next_continue: Option<String>,
}

impl RecentChangesBatch {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    #[must_use]
    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    #[must_use]
    pub fn checkpoint(&self) -> Option<&str> {
        self.next_continue.as_deref()
    }
}

/// Build a `MediaWiki` `list=recentchanges` polling request.
///
/// # Errors
///
/// Returns [`RecentChangesError`] when the query is internally inconsistent.
pub fn build_recent_changes_request(
    config: &WikiConfig,
    query: &RecentChangesQuery,
) -> Result<HttpRequest, RecentChangesError> {
    if query.limit == 0 {
        return Err(RecentChangesError::InvalidRequest {
            message: "limit must be non-zero".to_string(),
        });
    }

    let continue_token = normalize_continue_token(query.rccontinue.as_deref())?;

    let mut url = config.api_url.clone();
    {
        let mut pairs = url.query_pairs_mut();
        pairs
            .append_pair("action", "query")
            .append_pair("format", "json")
            .append_pair("formatversion", "2")
            .append_pair("list", "recentchanges")
            .append_pair(
                "rcprop",
                "title|ids|sizes|flags|user|tags|comment|timestamp|patrolled",
            )
            .append_pair("rclimit", &query.limit.to_string())
            .append_pair("rctype", "edit|new");
        {
            let mut show_flags: Vec<&str> = Vec::new();
            if !query.include_bots {
                show_flags.push("!bot");
            }
            if query.unpatrolled_only {
                show_flags.push("!patrolled");
            }
            if !query.include_minor {
                show_flags.push("!minor");
            }
            if !show_flags.is_empty() {
                pairs.append_pair("rcshow", &show_flags.join("|"));
            }
        }
        {
            let ns_list = query
                .namespace_override
                .as_deref()
                .unwrap_or(&config.namespace_allowlist);
            if !ns_list.is_empty() {
                let namespaces = ns_list
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("|");
                pairs.append_pair("rcnamespace", &namespaces);
            }
        }
        if let Some(continue_token) = continue_token.as_deref() {
            pairs.append_pair("rccontinue", continue_token);
        }
    }

    Ok(HttpRequest {
        method: HttpMethod::Get,
        url,
        headers: BTreeMap::new(),
        body: Vec::new(),
    })
}

/// Execute a recentchanges backlog request through the injected HTTP client.
///
/// # Errors
///
/// Returns [`RecentChangesError`] when request construction fails, the injected
/// client errors, or the HTTP response cannot be parsed into a backlog batch.
pub async fn execute_recent_changes<C>(
    client: &C,
    config: &WikiConfig,
    query: &RecentChangesQuery,
) -> Result<RecentChangesBatch, RecentChangesError>
where
    C: HttpClient + ?Sized,
{
    let request = build_recent_changes_request(config, query)?;
    let response =
        client
            .execute(request)
            .await
            .map_err(|error| RecentChangesError::InvalidResponse {
                message: error.to_string(),
            })?;

    parse_recent_changes_response(config, &response, query)
}

/// Parse a `MediaWiki` recentchanges response into normalized edit events.
///
/// # Errors
///
/// Returns [`RecentChangesError`] when the HTTP status is not successful, the
/// JSON structure is invalid, or the timestamps cannot be parsed.
pub fn parse_recent_changes_response(
    config: &WikiConfig,
    response: &HttpResponse,
    query: &RecentChangesQuery,
) -> Result<RecentChangesBatch, RecentChangesError> {
    if !(200..300).contains(&response.status) {
        return Err(RecentChangesError::InvalidResponse {
            message: format!("unexpected HTTP status {}", response.status),
        });
    }

    let parsed: RecentChangesEnvelope =
        serde_json::from_slice(&response.body).map_err(RecentChangesError::from)?;

    let mut events = Vec::new();
    for change in parsed.query.recentchanges {
        if !matches!(change.change_type.as_str(), "edit" | "new") {
            continue;
        }
        if !query.include_bots && change.bot {
            continue;
        }
        if !query.include_minor && change.minor {
            continue;
        }
        if !config.namespace_allowlist.is_empty()
            && !config.namespace_allowlist.contains(&change.namespace)
        {
            continue;
        }

        events.push(EditEvent {
            wiki_id: config.wiki_id.clone(),
            title: change.title,
            namespace: change.namespace,
            rev_id: change.revid,
            old_rev_id: change.old_revid,
            performer: classify_editor(&change.user),
            timestamp_ms: parse_rfc3339_utc_to_ms(&change.timestamp)?,
            is_bot: change.bot,
            is_minor: change.minor,
            is_new_page: change.change_type == "new" || change.is_new,
            tags: change.tags,
            comment: change.comment.filter(|value| !value.is_empty()),
            byte_delta: compute_byte_delta(change.newlen, change.oldlen),
            is_patrolled: change.patrolled,
        });
    }

    Ok(RecentChangesBatch {
        events,
        next_continue: normalize_response_continue(
            parsed.r#continue.and_then(|value| value.rccontinue),
        ),
    })
}

fn classify_editor(user: &str) -> EditorIdentity {
    if user.starts_with('~') {
        return EditorIdentity::Temporary {
            label: user.to_string(),
        };
    }

    if user.parse::<std::net::IpAddr>().is_ok() {
        return EditorIdentity::Anonymous {
            label: user.to_string(),
        };
    }

    EditorIdentity::Registered {
        username: user.to_string(),
    }
}

fn parse_rfc3339_utc_to_ms(value: &str) -> Result<i64, RecentChangesError> {
    if value.len() < 20 || !value.ends_with('Z') || !has_rfc3339_utc_layout(value) {
        return Err(invalid_timestamp(value));
    }

    let year = value[0..4]
        .parse::<i32>()
        .map_err(|_| invalid_timestamp(value))?;
    let month = value[5..7]
        .parse::<u32>()
        .map_err(|_| invalid_timestamp(value))?;
    let day = value[8..10]
        .parse::<u32>()
        .map_err(|_| invalid_timestamp(value))?;
    let hour = value[11..13]
        .parse::<u32>()
        .map_err(|_| invalid_timestamp(value))?;
    let minute = value[14..16]
        .parse::<u32>()
        .map_err(|_| invalid_timestamp(value))?;
    let second = value[17..19]
        .parse::<u32>()
        .map_err(|_| invalid_timestamp(value))?;

    if !is_valid_utc_date(year, month, day) || hour > 23 || minute > 59 || second > 59 {
        return Err(invalid_timestamp(value));
    }

    let fractional_ms = parse_fractional_millis(&value[19..value.len() - 1])
        .ok_or_else(|| invalid_timestamp(value))?;

    let days = days_from_civil(year, month, day);
    let seconds = days
        .saturating_mul(86_400)
        .saturating_add(i64::from(hour) * 3_600)
        .saturating_add(i64::from(minute) * 60)
        .saturating_add(i64::from(second));

    Ok(seconds.saturating_mul(1_000).saturating_add(fractional_ms))
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

fn invalid_timestamp(value: &str) -> RecentChangesError {
    RecentChangesError::InvalidResponse {
        message: format!("unsupported timestamp format: {value}"),
    }
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

fn parse_fractional_millis(segment: &str) -> Option<i64> {
    let Some(fractional) = segment.strip_prefix('.') else {
        return if segment.is_empty() { Some(0) } else { None };
    };

    if fractional.is_empty() || !fractional.chars().all(|digit| digit.is_ascii_digit()) {
        return None;
    }

    let digits = fractional.chars().take(3).collect::<Vec<_>>();
    let millis = digits.into_iter().fold(0i64, |acc, digit| {
        acc * 10 + i64::from(digit.to_digit(10).unwrap_or(0))
    });
    let scale = 3usize.saturating_sub(fractional.len().min(3));

    Some(millis.saturating_mul(10i64.saturating_pow(u32::try_from(scale).unwrap_or(0))))
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

fn compute_byte_delta(newlen: i32, oldlen: Option<i32>) -> i32 {
    newlen.saturating_sub(oldlen.unwrap_or(0))
}

pub(crate) fn normalize_continue_token(
    token: Option<&str>,
) -> Result<Option<String>, RecentChangesError> {
    match token {
        Some(token) if token.trim().is_empty() => Err(RecentChangesError::InvalidRequest {
            message: "rccontinue must not be empty".to_string(),
        }),
        Some(token) => Ok(Some(token.to_string())),
        None => Ok(None),
    }
}

fn normalize_response_continue(token: Option<String>) -> Option<String> {
    token.filter(|value| !value.trim().is_empty())
}

#[derive(Debug, Deserialize)]
struct RecentChangesEnvelope {
    #[serde(rename = "continue")]
    r#continue: Option<RecentChangesContinue>,
    query: RecentChangesQueryPayload,
}

#[derive(Debug, Deserialize)]
struct RecentChangesContinue {
    rccontinue: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RecentChangesQueryPayload {
    recentchanges: Vec<RecentChangeItem>,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Deserialize)]
struct RecentChangeItem {
    #[serde(rename = "type")]
    change_type: String,
    #[serde(rename = "ns")]
    namespace: i32,
    title: String,
    user: String,
    timestamp: String,
    #[serde(default)]
    bot: bool,
    #[serde(default)]
    minor: bool,
    #[serde(default, rename = "new")]
    is_new: bool,
    revid: u64,
    old_revid: Option<u64>,
    oldlen: Option<i32>,
    newlen: i32,
    comment: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    patrolled: bool,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use futures::executor::block_on;
    use proptest::prelude::*;

    use super::{
        RecentChangesBatch, RecentChangesQuery, build_recent_changes_request,
        execute_recent_changes, parse_recent_changes_response, parse_rfc3339_utc_to_ms,
    };
    use crate::config_parser::parse_wiki_config;
    use crate::traits::StubHttpClient;
    use crate::types::HttpResponse;

    const CONFIG: &str = include_str!("../../../configs/frwiki.yaml");

    #[test]
    fn builds_recentchanges_request() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let request = build_recent_changes_request(
            &config,
            &RecentChangesQuery {
                limit: 25,
                rccontinue: Some("20260324120000|123".to_string()),
                include_bots: false,
                unpatrolled_only: false,
                include_minor: true,
                namespace_override: None,
            },
        )
        .expect("request should build");

        assert!(request.url.as_str().contains("list=recentchanges"));
        assert!(request.url.as_str().contains("rclimit=25"));
        assert!(
            request
                .url
                .as_str()
                .contains("rccontinue=20260324120000%7C123")
        );
        assert!(request.url.as_str().contains("rcshow=%21bot"));
    }

    #[test]
    fn builds_recentchanges_request_with_bots_enabled_without_filter() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let request = build_recent_changes_request(
            &config,
            &RecentChangesQuery {
                limit: 25,
                rccontinue: None,
                include_bots: true,
                unpatrolled_only: false,
                include_minor: true,
                namespace_override: None,
            },
        )
        .expect("request should build");

        assert!(!request.url.as_str().contains("rcshow=%21bot"));
    }

    #[test]
    fn rejects_empty_rccontinue_tokens() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");

        let error = build_recent_changes_request(
            &config,
            &RecentChangesQuery {
                limit: 25,
                rccontinue: Some("   ".to_string()),
                include_bots: false,
                unpatrolled_only: false,
                include_minor: true,
                namespace_override: None,
            },
        )
        .expect_err("empty checkpoint should fail");

        assert!(error.to_string().contains("rccontinue must not be empty"));
    }

    #[test]
    fn recentchanges_helpers_track_initial_and_checkpoint_state() {
        let query = RecentChangesQuery::initial(15, false);
        let batch = RecentChangesBatch {
            events: Vec::new(),
            next_continue: Some("token-1".to_string()),
        };

        assert!(query.is_initial_poll());
        assert_eq!(batch.event_count(), 0);
        assert!(batch.is_empty());
        assert_eq!(batch.checkpoint(), Some("token-1"));
    }

    #[test]
    fn parses_recentchanges_response() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let response = HttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: br#"{
                "continue":{"rccontinue":"next-token"},
                "query":{"recentchanges":[
                    {
                        "type":"edit",
                        "ns":0,
                        "title":"Example",
                        "user":"192.0.2.1",
                        "timestamp":"2026-03-24T15:42:00Z",
                        "bot":false,
                        "minor":true,
                        "revid":123456,
                        "old_revid":123455,
                        "oldlen":100,
                        "newlen":120,
                        "comment":"cleanup",
                        "tags":["mobile edit"]
                    }
                ]}}"#
                .to_vec(),
        };

        let query = RecentChangesQuery::initial(25, false);
        let batch = parse_recent_changes_response(&config, &response, &query)
            .expect("response should parse");

        assert_eq!(batch.events.len(), 1);
        assert_eq!(batch.events[0].rev_id, 123_456);
        assert_eq!(batch.next_continue.as_deref(), Some("next-token"));
    }

    #[test]
    fn normalizes_empty_response_continue_tokens_to_none() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let response = HttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: br#"{
                "continue":{"rccontinue":"   "},
                "query":{"recentchanges":[]}
            }"#
            .to_vec(),
        };

        let query = RecentChangesQuery::initial(25, false);
        let batch = parse_recent_changes_response(&config, &response, &query)
            .expect("response should parse");

        assert!(batch.checkpoint().is_none());
    }

    #[test]
    fn parses_recentchanges_response_and_filters_unwanted_events() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let response = HttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: br#"{
                "query":{"recentchanges":[
                    {
                        "type":"log",
                        "ns":0,
                        "title":"Ignored log event",
                        "user":"Example",
                        "timestamp":"2026-03-24T15:42:00Z",
                        "bot":false,
                        "minor":false,
                        "revid":123450,
                        "old_revid":123449,
                        "oldlen":10,
                        "newlen":11,
                        "comment":"",
                        "tags":[]
                    },
                    {
                        "type":"new",
                        "ns":0,
                        "title":"Example",
                        "user":"192.0.2.1",
                        "timestamp":"2026-03-24T15:42:00Z",
                        "bot":true,
                        "minor":false,
                        "revid":123456,
                        "old_revid":123455,
                        "oldlen":100,
                        "newlen":120,
                        "comment":"",
                        "tags":["mw-blank"]
                    }
                ]}}"#
                .to_vec(),
        };

        let query = RecentChangesQuery::initial(25, false);
        let batch = parse_recent_changes_response(&config, &response, &query)
            .expect("response should parse");

        assert!(batch.events.is_empty());
        assert!(batch.is_empty());
    }

    #[test]
    fn parses_rfc3339_timestamp() {
        let timestamp =
            parse_rfc3339_utc_to_ms("2026-03-24T15:42:00Z").expect("timestamp should parse");

        assert_eq!(timestamp, 1_774_366_920_000);
    }

    #[test]
    fn parses_fractional_rfc3339_timestamp() {
        let timestamp =
            parse_rfc3339_utc_to_ms("2026-03-24T15:42:00.125Z").expect("timestamp should parse");

        assert_eq!(timestamp, 1_774_366_920_125);
    }

    #[test]
    fn rejects_impossible_rfc3339_calendar_dates() {
        let error = parse_rfc3339_utc_to_ms("2026-02-30T15:42:00Z")
            .expect_err("impossible date should fail");

        assert!(error.to_string().contains("unsupported timestamp format"));
    }

    #[test]
    fn rejects_non_success_http_status() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let response = HttpResponse {
            status: 500,
            headers: BTreeMap::new(),
            body: br#"{"error":"boom"}"#.to_vec(),
        };

        let query = RecentChangesQuery::initial(25, false);
        let error = parse_recent_changes_response(&config, &response, &query)
            .expect_err("response should fail");

        assert!(error.to_string().contains("unexpected HTTP status 500"));
    }

    #[test]
    fn executes_recentchanges_request_through_http_trait() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let client = StubHttpClient::new([Ok(HttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: br#"{"query":{"recentchanges":[]}}"#.to_vec(),
        })]);

        let batch = block_on(execute_recent_changes(
            &client,
            &config,
            &RecentChangesQuery {
                limit: 10,
                rccontinue: None,
                include_bots: false,
                unpatrolled_only: false,
                include_minor: true,
                namespace_override: None,
            },
        ))
        .expect("execution should succeed");

        assert!(batch.events.is_empty());
        assert!(batch.checkpoint().is_none());
    }

    #[test]
    fn rejects_invalid_timestamp_in_response() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let response = HttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: br#"{
                "query":{"recentchanges":[
                    {
                        "type":"edit",
                        "ns":0,
                        "title":"Example",
                        "user":"192.0.2.1",
                        "timestamp":"invalid",
                        "bot":false,
                        "minor":false,
                        "revid":123456,
                        "old_revid":123455,
                        "oldlen":100,
                        "newlen":120,
                        "comment":"cleanup",
                        "tags":[]
                    }
                ]}}"#
                .to_vec(),
        };

        let query = RecentChangesQuery::initial(25, false);
        let error = parse_recent_changes_response(&config, &response, &query)
            .expect_err("timestamp should fail");

        assert!(error.to_string().contains("unsupported timestamp format"));
    }

    #[test]
    fn parses_temporary_users_and_keeps_bots_when_requested() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let response = HttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: br#"{
                "query":{"recentchanges":[
                    {
                        "type":"edit",
                        "ns":0,
                        "title":"Example",
                        "user":"~2026-17",
                        "timestamp":"2026-03-24T15:42:00.500Z",
                        "bot":true,
                        "minor":false,
                        "revid":123456,
                        "old_revid":123455,
                        "oldlen":100,
                        "newlen":120,
                        "comment":"",
                        "tags":[]
                    }
                ]}}"#
                .to_vec(),
        };

        let query = RecentChangesQuery::initial(25, true);
        let batch = parse_recent_changes_response(&config, &response, &query)
            .expect("response should parse");

        assert_eq!(batch.events.len(), 1);
        assert!(batch.events[0].is_bot);
        assert!(matches!(
            batch.events[0].performer,
            crate::types::EditorIdentity::Temporary { .. }
        ));
        assert_eq!(batch.events[0].timestamp_ms, 1_774_366_920_500);
    }

    #[test]
    fn saturates_extreme_byte_delta_in_response() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let response = HttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: format!(
                r#"{{
                    "query":{{"recentchanges":[
                        {{
                            "type":"edit",
                            "ns":0,
                            "title":"Example",
                            "user":"ExampleUser",
                            "timestamp":"2026-03-24T15:42:00Z",
                            "bot":false,
                            "minor":false,
                            "revid":123456,
                            "old_revid":123455,
                            "oldlen":{},
                            "newlen":{},
                            "comment":"cleanup",
                            "tags":[]
                        }}
                    ]}}
                }}"#,
                i32::MIN,
                i32::MAX
            )
            .into_bytes(),
        };

        let query = RecentChangesQuery::initial(25, false);
        let batch = parse_recent_changes_response(&config, &response, &query)
            .expect("response should parse");

        assert_eq!(batch.events[0].byte_delta, i32::MAX);
    }

    proptest! {
        #[test]
        fn property_fractional_recentchanges_timestamps_truncate_to_millis(millis in 0u16..1000) {
            let timestamp = format!("2026-03-24T15:42:00.{millis:03}987Z");
            let parsed = parse_rfc3339_utc_to_ms(&timestamp).expect("timestamp should parse");

            prop_assert_eq!(parsed, 1_774_366_920_000 + i64::from(millis));
        }
    }
}
