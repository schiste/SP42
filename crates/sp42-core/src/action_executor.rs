//! `MediaWiki` action request building and execution.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;
use url::Url;
use url::form_urlencoded::Serializer;

use crate::action_contracts::{
    ActionResponseSummary, PatrolRequest, RollbackRequest, TokenKind, UndoRequest,
    WikiPageSaveRequest, is_retryable_action_api_error,
};
use crate::errors::ActionError;
use crate::traits::HttpClient;
use crate::types::{HttpMethod, HttpRequest, HttpResponse, WikiConfig};

fn action_error(message: impl Into<String>) -> ActionError {
    ActionError::Execution {
        message: message.into(),
        code: None,
        http_status: None,
        retryable: false,
    }
}

fn api_action_error(
    message: impl Into<String>,
    code: Option<String>,
    http_status: Option<u16>,
    retryable: bool,
) -> ActionError {
    ActionError::Execution {
        message: message.into(),
        code,
        http_status,
        retryable,
    }
}

/// Build a `MediaWiki` rollback API request.
///
/// # Errors
///
/// Returns [`ActionError`] when required request fields are empty.
pub fn build_rollback_request(
    config: &WikiConfig,
    request: &RollbackRequest,
) -> Result<HttpRequest, ActionError> {
    if request.title.trim().is_empty() {
        return Err(action_error("title is required"));
    }

    if request.user.trim().is_empty() {
        return Err(action_error("user is required"));
    }

    if request.token.trim().is_empty() {
        return Err(action_error("token is required"));
    }

    Ok(HttpRequest {
        method: HttpMethod::Post,
        url: config.api_url.clone(),
        headers: default_form_headers(),
        body: encode_form(&[
            ("action", "rollback"),
            ("format", "json"),
            ("formatversion", "2"),
            ("title", request.title.as_str()),
            ("user", request.user.as_str()),
            ("token", request.token.as_str()),
            ("summary", request.summary.as_deref().unwrap_or("")),
        ]),
    })
}

/// Build a `MediaWiki` patrol API request.
///
/// # Errors
///
/// Returns [`ActionError`] when required request fields are empty or invalid.
pub fn build_patrol_request(
    config: &WikiConfig,
    request: &PatrolRequest,
) -> Result<HttpRequest, ActionError> {
    if request.rev_id == 0 {
        return Err(action_error("rev_id must be non-zero"));
    }

    if request.token.trim().is_empty() {
        return Err(action_error("token is required"));
    }

    Ok(HttpRequest {
        method: HttpMethod::Post,
        url: config.api_url.clone(),
        headers: default_form_headers(),
        body: encode_form(&[
            ("action", "patrol"),
            ("format", "json"),
            ("formatversion", "2"),
            ("revid", &request.rev_id.to_string()),
            ("token", request.token.as_str()),
        ]),
    })
}

/// Build a `MediaWiki` token query request.
#[must_use]
pub fn build_token_request(config: &WikiConfig, token_kind: TokenKind) -> HttpRequest {
    HttpRequest {
        method: HttpMethod::Get,
        url: build_query_url(
            &config.api_url,
            &[
                ("action", "query"),
                ("meta", "tokens"),
                ("type", token_kind.api_value()),
                ("format", "json"),
                ("formatversion", "2"),
            ],
        ),
        headers: BTreeMap::new(),
        body: Vec::new(),
    }
}

/// Build a `MediaWiki` undo request using the edit API.
///
/// # Errors
///
/// Returns [`ActionError`] when required request fields are empty or invalid.
pub fn build_undo_request(
    config: &WikiConfig,
    request: &UndoRequest,
) -> Result<HttpRequest, ActionError> {
    if request.title.trim().is_empty() {
        return Err(action_error("title is required"));
    }

    if request.undo_rev_id == 0 {
        return Err(action_error("undo_rev_id must be non-zero"));
    }

    if request.undo_after_rev_id == 0 {
        return Err(action_error("undo_after_rev_id must be non-zero"));
    }

    if request.undo_rev_id <= request.undo_after_rev_id {
        return Err(action_error(
            "undo_rev_id must be newer than undo_after_rev_id",
        ));
    }

    if request.token.trim().is_empty() {
        return Err(action_error("token is required"));
    }

    Ok(HttpRequest {
        method: HttpMethod::Post,
        url: config.api_url.clone(),
        headers: default_form_headers(),
        body: encode_form(&[
            ("action", "edit"),
            ("format", "json"),
            ("formatversion", "2"),
            ("title", request.title.as_str()),
            ("undo", &request.undo_rev_id.to_string()),
            ("undoafter", &request.undo_after_rev_id.to_string()),
            ("token", request.token.as_str()),
            ("summary", request.summary.as_deref().unwrap_or("")),
        ]),
    })
}

