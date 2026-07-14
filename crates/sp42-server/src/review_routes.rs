//! Interactive review-session bridge routes (PRD-0017).
//!
//! FCIS: every session transition (queue, drain-before-ended, reopen gate,
//! end etiquette) is a pure `sp42_core::ReviewSession` method; this module
//! owns the imperative edges — the shared store, the bounded poll wait, the
//! wake-up notification, and the route glue. All POST routes are
//! session+CSRF gated (ADR-0002): the agent side rides the CLI's bridge
//! bootstrap, the operator side rides the browser session cookie.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use sp42_core::{
    REVIEW_SESSION_CONTRACT_VERSION, ReviewAckResponse, ReviewBlockOutline, ReviewEndRequest,
    ReviewEndedBy, ReviewFeedbackTake, ReviewFindingsRequest, ReviewFindingsResponse,
    ReviewOpenRequest, ReviewOpenResponse, ReviewPollRequest, ReviewPollResponse, ReviewPollStatus,
    ReviewQueueRequest, ReviewQueueResponse, ReviewSession, ReviewSessionSnapshot,
    ReviewSessionsResponse, WikitextPageRef, annotate_outline, build_article_outline,
    open_next_step, poll_next_step, poll_status,
};
use tokio::sync::{Notify, RwLock};
use tracing::info;

use crate::citation_routes::fetch_latest_revid;
use crate::config_for_state_wiki;
use crate::http_errors::unauthorized_error;
use crate::runtime_adapters::BearerHttpClient;
use crate::session_runtime::{current_session_snapshot, validate_csrf_header};
use crate::state::AppState;

/// Default server-side wait when a poll request does not bound it.
pub(crate) const POLL_WAIT_DEFAULT_MS: u64 = 25_000;

/// Upper clamp on the server-side poll wait: long enough that an agent loop
/// is quiet, short enough to stay under common client/proxy request
/// timeouts. The CLI re-arms between waits, so the loop is unbounded even
/// though each request is not.
pub(crate) const POLL_WAIT_MAX_MS: u64 = 55_000;

/// One stored session plus its poll wake-up channel. `notify_one` stores a
/// permit when no poll is waiting, so feedback queued between an empty drain
/// and the wait registration still wakes the next `notified().await`.
#[derive(Debug)]
pub(crate) struct ReviewSessionEntry {
    pub(crate) session: ReviewSession,
    pub(crate) notify: Arc<Notify>,
}

pub(crate) type SharedReviewSessions = Arc<RwLock<HashMap<String, ReviewSessionEntry>>>;

/// Fresh empty review-session store for state construction.
pub(crate) fn new_review_session_store() -> SharedReviewSessions {
    Arc::new(RwLock::new(HashMap::new()))
}

type RouteError = (StatusCode, Json<serde_json::Value>);

/// Require an authenticated bridge session with a valid CSRF header —
/// the shared gate for every review POST route.
async fn require_review_session(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<crate::state::SessionSnapshot, RouteError> {
    let Some(session) = current_session_snapshot(state, headers, true).await else {
        return Err(unauthorized_error(
            "No authenticated bridge session is active.",
        ));
    };
    validate_csrf_header(headers, &session)?;
    Ok(session)
}

fn missing_session_error(wiki_id: &str, title: &str) -> RouteError {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({
            "error": format!("no review session is open for {title} on {wiki_id}"),
            "code": "review-session-not-found",
        })),
    )
}

