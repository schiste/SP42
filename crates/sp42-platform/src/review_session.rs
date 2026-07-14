//! Interactive review-session contracts (PRD-0017).
//!
//! An agent opens a review session on a wiki page, the operator queues
//! anchored feedback prompts against the rendered article, and the agent
//! polls the session to collect that feedback. The loop is ported from the
//! local-first artifact-review pattern popularized by `lavish-axi`
//! (<https://github.com/kunchenguid/lavish-axi>), re-keyed from local file
//! paths to `(wiki_id, title)` targets.
//!
//! This module is the pure core: session state transitions, the
//! drain-before-ended feedback semantics, the operator-ended reopen gate,
//! and the `next_step` guidance strings. The imperative edges — route glue,
//! waiting, notification — live in `sp42-server`; the agent loop lives in
//! `sp42-cli`.

use serde::{Deserialize, Serialize};

use crate::wikitext_editor::{BlockKind, ParsoidBlock};

/// Version tag embedded in every review-session response so agents can
/// detect contract drift (Constitution Art. 9.1).
pub const REVIEW_SESSION_CONTRACT_VERSION: u32 = 1;

/// Outline text is truncated to this many characters so a full-article
/// outline stays a compact, token-friendly snapshot.
pub const REVIEW_OUTLINE_TEXT_LIMIT: usize = 160;

/// Who closed a review session. Mirrors the `ended_by` etiquette of the
/// ported loop: an operator-ended session refuses a plain reopen, while an
/// agent-ended session may be reopened freely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewEndedBy {
    Operator,
    Agent,
}

/// Lifecycle of a review session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewSessionStatus {
    /// Open with no undelivered feedback.
    Open,
    /// Open with queued prompts awaiting an agent poll.
    Feedback,
    /// Closed; `ended_by` records who closed it.
    Ended,
}

/// What a queued prompt is anchored to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewPromptKind {
    /// Anchored to one article block (paragraph, list item, table cell).
    Block,
    /// Anchored to selected text inside a block.
    Text,
    /// Free-form message with no article anchor.
    Message,
}

/// Anchor tying a prompt to article structure. Block ordinals and cite ids
/// come from the Parsoid block decomposition, so anchors survive rendering
/// differences that CSS selectors would not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewAnchor {
    /// Document-order index of the anchored block.
    pub block_ordinal: usize,
    /// Stable cite id (e.g. `"cite_ref-smith_3-0"`) when the prompt targets
    /// a specific reference in the block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ref_id: Option<String>,
    /// Verbatim selected text when the prompt targets a text range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_text: Option<String>,
}

/// One verification finding projected onto review-session coordinates — the
/// overlay marker that lets a review surface show a page verification
/// report's problems *in the context of the article* instead of as detached
/// report text. Markers are citation-agnostic here: the citation domain
/// projects its report into this shape at the edges (`ref_id` is the join
/// key onto outline blocks), so this crate never depends on report types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewFindingMarker {
    /// The originating ref's marker id (e.g. `cite_ref-smith_3-0`) — the
    /// same anchor coordinate prompts use.
    pub ref_id: String,
    /// Verdict wire label (e.g. `"unsupported"`, `"source_unavailable"`).
    pub verdict: String,
    /// The judged claim sentence, truncated like outline text.
    pub claim: String,
    /// Optional short qualifier — a grounding caveat or unavailable reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Refusal reason when attached findings were produced against a different
/// revision than the session is pinned to — a stale overlay would point at
/// blocks that may no longer exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FindingsRevisionMismatch {
    /// The revision the session is pinned to.
    pub session_rev: u64,
}

/// One operator feedback item queued for the agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewPrompt {
    pub kind: ReviewPromptKind,
    /// The operator's instruction or question.
    pub prompt: String,
    /// Article anchor; `None` for [`ReviewPromptKind::Message`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<ReviewAnchor>,
}

/// Who wrote a chat entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewChatRole {
    Operator,
    Agent,
}

/// One line of the session's operator/agent conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewChatEntry {
    pub role: ReviewChatRole,
    pub text: String,
    pub at_ms: i64,
}

