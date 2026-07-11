use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::Instant;

use sp42_core::{ActionExecutionLogEntry, DevAuthCapabilityReport, WikitextEditor};
use sp42_types::Clock;
use sp42_wiki::WikiRegistry;
use tokio::sync::RwLock;

use crate::coordination::CoordinationRegistry;
use crate::deployment::DeploymentConfig;
use crate::live_queue::IngestionSupervisorSnapshot;
use crate::local_env::LocalOAuthConfig;
use crate::review_routes::SharedReviewSessions;
use crate::revision_artifacts::{CachedRenderedHunkPreview, CachedRevisionArtifacts};
use crate::wikimedia_capabilities::CapabilityProbeTargets;

pub(crate) type SharedSessions = Arc<RwLock<HashMap<String, StoredSession>>>;
pub(crate) type SharedCapabilityCache = Arc<RwLock<Option<CachedCapabilityReport>>>;
pub(crate) type SharedPendingOAuthLogins = Arc<RwLock<HashMap<String, PendingOAuthLogin>>>;
pub(crate) type SharedIngestionSupervisor =
    Arc<RwLock<HashMap<String, IngestionSupervisorSnapshot>>>;
pub(crate) type SharedRevisionArtifactCache = Arc<RwLock<HashMap<String, CachedRevisionArtifacts>>>;
pub(crate) type SharedRenderedHunkCache = Arc<RwLock<HashMap<String, CachedRenderedHunkPreview>>>;

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) capability_cache: SharedCapabilityCache,
    pub(crate) sessions: SharedSessions,
    pub(crate) pending_oauth_logins: SharedPendingOAuthLogins,
    pub(crate) revision_artifacts: SharedRevisionArtifactCache,
    pub(crate) rendered_hunks: SharedRenderedHunkCache,
    pub(crate) http_client: reqwest::Client,
    pub(crate) local_oauth: LocalOAuthConfig,
    pub(crate) runtime_storage_root: PathBuf,
    pub(crate) ingestion_supervisor: SharedIngestionSupervisor,
    pub(crate) capability_targets: CapabilityProbeTargets,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) coordination: CoordinationRegistry,
    pub(crate) review_sessions: SharedReviewSessions,
    pub(crate) deployment: DeploymentConfig,
    pub(crate) wiki_registry: WikiRegistry,
    pub(crate) wikitext_editor: Arc<dyn WikitextEditor>,
    pub(crate) next_client_id: Arc<AtomicU64>,
    pub(crate) started_at: Instant,
}

impl AppState {
    pub(crate) fn default_wiki_id(&self) -> &str {
        self.wiki_registry.default_wiki_id()
    }

    /// The shared local dev access token, usable as a session identity ONLY in
    /// local deployment mode. This is the single gate for the env token: outside
    /// local mode per-user OAuth is the required identity, so every consumer —
    /// request fallback, capability probe, and the availability/bootstrap flags
    /// reported to clients — goes through here rather than reading the raw token
    /// directly. Returns `None` (token absent or non-local mode) accordingly.
    /// Codex review #90.
    pub(crate) fn shared_local_access_token(&self) -> Option<&str> {
        self.deployment
            .mode
            .permits_dev_token_bootstrap()
            .then(|| self.local_oauth.access_token())
            .flatten()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CachedCapabilityReport {
    pub(crate) fetched_at_ms: i64,
    pub(crate) report: DevAuthCapabilityReport,
}

#[derive(Debug, Clone)]
pub(crate) struct StoredSession {
    pub(crate) username: String,
    pub(crate) scopes: Vec<String>,
    pub(crate) expires_at_ms: Option<i64>,
    pub(crate) access_token: String,
    pub(crate) refresh_token: Option<String>,
    pub(crate) upstream_access_expires_at_ms: Option<i64>,
    pub(crate) bridge_mode: String,
    pub(crate) csrf_token: String,
    pub(crate) created_at_ms: i64,
    pub(crate) last_seen_at_ms: i64,
    pub(crate) capability_cache: HashMap<String, CachedCapabilityReport>,
    pub(crate) action_history: Vec<ActionExecutionLogEntry>,
}

#[derive(Debug, Clone)]
pub(crate) struct PendingOAuthLogin {
    pub(crate) wiki_id: String,
    pub(crate) state: String,
    pub(crate) verifier: String,
    pub(crate) redirect_uri: String,
    pub(crate) redirect_after_login: String,
    pub(crate) expires_at_ms: i64,
}

#[derive(Debug, Clone)]
pub(crate) struct SessionSnapshot {
    pub(crate) session_id: String,
    pub(crate) username: String,
    pub(crate) scopes: Vec<String>,
    pub(crate) expires_at_ms: Option<i64>,
    pub(crate) access_token: String,
    pub(crate) bridge_mode: String,
    pub(crate) csrf_token: String,
}