/// Resolve the open target: unwrap pasted URLs, pin `rev_id == 0` to the
/// current revision (so the outline and the session record agree on what
/// was reviewed), and decompose the page into outline blocks.
async fn resolve_open_target(
    state: &AppState,
    session: &crate::state::SessionSnapshot,
    payload: &ReviewOpenRequest,
) -> Result<(String, u64, Vec<ReviewBlockOutline>), RouteError> {
    let config = config_for_state_wiki(state, &payload.wiki_id)?;
    let target = sp42_core::parse_page_target(&payload.target);
    let mut rev_id = if payload.rev_id == 0 {
        target.rev_id
    } else {
        payload.rev_id
    };
    if rev_id == 0 {
        let wiki_client =
            BearerHttpClient::new(state.http_client.clone(), session.access_token.clone());
        rev_id = fetch_latest_revid(&wiki_client, &config, &target.title)
            .await
            .map_err(|error| crate::action_routes::action_error_response(&error))?;
    }
    let page_ref = WikitextPageRef {
        title: target.title.clone(),
        rev_id,
    };
    let blocks = state
        .wikitext_editor
        .extract_blocks(&config, &page_ref)
        .await
        .map_err(|error| {
            crate::action_routes::action_error_response(
                &crate::action_routes::action_error_from_editor(&error),
            )
        })?;
    Ok((target.title, rev_id, build_article_outline(&blocks)))
}

/// The 409 etiquette response when a plain open hits a session the operator
/// explicitly ended.
fn reopen_gate_error(
    session: &ReviewSession,
    reopen_requested: bool,
    title: &str,
) -> Result<(), RouteError> {
    if session.gate_reopen(reopen_requested).is_err() {
        return Err((
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": format!(
                    "the operator ended the review of {title}; do not reopen uninvited — \
                     pass reopen=true only when the operator asks for further review"
                ),
                "code": "review-session-operator-ended",
            })),
        ));
    }
    Ok(())
}

/// Open a new session or resume an existing one, honoring the reopen gate:
/// a session the operator explicitly ended refuses a plain reopen.
fn open_or_resume(
    entry: Option<&mut ReviewSessionEntry>,
    reopen_requested: bool,
    title: &str,
    rev_id: u64,
    now_ms: i64,
) -> Result<Option<ReviewSessionSnapshot>, RouteError> {
    let Some(entry) = entry else {
        return Ok(None);
    };
    reopen_gate_error(&entry.session, reopen_requested, title)?;
    entry.session.resume(rev_id, now_ms);
    Ok(Some(entry.session.snapshot()))
}

/// `POST /dev/review/open` — open or resume a review session on a page.
///
/// Session + CSRF gated: the route mutates server state and (for a latest-
/// revision open) reads through the caller's wiki identity.
pub(crate) async fn post_review_open(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ReviewOpenRequest>,
) -> Result<impl IntoResponse, RouteError> {
    let session = require_review_session(&state, &headers).await?;

    // Check the reopen gate before any remote read: a plain reopen of an
    // operator-ended session must get the 409 etiquette response from the
    // stored session alone, not depend on (or pay for) revision/Parsoid
    // lookups. The key from the locally parsed title matches the one built
    // after resolution. Re-gated under the write lock below for the race.
    let parsed_title = sp42_core::parse_page_target(&payload.target).title;
    {
        let store = state.review_sessions.read().await;
        let key = ReviewSession::canonical_key(&payload.wiki_id, &parsed_title);
        if let Some(entry) = store.get(&key) {
            reopen_gate_error(&entry.session, payload.reopen, &parsed_title)?;
        }
    }

    let (title, rev_id, outline) = resolve_open_target(&state, &session, &payload).await?;
    let key = ReviewSession::canonical_key(&payload.wiki_id, &title);
    let now_ms = state.clock.now_ms();

    let mut store = state.review_sessions.write().await;
    let resumed = open_or_resume(store.get_mut(&key), payload.reopen, &title, rev_id, now_ms)?;
    let (session, findings, chat) = if let Some(snapshot) = resumed {
        let (findings, chat) = store
            .get(&key)
            .map(|entry| (entry.session.findings.clone(), entry.session.chat.clone()))
            .unwrap_or_default();
        (snapshot, findings, chat)
    } else {
        let session = ReviewSession::open(&payload.wiki_id, &title, rev_id, now_ms);
        let snapshot = session.snapshot();
        store.insert(
            key,
            ReviewSessionEntry {
                session,
                notify: Arc::new(Notify::new()),
            },
        );
        (snapshot, Vec::new(), Vec::new())
    };
    // Attached verification findings overlay the outline: each marker joins
    // its block by ref_id, so the operator sees report problems in article
    // context rather than as detached text.
    let (outline, unanchored_findings) = annotate_outline(outline, &findings);
    info!(
        wiki_id = %session.wiki_id,
        title = %session.title,
        rev_id = session.rev_id,
        findings = findings.len(),
        "review session opened"
    );
    Ok(Json(ReviewOpenResponse {
        contract_version: REVIEW_SESSION_CONTRACT_VERSION,
        next_step: open_next_step(&session.title, findings.len()),
        session,
        outline,
        unanchored_findings,
        chat,
    }))
}