/// Pure state of one review session. The server wraps this in its shared
/// store; every transition is a method here so the semantics stay testable
/// without I/O.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewSession {
    pub wiki_id: String,
    /// Namespace-qualified page title (spaces, not underscores).
    pub title: String,
    /// Revision the session was opened against (`0` = latest at open time).
    pub rev_id: u64,
    pub status: ReviewSessionStatus,
    /// Prompts queued since the last successful poll delivery.
    pub queued_prompts: Vec<ReviewPrompt>,
    /// Verification findings attached to this session's pinned revision —
    /// the report overlay the open response joins onto the outline.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<ReviewFindingMarker>,
    pub chat: Vec<ReviewChatEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_by: Option<ReviewEndedBy>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

/// Result of draining a session's queued feedback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewFeedbackTake {
    /// Nothing queued and the session is open.
    Waiting,
    /// Queued prompts, delivered exactly once. `session_ended` is `true`
    /// when this is the final batch of a session that has already closed —
    /// queued feedback always delivers before the `Ended` status does.
    Feedback {
        prompts: Vec<ReviewPrompt>,
        session_ended: bool,
        ended_by: Option<ReviewEndedBy>,
    },
    /// Session closed with nothing left to deliver.
    Ended { ended_by: ReviewEndedBy },
}

/// Refusal reason when an open request hits the reopen gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReopenRefused;

impl ReviewSession {
    /// A fresh open session on `(wiki_id, title)` at `rev_id`.
    #[must_use]
    pub fn open(wiki_id: &str, title: &str, rev_id: u64, now_ms: i64) -> Self {
        Self {
            wiki_id: wiki_id.to_string(),
            title: title.to_string(),
            rev_id,
            status: ReviewSessionStatus::Open,
            queued_prompts: Vec::new(),
            findings: Vec::new(),
            chat: Vec::new(),
            ended_by: None,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
        }
    }

    /// Canonical store key for a target. The pair is the identity — the
    /// analog of the ported loop's canonical-file-path session key.
    #[must_use]
    pub fn canonical_key(wiki_id: &str, title: &str) -> String {
        format!("{wiki_id}\u{1f}{title}")
    }

    /// Gate a reopen attempt: a session the *operator* ended refuses a plain
    /// reopen so the agent does not reopen the review surface uninvited; an
    /// agent-ended (or still-open) session resumes freely.
    ///
    /// # Errors
    ///
    /// [`ReopenRefused`] when the operator ended the session and the caller
    /// did not explicitly request a reopen.
    pub fn gate_reopen(&self, reopen_requested: bool) -> Result<(), ReopenRefused> {
        if self.ended_by == Some(ReviewEndedBy::Operator) && !reopen_requested {
            return Err(ReopenRefused);
        }
        Ok(())
    }

    /// Resume an ended session as open again (after [`Self::gate_reopen`]).
    /// Re-pinning to a *different* revision drops the findings overlay: the
    /// markers describe the old revision's blocks.
    pub fn resume(&mut self, rev_id: u64, now_ms: i64) {
        self.status = if self.queued_prompts.is_empty() {
            ReviewSessionStatus::Open
        } else {
            ReviewSessionStatus::Feedback
        };
        self.ended_by = None;
        if self.rev_id != rev_id {
            self.findings.clear();
        }
        self.rev_id = rev_id;
        self.updated_at_ms = now_ms;
    }

    /// Replace the session's findings overlay with one report's markers.
    /// Findings do not accumulate across attaches — each attach describes
    /// one verification run of the pinned revision.
    ///
    /// # Errors
    ///
    /// [`FindingsRevisionMismatch`] when `report_rev_id` differs from the
    /// session's pinned revision — a stale overlay must not annotate blocks
    /// it was not produced against.
    pub fn attach_findings(
        &mut self,
        report_rev_id: u64,
        findings: Vec<ReviewFindingMarker>,
        now_ms: i64,
    ) -> Result<usize, FindingsRevisionMismatch> {
        if report_rev_id != self.rev_id {
            return Err(FindingsRevisionMismatch {
                session_rev: self.rev_id,
            });
        }
        let attached = findings.len();
        self.findings = findings;
        self.updated_at_ms = now_ms;
        Ok(attached)
    }

