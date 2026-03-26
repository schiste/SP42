use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use sp42_core::parse_public_storage_document;

use crate::{
    ActionExecutionLogEntry, AppState, BearerHttpClient, DevAuthCapabilityReport,
    DevAuthSessionStatus, FlagState, LiveOperatorPublicContextState, LiveOperatorPublicDocuments,
    LivePublicDocumentLoadSpec, LogicalStorageDocumentQuery, LogicalStorageDocumentSavePayload,
    LogicalStorageDocumentView, LogicalStorageDocumentWriteView, PublicAuditLedgerEntry,
    PublicStorageDocumentData, PublicStorageDocumentQuery, PublicStorageDocumentRouteKind,
    PublicStorageDocumentSavePayload, PublicStorageDocumentView, PublicStorageDocumentWriteView,
    ResolvedPublicStorageDocument, SessionActionExecutionRequest, SessionSnapshot,
    StorageDocumentKindInput, StorageDocumentQuery, StorageDocumentRealmInput,
    StorageDocumentSavePayload, StoragePlanRequest, TokenKind, WikiConfig, WikiStorageConfig,
    WikiStorageDocument, WikiStorageDocumentKind, WikiStorageLoadedDocument,
    WikiStorageWriteOutcome, WikiStorageWriteRequest, action_feedback_for_entry,
    authenticated_wiki_context, build_wiki_storage_plan, default_public_storage_document,
    execute_fetch_token, invalid_payload, load_storage_document_with_context,
    load_wiki_storage_document, require_logical_storage_slug, required_csrf_token,
    resolve_logical_storage_document, resolve_wiki_storage_document, resolved_wiki_config,
    save_storage_document_with_context, save_wiki_storage_document, storage_plan_input,
};

pub(crate) async fn get_storage_document(
    Path(wiki_id): Path<String>,
    Query(query): Query<StorageDocumentQuery>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<WikiStorageLoadedDocument>, (StatusCode, Json<serde_json::Value>)> {
    let title = query
        .title
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| invalid_payload("title query parameter is required"))?;
    let context = authenticated_wiki_context(&state, &headers, &wiki_id).await?;
    load_storage_document_with_context(&context, title)
        .await
        .map(Json)
}

