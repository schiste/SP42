use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use tracing::info;

use sp42_core::{
    ActionError, ActionExecutionHistoryReport, ActionExecutionLogEntry,
    ActionExecutionStatusReport, ActionResponseSummary, DevAuthCapabilityReport, FlagState,
    PatrolRequest, RollbackRequest, SessionActionExecutionRequest, SessionActionExecutionResponse,
    SessionActionKind, TokenKind, UndoRequest, WikiPageSaveRequest, WikitextEditOutcome,
    WikitextEditor, WikitextEditorError, WikitextNodeLocator, WikitextPageRef, execute_fetch_token,
    execute_patrol, execute_rollback, execute_undo, execute_wiki_page_save,
    parse_action_response_summary, replace_exactly_once,
};
use sp42_types::HttpResponse;

use crate::http_errors::{forbidden_error, invalid_payload, unauthorized_error};
use crate::runtime_adapters::BearerHttpClient;
use crate::session_runtime::{
    current_session_snapshot, prune_expired_sessions, validate_csrf_header,
};
use crate::state::{AppState, SessionSnapshot};
use crate::{capability_report_for_session, config_for_state_wiki, storage_routes};

const ACTION_HISTORY_LIMIT: usize = 50;
const RESPONSE_BODY_PREVIEW_LIMIT: usize = 1_000;

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub(crate) struct ActionHistoryQuery {
    limit: Option<usize>,
}

pub(crate) async fn get_action_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Json<ActionExecutionStatusReport> {
    Json(action_status_report(&state, &headers).await)
}

pub(crate) async fn get_action_history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ActionHistoryQuery>,
) -> Json<ActionExecutionHistoryReport> {
    Json(action_history_report(&state, &headers, query.limit).await)
}

pub(crate) async fn post_execute_action(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<SessionActionExecutionRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let Some(session) = current_session_snapshot(&state, &headers, true).await else {
        return Err(unauthorized_error(
            "No authenticated bridge session is active.",
        ));
    };
    validate_csrf_header(&headers, &session)?;

    let capabilities =
        capability_report_for_session(&state, &session, &payload.wiki_id, false).await;
    validate_action_request(&payload, &capabilities)?;
    if matches!(payload.kind, SessionActionKind::Undo) && payload.undo_after_rev_id.is_none() {
        return Err(invalid_payload("undo_after_rev_id is required for undo"));
    }
    let config = config_for_state_wiki(&state, &payload.wiki_id)?;
    let client = BearerHttpClient::new(state.http_client.clone(), session.access_token.clone());
    let executed_at_ms = state.clock.now_ms();
    info!(
        session_id = session.session_id.as_str(),
        wiki_id = payload.wiki_id.as_str(),
        rev_id = payload.rev_id,
        kind = ?payload.kind,
        "executing session action"
    );
    let outcome =
        execute_session_action(&client, &config, &payload, state.wikitext_editor.as_ref()).await;

    match outcome {
        Ok(response) => {
            let result = handle_action_success(
                &state,
                &session,
                &headers,
                &payload,
                executed_at_ms,
                response,
            )
            .await?;
            Ok(result)
        }
        Err(error) => {
            Err(
                handle_action_failure(&state, &session, &headers, &payload, executed_at_ms, error)
                    .await,
            )
        }
    }
}

fn action_response_payload(
    payload: &SessionActionExecutionRequest,
    actor: String,
    response: &HttpResponse,
    summary: &ActionResponseSummary,
    response_preview: &str,
) -> SessionActionExecutionResponse {
    let mut warnings = summary.warnings.clone();
    if summary.nochange {
        warnings.push("no change — the edit may have already been reverted".to_string());
    }
    SessionActionExecutionResponse {
        wiki_id: payload.wiki_id.clone(),
        kind: payload.kind,
        rev_id: payload.rev_id,
        accepted: !summary.nochange,
        actor: Some(actor),
        http_status: Some(response.status),
        api_code: summary.api_code.clone(),
        retryable: summary.retryable,
        warnings,
        result: summary.result.clone(),
        message: if summary.nochange {
            Some("no change — the edit may have already been reverted".to_string())
        } else {
            Some(format!(
                "MediaWiki HTTP {} {}",
                response.status, response_preview
            ))
        },
    }
}