/// Build a `MediaWiki` edit request for saving a full page payload.
///
/// # Errors
///
/// Returns [`ActionError`] when required request fields are empty.
pub fn build_wiki_page_save_request(
    config: &WikiConfig,
    request: &WikiPageSaveRequest,
) -> Result<HttpRequest, ActionError> {
    if request.title.trim().is_empty() {
        return Err(action_error("title is required"));
    }

    if request.text.trim().is_empty() {
        return Err(action_error("text is required"));
    }

    if request.token.trim().is_empty() {
        return Err(action_error("token is required"));
    }

    let baserevid = request
        .baserevid
        .map(|value| value.to_string())
        .unwrap_or_default();
    let tags = if request.tags.is_empty() {
        String::new()
    } else {
        request.tags.join("|")
    };

    Ok(HttpRequest {
        method: HttpMethod::Post,
        url: config.api_url.clone(),
        headers: default_form_headers(),
        body: encode_form(&[
            ("action", "edit"),
            ("format", "json"),
            ("formatversion", "2"),
            ("title", request.title.as_str()),
            ("text", request.text.as_str()),
            ("token", request.token.as_str()),
            ("summary", request.summary.as_deref().unwrap_or("")),
            ("baserevid", baserevid.as_str()),
            ("tags", tags.as_str()),
            ("watchlist", request.watchlist.as_deref().unwrap_or("")),
            ("createonly", bool_flag(request.create_only.is_enabled())),
            ("minor", bool_flag(request.minor.is_enabled())),
        ]),
    })
}

/// Execute a `MediaWiki` rollback request with an injected HTTP client.
///
/// # Errors
///
/// Returns [`ActionError`] when request construction fails or the injected
/// client returns an error.
pub async fn execute_rollback<C>(
    client: &C,
    config: &WikiConfig,
    request: &RollbackRequest,
) -> Result<HttpResponse, ActionError>
where
    C: HttpClient + ?Sized,
{
    let http_request = build_rollback_request(config, request)?;
    let response = execute_request(client, http_request, "rollback").await?;
    validate_action_response(&response, "rollback")?;
    Ok(response)
}

/// Execute a `MediaWiki` patrol request with an injected HTTP client.
///
/// # Errors
///
/// Returns [`ActionError`] when request construction fails or the injected
/// client returns an error.
pub async fn execute_patrol<C>(
    client: &C,
    config: &WikiConfig,
    request: &PatrolRequest,
) -> Result<HttpResponse, ActionError>
where
    C: HttpClient + ?Sized,
{
    let http_request = build_patrol_request(config, request)?;
    let response = execute_request(client, http_request, "patrol").await?;
    validate_action_response(&response, "patrol")?;
    Ok(response)
}

/// Execute a `MediaWiki` token query and extract the requested token.
///
/// # Errors
///
/// Returns [`ActionError`] when the injected client fails, the response status
/// is not successful, or the JSON body does not contain the requested token.
pub async fn execute_fetch_token<C>(
    client: &C,
    config: &WikiConfig,
    token_kind: TokenKind,
) -> Result<String, ActionError>
where
    C: HttpClient + ?Sized,
{
    let response = execute_request(
        client,
        build_token_request(config, token_kind),
        "token query",
    )
    .await?;
    parse_token_response(token_kind, &response)
}

/// Execute a `MediaWiki` undo request with an injected HTTP client.
///
/// # Errors
///
/// Returns [`ActionError`] when request construction fails or the injected
/// client returns an error.
pub async fn execute_undo<C>(
    client: &C,
    config: &WikiConfig,
    request: &UndoRequest,
) -> Result<HttpResponse, ActionError>
where
    C: HttpClient + ?Sized,
{
    let http_request = build_undo_request(config, request)?;
    let response = execute_request(client, http_request, "undo").await?;
    validate_action_response(&response, "undo")?;
    Ok(response)
}

