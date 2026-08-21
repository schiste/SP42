mod action_routes;
mod auth_routes;
mod citation_routes;
mod coordination;
mod deployment;
mod endpoint_manifest;
mod http_errors;
mod ingestion_supervisor;
mod live_queue;
mod local_env;
mod oauth_runtime;
mod operator_live;
mod parsoid_editor;
mod review_routes;
mod revision_artifacts;
mod routes;
pub(crate) mod runtime_adapters;
mod runtime_status;
mod session_runtime;
mod state;
mod static_assets;
mod storage_routes;
mod wikimedia_capabilities;

use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use axum::Json;
use axum::extract::Path;
use axum::extract::Query;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use futures::{SinkExt, StreamExt};
use sp42_types::{Clock, HttpClient, HttpMethod, HttpRequest, SystemClock};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tracing::{info, warn};

use sp42_coordination::CoordinationSnapshot;
use sp42_core::{
    ArticleInventory, DevAuthCapabilityReport, DevAuthSessionStatus, FlagState,
    PublicStorageDocumentData, TokenKind, WikiConfig, WikiStorageConfig, WikiStorageDocument,
    WikiStorageDocumentKind, WikiStorageLoadedDocument, WikiStoragePlan, WikiStoragePlanInput,
    WikiStorageWriteOutcome, WikiStorageWriteRequest, build_article_inventory,
    build_wiki_storage_plan, execute_fetch_token, load_wiki_storage_document,
    render_wiki_storage_document_page, render_wiki_storage_index_page,
    resolve_wiki_storage_document, save_wiki_storage_document,
};
use sp42_live::LiveOperatorBackendStatus;
use sp42_wiki::WikiRegistry;

use crate::coordination::{CoordinationEnvelope, CoordinationRegistry};
use crate::deployment::DeploymentConfig;
use crate::http_errors::{gateway_error, invalid_payload, unauthorized_error};
use crate::local_env::LocalOAuthConfig;
use crate::routes::build_router;
use crate::runtime_adapters::{
    BearerHttpClient, build_http_client, init_tracing, runtime_storage_root,
};
use crate::runtime_status::{
    RoomInspectionCollection, ServerHealthStatus, empty_room_inspection, room_inspection,
};
use crate::session_runtime::{
    current_session_snapshot, prune_expired_sessions, session_expires_at_ms,
};
use crate::state::{AppState, CachedCapabilityReport, SessionSnapshot};
use crate::wikimedia_capabilities::{CapabilityProbeTargets, probe_with_targets};

const CAPABILITY_CACHE_TTL_MS: i64 = 30_000;