async fn record_action_side_effects(
    state: &AppState,
    session: &SessionSnapshot,
    headers: &HeaderMap,
    payload: &SessionActionExecutionRequest,
    log_entry: &ActionExecutionLogEntry,
) -> Option<String> {
    record_action_execution(state, &session.session_id, log_entry.clone()).await;
    storage_routes::append_public_audit_entry(state, headers, session, payload, log_entry)
        .await
        .err()
}

async fn handle_action_success(
    state: &AppState,
    session: &SessionSnapshot,
    headers: &HeaderMap,
    payload: &SessionActionExecutionRequest,
    executed_at_ms: i64,
    response: HttpResponse,
) -> Result<(StatusCode, Json<SessionActionExecutionResponse>), (StatusCode, Json<serde_json::Value>)>
{
    let response_preview = truncate_response_body(&response.body);
    let response_summary = parse_action_response_summary(&response, payload.kind.label())
        .map_err(|error| action_error_response(&error))?;
    let mut warnings = response_summary.warnings.clone();
    if response_summary.nochange {
        warnings.push("no change — the edit may have already been reverted".to_string());
    }
    let log_entry = build_action_log_entry(
        executed_at_ms,
        payload,
        ActionLogOutcome {
            accepted: !response_summary.nochange,
            http_status: Some(response.status),
            api_code: response_summary.api_code.clone(),
            retryable: response_summary.retryable,
            warnings,
            result: response_summary.result.clone(),
            response_preview: Some(response_preview.clone()),
            error: if response_summary.nochange {
                Some("no change — the edit may have already been reverted".to_string())
            } else {
                None
            },
        },
    );
    // Skip the audit log write when the action produced no change on wiki
    let audit_warning = if response_summary.nochange {
        None
    } else {
        record_action_side_effects(state, session, headers, payload, &log_entry).await
    };
    let mut response_payload = action_response_payload(
        payload,
        session.username.clone(),
        &response,
        &response_summary,
        &response_preview,
    );
    if let Some(warning) = audit_warning {
        response_payload
            .warnings
            .push(format!("public audit write failed: {warning}"));
    }

    Ok((StatusCode::OK, Json(response_payload)))
}

async fn handle_action_failure(
    state: &AppState,
    session: &SessionSnapshot,
    headers: &HeaderMap,
    payload: &SessionActionExecutionRequest,
    executed_at_ms: i64,
    error: ActionError,
) -> (StatusCode, Json<serde_json::Value>) {
    let (api_code, retryable, logged_http_status) = match &error {
        ActionError::Execution {
            code,
            http_status,
            retryable,
            ..
        } => (code.clone(), *retryable, *http_status),
    };
    let api_error = action_error_response(&error);
    let status = api_error.0.as_u16();
    let error_message = action_error_message(&api_error.1);
    let log_entry = build_action_log_entry(
        executed_at_ms,
        payload,
        ActionLogOutcome {
            accepted: false,
            http_status: logged_http_status.or(Some(status)),
            api_code,
            retryable,
            warnings: Vec::new(),
            result: None,
            response_preview: None,
            error: Some(error_message),
        },
    );
    let audit_warning =
        record_action_side_effects(state, session, headers, payload, &log_entry).await;
    if let Some(warning) = audit_warning {
        let mut body = api_error.1.0;
        body["audit_warning"] = serde_json::Value::String(warning);
        (api_error.0, Json(body))
    } else {
        api_error
    }
}

