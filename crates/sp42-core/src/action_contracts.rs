//! Shared `MediaWiki` action contracts.
//!
//! This module owns the pure request, response, and retry contracts for actions.
//! Server-side session validation and token handling live outside this boundary.

use serde::{Deserialize, Serialize};

use crate::types::FlagState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RollbackRequest {
    pub title: String,
    pub user: String,
    pub token: String,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatrolRequest {
    pub rev_id: u64,
    pub token: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoRequest {
    pub title: String,
    pub undo_rev_id: u64,
    pub undo_after_rev_id: u64,
    pub token: String,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikiPageSaveRequest {
    pub title: String,
    pub text: String,
    pub token: String,
    pub summary: Option<String>,
    pub baserevid: Option<u64>,
    pub tags: Vec<String>,
    pub watchlist: Option<String>,
    pub create_only: FlagState,
    pub minor: FlagState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Rollback,
    Patrol,
    Csrf,
}

impl TokenKind {
    #[must_use]
    pub const fn api_value(self) -> &'static str {
        match self {
            Self::Rollback => "rollback",
            Self::Patrol => "patrol",
            Self::Csrf => "csrf",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionActionKind {
    Rollback,
    Patrol,
    Undo,
    TagCitationNeeded,
    InlineEdit,
}

impl SessionActionKind {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Rollback => "rollback",
            Self::Patrol => "patrol",
            Self::Undo => "undo",
            Self::TagCitationNeeded => "tag-citation-needed",
            Self::InlineEdit => "inline-edit",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionActionExecutionRequest {
    pub wiki_id: String,
    pub kind: SessionActionKind,
    pub rev_id: u64,
    pub title: Option<String>,
    pub target_user: Option<String>,
    pub undo_after_rev_id: Option<u64>,
    pub summary: Option<String>,
    #[serde(default)]
    pub selected_text: Option<String>,
    #[serde(default)]
    pub batch_rev_ids: Option<Vec<u64>>,
    #[serde(default)]
    pub replacement_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionActionExecutionResponse {
    pub wiki_id: String,
    pub kind: SessionActionKind,
    pub rev_id: u64,
    pub accepted: bool,
    pub actor: Option<String>,
    pub http_status: Option<u16>,
    pub api_code: Option<String>,
    #[serde(default)]
    pub retryable: bool,
    #[serde(default)]
    pub warnings: Vec<String>,
    pub result: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionResponseSummary {
    pub status: u16,
    pub warnings: Vec<String>,
    pub result: Option<String>,
    pub error: Option<String>,
    pub api_code: Option<String>,
    pub retryable: bool,
    pub nochange: bool,
}

#[must_use]
pub fn is_retryable_action_api_error(code: &str) -> bool {
    matches!(
        code,
        "maxlag"
            | "readonly"
            | "ratelimited"
            | "internal_api_error_DBQueryError"
            | "internal_api_error_DBConnectionError"
            | "internal_api_error_Exception"
            | "failed-save"
    )
}

#[cfg(test)]
mod tests {
    use super::{
        SessionActionExecutionRequest, SessionActionExecutionResponse, SessionActionKind,
        is_retryable_action_api_error,
    };

    #[test]
    fn session_action_contract_serializes_without_token_material() {
        let request = SessionActionExecutionRequest {
            wiki_id: "frwiki".to_string(),
            kind: SessionActionKind::Rollback,
            rev_id: 123_456,
            title: Some("Example".to_string()),
            target_user: Some("ExampleUser".to_string()),
            selected_text: None,
            undo_after_rev_id: None,
            summary: Some("test note".to_string()),
            batch_rev_ids: None,
            replacement_text: None,
        };
        let response = SessionActionExecutionResponse {
            wiki_id: "frwiki".to_string(),
            kind: SessionActionKind::Rollback,
            rev_id: 123_456,
            accepted: true,
            actor: Some("Schiste".to_string()),
            http_status: Some(200),
            api_code: None,
            retryable: false,
            warnings: Vec::new(),
            result: Some("rollback=true".to_string()),
            message: Some("queued".to_string()),
        };

        let request_json = serde_json::to_string(&request).expect("request should serialize");
        let response_json = serde_json::to_string(&response).expect("response should serialize");

        assert!(request_json.contains("\"wiki_id\":\"frwiki\""));
        assert!(!request_json.contains("token"));
        assert!(response_json.contains("\"accepted\":true"));
    }

    #[test]
    fn identifies_retryable_mediawiki_api_codes() {
        assert!(is_retryable_action_api_error("maxlag"));
        assert!(is_retryable_action_api_error("readonly"));
        assert!(is_retryable_action_api_error("ratelimited"));
        assert!(is_retryable_action_api_error("failed-save"));
        assert!(!is_retryable_action_api_error("badtoken"));
        assert!(!is_retryable_action_api_error("permissiondenied"));
    }
}