pub(crate) async fn put_storage_document(
    Path(wiki_id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<StorageDocumentSavePayload>,
) -> Result<Json<WikiStorageWriteOutcome>, (StatusCode, Json<serde_json::Value>)> {
    let StorageDocumentSavePayload {
        document,
        human_summary,
        data,
        baserevid,
        tags,
        watchlist,
        create_only,
        minor,
        summary,
    } = payload;

    if document.title.trim().is_empty() {
        return Err(invalid_payload("document.title is required"));
    }

    let context = authenticated_wiki_context(&state, &headers, &wiki_id).await?;
    let csrf_token = required_csrf_token(&context).await?;
    let request = WikiStorageWriteRequest {
        document: document.clone(),
        human_summary,
        data,
        token: csrf_token,
        baserevid,
        tags,
        watchlist,
        create_only,
        minor,
        summary,
    };

    save_storage_document_with_context(&context, document, request)
        .await
        .map(Json)
}

pub(crate) async fn get_logical_storage_document(
    Path((wiki_id, realm, kind)): Path<(
        String,
        StorageDocumentRealmInput,
        StorageDocumentKindInput,
    )>,
    Query(query): Query<LogicalStorageDocumentQuery>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<LogicalStorageDocumentView>, (StatusCode, Json<serde_json::Value>)> {
    let document =
        resolve_logical_storage_document(&state, &headers, &wiki_id, &realm, &kind, &query)
            .await
            .map_err(|message| invalid_payload(&message))?;
    let context = authenticated_wiki_context(&state, &headers, &wiki_id).await?;
    let loaded = load_storage_document_with_context(&context, &document.title).await?;

    Ok(Json(LogicalStorageDocumentView { document, loaded }))
}

pub(crate) async fn put_logical_storage_document(
    Path((wiki_id, realm, kind)): Path<(
        String,
        StorageDocumentRealmInput,
        StorageDocumentKindInput,
    )>,
    Query(query): Query<LogicalStorageDocumentQuery>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<LogicalStorageDocumentSavePayload>,
) -> Result<Json<LogicalStorageDocumentWriteView>, (StatusCode, Json<serde_json::Value>)> {
    let document =
        resolve_logical_storage_document(&state, &headers, &wiki_id, &realm, &kind, &query)
            .await
            .map_err(|message| invalid_payload(&message))?;
    let context = authenticated_wiki_context(&state, &headers, &wiki_id).await?;
    let csrf_token = required_csrf_token(&context).await?;
    let request = WikiStorageWriteRequest {
        document: document.clone(),
        human_summary: payload.human_summary,
        data: payload.data,
        token: csrf_token,
        baserevid: payload.baserevid,
        tags: payload.tags,
        watchlist: payload.watchlist,
        create_only: payload.create_only,
        minor: payload.minor,
        summary: payload.summary,
    };

    save_storage_document_with_context(&context, document.clone(), request)
        .await
        .map(|outcome| Json(LogicalStorageDocumentWriteView { document, outcome }))
}

pub(crate) fn public_storage_document_kind(
    wiki_id: &str,
    kind: &PublicStorageDocumentRouteKind,
    slug: Option<String>,
) -> Result<WikiStorageDocumentKind, String> {
    match kind {
        PublicStorageDocumentRouteKind::Preferences => {
            Ok(WikiStorageDocumentKind::PersonalPreferences)
        }
        PublicStorageDocumentRouteKind::Registry => Ok(WikiStorageDocumentKind::SharedRegistry {
            wiki_id: wiki_id.to_string(),
        }),
        PublicStorageDocumentRouteKind::Team => Ok(WikiStorageDocumentKind::SharedTeam {
            wiki_id: wiki_id.to_string(),
            team_slug: require_logical_storage_slug(&StorageDocumentKindInput::Team, slug)?,
        }),
        PublicStorageDocumentRouteKind::RuleSet => Ok(WikiStorageDocumentKind::SharedRuleSet {
            wiki_id: wiki_id.to_string(),
            rule_set_slug: require_logical_storage_slug(&StorageDocumentKindInput::RuleSet, slug)?,
        }),
        PublicStorageDocumentRouteKind::AuditPeriod => {
            Ok(WikiStorageDocumentKind::SharedAuditPeriod {
                wiki_id: wiki_id.to_string(),
                period_slug: require_logical_storage_slug(
                    &StorageDocumentKindInput::AuditPeriod,
                    slug,
                )?,
            })
        }
    }
}

pub(crate) async fn resolve_public_storage_document(
    state: &AppState,
    headers: &HeaderMap,
    wiki_id: &str,
    kind: &PublicStorageDocumentRouteKind,
    query: &PublicStorageDocumentQuery,
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
            training_dataset_slugs: Vec::new(),
            audit_period_slugs: slug.clone().into_iter().collect(),
        },
    )
    .await?;
    let plan = build_wiki_storage_plan(&WikiStorageConfig::default(), &input);
    let document_kind = public_storage_document_kind(wiki_id, kind, slug)?;

    resolve_wiki_storage_document(&plan, &document_kind)
        .ok_or_else(|| format!("no public storage document matched `{document_kind:?}`"))
}

pub(crate) fn public_payload_for_loaded_document(
    document: &WikiStorageDocument,
    loaded: &WikiStorageLoadedDocument,
) -> Result<(PublicStorageDocumentData, FlagState), String> {
    match loaded.envelope.as_ref() {
        Some(envelope) => parse_public_storage_document(&document.kind, envelope.data.clone())
            .map(|payload| (payload, FlagState::Disabled))
            .map_err(|error| error.to_string()),
        None => default_public_storage_document(&document.kind)
            .map(|payload| (payload, FlagState::Enabled))
            .map_err(|error| error.to_string()),
    }
}

