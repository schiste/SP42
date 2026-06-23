//! Shared HTTP route contracts for SP42 server, clients, and diagnostics.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteMethod {
    Get,
    Post,
    Put,
    Delete,
}

impl RouteMethod {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteDefinition {
    pub method: RouteMethod,
    pub path: String,
    pub purpose: &'static str,
    pub available: bool,
}

impl RouteDefinition {
    #[must_use]
    pub fn new(method: RouteMethod, path: impl Into<String>, purpose: &'static str) -> Self {
        Self {
            method,
            path: path.into(),
            purpose,
            available: true,
        }
    }
}

pub const AUTH_LOGIN_PATH: &str = "/auth/login";
pub const AUTH_CALLBACK_PATH: &str = "/auth/callback";
pub const AUTH_SESSION_PATH: &str = "/auth/session";
pub const AUTH_LOGOUT_PATH: &str = "/auth/logout";

pub const HEALTHZ_PATH: &str = "/healthz";
pub const DEBUG_SUMMARY_PATH: &str = "/debug/summary";
pub const DEBUG_RUNTIME_PATH: &str = "/debug/runtime";
pub const OPERATOR_READINESS_PATH: &str = "/operator/readiness";
pub const OPERATOR_REPORT_PATH: &str = "/operator/report";
pub const OPERATOR_LIVE_PATTERN: &str = "/operator/live/{wiki_id}";
pub const OPERATOR_ARTICLE_PATTERN: &str = "/operator/article/{wiki_id}";
pub const OPERATOR_DIFF_PATTERN: &str = "/operator/diff/{wiki_id}/{rev_id}/{old_rev_id}";
pub const OPERATOR_MEDIA_DIFF_PATTERN: &str =
    "/operator/media-diff/{wiki_id}/{rev_id}/{old_rev_id}";
pub const OPERATOR_RENDERED_HUNK_PATTERN: &str =
    "/operator/rendered-hunk/{wiki_id}/{rev_id}/{old_rev_id}/{hunk_index}";
pub const OPERATOR_RUNTIME_PATTERN: &str = "/operator/runtime/{wiki_id}";
pub const OPERATOR_STORAGE_LAYOUT_PATH: &str = "/operator/storage/layout";
pub const OPERATOR_STORAGE_LAYOUT_PATTERN: &str = "/operator/storage/layout/{wiki_id}";
pub const OPERATOR_STORAGE_DOCUMENT_PATTERN: &str = "/operator/storage/document/{wiki_id}";
pub const OPERATOR_STORAGE_LOGICAL_PATTERN: &str =
    "/operator/storage/logical/{wiki_id}/{realm}/{kind}";
pub const OPERATOR_STORAGE_PUBLIC_PATTERN: &str = "/operator/storage/public/{wiki_id}/{kind}";

pub const COORDINATION_ROOMS_PATH: &str = "/coordination/rooms";
pub const COORDINATION_ROOM_PATTERN: &str = "/coordination/rooms/{wiki_id}";
pub const COORDINATION_ROOM_INSPECTION_PATTERN: &str = "/coordination/rooms/{wiki_id}/inspection";
pub const COORDINATION_INSPECTIONS_PATH: &str = "/coordination/inspections";
pub const COORDINATION_WS_PATTERN: &str = "/ws/{wiki_id}";

pub const DEV_AUTH_SESSION_PATH: &str = "/dev/auth/session";
pub const DEV_AUTH_CAPABILITIES_PATTERN: &str = "/dev/auth/capabilities/{wiki_id}";
pub const DEV_AUTH_BOOTSTRAP_SESSION_PATH: &str = "/dev/auth/session/bootstrap";
pub const DEV_AUTH_BOOTSTRAP_STATUS_PATH: &str = "/dev/auth/bootstrap/status";
pub const DEV_ACTION_EXECUTE_PATH: &str = "/dev/actions/execute";
pub const DEV_ACTION_STATUS_PATH: &str = "/dev/actions/status";
pub const DEV_ACTION_HISTORY_PATH: &str = "/dev/actions/history";
pub const DEV_CITATION_BARE_URL_PROPOSALS_PATH: &str = "/dev/citation/bare-url-proposals";
pub const DEV_CITATION_BARE_URL_APPLY_PATH: &str = "/dev/citation/bare-url-apply";

/// Header carrying the bridge session's CSRF token on state-changing routes.
pub const CSRF_HEADER_NAME: &str = "x-sp42-csrf-token";

pub const DEV_AUTH_ACTION_STATUS_PATH: &str = DEV_ACTION_STATUS_PATH;
pub const DEV_AUTH_ACTION_HISTORY_PATH: &str = DEV_ACTION_HISTORY_PATH;
pub const ACTION_STATUS_PATH: &str = DEV_ACTION_STATUS_PATH;
pub const ACTION_HISTORY_PATH: &str = DEV_ACTION_HISTORY_PATH;

pub const MANIFEST_JSON_PATH: &str = "/manifest.json";
pub const RUNTIME_CONFIG_JS_PATH: &str = "/runtime-config.js";
pub const SERVICE_WORKER_PATH: &str = "/sw.js";
pub const OFFLINE_HTML_PATH: &str = "/offline.html";
pub const ICON_PATTERN: &str = "/icons/{icon_name}";
pub const SP42_ICON_192_PATH: &str = "/icons/sp42-icon-192.svg";
pub const SP42_ICON_512_PATH: &str = "/icons/sp42-icon-512.svg";
pub const FAVICON_PATH: &str = "/favicon.ico";

#[must_use]
pub fn operator_live_path(wiki_id: &str) -> String {
    format!("/operator/live/{wiki_id}")
}

#[must_use]
pub fn operator_live_path_with_query(wiki_id: &str, query: &str) -> String {
    with_optional_query(operator_live_path(wiki_id), query)
}

#[must_use]
pub fn operator_article_path(wiki_id: &str) -> String {
    format!("/operator/article/{wiki_id}")
}

#[must_use]
pub fn operator_diff_path(wiki_id: &str, rev_id: u64, old_rev_id: u64) -> String {
    format!("/operator/diff/{wiki_id}/{rev_id}/{old_rev_id}")
}

#[must_use]
pub fn operator_media_diff_path(wiki_id: &str, rev_id: u64, old_rev_id: u64) -> String {
    format!("/operator/media-diff/{wiki_id}/{rev_id}/{old_rev_id}")
}

#[must_use]
pub fn operator_rendered_hunk_path(
    wiki_id: &str,
    rev_id: u64,
    old_rev_id: u64,
    hunk_index: usize,
) -> String {
    format!("/operator/rendered-hunk/{wiki_id}/{rev_id}/{old_rev_id}/{hunk_index}")
}

#[must_use]
pub fn operator_runtime_path(wiki_id: &str) -> String {
    format!("/operator/runtime/{wiki_id}")
}

#[must_use]
pub fn operator_storage_layout_path(wiki_id: &str) -> String {
    format!("/operator/storage/layout/{wiki_id}")
}

#[must_use]
pub fn operator_storage_document_path(wiki_id: &str) -> String {
    format!("/operator/storage/document/{wiki_id}")
}

#[must_use]
pub fn operator_storage_logical_path(wiki_id: &str, realm: &str, kind: &str) -> String {
    format!("/operator/storage/logical/{wiki_id}/{realm}/{kind}")
}

#[must_use]
pub fn operator_storage_public_path(wiki_id: &str, kind: &str) -> String {
    format!("/operator/storage/public/{wiki_id}/{kind}")
}

#[must_use]
pub fn coordination_room_path(wiki_id: &str) -> String {
    format!("/coordination/rooms/{wiki_id}")
}

#[must_use]
pub fn coordination_room_inspection_path(wiki_id: &str) -> String {
    format!("/coordination/rooms/{wiki_id}/inspection")
}

#[must_use]
pub fn coordination_ws_path(wiki_id: &str) -> String {
    format!("/ws/{wiki_id}")
}

#[must_use]
pub fn dev_auth_capabilities_path(wiki_id: &str) -> String {
    format!("/dev/auth/capabilities/{wiki_id}")
}

#[must_use]
pub fn dev_action_history_path_with_limit(limit: usize) -> String {
    format!("{DEV_ACTION_HISTORY_PATH}?limit={}", limit.max(1))
}

#[must_use]
pub fn with_optional_query(path: impl Into<String>, query: &str) -> String {
    let path = path.into();
    if query.is_empty() {
        path
    } else {
        format!("{path}?{query}")
    }
}

#[must_use]
pub fn operator_endpoint_routes(default_wiki_id: &str) -> Vec<RouteDefinition> {
    let mut endpoints = operator_core_endpoints();
    endpoints.extend(operator_storage_endpoints());
    endpoints.extend(operator_dev_endpoints(default_wiki_id));
    endpoints
}

fn operator_core_endpoints() -> Vec<RouteDefinition> {
    vec![
        RouteDefinition::new(
            RouteMethod::Get,
            HEALTHZ_PATH,
            "Minimal health indicator for probes and process supervisors.",
        ),
        RouteDefinition::new(
            RouteMethod::Get,
            DEBUG_SUMMARY_PATH,
            "Shared auth, capability, and coordination summary.",
        ),
        RouteDefinition::new(
            RouteMethod::Get,
            DEBUG_RUNTIME_PATH,
            "Runtime-oriented operator state with cache and room counts.",
        ),
        RouteDefinition::new(
            RouteMethod::Get,
            OPERATOR_READINESS_PATH,
            "Consolidated operator readiness report.",
        ),
        RouteDefinition::new(
            RouteMethod::Get,
            OPERATOR_REPORT_PATH,
            "Full operator report with debug summary and endpoint manifest.",
        ),
        RouteDefinition::new(
            RouteMethod::Get,
            OPERATOR_LIVE_PATTERN,
            "Authoritative live patrol queue, selected review details, backend auth status, and shell state.",
        ),
        RouteDefinition::new(
            RouteMethod::Get,
            OPERATOR_RUNTIME_PATTERN,
            "Persistent backlog and stream checkpoint inspection for the selected wiki.",
        ),
        RouteDefinition::new(
            RouteMethod::Get,
            OPERATOR_STORAGE_LAYOUT_PATTERN,
            "Canonical personal/shared on-wiki storage layout and sample page renderings.",
        ),
    ]
}

fn operator_storage_endpoints() -> Vec<RouteDefinition> {
    vec![
        RouteDefinition::new(
            RouteMethod::Get,
            format!("{OPERATOR_STORAGE_DOCUMENT_PATTERN}?title=..."),
            "Load a canonical public SP42 on-wiki document and parse its machine payload.",
        ),
        RouteDefinition::new(
            RouteMethod::Put,
            OPERATOR_STORAGE_DOCUMENT_PATTERN,
            "Save a canonical public SP42 on-wiki document with conflict-aware writes.",
        ),
        RouteDefinition::new(
            RouteMethod::Get,
            OPERATOR_STORAGE_LOGICAL_PATTERN,
            "Resolve a canonical SP42 public document by logical kind and load its current on-wiki content.",
        ),
        RouteDefinition::new(
            RouteMethod::Put,
            OPERATOR_STORAGE_LOGICAL_PATTERN,
            "Save a canonical SP42 public document by logical kind without exposing raw wiki titles to clients.",
        ),
        RouteDefinition::new(
            RouteMethod::Get,
            OPERATOR_STORAGE_PUBLIC_PATTERN,
            "Load a typed public SP42 document like preferences, registry, team, rules, or audit ledger.",
        ),
        RouteDefinition::new(
            RouteMethod::Put,
            OPERATOR_STORAGE_PUBLIC_PATTERN,
            "Save a typed public SP42 document while keeping durable state on canonical wiki pages.",
        ),
    ]
}

fn operator_dev_endpoints(default_wiki_id: &str) -> Vec<RouteDefinition> {
    vec![
        RouteDefinition::new(
            RouteMethod::Get,
            DEV_AUTH_BOOTSTRAP_STATUS_PATH,
            "Authoritative local token bootstrap and source-report status.",
        ),
        RouteDefinition::new(
            RouteMethod::Get,
            dev_auth_capabilities_path(default_wiki_id),
            "Capability probe for the configured default wiki.",
        ),
        RouteDefinition::new(
            RouteMethod::Get,
            DEV_ACTION_STATUS_PATH,
            "Current shell feedback and latest action result.",
        ),
        RouteDefinition::new(
            RouteMethod::Get,
            DEV_ACTION_HISTORY_PATH,
            "Recent local action execution history.",
        ),
        RouteDefinition::new(
            RouteMethod::Get,
            COORDINATION_ROOMS_PATH,
            "Coordination room inventory and summaries.",
        ),
        RouteDefinition::new(
            RouteMethod::Get,
            COORDINATION_INSPECTIONS_PATH,
            "Room-by-room coordination inspection collection.",
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::{
        ACTION_HISTORY_PATH, DEV_ACTION_HISTORY_PATH, OPERATOR_LIVE_PATTERN,
        dev_action_history_path_with_limit, operator_diff_path, operator_endpoint_routes,
        operator_live_path_with_query,
    };

    #[test]
    fn builders_match_existing_paths() {
        assert_eq!(
            operator_live_path_with_query("frwiki", "limit=1"),
            "/operator/live/frwiki?limit=1"
        );
        assert_eq!(
            operator_diff_path("frwiki", 42, 41),
            "/operator/diff/frwiki/42/41"
        );
        assert_eq!(
            dev_action_history_path_with_limit(0),
            "/dev/actions/history?limit=1"
        );
    }

    #[test]
    fn compatibility_aliases_share_one_contract() {
        assert_eq!(ACTION_HISTORY_PATH, DEV_ACTION_HISTORY_PATH);
    }

    #[test]
    fn endpoint_routes_include_templates_and_default_wiki_examples() {
        let endpoints = operator_endpoint_routes("frwiki");

        assert!(
            endpoints
                .iter()
                .any(|entry| entry.path == OPERATOR_LIVE_PATTERN)
        );
        assert!(
            endpoints
                .iter()
                .any(|entry| entry.path == "/dev/auth/capabilities/frwiki")
        );
    }
}