/// Execute a `MediaWiki` page save request with an injected HTTP client.
///
/// # Errors
///
/// Returns [`ActionError`] when request construction fails or the injected
/// client returns an error.
pub async fn execute_wiki_page_save<C>(
    client: &C,
    config: &WikiConfig,
    request: &WikiPageSaveRequest,
) -> Result<HttpResponse, ActionError>
where
    C: HttpClient + ?Sized,
{
    let http_request = build_wiki_page_save_request(config, request)?;
    let response = execute_request(client, http_request, "page save").await?;
    validate_action_response(&response, "page save")?;
    Ok(response)
}

/// Parse a token from a `MediaWiki` query response.
///
/// # Errors
///
/// Returns [`ActionError`] when the response body is not valid JSON or does not
/// contain the expected token field.
pub fn parse_token_response(
    token_kind: TokenKind,
    response: &HttpResponse,
) -> Result<String, ActionError> {
    let parsed: TokenResponse = serde_json::from_slice(&response.body)
        .map_err(|error| action_error(format!("token response is not valid JSON: {error}")))?;

    let token = match token_kind {
        TokenKind::Rollback => parsed.query.tokens.rollbacktoken,
        TokenKind::Patrol => parsed.query.tokens.patroltoken,
        TokenKind::Csrf => parsed.query.tokens.csrftoken,
    };

    let token = token.ok_or_else(|| {
        action_error(format!(
            "token response does not contain `{}`",
            token_field_name(token_kind)
        ))
    })?;

    if token.trim().is_empty() {
        return Err(action_error(format!(
            "token field `{}` is empty",
            token_field_name(token_kind)
        )));
    }

    Ok(token)
}

fn default_form_headers() -> BTreeMap<String, String> {
    BTreeMap::from([(
        "content-type".to_string(),
        "application/x-www-form-urlencoded".to_string(),
    )])
}

fn build_query_url(base_url: &Url, params: &[(&str, &str)]) -> Url {
    let mut url = base_url.clone();
    url.query_pairs_mut()
        .clear()
        .extend_pairs(params.iter().copied());
    url
}

fn encode_form(fields: &[(&str, &str)]) -> Vec<u8> {
    let mut serializer = Serializer::new(String::new());
    for (key, value) in fields {
        if !value.is_empty() {
            serializer.append_pair(key, value);
        }
    }

    serializer.finish().into_bytes()
}

const fn bool_flag(enabled: bool) -> &'static str {
    if enabled { "1" } else { "" }
}

async fn execute_request<C>(
    client: &C,
    request: HttpRequest,
    action_label: &str,
) -> Result<HttpResponse, ActionError>
where
    C: HttpClient + ?Sized,
{
    let response = client
        .execute(request)
        .await
        .map_err(|error| api_action_error(error.to_string(), None, None, true))?;

    if !(200..300).contains(&response.status) {
        return Err(api_action_error(
            format!(
                "{action_label} failed with HTTP {}: {}",
                response.status,
                summarize_response_body(&response.body)
            ),
            None,
            Some(response.status),
            response.status >= 500 || response.status == 429,
        ));
    }

    Ok(response)
}

fn validate_action_response(
    response: &HttpResponse,
    action_label: &str,
) -> Result<(), ActionError> {
    let summary = parse_action_response_summary(response, action_label)?;

    if let Some(error) = summary.error {
        return Err(api_action_error(
            error,
            summary.api_code,
            Some(summary.status),
            summary.retryable,
        ));
    }

    Ok(())
}