/// `GET /dev/review/sessions` — read-only session inventory.
pub(crate) async fn get_review_sessions(State(state): State<AppState>) -> impl IntoResponse {
    let store = state.review_sessions.read().await;
    let mut sessions: Vec<ReviewSessionSnapshot> = store
        .values()
        .map(|entry| entry.session.snapshot())
        .collect();
    sessions.sort_by_key(|session| std::cmp::Reverse(session.updated_at_ms));
    Json(ReviewSessionsResponse {
        contract_version: REVIEW_SESSION_CONTRACT_VERSION,
        sessions,
    })
}

/// `POST /dev/review/prompts` — the operator queues feedback (optionally
/// ending the session in the same action) and wakes any waiting poll.
pub(crate) async fn post_review_prompts(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ReviewQueueRequest>,
) -> Result<impl IntoResponse, RouteError> {
    require_review_session(&state, &headers).await?;
    let key = ReviewSession::canonical_key(&payload.wiki_id, &payload.title);
    let now_ms = state.clock.now_ms();

    let mut store = state.review_sessions.write().await;
    let Some(entry) = store.get_mut(&key) else {
        return Err(missing_session_error(&payload.wiki_id, &payload.title));
    };
    let queued = payload.prompts.len();
    entry
        .session
        .queue_prompts(payload.prompts, payload.end_session, now_ms)
        .map_err(|ended| {
            let by = match ended.ended_by {
                ReviewEndedBy::Operator => "operator",
                ReviewEndedBy::Agent => "agent",
            };
            (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": format!(
                        "the review of {} was already ended by the {by}; open the session \
                         again before queueing more feedback",
                        payload.title
                    ),
                    "code": "review-session-ended",
                })),
            )
        })?;
    entry.notify.notify_one();
    info!(
        wiki_id = %payload.wiki_id,
        title = %payload.title,
        queued,
        end_session = payload.end_session,
        "review feedback queued"
    );
    Ok(Json(ReviewQueueResponse {
        contract_version: REVIEW_SESSION_CONTRACT_VERSION,
        session: entry.session.snapshot(),
        queued,
    }))
}

/// `POST /dev/review/findings` — the agent attaches a verification report's
/// finding markers to the session (replace-all; a stale-revision report is
/// refused). The markers overlay the outline on the next open and feed the
/// operator surface; they are not agent feedback, so no poll is woken.
pub(crate) async fn post_review_findings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ReviewFindingsRequest>,
) -> Result<impl IntoResponse, RouteError> {
    require_review_session(&state, &headers).await?;
    let key = ReviewSession::canonical_key(&payload.wiki_id, &payload.title);
    let now_ms = state.clock.now_ms();
    let mut store = state.review_sessions.write().await;
    let Some(entry) = store.get_mut(&key) else {
        return Err(missing_session_error(&payload.wiki_id, &payload.title));
    };
    let attached = entry
        .session
        .attach_findings(payload.rev_id, payload.findings, now_ms)
        .map_err(|mismatch| {
            (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": format!(
                        "the findings were produced against revision {} but the review \
                         session is pinned to revision {}; re-run verification (or reopen \
                         the session) on one revision",
                        payload.rev_id, mismatch.session_rev
                    ),
                    "code": "review-findings-revision-mismatch",
                })),
            )
        })?;
    info!(
        wiki_id = %payload.wiki_id,
        title = %payload.title,
        attached,
        "review findings attached"
    );
    Ok(Json(ReviewFindingsResponse {
        contract_version: REVIEW_SESSION_CONTRACT_VERSION,
        session: entry.session.snapshot(),
        attached,
    }))
}