    /// Queue operator prompts; optionally close the session in the same
    /// action ("send & end"). Queued prompts survive the end and deliver on
    /// the next poll.
    pub fn queue_prompts(&mut self, prompts: Vec<ReviewPrompt>, end_session: bool, now_ms: i64) {
        self.queued_prompts.extend(prompts);
        if end_session {
            self.status = ReviewSessionStatus::Ended;
            self.ended_by = Some(ReviewEndedBy::Operator);
        } else if !self.queued_prompts.is_empty() {
            self.status = ReviewSessionStatus::Feedback;
        }
        self.updated_at_ms = now_ms;
    }

    /// Drain queued feedback with deliver-before-ended semantics: prompts
    /// queued before an end still deliver first (marked `session_ended`);
    /// only a later poll reports `Ended`.
    pub fn take_feedback(&mut self, now_ms: i64) -> ReviewFeedbackTake {
        if !self.queued_prompts.is_empty() {
            let prompts = std::mem::take(&mut self.queued_prompts);
            let session_ended = self.status == ReviewSessionStatus::Ended;
            if !session_ended {
                self.status = ReviewSessionStatus::Open;
            }
            self.updated_at_ms = now_ms;
            return ReviewFeedbackTake::Feedback {
                prompts,
                session_ended,
                ended_by: self.ended_by,
            };
        }
        match self.ended_by {
            Some(ended_by) if self.status == ReviewSessionStatus::Ended => {
                ReviewFeedbackTake::Ended { ended_by }
            }
            _ => ReviewFeedbackTake::Waiting,
        }
    }

    /// Append an agent chat reply (shown on the operator surface).
    pub fn agent_reply(&mut self, text: &str, now_ms: i64) {
        self.chat.push(ReviewChatEntry {
            role: ReviewChatRole::Agent,
            text: text.to_string(),
            at_ms: now_ms,
        });
        self.updated_at_ms = now_ms;
    }

    /// Close the session, recording who closed it.
    pub fn end(&mut self, by: ReviewEndedBy, now_ms: i64) {
        self.status = ReviewSessionStatus::Ended;
        self.ended_by = Some(by);
        self.updated_at_ms = now_ms;
    }

    /// Wire-facing snapshot of this session.
    #[must_use]
    pub fn snapshot(&self) -> ReviewSessionSnapshot {
        ReviewSessionSnapshot {
            wiki_id: self.wiki_id.clone(),
            title: self.title.clone(),
            rev_id: self.rev_id,
            status: self.status,
            pending_prompts: self.queued_prompts.len(),
            findings: self.findings.len(),
            ended_by: self.ended_by,
            updated_at_ms: self.updated_at_ms,
        }
    }
}

/// Wire-facing summary of one session for listings and command output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewSessionSnapshot {
    pub wiki_id: String,
    pub title: String,
    pub rev_id: u64,
    pub status: ReviewSessionStatus,
    pub pending_prompts: usize,
    /// Attached verification-finding markers (the report overlay).
    #[serde(default)]
    pub findings: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_by: Option<ReviewEndedBy>,
    pub updated_at_ms: i64,
}

/// One block of the article outline — the compact structure snapshot an
/// agent uses to resolve prompt anchors (the analog of the ported loop's
/// DOM snapshot, built from Parsoid blocks instead of live DOM).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewBlockOutline {
    pub block_ordinal: usize,
    pub kind: String,
    /// Block text truncated to [`REVIEW_OUTLINE_TEXT_LIMIT`] characters.
    pub text: String,
    /// Stable cite ids of the block's inline references, in order.
    pub ref_ids: Vec<String>,
    /// Verification findings anchored to this block (the report overlay),
    /// joined by `ref_id`. Empty when no report is attached.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<ReviewFindingMarker>,
}