async fn execute_session_action(
    client: &BearerHttpClient,
    config: &sp42_core::WikiConfig,
    payload: &SessionActionExecutionRequest,
    editor: &dyn WikitextEditor,
) -> Result<HttpResponse, ActionError> {
    match payload.kind {
        SessionActionKind::Rollback => {
            let token = execute_fetch_token(client, config, TokenKind::Rollback).await?;
            execute_rollback(
                client,
                config,
                &RollbackRequest {
                    title: payload.title.clone().unwrap_or_default(),
                    user: payload.target_user.clone().unwrap_or_default(),
                    token,
                    summary: payload.summary.clone(),
                },
            )
            .await
        }
        SessionActionKind::Patrol => {
            let token = execute_fetch_token(client, config, TokenKind::Patrol).await?;
            let rev_ids = payload
                .batch_rev_ids
                .clone()
                .unwrap_or_else(|| vec![payload.rev_id]);
            let mut response = execute_patrol(
                client,
                config,
                &PatrolRequest {
                    rev_id: rev_ids[0],
                    token: token.clone(),
                },
            )
            .await?;
            for rid in rev_ids.iter().skip(1) {
                response = execute_patrol(
                    client,
                    config,
                    &PatrolRequest {
                        rev_id: *rid,
                        token: token.clone(),
                    },
                )
                .await?;
            }
            Ok(response)
        }
        SessionActionKind::Undo => {
            let token = execute_fetch_token(client, config, TokenKind::Csrf).await?;
            let Some(undo_after_rev_id) = payload.undo_after_rev_id else {
                return Err(ActionError::Execution {
                    message: "undo actions require undo_after_rev_id to be present".to_string(),
                    code: Some("invalid-input".to_string()),
                    http_status: None,
                    retryable: false,
                });
            };
            execute_undo(
                client,
                config,
                &UndoRequest {
                    title: payload.title.clone().unwrap_or_default(),
                    undo_rev_id: payload.rev_id,
                    undo_after_rev_id,
                    token,
                    summary: payload.summary.clone(),
                },
            )
            .await
        }
        SessionActionKind::TagCitationNeeded => {
            execute_tag_citation_needed_action(client, config, payload).await
        }
        SessionActionKind::InlineEdit => {
            execute_inline_edit_action(client, config, payload, editor).await
        }
    }
}

async fn execute_tag_citation_needed_action(
    client: &BearerHttpClient,
    config: &sp42_core::WikiConfig,
    payload: &SessionActionExecutionRequest,
) -> Result<HttpResponse, ActionError> {
    let token = execute_fetch_token(client, config, TokenKind::Csrf).await?;
    let title = payload.title.clone().unwrap_or_default();
    let selected_text = payload.selected_text.clone().unwrap_or_default();
    if selected_text.trim().is_empty() {
        return Err(ActionError::Execution {
            message: "selected_text is required for citation tagging".to_string(),
            code: Some("invalid-input".to_string()),
            http_status: None,
            retryable: false,
        });
    }
    let template = &config.templates.citation_needed;
    let date = current_french_date();
    let page_text = crate::fetch_page_wikitext(client, config, &title).await?;
    let updated_text = apply_citation_template(&page_text, &selected_text, template, &date)?;
    let summary = payload
        .summary
        .clone()
        .unwrap_or_else(|| format!("SP42: {{{{{template}|date={date}}}}}"));
    let save_response = execute_wiki_page_save(
        client,
        config,
        &WikiPageSaveRequest {
            title,
            text: updated_text,
            token,
            summary: Some(summary),
            baserevid: Some(payload.rev_id),
            tags: Vec::new(),
            watchlist: None,
            create_only: FlagState::Disabled,
            minor: FlagState::Disabled,
        },
    )
    .await?;
    patrol_original_edit_if_possible(client, config, payload.rev_id).await;
    Ok(save_response)
}