#[derive(Clone)]
struct AuthenticatedWikiContext {
    client: BearerHttpClient,
    config: WikiConfig,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
struct StorageLayoutQuery {
    username: Option<String>,
    home_wiki_id: Option<String>,
    shared_owner_username: Option<String>,
    team_slug: Option<String>,
    rule_set_slug: Option<String>,
    training_dataset_slug: Option<String>,
    audit_period_slug: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct OperatorStorageLayoutView {
    plan: WikiStoragePlan,
    personal_index_page: String,
    shared_registry_page: String,
    sample_document_pages: Vec<RenderedStorageDocumentPage>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct RenderedStorageDocumentPage {
    title: String,
    body: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
struct StorageDocumentQuery {
    title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum StorageDocumentRealmInput {
    Personal,
    Shared,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum StorageDocumentKindInput {
    Index,
    Profile,
    Preferences,
    Queue,
    Workspace,
    Labels,
    Registry,
    Team,
    RuleSet,
    TrainingDataset,
    AuditPeriod,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
struct LogicalStorageDocumentQuery {
    username: Option<String>,
    home_wiki_id: Option<String>,
    shared_owner_username: Option<String>,
    slug: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
struct StorageDocumentSavePayload {
    document: sp42_core::WikiStorageDocument,
    #[serde(default)]
    human_summary: Vec<String>,
    data: serde_json::Value,
    baserevid: Option<u64>,
    #[serde(default)]
    tags: Vec<String>,
    watchlist: Option<String>,
    create_only: FlagState,
    minor: FlagState,
    summary: Option<String>,
}

#[derive(Debug, Clone)]
struct StoragePlanRequest {
    username_override: Option<String>,
    home_wiki_id_override: Option<String>,
    shared_owner_username_override: Option<String>,
    team_slugs: Vec<String>,
    rule_set_slugs: Vec<String>,
    training_dataset_slugs: Vec<String>,
    audit_period_slugs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
struct LogicalStorageDocumentSavePayload {
    #[serde(default)]
    human_summary: Vec<String>,
    data: serde_json::Value,
    baserevid: Option<u64>,
    #[serde(default)]
    tags: Vec<String>,
    watchlist: Option<String>,
    create_only: FlagState,
    minor: FlagState,
    summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
struct LogicalStorageDocumentView {
    document: WikiStorageDocument,
    loaded: WikiStorageLoadedDocument,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
struct LogicalStorageDocumentWriteView {
    document: WikiStorageDocument,
    outcome: WikiStorageWriteOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum PublicStorageDocumentRouteKind {
    Preferences,
    Registry,
    Team,
    RuleSet,
    AuditPeriod,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
struct PublicStorageDocumentQuery {
    username: Option<String>,
    home_wiki_id: Option<String>,
    shared_owner_username: Option<String>,
    slug: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
struct PublicStorageDocumentSavePayload {
    payload: PublicStorageDocumentData,
    #[serde(default)]
    human_summary: Vec<String>,
    baserevid: Option<u64>,
    #[serde(default)]
    tags: Vec<String>,
    watchlist: Option<String>,
    create_only: FlagState,
    minor: FlagState,
    summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
struct PublicStorageDocumentView {
    document: WikiStorageDocument,
    loaded: WikiStorageLoadedDocument,
    payload: PublicStorageDocumentData,
    defaulted: FlagState,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
struct PublicStorageDocumentWriteView {
    document: WikiStorageDocument,
    payload: PublicStorageDocumentData,
    outcome: WikiStorageWriteOutcome,
}

#[derive(Debug, Clone)]
struct ResolvedPublicStorageDocument {
    document: WikiStorageDocument,
    loaded: WikiStorageLoadedDocument,
    payload: PublicStorageDocumentData,
    defaulted: FlagState,
}

async fn current_storage_document_on_conflict(
    client: &BearerHttpClient,
    config: &WikiConfig,
    title: &str,
) -> Option<WikiStorageLoadedDocument> {
    load_wiki_storage_document(client, config, title).await.ok()
}

async fn wiki_storage_save_error_response(
    client: &BearerHttpClient,
    config: &WikiConfig,
    document: &WikiStorageDocument,
    error: sp42_core::WikiStorageError,
) -> (StatusCode, Json<serde_json::Value>) {
    match error {
        sp42_core::WikiStorageError::Conflict { title, message } => {
            let current = current_storage_document_on_conflict(client, config, &title).await;
            (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": message,
                    "document": document,
                    "current": current,
                })),
            )
        }
        other => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({
                "error": other.to_string(),
                "document": document,
            })),
        ),
    }
}

#[tokio::main]
async fn main() -> Result<(), std::io::Error> {
    init_tracing();
    let bind_addr =
        std::env::var("SP42_BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:8788".to_string());
    let listener = TcpListener::bind(&bind_addr).await?;
    let local_addr = listener.local_addr()?;
    info!(
        bind = %local_addr,
        project = sp42_core::branding::PROJECT_NAME,
        "starting localhost server"
    );
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let deployment = DeploymentConfig::load().map_err(io::Error::other)?;
    let wiki_registry = WikiRegistry::load().map_err(io::Error::other)?;
    info!(
        deployment_mode = deployment.mode.as_str(),
        public_base_url = deployment.public_base_url.as_deref().unwrap_or(""),
        allowed_origin_count = deployment.allowed_origins.len(),
        "loaded runtime deployment configuration"
    );
    info!(
        default_wiki_id = wiki_registry.default_wiki_id(),
        wiki_count = wiki_registry.wiki_count(),
        source = wiki_registry.source(),
        "loaded wiki registry"
    );
    let state = AppState {
        capability_cache: Arc::new(RwLock::new(None)),
        sessions: Arc::new(RwLock::new(HashMap::new())),
        pending_oauth_logins: Arc::new(RwLock::new(HashMap::new())),
        revision_artifacts: Arc::new(RwLock::new(HashMap::new())),
        rendered_hunks: Arc::new(RwLock::new(HashMap::new())),
        http_client: build_http_client()?,
        local_oauth: LocalOAuthConfig::load(),
        runtime_storage_root: runtime_storage_root(),
        ingestion_supervisor: Arc::new(RwLock::new(HashMap::new())),
        capability_targets: CapabilityProbeTargets::default(),
        clock: clock.clone(),
        coordination: CoordinationRegistry::new(clock),
        review_sessions: review_routes::new_review_session_store(),
        deployment,
        wiki_registry,
        wikitext_editor: Arc::new(parsoid_editor::ParsoidWikitextEditor::new()),
        next_client_id: Arc::new(AtomicU64::new(1)),
        started_at: Instant::now(),
    };
    spawn_ingestion_supervisors(&state);
    let router = build_router(state);

    axum::serve(listener, router).await
}

fn spawn_ingestion_supervisors(state: &AppState) {
    ingestion_supervisor::spawn_ingestion_supervisors(state);
}

async fn authenticated_wiki_context(
    state: &AppState,
    headers: &HeaderMap,
    wiki_id: &str,
) -> Result<AuthenticatedWikiContext, (StatusCode, Json<serde_json::Value>)> {
    let access_token = access_token_for_request(state, headers)
        .await
        .ok_or_else(|| unauthorized_error("No authenticated Wikimedia session is active."))?;
    let config =
        resolved_wiki_config(state, wiki_id).map_err(|message| invalid_payload(&message))?;
    Ok(AuthenticatedWikiContext {
        client: BearerHttpClient::new(state.http_client.clone(), access_token),
        config,
    })
}

async fn required_csrf_token(
    context: &AuthenticatedWikiContext,
) -> Result<String, (StatusCode, Json<serde_json::Value>)> {
    execute_fetch_token(&context.client, &context.config, TokenKind::Csrf)
        .await
        .map_err(|error| gateway_error(format!("csrf token fetch failed: {error}")))
}

async fn load_storage_document_with_context(
    context: &AuthenticatedWikiContext,
    title: &str,
) -> Result<WikiStorageLoadedDocument, (StatusCode, Json<serde_json::Value>)> {
    load_wiki_storage_document(&context.client, &context.config, title)
        .await
        .map_err(|error| gateway_error(error.to_string()))
}

async fn save_storage_document_with_context(
    context: &AuthenticatedWikiContext,
    document: WikiStorageDocument,
    request: WikiStorageWriteRequest,
) -> Result<WikiStorageWriteOutcome, (StatusCode, Json<serde_json::Value>)> {
    match save_wiki_storage_document(&context.client, &context.config, &request).await {
        Ok(outcome) => Ok(outcome),
        Err(error) => Err(wiki_storage_save_error_response(
            &context.client,
            &context.config,
            &document,
            error,
        )
        .await),
    }
}

async fn coordination_socket(
    ws: WebSocketUpgrade,
    Path(wiki_id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let actor = current_session_snapshot(&state, &headers, true)
        .await
        .map(|session| session.username);
    ws.on_upgrade(move |socket| handle_socket(socket, wiki_id, actor, state))
}

async fn handle_socket(socket: WebSocket, wiki_id: String, actor: Option<String>, state: AppState) {
    let client_id = state.next_client_id.fetch_add(1, Ordering::Relaxed);
    state.coordination.connect_client(&wiki_id).await;
    let mut subscriber = state.coordination.subscribe(&wiki_id).await;
    let (mut sender, mut receiver) = socket.split();

    let log_wiki_id = wiki_id.clone();
    let send_task = tokio::spawn(async move {
        loop {
            let envelope = match subscriber.recv().await {
                Ok(envelope) => envelope,
                // A slow client that fell more than the room's buffer behind
                // dropped `skipped` messages. Resync and keep serving live
                // updates rather than killing the fan-out task (which would
                // silently stop delivering to an otherwise-open socket).
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    warn!(
                        wiki_id = %log_wiki_id,
                        client_id,
                        skipped,
                        "coordination subscriber lagged; resyncing to live"
                    );
                    continue;
                }
                // Sender dropped (room evicted): nothing more will arrive.
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            };
            if envelope.sender_id == client_id {
                continue;
            }

            if sender
                .send(Message::Binary(envelope.payload.into()))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    while let Some(message_result) = receiver.next().await {
        let Ok(message) = message_result else {
            break;
        };

        match message {
            Message::Binary(bytes) => {
                let payload = sanitize_coordination_payload(bytes.to_vec(), actor.as_deref());
                state
                    .coordination
                    .publish(
                        &wiki_id,
                        CoordinationEnvelope {
                            sender_id: client_id,
                            payload,
                        },
                    )
                    .await;
            }
            Message::Text(text) => {
                let payload = sanitize_coordination_payload(
                    text.as_str().as_bytes().to_vec(),
                    actor.as_deref(),
                );
                state
                    .coordination
                    .publish(
                        &wiki_id,
                        CoordinationEnvelope {
                            sender_id: client_id,
                            payload,
                        },
                    )
                    .await;
            }
            Message::Close(_) => break,
            Message::Ping(_) | Message::Pong(_) => {}
        }
    }

    send_task.abort();
    state.coordination.disconnect_client(&wiki_id).await;
}

async fn get_coordination_snapshot(State(state): State<AppState>) -> Json<CoordinationSnapshot> {
    Json(state.coordination.snapshot().await)
}

async fn get_coordination_room_state(
    Path(wiki_id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if let Some(summary) = state.coordination.room_state_summary(&wiki_id).await {
        Ok::<_, StatusCode>(Json(summary))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

async fn get_coordination_room_inspection(
    Path(wiki_id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    Ok::<_, StatusCode>(Json(
        room_inspection(&state.coordination, &wiki_id)
            .await
            .unwrap_or_else(|| empty_room_inspection(&wiki_id)),
    ))
}

async fn get_coordination_inspections(
    State(state): State<AppState>,
) -> Json<RoomInspectionCollection> {
    Json(RoomInspectionCollection {
        rooms: state.coordination.room_inspections().await,
    })
}

#[derive(Debug, serde::Deserialize)]
struct ArticleInventoryQuery {
    title: String,
}

enum CapabilityProbeSubject<'a> {
    LocalToken,
    Session(&'a SessionSnapshot),
}

async fn get_operator_storage_layout(
    Path(wiki_id): Path<String>,
    Query(query): Query<StorageLayoutQuery>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<OperatorStorageLayoutView>, (StatusCode, Json<serde_json::Value>)> {
    operator_storage_layout_view(&state, &headers, &wiki_id, &query)
        .await
        .map(Json)
        .map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": error })),
            )
        })
}

async fn operator_storage_layout_view(
    state: &AppState,
    headers: &HeaderMap,
    wiki_id: &str,
    query: &StorageLayoutQuery,
) -> Result<OperatorStorageLayoutView, String> {
    let input = storage_plan_input(
        state,
        headers,
        wiki_id,
        StoragePlanRequest {
            username_override: query.username.clone(),
            home_wiki_id_override: query.home_wiki_id.clone(),
            shared_owner_username_override: query.shared_owner_username.clone(),
            team_slugs: query.team_slug.clone().into_iter().collect(),
            rule_set_slugs: query.rule_set_slug.clone().into_iter().collect(),
            training_dataset_slugs: query.training_dataset_slug.clone().into_iter().collect(),
            audit_period_slugs: query.audit_period_slug.clone().into_iter().collect(),
        },
    )
    .await?;
    let plan = build_wiki_storage_plan(&WikiStorageConfig::default(), &input);
    let personal_index_page =
        render_wiki_storage_index_page(&plan.personal_root, &plan.personal_documents, &plan.notes);
    let shared_registry_page =
        render_wiki_storage_index_page(&plan.shared_root, &plan.shared_documents, &plan.notes);
    let sample_document_pages = plan
        .personal_documents
        .iter()
        .chain(plan.shared_documents.iter())
        .take(3)
        .map(|document| {
            let body = render_wiki_storage_document_page(
                document,
                &[format!(
                    "Canonical public SP42 document for `{}`.",
                    document.title
                )],
                &serde_json::json!({
                    "wiki_id": wiki_id,
                    "owner": input.shared_owner_username,
                    "subject": input.username,
                    "document": document.title,
                }),
            )
            .map_err(|error| error.to_string())?;
            Ok(RenderedStorageDocumentPage {
                title: document.title.clone(),
                body,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    Ok(OperatorStorageLayoutView {
        plan,
        personal_index_page,
        shared_registry_page,
        sample_document_pages,
    })
}

async fn storage_plan_input(
    state: &AppState,
    headers: &HeaderMap,
    wiki_id: &str,
    request: StoragePlanRequest,
) -> Result<WikiStoragePlanInput, String> {
    let session = current_session_snapshot(state, headers, false).await;
    let username = request
        .username_override
        .or_else(|| session.as_ref().map(|session| session.username.clone()))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "username is required via query or authenticated session".to_string())?;
    let shared_owner_username = request
        .shared_owner_username_override
        .unwrap_or_else(|| username.clone());

    Ok(WikiStoragePlanInput {
        username,
        home_wiki_id: request
            .home_wiki_id_override
            .unwrap_or_else(|| wiki_id.to_string()),
        target_wiki_id: wiki_id.to_string(),
        shared_owner_username,
        team_slugs: request.team_slugs,
        rule_set_slugs: request.rule_set_slugs,
        training_dataset_slugs: request.training_dataset_slugs,
        audit_period_slugs: request.audit_period_slugs,
    })
}

async fn resolve_logical_storage_document(
    state: &AppState,
    headers: &HeaderMap,
    wiki_id: &str,
    realm: &StorageDocumentRealmInput,
    kind: &StorageDocumentKindInput,
    query: &LogicalStorageDocumentQuery,
) -> Result<WikiStorageDocument, String> {
    let slug = query
        .slug
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let input = storage_plan_input(
        state,
        headers,
        wiki_id,
        StoragePlanRequest {
            username_override: query.username.clone(),
            home_wiki_id_override: query.home_wiki_id.clone(),
            shared_owner_username_override: query.shared_owner_username.clone(),
            team_slugs: slug.clone().into_iter().collect(),
            rule_set_slugs: slug.clone().into_iter().collect(),
            training_dataset_slugs: slug.clone().into_iter().collect(),
            audit_period_slugs: slug.clone().into_iter().collect(),
        },
    )
    .await?;
    let plan = build_wiki_storage_plan(&WikiStorageConfig::default(), &input);
    let document_kind = logical_storage_document_kind(wiki_id, realm, kind, slug)?;

    resolve_wiki_storage_document(&plan, &document_kind)
        .ok_or_else(|| format!("no canonical storage document matched `{document_kind:?}`"))
}

fn logical_storage_document_kind(
    wiki_id: &str,
    realm: &StorageDocumentRealmInput,
    kind: &StorageDocumentKindInput,
    slug: Option<String>,
) -> Result<WikiStorageDocumentKind, String> {
    match (realm, kind) {
        (StorageDocumentRealmInput::Personal, StorageDocumentKindInput::Index) => {
            Ok(WikiStorageDocumentKind::PersonalIndex)
        }
        (StorageDocumentRealmInput::Personal, StorageDocumentKindInput::Profile) => {
            Ok(WikiStorageDocumentKind::PersonalProfile)
        }
        (StorageDocumentRealmInput::Personal, StorageDocumentKindInput::Preferences) => {
            Ok(WikiStorageDocumentKind::PersonalPreferences)
        }
        (StorageDocumentRealmInput::Personal, StorageDocumentKindInput::Queue) => {
            Ok(WikiStorageDocumentKind::PersonalQueue {
                wiki_id: wiki_id.to_string(),
            })
        }
        (StorageDocumentRealmInput::Personal, StorageDocumentKindInput::Workspace) => {
            Ok(WikiStorageDocumentKind::PersonalWorkspace {
                wiki_id: wiki_id.to_string(),
            })
        }
        (StorageDocumentRealmInput::Personal, StorageDocumentKindInput::Labels) => {
            Ok(WikiStorageDocumentKind::PersonalLabels {
                wiki_id: wiki_id.to_string(),
            })
        }
        (StorageDocumentRealmInput::Shared, StorageDocumentKindInput::Registry) => {
            Ok(WikiStorageDocumentKind::SharedRegistry {
                wiki_id: wiki_id.to_string(),
            })
        }
        (StorageDocumentRealmInput::Shared, StorageDocumentKindInput::Team) => {
            Ok(WikiStorageDocumentKind::SharedTeam {
                wiki_id: wiki_id.to_string(),
                team_slug: require_logical_storage_slug(kind, slug)?,
            })
        }
        (StorageDocumentRealmInput::Shared, StorageDocumentKindInput::RuleSet) => {
            Ok(WikiStorageDocumentKind::SharedRuleSet {
                wiki_id: wiki_id.to_string(),
                rule_set_slug: require_logical_storage_slug(kind, slug)?,
            })
        }
        (StorageDocumentRealmInput::Shared, StorageDocumentKindInput::TrainingDataset) => {
            Ok(WikiStorageDocumentKind::SharedTrainingDataset {
                wiki_id: wiki_id.to_string(),
                dataset_slug: require_logical_storage_slug(kind, slug)?,
            })
        }
        (StorageDocumentRealmInput::Shared, StorageDocumentKindInput::AuditPeriod) => {
            Ok(WikiStorageDocumentKind::SharedAuditPeriod {
                wiki_id: wiki_id.to_string(),
                period_slug: require_logical_storage_slug(kind, slug)?,
            })
        }
        _ => Err(format!(
            "logical storage document `{realm:?}/{kind:?}` is not supported"
        )),
    }
}

fn require_logical_storage_slug(
    kind: &StorageDocumentKindInput,
    slug: Option<String>,
) -> Result<String, String> {
    slug.ok_or_else(|| format!("slug is required for shared `{kind:?}` documents"))
}

async fn access_token_for_request(state: &AppState, headers: &HeaderMap) -> Option<String> {
    current_session_snapshot(state, headers, true)
        .await
        .map(|session| session.access_token)
        // The shared env token is a local-dev convenience only; outside local mode
        // an unauthenticated request must NOT borrow it. Gated centrally by
        // `shared_local_access_token`. Codex review #90.
        .or_else(|| state.shared_local_access_token().map(ToString::to_string))
}

async fn get_article_inventory(
    Path(wiki_id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ArticleInventoryQuery>,
) -> Result<Json<ArticleInventory>, (StatusCode, Json<serde_json::Value>)> {
    let title = query.title.trim();
    if title.is_empty() {
        return Err(invalid_payload("title is required"));
    }

    let context = authenticated_wiki_context(&state, &headers, &wiki_id).await?;
    let wikitext = fetch_page_wikitext(&context.client, &context.config, title)
        .await
        .map_err(|error| gateway_error(format!("article fetch failed: {error}")))?;

    Ok(Json(build_article_inventory(&wiki_id, title, &wikitext)))
}

pub(crate) async fn fetch_page_wikitext(
    client: &BearerHttpClient,
    config: &WikiConfig,
    title: &str,
) -> Result<String, sp42_core::ActionError> {
    use sp42_core::ActionError;

    let mut url = config.api_url.clone();
    url.query_pairs_mut()
        .append_pair("action", "query")
        .append_pair("prop", "revisions")
        .append_pair("titles", title)
        .append_pair("rvprop", "content")
        .append_pair("rvslots", "main")
        .append_pair("rvlimit", "1")
        .append_pair("format", "json")
        .append_pair("formatversion", "2");

    let response = client
        .execute(HttpRequest {
            method: HttpMethod::Get,
            url,
            headers: std::collections::BTreeMap::new(),
            body: Vec::new(),
        })
        .await
        .map_err(|e| ActionError::Execution {
            message: format!("page fetch failed: {e}"),
            code: None,
            http_status: None,
            retryable: true,
        })?;

    if !(200..300).contains(&response.status) {
        return Err(ActionError::Execution {
            message: format!("page fetch HTTP {}", response.status),
            code: None,
            http_status: Some(response.status),
            retryable: response.status >= 500,
        });
    }

    let v: serde_json::Value =
        serde_json::from_slice(&response.body).map_err(|e| ActionError::Execution {
            message: format!("page JSON failed: {e}"),
            code: None,
            http_status: None,
            retryable: false,
        })?;

    // Navigate: query.pages[0].revisions[0].slots.main.content
    let content = v
        .pointer("/query/pages/0/revisions/0/slots/main/content")
        .and_then(serde_json::Value::as_str);

    content
        .map(ToString::to_string)
        .ok_or_else(|| ActionError::Execution {
            message: format!("page content not found for: {title}"),
            code: Some("missing-content".to_string()),
            http_status: None,
            retryable: false,
        })
}

async fn capability_report_for_request(
    state: &AppState,
    headers: &HeaderMap,
    wiki_id: &str,
    force_refresh: bool,
) -> DevAuthCapabilityReport {
    if let Some(session) = current_session_snapshot(state, headers, true).await {
        return capability_report_for_subject(
            state,
            CapabilityProbeSubject::Session(&session),
            wiki_id,
            force_refresh,
        )
        .await;
    }

    capability_report_for_subject(
        state,
        CapabilityProbeSubject::LocalToken,
        wiki_id,
        force_refresh,
    )
    .await
}

async fn cached_capability_report_for_subject(
    state: &AppState,
    subject: &CapabilityProbeSubject<'_>,
    wiki_id: &str,
    force_refresh: bool,
) -> Option<DevAuthCapabilityReport> {
    if force_refresh {
        return None;
    }

    match subject {
        // Never serve the shared local-token capability cache outside local mode,
        // so a present env token can't grant capabilities to an unauthenticated
        // request. Codex review #90.
        CapabilityProbeSubject::LocalToken
            if state.deployment.mode.permits_dev_token_bootstrap() =>
        {
            let guard = state.capability_cache.read().await;
            if let Some(cache) = guard.as_ref()
                && cache.report.wiki_id == wiki_id
                && cache_is_fresh(cache, state.clock.now_ms())
            {
                return Some(cache.report.clone());
            }
        }
        CapabilityProbeSubject::LocalToken => {}
        CapabilityProbeSubject::Session(session) => {
            if let Some(report) =
                cached_capabilities_for_session(state, &session.session_id, wiki_id).await
            {
                return Some(report);
            }
        }
    }

    None
}

fn capability_probe_token<'a>(
    state: &'a AppState,
    subject: &'a CapabilityProbeSubject<'a>,
) -> Option<&'a str> {
    match subject {
        // The shared env token is a local-dev convenience; outside local mode the
        // probe runs token-less (unauthenticated). Gated centrally by
        // `shared_local_access_token`. Codex review #90.
        CapabilityProbeSubject::LocalToken => state.shared_local_access_token(),
        CapabilityProbeSubject::Session(session) => Some(session.access_token.as_str()),
    }
}

fn log_capability_probe_result(
    subject: &CapabilityProbeSubject<'_>,
    wiki_id: &str,
    report: &DevAuthCapabilityReport,
) {
    if let Some(error) = &report.error {
        match subject {
            CapabilityProbeSubject::LocalToken => {
                warn!(wiki_id, error, "local capability probe failed");
            }
            CapabilityProbeSubject::Session(session) => {
                warn!(
                    session_id = session.session_id.as_str(),
                    wiki_id, error, "session capability probe failed"
                );
            }
        }
    } else {
        match subject {
            CapabilityProbeSubject::LocalToken => {
                info!(wiki_id, username = ?report.username, "local capability probe succeeded");
            }
            CapabilityProbeSubject::Session(session) => {
                info!(
                    session_id = session.session_id.as_str(),
                    wiki_id,
                    username = ?report.username,
                    "session capability probe succeeded"
                );
            }
        }
    }
}

async fn store_capability_report_for_subject(
    state: &AppState,
    subject: &CapabilityProbeSubject<'_>,
    report: &DevAuthCapabilityReport,
) {
    match subject {
        CapabilityProbeSubject::LocalToken => {
            let mut guard = state.capability_cache.write().await;
            *guard = Some(CachedCapabilityReport {
                fetched_at_ms: state.clock.now_ms(),
                report: report.clone(),
            });
        }
        CapabilityProbeSubject::Session(session) => {
            store_capabilities_for_session(state, &session.session_id, report).await;
        }
    }
}

async fn capability_report_for_subject(
    state: &AppState,
    subject: CapabilityProbeSubject<'_>,
    wiki_id: &str,
    force_refresh: bool,
) -> DevAuthCapabilityReport {
    if let Some(report) =
        cached_capability_report_for_subject(state, &subject, wiki_id, force_refresh).await
    {
        return report;
    }

    debug_assert!(!wiki_id.is_empty());
    let config = match resolved_wiki_config(state, wiki_id) {
        Ok(config) => config,
        Err(error) => {
            return DevAuthCapabilityReport {
                checked: true,
                wiki_id: wiki_id.to_string(),
                error: Some(error),
                ..DevAuthCapabilityReport::default()
            };
        }
    };
    let oauth = state.local_oauth.status();
    let report = probe_with_targets(
        &state.http_client,
        capability_probe_token(state, &subject),
        &oauth,
        &config,
        &state.capability_targets,
    )
    .await;
    log_capability_probe_result(&subject, wiki_id, &report);
    store_capability_report_for_subject(state, &subject, &report).await;
    report
}

async fn capability_report_for_local_token(
    state: &AppState,
    wiki_id: &str,
    force_refresh: bool,
) -> DevAuthCapabilityReport {
    capability_report_for_subject(
        state,
        CapabilityProbeSubject::LocalToken,
        wiki_id,
        force_refresh,
    )
    .await
}

async fn capability_report_for_session(
    state: &AppState,
    session: &SessionSnapshot,
    wiki_id: &str,
    force_refresh: bool,
) -> DevAuthCapabilityReport {
    capability_report_for_subject(
        state,
        CapabilityProbeSubject::Session(session),
        wiki_id,
        force_refresh,
    )
    .await
}

fn split_scope_string(value: &str) -> Vec<String> {
    value
        .split_whitespace()
        .filter(|scope| !scope.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn live_operator_backend_status(
    readiness: &ServerHealthStatus,
    auth: &DevAuthSessionStatus,
) -> LiveOperatorBackendStatus {
    LiveOperatorBackendStatus {
        ready_for_local_testing: FlagState::from(readiness.ready_for_local_testing),
        readiness_issues: readiness.readiness_issues.clone(),
        bootstrap_ready: FlagState::from(readiness.bootstrap.bootstrap_ready),
        oauth: readiness.oauth.clone(),
        session: auth.clone(),
        source_report: readiness.bootstrap.source_report.clone(),
        capability_cache_present: FlagState::from(readiness.capability_cache.present),
        capability_cache_fresh: FlagState::from(readiness.capability_cache.fresh),
        capability_cache_age_ms: readiness.capability_cache.age_ms,
        capability_cache_wiki_id: readiness.capability_cache.wiki_id.clone(),
    }
}

#[cfg(test)]
fn now_ms() -> i64 {
    SystemClock.now_ms()
}

async fn cached_capabilities_for_session(
    state: &AppState,
    session_id: &str,
    wiki_id: &str,
) -> Option<DevAuthCapabilityReport> {
    let mut sessions = state.sessions.write().await;
    let current_time_ms = state.clock.now_ms();
    prune_expired_sessions(&mut sessions, current_time_ms);
    let session = sessions.get_mut(session_id)?;
    let cache = session.capability_cache.get(wiki_id)?;
    if cache_is_fresh(cache, current_time_ms) {
        session.last_seen_at_ms = current_time_ms;
        session.expires_at_ms = Some(session_expires_at_ms(session, current_time_ms));
        Some(cache.report.clone())
    } else {
        session.capability_cache.remove(wiki_id);
        None
    }
}

async fn store_capabilities_for_session(
    state: &AppState,
    session_id: &str,
    report: &DevAuthCapabilityReport,
) {
    let mut sessions = state.sessions.write().await;
    let current_time_ms = state.clock.now_ms();
    prune_expired_sessions(&mut sessions, current_time_ms);
    if let Some(session) = sessions.get_mut(session_id) {
        session.capability_cache.insert(
            report.wiki_id.clone(),
            CachedCapabilityReport {
                fetched_at_ms: current_time_ms,
                report: report.clone(),
            },
        );
    }
}

fn cache_is_fresh(cache: &CachedCapabilityReport, current_time_ms: i64) -> bool {
    current_time_ms.saturating_sub(cache.fetched_at_ms) < CAPABILITY_CACHE_TTL_MS
}

fn sanitize_coordination_payload(payload: Vec<u8>, actor: Option<&str>) -> Vec<u8> {
    let Some(actor) = actor else {
        return payload;
    };
    let Ok(message) = sp42_coordination::decode_message(&payload) else {
        warn!("received undecodable coordination payload while rewriting actor");
        return payload;
    };
    let rewritten = match message {
        sp42_coordination::CoordinationMessage::ActionBroadcast(mut action) => {
            action.actor = actor.to_string();
            sp42_coordination::CoordinationMessage::ActionBroadcast(action)
        }
        sp42_coordination::CoordinationMessage::EditClaim(mut claim) => {
            claim.actor = actor.to_string();
            sp42_coordination::CoordinationMessage::EditClaim(claim)
        }
        sp42_coordination::CoordinationMessage::PresenceHeartbeat(mut presence) => {
            presence.actor = actor.to_string();
            sp42_coordination::CoordinationMessage::PresenceHeartbeat(presence)
        }
        sp42_coordination::CoordinationMessage::RaceResolution(mut resolution) => {
            resolution.winning_actor = actor.to_string();
            sp42_coordination::CoordinationMessage::RaceResolution(resolution)
        }
        other => other,
    };

    sp42_coordination::encode_message(&rewritten).unwrap_or(payload)
}

fn config_for_state_wiki(
    state: &AppState,
    wiki_id: &str,
) -> Result<sp42_core::WikiConfig, (StatusCode, Json<serde_json::Value>)> {
    resolved_wiki_config(state, wiki_id).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": error })),
        )
    })
}

fn resolved_wiki_config(state: &AppState, wiki_id: &str) -> Result<sp42_core::WikiConfig, String> {
    // resolve() falls back to deriving any Wikimedia project from the embedded
    // authoritative site list when it isn't hand-configured (ADR-0014).
    let mut config = state
        .wiki_registry
        .resolve(wiki_id)
        .map_err(|error| error.to_string())?;
    if let Some(api_url) = &state.capability_targets.api_url {
        config.api_url = reqwest::Url::parse(api_url)
            .map_err(|error| format!("api_url override was invalid: {error}"))?;
    }
    if let Some(liftwing_url) = std::env::var("SP42_TEST_LIFTWING_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        config.liftwing_url = Some(
            reqwest::Url::parse(&liftwing_url)
                .map_err(|error| format!("liftwing_url override was invalid: {error}"))?,
        );
    }
    Ok(config)
}

#[cfg(test)]
mod tests;