/// Parse and summarize a `MediaWiki` action response body.
///
/// # Errors
///
/// Returns [`ActionError`] when the response body is not valid JSON or when the
/// body contains an API-level error payload.
pub fn parse_action_response_summary(
    response: &HttpResponse,
    action_label: &str,
) -> Result<ActionResponseSummary, ActionError> {
    let parsed: MediaWikiActionResponse =
        serde_json::from_slice(&response.body).map_err(|error| {
            action_error(format!(
                "{action_label} response is not valid JSON: {error}"
            ))
        })?;

    let result = parsed.result();
    let warnings = parsed
        .warnings
        .as_ref()
        .map(parse_warning_lines)
        .unwrap_or_default();
    let retryable = parsed
        .error
        .as_ref()
        .is_some_and(|error| is_retryable_action_api_error(error.code.as_str()));
    let api_code = parsed.error.as_ref().map(|error| error.code.clone());
    let error = parsed.error.map(|error| {
        let MediaWikiApiError {
            code,
            info,
            details,
        } = error;
        let mut message = match info {
            Some(info) if !info.trim().is_empty() => {
                format!("{action_label} failed with API error `{code}`: {info}")
            }
            _ => format!("{action_label} failed with API error `{code}`"),
        };

        if let Some(details) = details
            && !details.trim().is_empty()
        {
            message.push_str(" (");
            message.push_str(&details);
            message.push(')');
        }

        message
    });

    let nochange = parsed
        .edit
        .as_ref()
        .and_then(|value| value.get("nochange"))
        .is_some();

    Ok(ActionResponseSummary {
        status: response.status,
        warnings,
        result,
        error,
        api_code,
        retryable,
        nochange,
    })
}

fn summarize_response_body(body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body);
    let trimmed = text.trim();

    if trimmed.is_empty() {
        return "empty response body".to_string();
    }

    let summary = trimmed.chars().take(120).collect::<String>();
    if trimmed.chars().count() > 120 {
        format!("{summary}...")
    } else {
        summary
    }
}

fn parse_warning_lines(value: &Value) -> Vec<String> {
    match value {
        Value::String(line) => vec![line.clone()],
        Value::Array(values) => values
            .iter()
            .filter_map(Value::as_str)
            .map(ToString::to_string)
            .collect(),
        Value::Object(map) => map.values().flat_map(parse_warning_lines).collect(),
        _ => vec![value.to_string()],
    }
}

const fn token_field_name(token_kind: TokenKind) -> &'static str {
    match token_kind {
        TokenKind::Rollback => "rollbacktoken",
        TokenKind::Patrol => "patroltoken",
        TokenKind::Csrf => "csrftoken",
    }
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    query: TokenQuery,
}

#[derive(Debug, Deserialize)]
struct MediaWikiActionResponse {
    #[serde(default)]
    warnings: Option<Value>,
    #[serde(default)]
    error: Option<MediaWikiApiError>,
    #[serde(default)]
    rollback: Option<Value>,
    #[serde(default)]
    patrol: Option<Value>,
    #[serde(default)]
    edit: Option<Value>,
}

impl MediaWikiActionResponse {
    fn result(&self) -> Option<String> {
        if let Some(rollback) = &self.rollback {
            return Some(format!("rollback={rollback}"));
        }
        if let Some(patrol) = &self.patrol {
            return Some(format!("patrol={patrol}"));
        }
        if let Some(edit) = &self.edit {
            return Some(format!("edit={edit}"));
        }
        None
    }
}