pub(crate) async fn execute_inline_edit_action(
    client: &BearerHttpClient,
    config: &sp42_core::WikiConfig,
    payload: &SessionActionExecutionRequest,
    editor: &dyn WikitextEditor,
) -> Result<HttpResponse, ActionError> {
    let token = execute_fetch_token(client, config, TokenKind::Csrf).await?;
    let title = payload.title.clone().unwrap_or_default();
    let updated_text = if let Some(locator) = payload.node_locator.as_ref() {
        node_anchored_replacement(config, &title, payload, locator, editor).await?
    } else {
        let original = payload.selected_text.clone().unwrap_or_default();
        let replacement = payload.replacement_text.clone().unwrap_or_default();
        if original.trim().is_empty() {
            return Err(ActionError::Execution {
                message: "selected_text (original) is required for inline edit".to_string(),
                code: Some("invalid-input".to_string()),
                http_status: None,
                retryable: false,
            });
        }
        let page_text = crate::fetch_page_wikitext(client, config, &title).await?;
        replace_exactly_once(&page_text, &original, &replacement)?
    };
    let summary = payload
        .summary
        .clone()
        .unwrap_or_else(|| "SP42: inline edit".to_string());
    let save_response = execute_wiki_page_save(
        client,
        config,
        &WikiPageSaveRequest {
            title,
            text: updated_text,
            token,
            summary: Some(summary),
            baserevid: Some(payload.rev_id),
            tags: Vec::new(),
            watchlist: None,
            create_only: FlagState::Disabled,
            minor: FlagState::Disabled,
        },
    )
    .await?;
    patrol_original_edit_if_possible(client, config, payload.rev_id).await;
    Ok(save_response)
}

/// Dispatches only `replace_node` today. `set_template_params` is deferred to
/// the citation-repair integration path (ADR-0008; tracked in SP42#26).
async fn node_anchored_replacement(
    config: &sp42_core::WikiConfig,
    title: &str,
    payload: &SessionActionExecutionRequest,
    locator: &WikitextNodeLocator,
    editor: &dyn WikitextEditor,
) -> Result<String, ActionError> {
    let Some(replacement) = payload.replacement_text.clone() else {
        return Err(ActionError::Execution {
            message: "replacement_text is required for node-anchored inline edit".to_string(),
            code: Some("invalid-input".to_string()),
            http_status: Some(400),
            retryable: false,
        });
    };
    replace_node_or_refuse(config, title, payload.rev_id, locator, &replacement, editor).await
}

/// Replace one node-anchored target, mapping editor failures to `editor-*`
/// codes and ADR-0003 refusals (drift / out-of-range) to a 409-in-body
/// `ActionError` — shared by inline edits and bare-URL repair (PRD-0008).
pub(crate) async fn replace_node_or_refuse(
    config: &sp42_core::WikiConfig,
    title: &str,
    rev_id: u64,
    locator: &WikitextNodeLocator,
    replacement: &str,
    editor: &dyn WikitextEditor,
) -> Result<String, ActionError> {
    let page = WikitextPageRef {
        title: title.to_string(),
        rev_id,
    };
    let outcome = editor
        .replace_node(config, &page, locator, replacement)
        .await
        .map_err(|e| action_error_from_editor(&e))?;
    match outcome {
        WikitextEditOutcome::Applied { new_wikitext } => Ok(new_wikitext),
        WikitextEditOutcome::Refused(refusal) => Err(ActionError::Execution {
            message: refusal.message(),
            code: Some(refusal.code().to_string()),
            http_status: Some(409),
            retryable: false,
        }),
    }
}

pub(crate) fn action_error_from_editor(error: &WikitextEditorError) -> ActionError {
    let (code, http_status, retryable) = match error {
        WikitextEditorError::Unavailable { retryable, .. } => {
            ("editor-unavailable", Some(502), *retryable)
        }
        WikitextEditorError::MissingTarget { .. } => ("editor-missing-target", Some(404), false),
        WikitextEditorError::NotConfigured { .. } => ("editor-not-configured", Some(400), false),
        WikitextEditorError::Unsupported { .. } => ("editor-unsupported", Some(400), false),
    };
    ActionError::Execution {
        message: error.to_string(),
        code: Some(code.to_string()),
        http_status,
        retryable,
    }
}