/// Build the outline for a decomposed page.
#[must_use]
pub fn build_article_outline(blocks: &[ParsoidBlock]) -> Vec<ReviewBlockOutline> {
    blocks
        .iter()
        .map(|block| ReviewBlockOutline {
            block_ordinal: block.block_ordinal,
            kind: block_kind_label(block.block_kind).to_string(),
            text: truncate_chars(&block.text, REVIEW_OUTLINE_TEXT_LIMIT),
            ref_ids: block
                .refs
                .iter()
                .map(|block_ref| block_ref.ref_id.clone())
                .collect(),
            findings: Vec::new(),
        })
        .collect()
}

/// Join finding markers onto outline blocks by `ref_id` — the projection
/// that turns the verification report into an in-article overlay. Markers
/// whose `ref_id` matches no block are returned, not dropped: the surface
/// must be able to say "these findings could not be placed" rather than
/// silently under-report.
#[must_use]
pub fn annotate_outline(
    mut outline: Vec<ReviewBlockOutline>,
    findings: &[ReviewFindingMarker],
) -> (Vec<ReviewBlockOutline>, Vec<ReviewFindingMarker>) {
    let mut unanchored = Vec::new();
    for marker in findings {
        match outline
            .iter_mut()
            .find(|block| block.ref_ids.iter().any(|ref_id| ref_id == &marker.ref_id))
        {
            Some(block) => block.findings.push(marker.clone()),
            None => unanchored.push(marker.clone()),
        }
    }
    (outline, unanchored)
}

/// Truncate text to the outline display limit — shared by outline blocks
/// and by edge crates projecting report claims into [`ReviewFindingMarker`]s.
#[must_use]
pub fn truncate_outline_text(text: &str) -> String {
    truncate_chars(text, REVIEW_OUTLINE_TEXT_LIMIT)
}

const fn block_kind_label(kind: BlockKind) -> &'static str {
    match kind {
        BlockKind::Paragraph => "paragraph",
        BlockKind::ListItem => "list-item",
        BlockKind::TableCell => "table-cell",
        BlockKind::Other => "other",
    }
}

/// Truncate on a character boundary, appending an ellipsis when shortened.
fn truncate_chars(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut shortened: String = text.chars().take(limit).collect();
    shortened.push('…');
    shortened
}

/// Open (or resume) a review session on a page target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewOpenRequest {
    pub wiki_id: String,
    /// Bare title or pasted wiki URL; the server unwraps URLs the same way
    /// verify-page does.
    pub target: String,
    /// Pinned revision; `0` means latest.
    #[serde(default)]
    pub rev_id: u64,
    /// Required to resume a session the operator explicitly ended.
    #[serde(default)]
    pub reopen: bool,
}

/// Open/resume response: session snapshot plus the article outline. When a
/// verification report is attached, outline blocks carry their findings and
/// markers that matched no block surface in `unanchored_findings`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewOpenResponse {
    pub contract_version: u32,
    pub session: ReviewSessionSnapshot,
    pub outline: Vec<ReviewBlockOutline>,
    /// Attached findings whose `ref_id` matched no outline block.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unanchored_findings: Vec<ReviewFindingMarker>,
    pub next_step: String,
}

/// Agent attaches a verification report's findings to the session (the
/// report becomes an in-article overlay for the operator surface).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewFindingsRequest {
    pub wiki_id: String,
    pub title: String,
    /// Revision the findings were produced against; must match the
    /// session's pinned revision.
    #[serde(default)]
    pub rev_id: u64,
    pub findings: Vec<ReviewFindingMarker>,
}

/// Findings-attach acknowledgement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewFindingsResponse {
    pub contract_version: u32,
    pub session: ReviewSessionSnapshot,
    pub attached: usize,
}

/// Operator queues prompts (the browser surface's send action).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewQueueRequest {
    pub wiki_id: String,
    pub title: String,
    pub prompts: Vec<ReviewPrompt>,
    /// "Send & end": queue and close in one action.
    #[serde(default)]
    pub end_session: bool,
}