#[derive(Debug, Deserialize)]
struct MediaWikiApiError {
    code: String,
    #[serde(default)]
    info: Option<String>,
    #[serde(default)]
    details: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenQuery {
    tokens: TokenPayload,
}

#[derive(Debug, Deserialize)]
struct TokenPayload {
    rollbacktoken: Option<String>,
    patroltoken: Option<String>,
    csrftoken: Option<String>,
}

/// Replace exactly one occurrence of `needle` within `haystack`.
///
/// ADR-0003 Decision 4: a literal-span content edit must never guess between
/// multiple matches, so zero matches and multiple matches both refuse instead
/// of silently editing the wrong span.
///
/// # Errors
///
/// Returns [`ActionError::Execution`] with code `invalid-input` when `needle`
/// is empty, `text-not-found` when it does not occur in `haystack`, and
/// `text-ambiguous` when it occurs more than once.
pub fn replace_exactly_once(
    haystack: &str,
    needle: &str,
    replacement: &str,
) -> Result<String, ActionError> {
    if needle.is_empty() {
        return Err(ActionError::Execution {
            message: "replacement target text must not be empty".to_string(),
            code: Some("invalid-input".to_string()),
            http_status: None,
            retryable: false,
        });
    }
    match haystack.matches(needle).count() {
        0 => Err(ActionError::Execution {
            message: "selected text not found in page content".to_string(),
            code: Some("text-not-found".to_string()),
            http_status: None,
            retryable: false,
        }),
        1 => Ok(haystack.replacen(needle, replacement, 1)),
        occurrences => Err(ActionError::Execution {
            message: format!(
                "selected text occurs {occurrences} times in page content; refusing ambiguous replacement"
            ),
            code: Some("text-ambiguous".to_string()),
            http_status: None,
            retryable: false,
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use futures::executor::block_on;

    use super::{
        build_patrol_request, build_rollback_request, build_token_request, build_undo_request,
        build_wiki_page_save_request, execute_fetch_token, execute_patrol, execute_rollback,
        execute_undo, execute_wiki_page_save, parse_action_response_summary, parse_token_response,
        replace_exactly_once,
    };
    use crate::action_contracts::{
        PatrolRequest, RollbackRequest, TokenKind, UndoRequest, WikiPageSaveRequest,
    };
    use crate::errors::ActionError;
    use crate::test_fixtures::fixture_wiki_config;
    use crate::traits::StubHttpClient;
    use crate::types::{FlagState, HttpMethod, HttpResponse};

    #[test]
    fn builds_rollback_request_body() {
        let config = fixture_wiki_config();
        let request = build_rollback_request(
            &config,
            &RollbackRequest {
                title: "Example".to_string(),
                user: "ExampleUser".to_string(),
                token: "rollback-token".to_string(),
                summary: Some("rollback spam".to_string()),
            },
        )
        .expect("rollback request should build");

        let body = String::from_utf8(request.body).expect("body should be utf-8");

        assert_eq!(request.method, HttpMethod::Post);
        assert!(body.contains("action=rollback"));
        assert!(body.contains("title=Example"));
        assert!(body.contains("user=ExampleUser"));
    }

    #[test]
    fn builds_patrol_request_body() {
        let config = fixture_wiki_config();
        let request = build_patrol_request(
            &config,
            &PatrolRequest {
                rev_id: 123_456,
                token: "patrol-token".to_string(),
            },
        )
        .expect("patrol request should build");

        let body = String::from_utf8(request.body).expect("body should be utf-8");

        assert_eq!(request.method, HttpMethod::Post);
        assert!(body.contains("action=patrol"));
        assert!(body.contains("revid=123456"));
    }

    #[test]
    fn builds_token_query_request() {
        let config = fixture_wiki_config();
        let request = build_token_request(&config, TokenKind::Rollback);

        assert_eq!(request.method, HttpMethod::Get);
        assert!(request.url.as_str().contains("action=query"));
        assert!(request.url.as_str().contains("meta=tokens"));
        assert!(request.url.as_str().contains("type=rollback"));
    }

    #[test]
    fn builds_undo_request_body() {
        let config = fixture_wiki_config();
        let request = build_undo_request(
            &config,
            &UndoRequest {
                title: "Example".to_string(),
                undo_rev_id: 123_456,
                undo_after_rev_id: 123_455,
                token: "csrf-token".to_string(),
                summary: Some("undo vandalism".to_string()),
            },
        )
        .expect("undo request should build");

        let body = String::from_utf8(request.body).expect("body should be utf-8");

        assert_eq!(request.method, HttpMethod::Post);
        assert!(body.contains("action=edit"));
        assert!(body.contains("undo=123456"));
        assert!(body.contains("undoafter=123455"));
    }

    #[test]
    fn builds_wiki_page_save_request_body() {
        let config = fixture_wiki_config();
        let request = build_wiki_page_save_request(
            &config,
            &WikiPageSaveRequest {
                title: "User:Schiste/SP42/Profile".to_string(),
                text: "content".to_string(),
                token: "csrf-token".to_string(),
                summary: Some("update profile".to_string()),
                baserevid: Some(123),
                tags: vec!["sp42".to_string(), "manual".to_string()],
                watchlist: Some("nochange".to_string()),
                create_only: FlagState::Disabled,
                minor: FlagState::Enabled,
            },
        )
        .expect("page save request should build");

        let body = String::from_utf8(request.body).expect("body should be utf-8");

        assert_eq!(request.method, HttpMethod::Post);
        assert!(body.contains("action=edit"));
        assert!(body.contains("title=User%3ASchiste%2FSP42%2FProfile"));
        assert!(body.contains("baserevid=123"));
        assert!(body.contains("tags=sp42%7Cmanual"));
        assert!(body.contains("minor=1"));
    }

    #[test]
    fn parses_patrol_token_response() {
        let response = HttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: br#"{"query":{"tokens":{"patroltoken":"patrol-token+\\"}}}"#.to_vec(),
        };

        let token = parse_token_response(TokenKind::Patrol, &response).expect("token should parse");

        assert_eq!(token, "patrol-token+\\");
    }

    #[test]
    fn executes_rollback_through_http_trait() {
        let config = fixture_wiki_config();
        let client = StubHttpClient::new([Ok(HttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: br#"{"rollback":true}"#.to_vec(),
        })]);

        let response = block_on(execute_rollback(
            &client,
            &config,
            &RollbackRequest {
                title: "Example".to_string(),
                user: "ExampleUser".to_string(),
                token: "rollback-token".to_string(),
                summary: None,
            },
        ))
        .expect("rollback execution should succeed");

        assert_eq!(response.status, 200);
    }

    #[test]
    fn executes_patrol_through_http_trait() {
        let config = fixture_wiki_config();
        let client = StubHttpClient::new([Ok(HttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: br#"{"patrol":true}"#.to_vec(),
        })]);

        let response = block_on(execute_patrol(
            &client,
            &config,
            &PatrolRequest {
                rev_id: 123_456,
                token: "patrol-token".to_string(),
            },
        ))
        .expect("patrol execution should succeed");

        assert_eq!(response.status, 200);
    }

    #[test]
    fn fetches_token_through_http_trait() {
        let config = fixture_wiki_config();
        let client = StubHttpClient::new([Ok(HttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: br#"{"query":{"tokens":{"rollbacktoken":"rollback-token+\\"}}}"#.to_vec(),
        })]);

        let token = block_on(execute_fetch_token(&client, &config, TokenKind::Rollback))
            .expect("token fetch should succeed");

        assert_eq!(token, "rollback-token+\\");
    }