fn apply_citation_template(
    page_text: &str,
    selected_text: &str,
    template: &str,
    date: &str,
) -> Result<String, ActionError> {
    let text_param = if selected_text.contains('=') {
        format!("1={selected_text}")
    } else {
        selected_text.to_string()
    };
    let tagged = format!("{{{{{template}|{text_param}|date={date}}}}}");
    replace_exactly_once(page_text, selected_text, &tagged)
}

pub(crate) async fn patrol_original_edit_if_possible(
    client: &BearerHttpClient,
    config: &sp42_core::WikiConfig,
    rev_id: u64,
) {
    if let Ok(patrol_token) = execute_fetch_token(client, config, TokenKind::Patrol).await
        && let Err(error) = execute_patrol(
            client,
            config,
            &PatrolRequest {
                rev_id,
                token: patrol_token,
            },
        )
        .await
    {
        tracing::warn!(rev_id, %error, "auto-patrol failed; revision left unpatrolled");
    }
}

fn build_action_log_entry(
    executed_at_ms: i64,
    payload: &SessionActionExecutionRequest,
    outcome: ActionLogOutcome,
) -> ActionExecutionLogEntry {
    ActionExecutionLogEntry {
        executed_at_ms,
        wiki_id: payload.wiki_id.clone(),
        kind: payload.kind,
        rev_id: payload.rev_id,
        title: payload.title.clone(),
        target_user: payload.target_user.clone(),
        summary: payload.summary.clone(),
        accepted: outcome.accepted,
        http_status: outcome.http_status,
        api_code: outcome.api_code,
        retryable: outcome.retryable,
        warnings: outcome.warnings,
        result: outcome.result,
        response_preview: outcome.response_preview,
        error: outcome.error,
    }
}

struct ActionLogOutcome {
    accepted: bool,
    http_status: Option<u16>,
    api_code: Option<String>,
    retryable: bool,
    warnings: Vec<String>,
    result: Option<String>,
    response_preview: Option<String>,
    error: Option<String>,
}

/// Current month and year in French for template date parameters.
fn current_french_date() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = secs / 86400;
    // Approximate year/month from epoch days
    let mut y = 1970i32;
    let mut remaining = i32::try_from(days).unwrap_or(i32::MAX);
    loop {
        let days_in_year = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) {
            366
        } else {
            365
        };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        y += 1;
    }
    let months_days = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut m = 0usize;
    for (i, &d) in months_days.iter().enumerate() {
        if remaining < d {
            m = i;
            break;
        }
        remaining -= d;
    }
    let month_name = match m {
        0 => "janvier",
        1 => "février",
        2 => "mars",
        3 => "avril",
        4 => "mai",
        5 => "juin",
        6 => "juillet",
        7 => "août",
        8 => "septembre",
        9 => "octobre",
        10 => "novembre",
        _ => "décembre",
    };
    format!("{month_name} {y}")
}

struct ActionHistoryStats {
    total_actions: usize,
    successful_actions: usize,
    retryable_failures: usize,
    last_execution: Option<ActionExecutionLogEntry>,
}

async fn record_action_execution(
    state: &AppState,
    session_id: &str,
    entry: ActionExecutionLogEntry,
) {
    let mut sessions = state.sessions.write().await;
    prune_expired_sessions(&mut sessions, state.clock.now_ms());
    if let Some(session) = sessions.get_mut(session_id) {
        session.action_history.push(entry);
        if session.action_history.len() > ACTION_HISTORY_LIMIT {
            let overflow = session.action_history.len() - ACTION_HISTORY_LIMIT;
            session.action_history.drain(0..overflow);
        }
    }
}