/// Drain one session's feedback; `None` when the session does not exist.
async fn drain_feedback(
    state: &AppState,
    key: &str,
) -> Option<(ReviewFeedbackTake, Arc<Notify>, String)> {
    let mut store = state.review_sessions.write().await;
    let entry = store.get_mut(key)?;
    let take = entry.session.take_feedback(state.clock.now_ms());
    Some((take, entry.notify.clone(), entry.session.title.clone()))
}

fn poll_response(take: ReviewFeedbackTake, title: &str) -> ReviewPollResponse {
    let status = poll_status(&take);
    let next_step = poll_next_step(&take, title);
    let (prompts, session_ended, ended_by) = match take {
        ReviewFeedbackTake::Waiting => (Vec::new(), false, None),
        ReviewFeedbackTake::Feedback {
            prompts,
            session_ended,
            ended_by,
        } => (prompts, session_ended, ended_by),
        ReviewFeedbackTake::Ended { ended_by } => (Vec::new(), true, Some(ended_by)),
    };
    ReviewPollResponse {
        contract_version: REVIEW_SESSION_CONTRACT_VERSION,
        status,
        prompts,
        session_ended,
        ended_by,
        next_step,
    }
}

fn missing_poll_response() -> ReviewPollResponse {
    ReviewPollResponse {
        contract_version: REVIEW_SESSION_CONTRACT_VERSION,
        status: ReviewPollStatus::Missing,
        prompts: Vec::new(),
        session_ended: false,
        ended_by: None,
        next_step: "No review session is open for this page. Run `sp42-cli review open` first."
            .to_string(),
    }
}

/// `POST /dev/review/poll` — the agent's bounded feedback wait.
///
/// Drains immediately when feedback is queued; otherwise waits (clamped to
/// [`POLL_WAIT_MAX_MS`]; an explicit `wait_ms: 0` skips the wait entirely
/// for a nonblocking status check) for a queue/end wake-up and drains once
/// more. The
/// wake-up uses `notify_one` permits, so feedback queued while no poll is
/// waiting is picked up by the next poll without racing.
pub(crate) async fn post_review_poll(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ReviewPollRequest>,
) -> Result<impl IntoResponse, RouteError> {
    require_review_session(&state, &headers).await?;
    let key = ReviewSession::canonical_key(&payload.wiki_id, &payload.title);
    let Some((take, notify, title)) = drain_feedback(&state, &key).await else {
        return Ok(Json(missing_poll_response()));
    };

    // Omitted wait = the server default; an explicit 0 is the contract's
    // nonblocking status check and must return the first drain immediately.
    let wait_ms = payload
        .wait_ms
        .map_or(POLL_WAIT_DEFAULT_MS, |ms| ms.min(POLL_WAIT_MAX_MS));
    let take = if take == ReviewFeedbackTake::Waiting && wait_ms > 0 {
        let _ = tokio::time::timeout(Duration::from_millis(wait_ms), notify.notified()).await;
        match drain_feedback(&state, &key).await {
            Some((take, _, _)) => take,
            None => return Ok(Json(missing_poll_response())),
        }
    } else {
        take
    };

    if !matches!(take, ReviewFeedbackTake::Waiting) {
        info!(
            wiki_id = %payload.wiki_id,
            title = %title,
            status = ?poll_status(&take),
            "review poll delivered"
        );
    }
    Ok(Json(poll_response(take, &title)))
}