pub(crate) async fn load_or_bootstrap_public_storage_document(
    client: &BearerHttpClient,
    config: &WikiConfig,
    document: WikiStorageDocument,
    bootstrap_actor: Option<&str>,
) -> Result<ResolvedPublicStorageDocument, String> {
    let loaded = load_wiki_storage_document(client, config, &document.title)
        .await
        .map_err(|error| error.to_string())?;
    if loaded.exists {
        let (payload, defaulted) = public_payload_for_loaded_document(&document, &loaded)?;
        return Ok(ResolvedPublicStorageDocument {
            document,
            loaded,
            payload,
            defaulted,
        });
    }

    let payload = bootstrap_public_storage_document(
        &document.kind,
        config,
        bootstrap_actor.or_else(|| owner_username_from_title(&document.title)),
    )
    .map_err(|error| error.to_string())?;
    let csrf_token = execute_fetch_token(client, config, TokenKind::Csrf)
        .await
        .map_err(|error| format!("csrf token fetch failed: {error}"))?;
    let human_summary = public_document_human_summary(&payload);
    let save_result = save_wiki_storage_document(
        client,
        config,
        &WikiStorageWriteRequest {
            document: document.clone(),
            human_summary,
            data: payload
                .clone()
                .into_json_value()
                .map_err(|error| error.to_string())?,
            token: csrf_token,
            baserevid: None,
            tags: Vec::new(),
            watchlist: None,
            create_only: FlagState::Enabled,
            minor: FlagState::Disabled,
            summary: Some("Bootstrap SP42 public document".to_string()),
        },
    )
    .await;
    if let Err(error) = save_result
        && !matches!(error, sp42_core::WikiStorageError::Conflict { .. })
    {
        return Err(error.to_string());
    }

    let loaded = load_wiki_storage_document(client, config, &document.title)
        .await
        .map_err(|error| error.to_string())?;
    let (payload, _) = public_payload_for_loaded_document(&document, &loaded)?;
    Ok(ResolvedPublicStorageDocument {
        document,
        loaded,
        payload,
        defaulted: FlagState::Enabled,
    })
}

fn bootstrap_public_storage_document(
    kind: &WikiStorageDocumentKind,
    config: &WikiConfig,
    bootstrap_actor: Option<&str>,
) -> Result<PublicStorageDocumentData, sp42_core::PublicDocumentError> {
    let actor = bootstrap_actor
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let mut payload = default_public_storage_document(kind)?;

    match &mut payload {
        PublicStorageDocumentData::Preferences(document) => {
            document.preferred_wiki_id.clone_from(&config.wiki_id);
            document.hide_bots = true;
            if document.editor_types.is_empty() {
                document.editor_types = vec!["anonymous".to_string(), "temporary".to_string()];
            }
        }
        PublicStorageDocumentData::Registry(document) => {
            if document.teams.is_empty() {
                document.teams.push(sp42_core::PublicTeamRegistryEntry {
                    slug: "core".to_string(),
                    title: "Core Patrol".to_string(),
                });
            }
        }
        PublicStorageDocumentData::Team(document) => {
            if document.title == document.slug {
                document.title = humanize_slug(&document.slug);
            }
            if document.description.trim().is_empty() {
                document.description = format!(
                    "Default SP42 patrol team for {} using public on-wiki coordination.",
                    config.display_name
                );
            }
            if let Some(actor) = actor {
                if !document.members.iter().any(|member| member == actor) {
                    document.members.push(actor.to_string());
                }
                if !document.trusted_users.iter().any(|member| member == actor) {
                    document.trusted_users.push(actor.to_string());
                }
            }
        }
        PublicStorageDocumentData::RuleSet(document) => {
            if document.title == document.slug {
                document.title = humanize_slug(&document.slug);
            }
            document
                .namespace_allowlist
                .clone_from(&config.namespace_allowlist);
            document.hide_bots = true;
            if let Some(actor) = actor
                && !document.trusted_users.iter().any(|member| member == actor)
            {
                document.trusted_users.push(actor.to_string());
            }
        }
        PublicStorageDocumentData::AuditLedger(_) => {}
    }

    Ok(payload)
}

fn owner_username_from_title(title: &str) -> Option<&str> {
    title
        .strip_prefix("User:")
        .and_then(|value| value.split('/').next())
        .filter(|value| !value.trim().is_empty())
}