pub(crate) fn action_feedback_for_entry(entry: &ActionExecutionLogEntry) -> String {
    let verb = match entry.kind {
        SessionActionKind::Rollback => "Rollback",
        SessionActionKind::Patrol => "Patrol",
        SessionActionKind::Undo => "Undo",
        SessionActionKind::TagCitationNeeded => "Citation needed",
        SessionActionKind::InlineEdit => "Inline edit",
    };
    let rationale = entry
        .summary
        .as_ref()
        .filter(|value| !value.trim().is_empty())
        .map(|value| format!(" rationale: {value}"))
        .unwrap_or_default();

    if entry.accepted {
        format!(
            "{verb} on {} rev {} accepted{}{}{}{}.",
            entry.wiki_id,
            entry.rev_id,
            entry
                .http_status
                .map(|status| format!(" with HTTP {status}"))
                .unwrap_or_default(),
            entry
                .result
                .as_ref()
                .map(|result| format!(" ({result})"))
                .unwrap_or_default(),
            entry
                .response_preview
                .as_ref()
                .map(|preview| format!(" {preview}"))
                .unwrap_or_default(),
            rationale,
        )
    } else {
        format!(
            "{verb} on {} rev {} failed{}{}{}.",
            entry.wiki_id,
            entry.rev_id,
            entry
                .api_code
                .as_ref()
                .map(|code| format!(" with code `{code}`"))
                .unwrap_or_default(),
            entry
                .error
                .as_ref()
                .map(|error| format!(": {error}"))
                .unwrap_or_default(),
            rationale,
        )
    }
}

pub(crate) async fn action_status_report(
    state: &AppState,
    headers: &HeaderMap,
) -> ActionExecutionStatusReport {
    let current = current_session_snapshot(state, headers, false).await;
    let Some(session) = current else {
        return ActionExecutionStatusReport {
            authenticated: false,
            session_id: None,
            username: None,
            total_actions: 0,
            successful_actions: 0,
            failed_actions: 0,
            retryable_failures: 0,
            last_execution: None,
            shell_feedback: vec!["No authenticated shell session is active.".to_string()],
        };
    };

    let history_summary = action_history_stats_for_session(state, &session.session_id).await;
    let last_execution = history_summary.last_execution.clone();
    let total_actions = history_summary.total_actions;
    let successful_actions = history_summary.successful_actions;
    let failed_actions = total_actions.saturating_sub(successful_actions);
    ActionExecutionStatusReport {
        authenticated: true,
        session_id: Some(session.session_id),
        username: Some(session.username),
        total_actions,
        successful_actions,
        failed_actions,
        retryable_failures: history_summary.retryable_failures,
        last_execution: last_execution.clone(),
        shell_feedback: action_shell_feedback(total_actions, last_execution.as_ref()),
    }
}

pub(crate) async fn action_history_report(
    state: &AppState,
    headers: &HeaderMap,
    limit: Option<usize>,
) -> ActionExecutionHistoryReport {
    let current = current_session_snapshot(state, headers, false).await;
    let Some(session) = current else {
        return ActionExecutionHistoryReport {
            authenticated: false,
            session_id: None,
            username: None,
            entries: Vec::new(),
        };
    };

    let entries = action_history_for_session(
        state,
        &session.session_id,
        limit.unwrap_or(10).min(ACTION_HISTORY_LIMIT),
    )
    .await;
    ActionExecutionHistoryReport {
        authenticated: true,
        session_id: Some(session.session_id),
        username: Some(session.username),
        entries,
    }
}

async fn action_history_for_session(
    state: &AppState,
    session_id: &str,
    limit: usize,
) -> Vec<ActionExecutionLogEntry> {
    let sessions = state.sessions.read().await;
    if let Some(session) = sessions.get(session_id) {
        session
            .action_history
            .iter()
            .rev()
            .take(limit)
            .cloned()
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    }
}