    #[test]
    fn executes_undo_through_http_trait() {
        let config = fixture_wiki_config();
        let client = StubHttpClient::new([Ok(HttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: br#"{"edit":{"result":"Success"}}"#.to_vec(),
        })]);

        let response = block_on(execute_undo(
            &client,
            &config,
            &UndoRequest {
                title: "Example".to_string(),
                undo_rev_id: 123_456,
                undo_after_rev_id: 123_455,
                token: "csrf-token".to_string(),
                summary: None,
            },
        ))
        .expect("undo execution should succeed");

        assert_eq!(response.status, 200);
    }

    #[test]
    fn executes_page_save_through_http_trait() {
        let config = fixture_wiki_config();
        let client = StubHttpClient::new([Ok(HttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: br#"{"edit":{"result":"Success","newrevid":321}}"#.to_vec(),
        })]);

        let response = block_on(execute_wiki_page_save(
            &client,
            &config,
            &WikiPageSaveRequest {
                title: "User:Schiste/SP42/Profile".to_string(),
                text: "content".to_string(),
                token: "csrf-token".to_string(),
                summary: Some("save".to_string()),
                baserevid: None,
                tags: vec!["sp42".to_string()],
                watchlist: None,
                create_only: FlagState::Enabled,
                minor: FlagState::Disabled,
            },
        ))
        .expect("page save execution should succeed");

        assert_eq!(response.status, 200);
    }

    #[test]
    fn rejects_non_success_http_status() {
        let config = fixture_wiki_config();
        let client = StubHttpClient::new([Ok(HttpResponse {
            status: 429,
            headers: BTreeMap::new(),
            body: br#"{"error":"rate limited"}"#.to_vec(),
        })]);

        let error = block_on(execute_patrol(
            &client,
            &config,
            &PatrolRequest {
                rev_id: 123_456,
                token: "patrol-token".to_string(),
            },
        ))
        .expect_err("non-success status should fail");

        assert!(error.to_string().contains("HTTP 429"));
    }

    #[test]
    fn parses_action_response_summary_for_success_payload() {
        let response = HttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: br#"{"edit":{"result":"Success","newrevid":123456}}"#.to_vec(),
        };

        let summary =
            parse_action_response_summary(&response, "undo").expect("summary should parse");

        assert_eq!(summary.status, 200);
        assert!(summary.warnings.is_empty());
        assert!(summary.error.is_none());
        assert!(!summary.retryable);
        assert!(
            summary
                .result
                .as_deref()
                .expect("result should exist")
                .contains(r#""result":"Success""#)
        );
    }

    #[test]
    fn parses_action_response_summary_for_api_error_payload() {
        let response = HttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: br#"{"error":{"code":"badtoken","info":"Invalid CSRF token","details":"token expired"}}"#
                .to_vec(),
        };

        let summary =
            parse_action_response_summary(&response, "undo").expect("summary should parse");

        assert!(
            summary
                .error
                .as_deref()
                .expect("error should be present")
                .contains("badtoken")
        );
        assert!(
            summary
                .error
                .as_deref()
                .expect("error should be present")
                .contains("Invalid CSRF token")
        );
        assert_eq!(summary.api_code.as_deref(), Some("badtoken"));
        assert!(!summary.retryable);
    }

    #[test]
    fn marks_retryable_api_errors() {
        let response = HttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: br#"{"error":{"code":"maxlag","info":"Waiting for replicas"}}"#.to_vec(),
        };

        let summary =
            parse_action_response_summary(&response, "undo").expect("summary should parse");

        assert_eq!(summary.api_code.as_deref(), Some("maxlag"));
        assert!(summary.retryable);
    }

    #[test]
    fn rejects_action_response_with_api_error_even_on_2xx() {
        let config = fixture_wiki_config();
        let client = StubHttpClient::new([Ok(HttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: br#"{"error":{"code":"badtoken","info":"Invalid CSRF token"}}"#.to_vec(),
        })]);

        let error = block_on(execute_undo(
            &client,
            &config,
            &UndoRequest {
                title: "Example".to_string(),
                undo_rev_id: 123_456,
                undo_after_rev_id: 123_455,
                token: "csrf-token".to_string(),
                summary: None,
            },
        ))
        .expect_err("api error should fail");

        assert!(error.to_string().contains("badtoken"));
        assert!(error.to_string().contains("Invalid CSRF token"));
    }

    #[test]
    fn parse_action_response_detects_nochange() {
        let response = HttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: br#"{"edit":{"result":"Success","nochange":""}}"#.to_vec(),
        };

        let summary =
            parse_action_response_summary(&response, "undo").expect("summary should parse");

        assert!(summary.nochange);
        assert!(summary.error.is_none());
    }

    #[test]
    fn parse_action_response_normal_edit_is_not_nochange() {
        let response = HttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: br#"{"edit":{"result":"Success","newrevid":321}}"#.to_vec(),
        };

        let summary =
            parse_action_response_summary(&response, "undo").expect("summary should parse");

        assert!(!summary.nochange);
    }

    #[test]
    fn replace_exactly_once_replaces_single_occurrence() {
        let result = replace_exactly_once("alpha beta gamma", "beta", "BETA")
            .expect("single occurrence should replace");
        assert_eq!(result, "alpha BETA gamma");
    }

    #[test]
    fn replace_exactly_once_rejects_missing_needle() {
        let error = replace_exactly_once("alpha beta", "delta", "DELTA")
            .expect_err("missing needle should refuse");
        let ActionError::Execution { code, retryable, .. } = error;
        assert_eq!(code.as_deref(), Some("text-not-found"));
        assert!(!retryable);
    }

    #[test]
    fn replace_exactly_once_rejects_ambiguous_needle() {
        let error = replace_exactly_once("ref one ref two", "ref", "REF")
            .expect_err("ambiguous needle should refuse");
        let ActionError::Execution { code, message, .. } = error;
        assert_eq!(code.as_deref(), Some("text-ambiguous"));
        assert!(message.contains("2 times"), "message should report the count: {message}");
    }

    #[test]
    fn replace_exactly_once_rejects_empty_needle() {
        let error = replace_exactly_once("alpha", "", "X")
            .expect_err("empty needle should refuse");
        let ActionError::Execution { code, .. } = error;
        assert_eq!(code.as_deref(), Some("invalid-input"));
    }
}