/// Queue acknowledgement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewQueueResponse {
    pub contract_version: u32,
    pub session: ReviewSessionSnapshot,
    pub queued: usize,
}

/// Agent polls for feedback, optionally bounding the server-side wait.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewPollRequest {
    pub wiki_id: String,
    pub title: String,
    /// Server-side wait bound in milliseconds; `0` returns immediately.
    /// The server clamps this to its own maximum.
    #[serde(default)]
    pub wait_ms: u64,
}

/// Poll outcome status on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewPollStatus {
    Waiting,
    Feedback,
    Ended,
    Missing,
}

/// Poll response. `next_step` carries the loop etiquette so agents do not
/// have to invent it: keep polling on `waiting`, apply feedback then poll
/// again with a reply on `feedback`, and stop (without reopening uninvited
/// when the operator ended) on `ended`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewPollResponse {
    pub contract_version: u32,
    pub status: ReviewPollStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prompts: Vec<ReviewPrompt>,
    #[serde(default)]
    pub session_ended: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_by: Option<ReviewEndedBy>,
    pub next_step: String,
}

/// Agent chat reply shown on the operator surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewReplyRequest {
    pub wiki_id: String,
    pub title: String,
    pub text: String,
}

/// Agent-initiated session end.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewEndRequest {
    pub wiki_id: String,
    pub title: String,
}

/// Generic acknowledgement for reply/end.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewAckResponse {
    pub contract_version: u32,
    pub session: ReviewSessionSnapshot,
}

/// Session listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewSessionsResponse {
    pub contract_version: u32,
    pub sessions: Vec<ReviewSessionSnapshot>,
}

/// `next_step` for a fresh or resumed open. Mentions the findings overlay
/// when one is attached so the agent tells the operator what to look at.
#[must_use]
pub fn open_next_step(title: &str, findings: usize) -> String {
    let overlay = if findings == 0 {
        String::new()
    } else {
        format!(" The outline carries {findings} verification finding(s) anchored to blocks.")
    };
    format!(
        "Session open on \"{title}\".{overlay} Ask the operator to review it in the SP42 browser \
         shell, then run `sp42-cli review poll` and wait for their feedback before responding \
         further."
    )
}

/// `next_step` for a poll outcome.
#[must_use]
pub fn poll_next_step(take: &ReviewFeedbackTake, title: &str) -> String {
    match take {
        ReviewFeedbackTake::Waiting => format!(
            "No feedback on \"{title}\" yet. Poll again (the wait is safe to re-run; queued \
             feedback is never lost)."
        ),
        ReviewFeedbackTake::Feedback {
            session_ended: false,
            ..
        } => format!(
            "Apply the operator's feedback on \"{title}\", then poll again with an agent reply \
             summarizing what changed."
        ),
        ReviewFeedbackTake::Feedback {
            session_ended: true,
            ..
        } => format!(
            "Final feedback batch: the operator ended the review of \"{title}\". Apply the \
             feedback and deliver remaining updates in chat; do not reopen unless asked."
        ),
        ReviewFeedbackTake::Ended {
            ended_by: ReviewEndedBy::Operator,
        } => format!(
            "The operator ended the review of \"{title}\". Stop polling and do not reopen \
             uninvited; deliver any remaining updates in chat."
        ),
        ReviewFeedbackTake::Ended {
            ended_by: ReviewEndedBy::Agent,
        } => format!(
            "Review of \"{title}\" was ended by the agent. Stop polling; open a new session if \
             further review is needed."
        ),
    }
}