async fn action_history_stats_for_session(
    state: &AppState,
    session_id: &str,
) -> ActionHistoryStats {
    let sessions = state.sessions.read().await;
    sessions.get(session_id).map_or(
        ActionHistoryStats {
            total_actions: 0,
            successful_actions: 0,
            retryable_failures: 0,
            last_execution: None,
        },
        |session| {
            let mut successful_actions = 0usize;
            let mut retryable_failures = 0usize;
            for entry in &session.action_history {
                if entry.accepted {
                    successful_actions = successful_actions.saturating_add(1);
                } else if entry.retryable {
                    retryable_failures = retryable_failures.saturating_add(1);
                }
            }

            ActionHistoryStats {
                total_actions: session.action_history.len(),
                successful_actions,
                retryable_failures,
                last_execution: session.action_history.last().cloned(),
            }
        },
    )
}

fn action_shell_feedback(
    total_actions: usize,
    last_execution: Option<&ActionExecutionLogEntry>,
) -> Vec<String> {
    let mut feedback = Vec::new();
    feedback.push(format!(
        "{total_actions} action(s) recorded in this shell session."
    ));

    if let Some(last) = last_execution {
        feedback.push(action_feedback_for_entry(last));
        if let Some(preview) = &last.response_preview {
            feedback.push(format!("Latest response excerpt: {preview}"));
        }
        if let Some(code) = &last.api_code {
            feedback.push(format!("Latest API code: {code}"));
        }
    } else {
        feedback.push("No actions have been executed yet.".to_string());
    }

    feedback
}

pub(crate) fn validate_action_request(
    payload: &SessionActionExecutionRequest,
    capabilities: &DevAuthCapabilityReport,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if payload.wiki_id.trim().is_empty() {
        return Err(invalid_payload("wiki_id is required"));
    }
    if payload.rev_id == 0 {
        return Err(invalid_payload("rev_id must be non-zero"));
    }

    match payload.kind {
        SessionActionKind::Rollback => {
            if payload.title.as_deref().is_none_or(str::is_empty) {
                return Err(invalid_payload("title is required for rollback"));
            }
            if payload.target_user.as_deref().is_none_or(str::is_empty) {
                return Err(invalid_payload("target_user is required for rollback"));
            }
            if !capabilities.capabilities.moderation.can_rollback {
                return Err(forbidden_error(
                    "The authenticated session does not currently have rollback capability on this wiki.",
                ));
            }
        }
        SessionActionKind::Patrol => {
            if !capabilities.capabilities.moderation.can_patrol {
                return Err(forbidden_error(
                    "The authenticated session does not currently have patrol capability on this wiki.",
                ));
            }
        }
        SessionActionKind::Undo => {
            if payload.title.as_deref().is_none_or(str::is_empty) {
                return Err(invalid_payload("title is required for undo"));
            }
            if payload.undo_after_rev_id.is_none() {
                return Err(invalid_payload("undo_after_rev_id is required for undo"));
            }
            if !capabilities.capabilities.editing.can_undo {
                return Err(forbidden_error(
                    "The authenticated session does not currently have undo capability on this wiki.",
                ));
            }
        }
        SessionActionKind::TagCitationNeeded => {
            if payload.title.as_deref().is_none_or(str::is_empty) {
                return Err(invalid_payload("title is required for citation tagging"));
            }
            if payload.selected_text.as_deref().is_none_or(str::is_empty) {
                return Err(invalid_payload(
                    "selected_text is required for citation tagging",
                ));
            }
            if payload.node_locator.is_some() {
                return Err(invalid_payload(
                    "node_locator is not supported for citation tagging",
                ));
            }
            if !capabilities.capabilities.editing.can_edit {
                return Err(forbidden_error(
                    "The authenticated session does not currently have edit capability on this wiki.",
                ));
            }
        }
        SessionActionKind::InlineEdit => {
            if payload.title.as_deref().is_none_or(str::is_empty) {
                return Err(invalid_payload("title is required for inline edit"));
            }
            match payload.node_locator.as_ref() {
                Some(locator) => {
                    if locator.expected_text.trim().is_empty() {
                        return Err(invalid_payload(
                            "node_locator.expected_text must not be empty",
                        ));
                    }
                    if payload.replacement_text.is_none() {
                        return Err(invalid_payload(
                            "replacement_text is required for node-anchored inline edit",
                        ));
                    }
                }
                None => {
                    if payload.selected_text.as_deref().is_none_or(str::is_empty) {
                        return Err(invalid_payload(
                            "selected_text or node_locator is required for inline edit",
                        ));
                    }
                }
            }
            if !capabilities.capabilities.editing.can_edit {
                return Err(forbidden_error(
                    "The authenticated session does not currently have edit capability on this wiki.",
                ));
            }
        }
    }

    Ok(())
}