fn humanize_slug(slug: &str) -> String {
    slug.split(['-', '_'])
        .filter(|segment| !segment.is_empty())
        .map(|segment| {
            let mut chars = segment.chars();
            match chars.next() {
                Some(first) => {
                    let mut word = String::new();
                    word.extend(first.to_uppercase());
                    word.push_str(chars.as_str());
                    word
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn build_live_public_plan_request(
    username: &str,
    wiki_id: &str,
) -> PublicStorageDocumentQuery {
    PublicStorageDocumentQuery {
        username: Some(username.to_string()),
        home_wiki_id: Some(wiki_id.to_string()),
        shared_owner_username: Some(username.to_string()),
        slug: None,
    }
}

pub(crate) async fn resolve_live_public_document(
    state: &AppState,
    headers: &HeaderMap,
    wiki_id: &str,
    spec: LivePublicDocumentLoadSpec,
    client: &BearerHttpClient,
    config: &WikiConfig,
    notes: &mut Vec<String>,
) -> Option<ResolvedPublicStorageDocument> {
    match resolve_public_storage_document(state, headers, wiki_id, &spec.kind, &spec.query).await {
        Ok(document) => {
            match load_or_bootstrap_public_storage_document(
                client,
                config,
                document,
                spec.query.username.as_deref(),
            )
            .await
            {
                Ok(resolved) => Some(resolved),
                Err(error) => {
                    notes.push(format!(
                        "{} could not be resolved: {error}",
                        spec.resolved_label
                    ));
                    None
                }
            }
        }
        Err(error) => {
            notes.push(format!(
                "{} could not be resolved: {error}",
                spec.plan_label
            ));
            None
        }
    }
}

fn live_operator_username(
    auth: &DevAuthSessionStatus,
    capabilities: &DevAuthCapabilityReport,
) -> Option<String> {
    auth.username
        .clone()
        .or_else(|| capabilities.username.clone())
}

async fn load_base_live_public_documents(
    state: &AppState,
    headers: &HeaderMap,
    wiki_id: &str,
    client: &BearerHttpClient,
    config: &WikiConfig,
    plan_request: &PublicStorageDocumentQuery,
    notes: &mut Vec<String>,
) -> (
    Option<ResolvedPublicStorageDocument>,
    Option<ResolvedPublicStorageDocument>,
    Option<ResolvedPublicStorageDocument>,
) {
    let preferences = resolve_live_public_document(
        state,
        headers,
        wiki_id,
        LivePublicDocumentLoadSpec {
            kind: PublicStorageDocumentRouteKind::Preferences,
            query: plan_request.clone(),
            resolved_label: "Preferences document",
            plan_label: "Preferences document plan",
        },
        client,
        config,
        notes,
    )
    .await;
    let registry = resolve_live_public_document(
        state,
        headers,
        wiki_id,
        LivePublicDocumentLoadSpec {
            kind: PublicStorageDocumentRouteKind::Registry,
            query: plan_request.clone(),
            resolved_label: "Team registry",
            plan_label: "Team registry plan",
        },
        client,
        config,
        notes,
    )
    .await;
    let active_rule_set = resolve_live_public_document(
        state,
        headers,
        wiki_id,
        LivePublicDocumentLoadSpec {
            kind: PublicStorageDocumentRouteKind::RuleSet,
            query: PublicStorageDocumentQuery {
                slug: Some("default".to_string()),
                ..plan_request.clone()
            },
            resolved_label: "Default rule set",
            plan_label: "Default rule-set plan",
        },
        client,
        config,
        notes,
    )
    .await;

    (preferences, registry, active_rule_set)
}

fn active_team_slug(registry: Option<&ResolvedPublicStorageDocument>) -> String {
    registry
        .and_then(|resolved| match &resolved.payload {
            PublicStorageDocumentData::Registry(registry) => {
                registry.teams.first().map(|team| team.slug.clone())
            }
            _ => None,
        })
        .unwrap_or_else(|| "core".to_string())
}

async fn load_active_team_document(
    state: &AppState,
    headers: &HeaderMap,
    wiki_id: &str,
    client: &BearerHttpClient,
    config: &WikiConfig,
    query: PublicStorageDocumentQuery,
    notes: &mut Vec<String>,
) -> Option<ResolvedPublicStorageDocument> {
    resolve_live_public_document(
        state,
        headers,
        wiki_id,
        LivePublicDocumentLoadSpec {
            kind: PublicStorageDocumentRouteKind::Team,
            query,
            resolved_label: "Active team document",
            plan_label: "Active team plan",
        },
        client,
        config,
        notes,
    )
    .await
}

fn append_bootstrap_notes(
    notes: &mut Vec<String>,
    preferences: Option<&ResolvedPublicStorageDocument>,
    registry: Option<&ResolvedPublicStorageDocument>,
    active_team: Option<&ResolvedPublicStorageDocument>,
    active_rule_set: Option<&ResolvedPublicStorageDocument>,
) {
    for (label, resolved) in [
        ("preferences", preferences),
        ("registry", registry),
        ("team", active_team),
        ("rule-set", active_rule_set),
    ] {
        if resolved.is_some_and(|document| document.defaulted.is_enabled()) {
            notes.push(format!(
                "Public {label} document was auto-bootstrapped from SP42 defaults."
            ));
        }
    }
}

pub(crate) async fn load_live_operator_public_context(
    state: &AppState,
    headers: &HeaderMap,
    wiki_id: &str,
    auth: &DevAuthSessionStatus,
    capabilities: &DevAuthCapabilityReport,
    client: &BearerHttpClient,
    config: &WikiConfig,
) -> LiveOperatorPublicContextState {
    let Some(username) = live_operator_username(auth, capabilities) else {
        return LiveOperatorPublicContextState {
            notes: vec!["Public SP42 documents were not resolved because no operator username is available yet.".to_string()],
            ..LiveOperatorPublicContextState::default()
        };
    };

    let mut notes = Vec::new();
    let plan_request = build_live_public_plan_request(&username, wiki_id);
    let (preferences, registry, active_rule_set) = load_base_live_public_documents(
        state,
        headers,
        wiki_id,
        client,
        config,
        &plan_request,
        &mut notes,
    )
    .await;
    let active_team = load_active_team_document(
        state,
        headers,
        wiki_id,
        client,
        config,
        PublicStorageDocumentQuery {
            slug: Some(active_team_slug(registry.as_ref())),
            ..plan_request
        },
        &mut notes,
    )
    .await;
    append_bootstrap_notes(
        &mut notes,
        preferences.as_ref(),
        registry.as_ref(),
        active_team.as_ref(),
        active_rule_set.as_ref(),
    );

    LiveOperatorPublicContextState {
        preferences,
        registry,
        active_team,
        active_rule_set,
        audit_period_slug: Some(audit_period_slug_from_clock(state.clock.now_ms())),
        notes,
    }
}

pub(crate) fn live_operator_public_documents_model(
    context: &LiveOperatorPublicContextState,
) -> LiveOperatorPublicDocuments {
    LiveOperatorPublicDocuments {
        preferences: context
            .preferences
            .as_ref()
            .and_then(|resolved| match &resolved.payload {
                PublicStorageDocumentData::Preferences(value) => Some(value.clone()),
                _ => None,
            }),
        preferences_defaulted: context
            .preferences
            .as_ref()
            .map_or(FlagState::Disabled, |resolved| resolved.defaulted),
        registry: context
            .registry
            .as_ref()
            .and_then(|resolved| match &resolved.payload {
                PublicStorageDocumentData::Registry(value) => Some(value.clone()),
                _ => None,
            }),
        registry_defaulted: context
            .registry
            .as_ref()
            .map_or(FlagState::Disabled, |resolved| resolved.defaulted),
        active_team: context
            .active_team
            .as_ref()
            .and_then(|resolved| match &resolved.payload {
                PublicStorageDocumentData::Team(value) => Some(value.clone()),
                _ => None,
            }),
        active_team_defaulted: context
            .active_team
            .as_ref()
            .map_or(FlagState::Disabled, |resolved| resolved.defaulted),
        active_rule_set: context.active_rule_set.as_ref().and_then(|resolved| {
            match &resolved.payload {
                PublicStorageDocumentData::RuleSet(value) => Some(value.clone()),
                _ => None,
            }
        }),
        active_rule_set_defaulted: context
            .active_rule_set
            .as_ref()
            .map_or(FlagState::Disabled, |resolved| resolved.defaulted),
        audit_period_slug: context.audit_period_slug.clone(),
        notes: context.notes.clone(),
    }
}

pub(crate) fn audit_period_slug_from_clock(current_ms: i64) -> String {
    let seconds = current_ms.div_euclid(1_000);
    let days = seconds.div_euclid(86_400);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = y + i64::from(month <= 2);
    format!("{year:04}-{month:02}")
}

pub(crate) async fn append_public_audit_entry(
    state: &AppState,
    headers: &HeaderMap,
    session: &SessionSnapshot,
    payload: &SessionActionExecutionRequest,
    entry: &ActionExecutionLogEntry,
) -> Result<(), String> {
    let config = resolved_wiki_config(state, &payload.wiki_id)?;
    let client = BearerHttpClient::new(state.http_client.clone(), session.access_token.clone());
    let period_slug = audit_period_slug_from_clock(entry.executed_at_ms);
    let document = resolve_public_storage_document(
        state,
        headers,
        &payload.wiki_id,
        &PublicStorageDocumentRouteKind::AuditPeriod,
        &PublicStorageDocumentQuery {
            username: Some(session.username.clone()),
            home_wiki_id: Some(payload.wiki_id.clone()),
            shared_owner_username: Some(session.username.clone()),
            slug: Some(period_slug.clone()),
        },
    )
    .await?;
    let resolved = load_or_bootstrap_public_storage_document(
        &client,
        &config,
        document.clone(),
        Some(session.username.as_str()),
    )
    .await?;
    let PublicStorageDocumentData::AuditLedger(mut ledger) = resolved.payload else {
        return Err("resolved public audit document did not decode as an audit ledger".to_string());
    };
    ledger.entries.push(PublicAuditLedgerEntry {
        timestamp_ms: entry.executed_at_ms,
        actor: session.username.clone(),
        action: payload.kind.label().to_string(),
        summary: action_feedback_for_entry(entry),
    });
    let csrf_token = execute_fetch_token(&client, &config, TokenKind::Csrf)
        .await
        .map_err(|error| format!("csrf token fetch failed: {error}"))?;
    save_wiki_storage_document(
        &client,
        &config,
        &WikiStorageWriteRequest {
            document,
            human_summary: public_document_human_summary(&PublicStorageDocumentData::AuditLedger(
                ledger.clone(),
            )),
            data: PublicStorageDocumentData::AuditLedger(ledger)
                .into_json_value()
                .map_err(|error| error.to_string())?,
            token: csrf_token,
            baserevid: resolved.loaded.revision_id,
            tags: Vec::new(),
            watchlist: None,
            create_only: FlagState::Disabled,
            minor: FlagState::Disabled,
            summary: Some(format!(
                "Record SP42 {} on rev {}",
                payload.kind.label(),
                payload.rev_id
            )),
        },
    )
    .await
    .map(|_| ())
    .map_err(|error| error.to_string())
}

pub(crate) fn public_document_human_summary(payload: &PublicStorageDocumentData) -> Vec<String> {
    match payload {
        PublicStorageDocumentData::Preferences(document) => vec![format!(
            "Public SP42 preferences for {} with queue limit {}.",
            document.preferred_wiki_id, document.queue_limit
        )],
        PublicStorageDocumentData::Registry(document) => vec![format!(
            "Public SP42 team registry for {} with {} teams.",
            document.wiki_id,
            document.teams.len()
        )],
        PublicStorageDocumentData::Team(document) => vec![format!(
            "Public SP42 team definition `{}` with {} members and {} trusted users.",
            document.slug,
            document.members.len(),
            document.trusted_users.len()
        )],
        PublicStorageDocumentData::RuleSet(document) => vec![format!(
            "Public SP42 rule set `{}` for {} namespaces and {} trusted users.",
            document.slug,
            document.namespace_allowlist.len(),
            document.trusted_users.len()
        )],
        PublicStorageDocumentData::AuditLedger(document) => vec![format!(
            "Public SP42 audit ledger `{}` with {} entries.",
            document.period_slug,
            document.entries.len()
        )],
    }
}

pub(crate) async fn get_public_storage_document(
    Path((wiki_id, kind)): Path<(String, PublicStorageDocumentRouteKind)>,
    Query(query): Query<PublicStorageDocumentQuery>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<PublicStorageDocumentView>, (StatusCode, Json<serde_json::Value>)> {
    let document = resolve_public_storage_document(&state, &headers, &wiki_id, &kind, &query)
        .await
        .map_err(|message| invalid_payload(&message))?;
    let context = authenticated_wiki_context(&state, &headers, &wiki_id).await?;
    let resolved = load_or_bootstrap_public_storage_document(
        &context.client,
        &context.config,
        document.clone(),
        query.username.as_deref(),
    )
    .await
    .map_err(|error| {
        (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": error, "document": document })),
        )
    })?;

    Ok(Json(PublicStorageDocumentView {
        document: resolved.document,
        loaded: resolved.loaded,
        payload: resolved.payload,
        defaulted: resolved.defaulted,
    }))
}

pub(crate) async fn put_public_storage_document(
    Path((wiki_id, kind)): Path<(String, PublicStorageDocumentRouteKind)>,
    Query(query): Query<PublicStorageDocumentQuery>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<PublicStorageDocumentSavePayload>,
) -> Result<Json<PublicStorageDocumentWriteView>, (StatusCode, Json<serde_json::Value>)> {
    let document = resolve_public_storage_document(&state, &headers, &wiki_id, &kind, &query)
        .await
        .map_err(|message| invalid_payload(&message))?;
    payload
        .payload
        .ensure_matches_document_kind(&document.kind)
        .map_err(|error| invalid_payload(&error.to_string()))?;

    let context = authenticated_wiki_context(&state, &headers, &wiki_id).await?;
    let csrf_token = required_csrf_token(&context).await?;

    let json_data = payload
        .payload
        .clone()
        .into_json_value()
        .map_err(|error| invalid_payload(&error.to_string()))?;
    let human_summary = if payload.human_summary.is_empty() {
        public_document_human_summary(&payload.payload)
    } else {
        payload.human_summary
    };
    let request = WikiStorageWriteRequest {
        document: document.clone(),
        human_summary,
        data: json_data,
        token: csrf_token,
        baserevid: payload.baserevid,
        tags: payload.tags,
        watchlist: payload.watchlist,
        create_only: payload.create_only,
        minor: payload.minor,
        summary: payload.summary,
    };

    save_storage_document_with_context(&context, document.clone(), request)
        .await
        .map(|outcome| {
            Json(PublicStorageDocumentWriteView {
                document,
                payload: payload.payload,
                outcome,
            })
        })
}

#[cfg(test)]
mod tests {
    use super::{bootstrap_public_storage_document, humanize_slug, owner_username_from_title};
    use sp42_core::{PublicStorageDocumentData, WikiStorageDocumentKind, parse_wiki_config};

    #[test]
    fn bootstrap_rule_set_uses_config_and_actor_defaults() {
        let config = parse_wiki_config(include_str!("../../../configs/frwiki.yaml"))
            .expect("config should parse");

        let payload = bootstrap_public_storage_document(
            &WikiStorageDocumentKind::SharedRuleSet {
                wiki_id: "frwiki".to_string(),
                rule_set_slug: "default".to_string(),
            },
            &config,
            Some("Schiste"),
        )
        .expect("bootstrap should succeed");

        let PublicStorageDocumentData::RuleSet(rule_set) = payload else {
            panic!("expected rule set payload");
        };

        assert_eq!(rule_set.namespace_allowlist, config.namespace_allowlist);
        assert!(rule_set.hide_bots);
        assert!(rule_set.trusted_users.iter().any(|user| user == "Schiste"));
    }

    #[test]
    fn bootstrap_team_populates_owner_as_member_and_trusted_user() {
        let config = parse_wiki_config(include_str!("../../../configs/frwiki.yaml"))
            .expect("config should parse");

        let payload = bootstrap_public_storage_document(
            &WikiStorageDocumentKind::SharedTeam {
                wiki_id: "frwiki".to_string(),
                team_slug: "core".to_string(),
            },
            &config,
            Some("Schiste"),
        )
        .expect("bootstrap should succeed");

        let PublicStorageDocumentData::Team(team) = payload else {
            panic!("expected team payload");
        };

        assert!(team.members.iter().any(|member| member == "Schiste"));
        assert!(team.trusted_users.iter().any(|member| member == "Schiste"));
        assert_eq!(team.title, "Core");
    }

    #[test]
    fn extracts_owner_username_from_storage_title() {
        assert_eq!(
            owner_username_from_title("User:Schiste/SP42/frwiki/Teams/core"),
            Some("Schiste")
        );
    }

    #[test]
    fn humanizes_slug_for_bootstrap_titles() {
        assert_eq!(humanize_slug("core-patrol"), "Core Patrol");
    }
}