/// `POST /dev/review/agent-reply` — the agent's chat message to the
/// operator surface.
pub(crate) async fn post_review_reply(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<sp42_core::ReviewReplyRequest>,
) -> Result<impl IntoResponse, RouteError> {
    require_review_session(&state, &headers).await?;
    let key = ReviewSession::canonical_key(&payload.wiki_id, &payload.title);
    let now_ms = state.clock.now_ms();
    let mut store = state.review_sessions.write().await;
    let Some(entry) = store.get_mut(&key) else {
        return Err(missing_session_error(&payload.wiki_id, &payload.title));
    };
    entry.session.agent_reply(&payload.text, now_ms);
    Ok(Json(ReviewAckResponse {
        contract_version: REVIEW_SESSION_CONTRACT_VERSION,
        session: entry.session.snapshot(),
    }))
}

/// `POST /dev/review/end` — agent-initiated end. Unlike an operator end, a
/// plain reopen stays allowed afterwards.
pub(crate) async fn post_review_end(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ReviewEndRequest>,
) -> Result<impl IntoResponse, RouteError> {
    require_review_session(&state, &headers).await?;
    let key = ReviewSession::canonical_key(&payload.wiki_id, &payload.title);
    let now_ms = state.clock.now_ms();
    let mut store = state.review_sessions.write().await;
    let Some(entry) = store.get_mut(&key) else {
        return Err(missing_session_error(&payload.wiki_id, &payload.title));
    };
    entry.session.end(ReviewEndedBy::Agent, now_ms);
    entry.notify.notify_one();
    info!(
        wiki_id = %payload.wiki_id,
        title = %payload.title,
        "review session ended by agent"
    );
    Ok(Json(ReviewAckResponse {
        contract_version: REVIEW_SESSION_CONTRACT_VERSION,
        session: entry.session.snapshot(),
    }))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use sp42_core::{ReviewFeedbackTake, ReviewPrompt, ReviewPromptKind, ReviewSession};
    use tokio::sync::Notify;

    use super::{ReviewSessionEntry, new_review_session_store};

    fn message_prompt(text: &str) -> ReviewPrompt {
        ReviewPrompt {
            kind: ReviewPromptKind::Message,
            prompt: text.to_string(),
            anchor: None,
        }
    }

    #[tokio::test]
    async fn queued_feedback_wakes_a_waiting_poll() {
        let store = new_review_session_store();
        let key = ReviewSession::canonical_key("frwiki", "Exemple");
        store.write().await.insert(
            key.clone(),
            ReviewSessionEntry {
                session: ReviewSession::open("frwiki", "Exemple", 42, 1_000),
                notify: Arc::new(Notify::new()),
            },
        );

        let waiter_store = store.clone();
        let waiter_key = key.clone();
        let waiter = tokio::spawn(async move {
            let notify = waiter_store
                .read()
                .await
                .get(&waiter_key)
                .expect("session should exist")
                .notify
                .clone();
            tokio::time::timeout(Duration::from_secs(5), notify.notified())
                .await
                .expect("queueing should wake the waiter");
            let mut sessions = waiter_store.write().await;
            let entry = sessions
                .get_mut(&waiter_key)
                .expect("session should still exist");
            entry.session.take_feedback(2_000)
        });

        // Let the waiter register before queueing.
        tokio::time::sleep(Duration::from_millis(50)).await;
        {
            let mut sessions = store.write().await;
            let entry = sessions.get_mut(&key).expect("session should exist");
            entry
                .session
                .queue_prompts(vec![message_prompt("wake up")], false, 1_500)
                .expect("open session should accept prompts");
            entry.notify.notify_one();
        }

        let take = waiter.await.expect("waiter should finish");
        let ReviewFeedbackTake::Feedback { prompts, .. } = take else {
            panic!("expected delivered feedback");
        };
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0].prompt, "wake up");
    }

    #[tokio::test]
    async fn a_permit_covers_feedback_queued_before_the_wait_registers() {
        let notify = Arc::new(Notify::new());
        // Feedback arrives while no poll is waiting: the permit is stored...
        notify.notify_one();
        // ...and the next wait completes immediately instead of blocking.
        tokio::time::timeout(Duration::from_millis(100), notify.notified())
            .await
            .expect("stored permit should complete the wait immediately");
    }
}