pub(crate) fn action_error_response(error: &ActionError) -> (StatusCode, Json<serde_json::Value>) {
    let (message, code, http_status, retryable) = match error {
        ActionError::Execution {
            message,
            code,
            http_status,
            retryable,
        } => (message.clone(), code.clone(), *http_status, *retryable),
    };
    (
        match http_status {
            Some(status) => StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY),
            None => StatusCode::BAD_GATEWAY,
        },
        Json(serde_json::json!({
            "error": format!("wiki action failed: {message}"),
            "code": code,
            "http_status": http_status,
            "retryable": retryable,
        })),
    )
}

pub(crate) fn truncate_response_body(body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body);
    if text.chars().count() > RESPONSE_BODY_PREVIEW_LIMIT {
        let truncated = text
            .chars()
            .take(RESPONSE_BODY_PREVIEW_LIMIT)
            .collect::<String>();
        format!("{truncated}...")
    } else {
        text.into_owned()
    }
}

fn action_error_message(body: &Json<serde_json::Value>) -> String {
    body.0
        .get("error")
        .and_then(serde_json::Value::as_str)
        .map_or_else(|| "wiki action failed".to_string(), ToString::to_string)
}

#[cfg(test)]
mod tests {
    use super::{action_feedback_for_entry, apply_citation_template};
    use sp42_core::{ActionError, ActionExecutionLogEntry, SessionActionKind};

    #[test]
    fn action_feedback_includes_rationale_summary() {
        let entry = ActionExecutionLogEntry {
            executed_at_ms: 1_710_000_000_000,
            wiki_id: "frwiki".to_string(),
            kind: SessionActionKind::Rollback,
            rev_id: 42,
            title: Some("Exemple".to_string()),
            target_user: Some("Example".to_string()),
            summary: Some("SP42 rationale: obvious-vandalism; rules=rule_set:default".to_string()),
            accepted: true,
            http_status: Some(200),
            api_code: None,
            retryable: false,
            warnings: Vec::new(),
            result: Some("Success".to_string()),
            response_preview: None,
            error: None,
        };

        let feedback = action_feedback_for_entry(&entry);

        assert!(feedback.contains("SP42 rationale: obvious-vandalism"));
    }

    #[test]
    fn apply_citation_template_tags_unique_selected_text() {
        let updated = apply_citation_template(
            "Une phrase sans source.",
            "phrase sans source",
            "Référence nécessaire",
            "juin 2026",
        )
        .expect("unique selected text should tag");
        assert_eq!(
            updated,
            "Une {{Référence nécessaire|phrase sans source|date=juin 2026}}."
        );
    }

    #[test]
    fn apply_citation_template_refuses_ambiguous_selected_text() {
        let error = apply_citation_template(
            "mot répété, mot répété.",
            "mot répété",
            "Référence nécessaire",
            "juin 2026",
        )
        .expect_err("ambiguous selected text should refuse");
        let ActionError::Execution { code, .. } = error;
        assert_eq!(code.as_deref(), Some("text-ambiguous"));
    }

    #[test]
    fn apply_citation_template_refuses_missing_selected_text() {
        let error = apply_citation_template(
            "Une phrase sans source.",
            "texte absent",
            "Référence nécessaire",
            "juin 2026",
        )
        .expect_err("missing selected text should refuse");
        let ActionError::Execution { code, .. } = error;
        assert_eq!(code.as_deref(), Some("text-not-found"));
    }
}