/// The wire status for a drained take.
#[must_use]
pub fn poll_status(take: &ReviewFeedbackTake) -> ReviewPollStatus {
    match take {
        ReviewFeedbackTake::Waiting => ReviewPollStatus::Waiting,
        ReviewFeedbackTake::Feedback { .. } => ReviewPollStatus::Feedback,
        ReviewFeedbackTake::Ended { .. } => ReviewPollStatus::Ended,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        REVIEW_OUTLINE_TEXT_LIMIT, ReopenRefused, ReviewEndedBy, ReviewFeedbackTake, ReviewPrompt,
        ReviewPromptKind, ReviewSession, ReviewSessionStatus, annotate_outline,
        build_article_outline, open_next_step, poll_next_step, poll_status,
    };
    use crate::wikitext_editor::{BlockKind, BlockRef, ParsoidBlock};

    fn message_prompt(text: &str) -> ReviewPrompt {
        ReviewPrompt {
            kind: ReviewPromptKind::Message,
            prompt: text.to_string(),
            anchor: None,
        }
    }

    #[test]
    fn queueing_prompts_moves_the_session_to_feedback() {
        let mut session = ReviewSession::open("frwiki", "Exemple", 42, 1_000);

        session.queue_prompts(vec![message_prompt("tighten the lede")], false, 2_000);

        assert_eq!(session.status, ReviewSessionStatus::Feedback);
        assert_eq!(session.queued_prompts.len(), 1);
        assert_eq!(session.updated_at_ms, 2_000);
    }

    #[test]
    fn take_feedback_drains_once_and_reopens() {
        let mut session = ReviewSession::open("frwiki", "Exemple", 42, 1_000);
        session.queue_prompts(vec![message_prompt("first")], false, 2_000);

        let take = session.take_feedback(3_000);
        let ReviewFeedbackTake::Feedback {
            prompts,
            session_ended,
            ..
        } = take
        else {
            panic!("expected feedback");
        };
        assert_eq!(prompts.len(), 1);
        assert!(!session_ended);
        assert_eq!(session.status, ReviewSessionStatus::Open);
        assert_eq!(session.take_feedback(4_000), ReviewFeedbackTake::Waiting);
    }

    #[test]
    fn feedback_queued_before_an_end_delivers_before_ended() {
        let mut session = ReviewSession::open("frwiki", "Exemple", 42, 1_000);
        session.queue_prompts(vec![message_prompt("last words")], true, 2_000);

        let take = session.take_feedback(3_000);
        let ReviewFeedbackTake::Feedback {
            session_ended,
            ended_by,
            ..
        } = take
        else {
            panic!("expected the final feedback batch");
        };
        assert!(session_ended);
        assert_eq!(ended_by, Some(ReviewEndedBy::Operator));

        assert_eq!(
            session.take_feedback(4_000),
            ReviewFeedbackTake::Ended {
                ended_by: ReviewEndedBy::Operator
            }
        );
    }

    #[test]
    fn operator_end_gates_a_plain_reopen_but_agent_end_does_not() {
        let mut operator_ended = ReviewSession::open("frwiki", "Exemple", 42, 1_000);
        operator_ended.end(ReviewEndedBy::Operator, 2_000);
        assert_eq!(operator_ended.gate_reopen(false), Err(ReopenRefused));
        assert_eq!(operator_ended.gate_reopen(true), Ok(()));

        let mut agent_ended = ReviewSession::open("frwiki", "Exemple", 42, 1_000);
        agent_ended.end(ReviewEndedBy::Agent, 2_000);
        assert_eq!(agent_ended.gate_reopen(false), Ok(()));
    }

    #[test]
    fn resume_clears_the_end_and_keeps_queued_feedback() {
        let mut session = ReviewSession::open("frwiki", "Exemple", 42, 1_000);
        session.queue_prompts(vec![message_prompt("pending")], true, 2_000);

        session.resume(43, 3_000);

        assert_eq!(session.status, ReviewSessionStatus::Feedback);
        assert_eq!(session.ended_by, None);
        assert_eq!(session.rev_id, 43);
        assert_eq!(session.queued_prompts.len(), 1);
    }

    #[test]
    fn canonical_key_separates_wiki_and_title_unambiguously() {
        assert_ne!(
            ReviewSession::canonical_key("a", "b:c"),
            ReviewSession::canonical_key("a:b", "c")
        );
    }

    #[test]
    fn outline_truncates_on_character_boundaries() {
        let long_text = "é".repeat(REVIEW_OUTLINE_TEXT_LIMIT + 5);
        let blocks = vec![ParsoidBlock {
            text: long_text,
            refs: vec![BlockRef {
                offset: 3,
                ref_id: "cite_ref-a_1-0".to_string(),
                sources: Vec::new(),
                book_sources: Vec::new(),
                ref_text: "[1]".to_string(),
                named: false,
                is_bare_url_ref: false,
            }],
            block_kind: BlockKind::Paragraph,
            block_ordinal: 7,
        }];

        let outline = build_article_outline(&blocks);

        assert_eq!(outline.len(), 1);
        assert_eq!(outline[0].block_ordinal, 7);
        assert_eq!(outline[0].kind, "paragraph");
        assert_eq!(
            outline[0].text.chars().count(),
            REVIEW_OUTLINE_TEXT_LIMIT + 1,
            "limit characters plus the ellipsis"
        );
        assert!(outline[0].text.ends_with('…'));
        assert_eq!(outline[0].ref_ids, vec!["cite_ref-a_1-0".to_string()]);
    }

    #[test]
    fn outline_passes_short_text_through_untruncated() {
        let blocks = vec![ParsoidBlock {
            text: "short".to_string(),
            refs: Vec::new(),
            block_kind: BlockKind::ListItem,
            block_ordinal: 0,
        }];

        let outline = build_article_outline(&blocks);

        assert_eq!(outline[0].text, "short");
        assert_eq!(outline[0].kind, "list-item");
    }

    #[test]
    fn next_step_matches_the_loop_etiquette() {
        let waiting = poll_next_step(&ReviewFeedbackTake::Waiting, "Exemple");
        assert!(waiting.contains("Poll again"));

        let operator_ended = poll_next_step(
            &ReviewFeedbackTake::Ended {
                ended_by: ReviewEndedBy::Operator,
            },
            "Exemple",
        );
        assert!(operator_ended.contains("do not reopen"));

        let agent_ended = poll_next_step(
            &ReviewFeedbackTake::Ended {
                ended_by: ReviewEndedBy::Agent,
            },
            "Exemple",
        );
        assert!(agent_ended.contains("new session"));
    }

    #[test]
    fn poll_status_maps_every_take() {
        use super::ReviewPollStatus;
        assert_eq!(
            poll_status(&ReviewFeedbackTake::Waiting),
            ReviewPollStatus::Waiting
        );
        assert_eq!(
            poll_status(&ReviewFeedbackTake::Feedback {
                prompts: Vec::new(),
                session_ended: false,
                ended_by: None,
            }),
            ReviewPollStatus::Feedback
        );
        assert_eq!(
            poll_status(&ReviewFeedbackTake::Ended {
                ended_by: ReviewEndedBy::Agent
            }),
            ReviewPollStatus::Ended
        );
    }

    #[test]
    fn attach_findings_replaces_the_overlay_and_counts() {
        let mut session = ReviewSession::open("frwiki", "Exemple", 42, 1_000);

        let attached = session
            .attach_findings(42, vec![marker("cite_ref-a_1-0", "unsupported")], 2_000)
            .expect("matching revision should attach");
        assert_eq!(attached, 1);
        assert_eq!(session.findings.len(), 1);
        assert_eq!(session.updated_at_ms, 2_000);

        // A later attach replaces the overlay wholesale — findings describe
        // one report, they do not accumulate across runs.
        let attached = session
            .attach_findings(
                42,
                vec![
                    marker("cite_ref-b_2-0", "partial"),
                    marker("cite_ref-c_3-0", "source_unavailable"),
                ],
                3_000,
            )
            .expect("matching revision should attach");
        assert_eq!(attached, 2);
        assert_eq!(session.findings.len(), 2);
        assert_eq!(session.findings[0].ref_id, "cite_ref-b_2-0");
    }

    #[test]
    fn attach_findings_refuses_a_revision_mismatch() {
        let mut session = ReviewSession::open("frwiki", "Exemple", 42, 1_000);

        let refused = session
            .attach_findings(41, vec![marker("cite_ref-a_1-0", "unsupported")], 2_000)
            .expect_err("stale-revision findings must not overlay the outline");
        assert_eq!(refused.session_rev, 42);
        assert!(session.findings.is_empty());
    }

    #[test]
    fn resume_to_a_new_revision_drops_stale_findings() {
        let mut session = ReviewSession::open("frwiki", "Exemple", 42, 1_000);
        session
            .attach_findings(42, vec![marker("cite_ref-a_1-0", "unsupported")], 2_000)
            .expect("attach");
        session.end(ReviewEndedBy::Agent, 3_000);

        session.resume(43, 4_000);
        assert!(
            session.findings.is_empty(),
            "findings for revision 42 must not overlay revision 43"
        );

        let mut same_rev = ReviewSession::open("frwiki", "Exemple", 42, 1_000);
        same_rev
            .attach_findings(42, vec![marker("cite_ref-a_1-0", "unsupported")], 2_000)
            .expect("attach");
        same_rev.resume(42, 3_000);
        assert_eq!(
            same_rev.findings.len(),
            1,
            "same-revision resume keeps the overlay"
        );
    }

    #[test]
    fn annotate_outline_joins_findings_onto_blocks_by_ref_id() {
        let outline = vec![
            outline_block(0, vec!["cite_ref-a_1-0"]),
            outline_block(1, vec!["cite_ref-b_2-0", "cite_ref-c_3-0"]),
        ];
        let findings = vec![
            marker("cite_ref-c_3-0", "unsupported"),
            marker("cite_ref-ghost_9-0", "partial"),
        ];

        let (annotated, unanchored) = annotate_outline(outline, &findings);

        assert!(annotated[0].findings.is_empty());
        assert_eq!(annotated[1].findings.len(), 1);
        assert_eq!(annotated[1].findings[0].verdict, "unsupported");
        assert_eq!(unanchored.len(), 1, "unmatched markers surface, never drop");
        assert_eq!(unanchored[0].ref_id, "cite_ref-ghost_9-0");
    }

    #[test]
    fn snapshot_counts_attached_findings() {
        let mut session = ReviewSession::open("frwiki", "Exemple", 42, 1_000);
        session
            .attach_findings(42, vec![marker("cite_ref-a_1-0", "unsupported")], 2_000)
            .expect("attach");
        assert_eq!(session.snapshot().findings, 1);
    }

    #[test]
    fn open_next_step_mentions_an_attached_overlay() {
        assert!(!open_next_step("Exemple", 0).contains("finding"));
        assert!(open_next_step("Exemple", 3).contains("3 verification finding"));
    }

    fn marker(ref_id: &str, verdict: &str) -> super::ReviewFindingMarker {
        super::ReviewFindingMarker {
            ref_id: ref_id.to_string(),
            verdict: verdict.to_string(),
            claim: "a claim".to_string(),
            detail: None,
        }
    }

    fn outline_block(block_ordinal: usize, ref_ids: Vec<&str>) -> super::ReviewBlockOutline {
        super::ReviewBlockOutline {
            block_ordinal,
            kind: "paragraph".to_string(),
            text: "text".to_string(),
            ref_ids: ref_ids.into_iter().map(str::to_string).collect(),
            findings: Vec::new(),
        }
    }

    #[test]
    fn wire_types_round_trip_with_the_contract_version() {
        let response = super::ReviewPollResponse {
            contract_version: super::REVIEW_SESSION_CONTRACT_VERSION,
            status: super::ReviewPollStatus::Feedback,
            prompts: vec![ReviewPrompt {
                kind: ReviewPromptKind::Text,
                prompt: "check this quote".to_string(),
                anchor: Some(super::ReviewAnchor {
                    block_ordinal: 3,
                    ref_id: Some("cite_ref-x_2-0".to_string()),
                    selected_text: Some("the quote".to_string()),
                }),
            }],
            session_ended: false,
            ended_by: None,
            next_step: "apply".to_string(),
        };

        let json = serde_json::to_string(&response).expect("poll response should serialize");
        let parsed: super::ReviewPollResponse =
            serde_json::from_str(&json).expect("poll response should deserialize");

        assert_eq!(parsed, response);
        assert_eq!(parsed.contract_version, 1);
    }
}
