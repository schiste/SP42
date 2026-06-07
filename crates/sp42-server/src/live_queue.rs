use std::collections::BTreeMap;

use axum::http::HeaderMap;
use sp42_coordination::{CoordinationRoomSummary, CoordinationStateSummary};
use sp42_core::{
    ActionExecutionHistoryReport, ActionExecutionStatusReport, ContextInputs,
    DevAuthCapabilityReport, DevAuthSessionStatus, FlagState, LiftWingRequest,
    PublicStorageDocumentData, QueueHeuristicPolicy, QueuedEdit, WikiConfig,
    build_ranked_queue_with_policy, build_review_workbench, build_scoring_context,
    execute_liftwing_score, score_edit_with_context,
};
use sp42_live::{
    BacklogRuntime, BacklogRuntimeConfig, BacklogRuntimeStatus, DEFAULT_LIVE_OPERATOR_LIMIT,
    LiveIngestionSupervisorStatus, LiveOperatorActionPreflight, LiveOperatorBackendStatus,
    LiveOperatorHeuristicProvenance, LiveOperatorPhaseTiming, LiveOperatorPublicDocuments,
    LiveOperatorQuery, LiveOperatorTelemetry, RecentChangesBatch, RecentChangesQuery,
    StreamRuntimeStatus, build_live_operator_action_preflight, execute_recent_changes,
    filter_live_operator_queue,
};
use sp42_reporting::{
    DebugSnapshot, DebugSnapshotInputs, LiveOperatorView, PatrolScenarioReportInputs,
    PatrolSessionDigestInputs, ShellStateInputs, build_debug_snapshot,
    build_patrol_scenario_report, build_patrol_session_digest, build_shell_state_model,
};

use crate::session_runtime::current_status;
use crate::{
    AppState, BearerHttpClient, PublicStorageDocumentQuery, PublicStorageDocumentRouteKind,
    ResolvedPublicStorageDocument, ServerHealthStatus, action_history_report, action_status_report,
    capability_report_for_request, fetch_revision_diff, fetch_revision_media_diff,
    live_operator_backend_status, resolved_wiki_config, runtime_storage_for, server_readiness,
};
use crate::{ingestion_supervisor, persisted_stream_status};

#[derive(Debug, Clone, Default)]
pub(crate) struct LiveOperatorPublicContextState {
    pub(crate) preferences: Option<ResolvedPublicStorageDocument>,
    pub(crate) registry: Option<ResolvedPublicStorageDocument>,
    pub(crate) active_team: Option<ResolvedPublicStorageDocument>,
    pub(crate) active_rule_set: Option<ResolvedPublicStorageDocument>,
    pub(crate) audit_period_slug: Option<String>,
    pub(crate) notes: Vec<String>,
}

pub(crate) struct LivePublicDocumentLoadSpec {
    pub(crate) kind: PublicStorageDocumentRouteKind,
    pub(crate) query: PublicStorageDocumentQuery,
    pub(crate) resolved_label: &'static str,
    pub(crate) plan_label: &'static str,
}

#[derive(Debug, Default, serde::Deserialize)]
pub(crate) struct LiveViewFilterParams {
    #[serde(default)]
    pub(crate) selected_index: Option<usize>,
    #[serde(flatten)]
    query_pairs: BTreeMap<String, String>,
}

impl LiveViewFilterParams {
    pub(crate) fn query(&self) -> LiveOperatorQuery {
        LiveOperatorQuery::from_query_pairs(
            self.query_pairs
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_str())),
        )
    }
}

pub(crate) struct LiveOperatorBootstrap {
    pub(crate) config: WikiConfig,
    pub(crate) auth: DevAuthSessionStatus,
    pub(crate) action_status: ActionExecutionStatusReport,
    pub(crate) action_history: ActionExecutionHistoryReport,
    pub(crate) capabilities: DevAuthCapabilityReport,
    pub(crate) stream_status: StreamRuntimeStatus,
}

pub(crate) struct LiveQueueState {
    pub(crate) query: LiveOperatorQuery,
    pub(crate) batch: RecentChangesBatch,
    pub(crate) backlog_status: BacklogRuntimeStatus,
    pub(crate) queue: Vec<QueuedEdit>,
    pub(crate) selected_index: Option<usize>,
}

#[derive(Debug, Clone)]
pub(crate) struct IngestionSupervisorSnapshot {
    pub(crate) status: LiveIngestionSupervisorStatus,
    pub(crate) queue: Vec<QueuedEdit>,
    pub(crate) next_continue: Option<String>,
}

pub(crate) struct SelectedReviewState {
    pub(crate) scoring_context: Option<sp42_core::ScoringContext>,
    pub(crate) selected_score: Option<sp42_core::CompositeScore>,
    pub(crate) diff: Option<sp42_core::StructuredDiff>,
    pub(crate) media_diff: Option<sp42_core::MediaDiffReport>,
    pub(crate) review_workbench: Option<sp42_core::ReviewWorkbench>,
    pub(crate) readiness: ServerHealthStatus,
    pub(crate) coordination_state: Option<CoordinationStateSummary>,
    pub(crate) coordination_room: Option<CoordinationRoomSummary>,
}

pub(crate) struct LiveOperatorProducts {
    pub(crate) scenario_report: sp42_reporting::PatrolScenarioReport,
    pub(crate) session_digest: sp42_reporting::PatrolSessionDigest,
    pub(crate) shell_state: sp42_reporting::ShellStateModel,
    pub(crate) backend: LiveOperatorBackendStatus,
    pub(crate) debug_snapshot: DebugSnapshot,
    pub(crate) action_preflight: LiveOperatorActionPreflight,
    pub(crate) heuristic_provenance: Vec<LiveOperatorHeuristicProvenance>,
    pub(crate) selected_heuristic_provenance: Option<LiveOperatorHeuristicProvenance>,
}

pub(crate) struct LiveOperatorProductContext<'a> {
    pub(crate) stream_status: &'a StreamRuntimeStatus,
    pub(crate) backlog_status: &'a BacklogRuntimeStatus,
    pub(crate) auth: &'a DevAuthSessionStatus,
    pub(crate) action_status: &'a ActionExecutionStatusReport,
    pub(crate) capabilities: &'a DevAuthCapabilityReport,
}

pub(crate) struct LiveOperatorFinalization {
    pub(crate) queue_state: LiveQueueState,
    pub(crate) bootstrap: LiveOperatorBootstrap,
    pub(crate) selected_review: SelectedReviewState,
    pub(crate) products: LiveOperatorProducts,
    pub(crate) public_documents: LiveOperatorPublicDocuments,
    pub(crate) telemetry: LiveOperatorTelemetry,
    pub(crate) notes: Vec<String>,
    pub(crate) ingestion_supervisor: Option<LiveIngestionSupervisorStatus>,
}

pub(crate) struct LiveOperatorAssembly {
    pub(crate) bootstrap: LiveOperatorBootstrap,
    pub(crate) public_context: LiveOperatorPublicContextState,
    pub(crate) queue_state: LiveQueueState,
    pub(crate) selected_review: SelectedReviewState,
    pub(crate) telemetry_phase_timings: Vec<LiveOperatorPhaseTiming>,
}

pub(crate) async fn load_live_operator_bootstrap(
    state: &AppState,
    headers: &HeaderMap,
    wiki_id: &str,
) -> Result<LiveOperatorBootstrap, String> {
    Ok(LiveOperatorBootstrap {
        config: resolved_wiki_config(state, wiki_id)?,
        auth: current_status(state, headers, true).await,
        action_status: action_status_report(state, headers).await,
        action_history: action_history_report(state, headers, Some(10)).await,
        capabilities: capability_report_for_request(state, headers, wiki_id, false).await,
        stream_status: persisted_stream_status(state, wiki_id).await?,
    })
}

pub(crate) async fn supervisor_snapshot_for_wiki(
    state: &AppState,
    wiki_id: &str,
) -> Option<IngestionSupervisorSnapshot> {
    ingestion_supervisor::supervisor_snapshot_for_wiki(state, wiki_id).await
}

fn apply_public_defaults_to_live_query(
    mut query: LiveOperatorQuery,
    context: &LiveOperatorPublicContextState,
) -> LiveOperatorQuery {
    if let Some(preferences) =
        context
            .preferences
            .as_ref()
            .and_then(|resolved| match &resolved.payload {
                PublicStorageDocumentData::Preferences(value) => Some(value),
                _ => None,
            })
    {
        if query.limit == DEFAULT_LIVE_OPERATOR_LIMIT {
            query.limit = preferences.queue_limit;
        }
        if !preferences.hide_bots {
            query.include_bots = FlagState::Enabled;
        }
        if preferences.hide_minor {
            query.include_minor = FlagState::Disabled;
        }
        if !preferences.editor_types.is_empty() {
            query.include_registered = FlagState::from(
                preferences
                    .editor_types
                    .iter()
                    .any(|value| value == "registered"),
            );
            query.include_anonymous = FlagState::from(
                preferences
                    .editor_types
                    .iter()
                    .any(|value| value == "anonymous"),
            );
            query.include_temporary = FlagState::from(
                preferences
                    .editor_types
                    .iter()
                    .any(|value| value == "temporary"),
            );
        }
        if query.tag_filter.is_none() {
            query.tag_filter = preferences.tag_filters.first().cloned();
        }
    }

    if let Some(rule_set) =
        context
            .active_rule_set
            .as_ref()
            .and_then(|resolved| match &resolved.payload {
                PublicStorageDocumentData::RuleSet(value) => Some(value),
                _ => None,
            })
    {
        if query.namespaces.is_empty() && !rule_set.namespace_allowlist.is_empty() {
            query.namespaces.clone_from(&rule_set.namespace_allowlist);
        }
        if rule_set.hide_minor {
            query.include_minor = FlagState::Disabled;
        }
        if rule_set.hide_bots {
            query.include_bots = FlagState::Disabled;
        }
        if query.tag_filter.is_none() {
            query.tag_filter = rule_set.tag_filters.first().cloned();
        }
        if query.min_score.is_none() {
            query.min_score = rule_set.min_score;
        }
    }

    query.normalized()
}

fn live_queue_policy_from_public_context(
    context: &LiveOperatorPublicContextState,
) -> QueueHeuristicPolicy {
    let mut trusted_usernames = Vec::new();
    let mut duplicate_cluster_boost = FlagState::Enabled;

    if let Some(rule_set) =
        context
            .active_rule_set
            .as_ref()
            .and_then(|resolved| match &resolved.payload {
                PublicStorageDocumentData::RuleSet(value) => Some(value),
                _ => None,
            })
    {
        trusted_usernames.extend(rule_set.trusted_users.iter().cloned());
        duplicate_cluster_boost = rule_set.duplicate_cluster_boost;
    }

    if let Some(team) = context
        .active_team
        .as_ref()
        .and_then(|resolved| match &resolved.payload {
            PublicStorageDocumentData::Team(value) => Some(value),
            _ => None,
        })
    {
        trusted_usernames.extend(team.trusted_users.iter().cloned());
    }

    trusted_usernames.sort();
    trusted_usernames.dedup();

    QueueHeuristicPolicy {
        trusted_usernames,
        duplicate_cluster_boost,
    }
}

fn queue_state_from_supervisor(
    wiki_id: &str,
    query: LiveOperatorQuery,
    snapshot: IngestionSupervisorSnapshot,
    scoring_config: &sp42_core::ScoringConfig,
    public_context: &LiveOperatorPublicContextState,
) -> LiveQueueState {
    let events = snapshot
        .queue
        .into_iter()
        .map(|item| item.event)
        .collect::<Vec<_>>();
    let policy = live_queue_policy_from_public_context(public_context);
    let rebuilt_queue =
        build_ranked_queue_with_policy(events, scoring_config, &policy).unwrap_or_default();
    let queue = filter_live_operator_queue(rebuilt_queue, &query);
    LiveQueueState {
        query,
        batch: RecentChangesBatch {
            events: queue.iter().map(|item| item.event.clone()).collect(),
            next_continue: snapshot.next_continue,
        },
        backlog_status: snapshot
            .status
            .backlog_status
            .unwrap_or(BacklogRuntimeStatus {
                checkpoint_key: format!("recentchanges.rccontinue.{wiki_id}"),
                next_continue: None,
                last_batch_size: 0,
                total_events: 0,
                poll_count: 0,
            }),
        selected_index: (!queue.is_empty()).then_some(0),
        queue,
    }
}

async fn queue_state_from_recentchanges(
    state: &AppState,
    wiki_id: &str,
    query: LiveOperatorQuery,
    config: &WikiConfig,
    client: &BearerHttpClient,
    public_context: &LiveOperatorPublicContextState,
) -> Result<LiveQueueState, String> {
    let namespace_override = (!query.namespaces.is_empty()).then(|| query.namespaces.clone());
    let mut backlog_runtime = BacklogRuntime::new(
        config.clone(),
        runtime_storage_for(state),
        BacklogRuntimeConfig {
            limit: query.limit,
            include_bots: query.include_bots.is_enabled(),
        },
        format!("recentchanges.rccontinue.{wiki_id}"),
    );
    backlog_runtime
        .initialize()
        .await
        .map_err(|error| format!("backlog runtime init failed: {error}"))?;
    let uses_custom_filters = query.rccontinue.is_some()
        || query.unpatrolled_only.is_enabled()
        || !query.include_minor.is_enabled()
        || !query.include_anonymous.is_enabled()
        || !query.include_registered.is_enabled()
        || !query.include_temporary.is_enabled()
        || !query.include_new_pages.is_enabled()
        || query.tag_filter.is_some()
        || namespace_override.is_some();
    let batch = if uses_custom_filters {
        let batch = execute_recent_changes(
            client,
            config,
            &RecentChangesQuery {
                limit: query.limit,
                rccontinue: query.rccontinue.clone(),
                include_bots: query.include_bots,
                unpatrolled_only: query.unpatrolled_only,
                include_minor: query.include_minor,
                include_anonymous: query.include_anonymous,
                include_new_pages: query.include_new_pages,
                tag_filter: query.tag_filter.clone(),
                namespace_override,
            },
        )
        .await
        .map_err(|error| format!("recentchanges fetch failed: {error}"))?;
        backlog_runtime
            .apply_batch(&batch)
            .await
            .map_err(|error| format!("backlog runtime apply failed: {error}"))?;
        batch
    } else {
        backlog_runtime
            .poll(client)
            .await
            .map_err(|error| format!("recentchanges fetch failed: {error}"))?
    };
    let backlog_status = backlog_runtime.status();
    let queue = filter_live_operator_queue(
        build_ranked_queue_with_policy(
            batch.events.clone(),
            &config.scoring,
            &live_queue_policy_from_public_context(public_context),
        )
        .map_err(|error| format!("queue build failed: {error}"))?,
        &query,
    );

    Ok(LiveQueueState {
        query,
        batch,
        backlog_status,
        selected_index: (!queue.is_empty()).then_some(0),
        queue,
    })
}

pub(crate) async fn load_live_queue_state(
    state: &AppState,
    wiki_id: &str,
    filters: &LiveViewFilterParams,
    config: &WikiConfig,
    client: &BearerHttpClient,
    public_context: &LiveOperatorPublicContextState,
) -> Result<LiveQueueState, String> {
    let query = apply_public_defaults_to_live_query(filters.query(), public_context);
    if query.can_use_supervisor_snapshot()
        && let Some(snapshot) = supervisor_snapshot_for_wiki(state, wiki_id).await
        && snapshot.status.active
    {
        return Ok(queue_state_from_supervisor(
            wiki_id,
            query,
            snapshot,
            &config.scoring,
            public_context,
        ));
    }
    queue_state_from_recentchanges(state, wiki_id, query, config, client, public_context).await
}

pub(crate) async fn load_selected_review_state(
    state: &AppState,
    headers: &HeaderMap,
    wiki_id: &str,
    config: &WikiConfig,
    access_token: &str,
    auth: &DevAuthSessionStatus,
    selected: Option<&QueuedEdit>,
) -> Result<SelectedReviewState, String> {
    let mut scoring_context = selected.map(|_| {
        build_scoring_context(&ContextInputs {
            talk_page_wikitext: None,
            liftwing_probability: None,
        })
    });
    let liftwing_risk = if let Some(item) = selected {
        execute_liftwing_score(
            &BearerHttpClient::new(state.http_client.clone(), access_token.to_string()),
            config,
            &LiftWingRequest {
                rev_id: item.event.rev_id,
            },
        )
        .await
        .ok()
    } else {
        None
    };
    if let Some(probability) = liftwing_risk
        && let Some(context) = &mut scoring_context
    {
        context.liftwing_risk = Some(probability);
    }
    let diff = if let Some(item) = selected {
        fetch_revision_diff(state, access_token, config, item).await?
    } else {
        None
    };
    let media_diff = if let Some(item) = selected {
        fetch_revision_media_diff(state, access_token, config, item).await?
    } else {
        None
    };
    if let Some(diff) = diff.as_ref()
        && let Some(context) = &mut scoring_context
    {
        let hints = sp42_core::diff_engine::analyze_diff_for_scoring(
            diff,
            &config.scoring.signal_parameters,
        );
        context.link_addition_only = FlagState::from(hints.link_addition_only());
        context.reference_addition_only = FlagState::from(hints.reference_addition_only());
        context.category_addition_only = FlagState::from(hints.category_addition_only());
        context.interwiki_addition_only = FlagState::from(hints.interwiki_addition_only());
        context.mass_blanking_detected = FlagState::from(hints.mass_blanking_detected());
        context.inserted_profanity_detected = FlagState::from(hints.inserted_profanity_detected());
        context.repeated_character_noise_detected =
            FlagState::from(hints.repeated_character_noise_detected());
    }
    let selected_score = selected
        .zip(scoring_context.as_ref())
        .and_then(|(item, context)| {
            score_edit_with_context(&item.event, &config.scoring, context).ok()
        });
    let readiness = server_readiness(state, headers).await;
    let coordination_state = state.coordination.room_state_summary(wiki_id).await;
    let coordination_room = state
        .coordination
        .snapshot()
        .await
        .rooms
        .into_iter()
        .find(|room| room.wiki_id == wiki_id);
    let review_workbench = selected.and_then(|item| {
        build_review_workbench(
            config,
            item,
            "SP42_REDACTED_TOKEN",
            auth.username.as_deref().unwrap_or("SP42"),
            Some("Generated from live operator state"),
        )
        .ok()
    });

    Ok(SelectedReviewState {
        scoring_context,
        selected_score,
        diff,
        media_diff,
        review_workbench,
        readiness,
        coordination_state,
        coordination_room,
    })
}

pub(crate) fn build_live_operator_notes(
    query: &LiveOperatorQuery,
    backlog_status: &BacklogRuntimeStatus,
    queue: &[QueuedEdit],
    scoring_context: Option<&sp42_core::ScoringContext>,
    diff: Option<&sp42_core::StructuredDiff>,
    review_workbench: Option<&sp42_core::ReviewWorkbench>,
    public_context: &LiveOperatorPublicContextState,
) -> Vec<String> {
    let mut notes =
        vec!["Queue and selected review are built from live recent changes.".to_string()];
    notes.push(format!(
        "Applied filters: limit={} include_bots={} unpatrolled_only={} include_minor={} namespaces={} min_score={} rccontinue={}",
        query.limit,
        query.include_bots,
        query.unpatrolled_only,
        query.include_minor,
        if query.namespaces.is_empty() {
            "default".to_string()
        } else {
            query
                .namespaces
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        },
        query
            .min_score
            .map_or_else(|| "none".to_string(), |value| value.to_string()),
        query.rccontinue.as_deref().unwrap_or("none"),
    ));
    if review_workbench.is_some() {
        notes.push(
            "Safe review workbench previews are attached with a redacted token placeholder."
                .to_string(),
        );
    }
    notes.push(format!(
        "Persistent backlog checkpoint={} next_continue={} total_events={} poll_count={}",
        backlog_status.checkpoint_key,
        backlog_status.next_continue.as_deref().unwrap_or("none"),
        backlog_status.total_events,
        backlog_status.poll_count
    ));
    if scoring_context.is_none() {
        notes.push("LiftWing score is unavailable for the selected edit right now.".to_string());
    }
    if diff.is_none() {
        notes.push("Live diff could not be loaded for the selected edit.".to_string());
    }
    if queue.is_empty() {
        notes.push("No edits matched the current live filter set.".to_string());
    }
    let trusted_user_count = live_queue_policy_from_public_context(public_context)
        .trusted_usernames
        .len();
    if trusted_user_count > 0 {
        notes.push(format!(
            "Trusted-user suppression is active for {trusted_user_count} public usernames."
        ));
    }
    if let Some(rule_set) = public_context
        .active_rule_set
        .as_ref()
        .and_then(|resolved| match &resolved.payload {
            PublicStorageDocumentData::RuleSet(value) => Some(value),
            _ => None,
        })
    {
        notes.push(format!(
            "Active public rule set `{}` was applied to the live queue.",
            rule_set.slug
        ));
    }
    notes
}

fn build_live_operator_heuristic_provenance(
    queue: &[QueuedEdit],
    public_context: &LiveOperatorPublicContextState,
) -> Vec<LiveOperatorHeuristicProvenance> {
    queue
        .iter()
        .map(|item| {
            let matched_trusted_sources = matched_trusted_sources_for_item(item, public_context);
            let applied_rule_sources = applied_rule_sources(public_context);
            let duplicate_cluster_size = duplicate_cluster_size_for_item(item);
            let obvious_vandalism = FlagState::from(has_signal(
                item,
                &sp42_core::ScoringSignal::ObviousVandalism,
            ));
            let mut notes = item
                .score
                .contributions
                .iter()
                .map(|entry| match &entry.note {
                    Some(note) => format!("{}: {}", entry.signal, note),
                    None => entry.signal.to_string(),
                })
                .collect::<Vec<_>>();
            if !matched_trusted_sources.is_empty() {
                notes.push(format!(
                    "trusted-user suppression matched {}",
                    matched_trusted_sources.join(", ")
                ));
            }

            LiveOperatorHeuristicProvenance {
                rev_id: item.event.rev_id,
                performer: item.event.performer.stable_label().to_string(),
                resolved_team_slug: active_team_slug_from_context(public_context),
                resolved_team_document_title: active_team_document_title(public_context),
                resolved_rule_set_slug: active_rule_set_slug_from_context(public_context),
                resolved_rule_set_document_title: active_rule_set_document_title(public_context),
                applied_rule_sources,
                matched_trusted_sources,
                duplicate_cluster_size,
                obvious_vandalism,
                notes,
            }
        })
        .collect()
}

fn active_team_slug_from_context(context: &LiveOperatorPublicContextState) -> Option<String> {
    context
        .active_team
        .as_ref()
        .and_then(|resolved| match &resolved.payload {
            PublicStorageDocumentData::Team(value) => Some(value.slug.clone()),
            _ => None,
        })
}

fn active_team_document_title(context: &LiveOperatorPublicContextState) -> Option<String> {
    context
        .active_team
        .as_ref()
        .map(|resolved| resolved.document.title.clone())
}

fn active_rule_set_slug_from_context(context: &LiveOperatorPublicContextState) -> Option<String> {
    context
        .active_rule_set
        .as_ref()
        .and_then(|resolved| match &resolved.payload {
            PublicStorageDocumentData::RuleSet(value) => Some(value.slug.clone()),
            _ => None,
        })
}

fn active_rule_set_document_title(context: &LiveOperatorPublicContextState) -> Option<String> {
    context
        .active_rule_set
        .as_ref()
        .map(|resolved| resolved.document.title.clone())
}

fn applied_rule_sources(public_context: &LiveOperatorPublicContextState) -> Vec<String> {
    let mut sources = Vec::new();
    if let Some(rule_set) = public_context
        .active_rule_set
        .as_ref()
        .and_then(|resolved| match &resolved.payload {
            PublicStorageDocumentData::RuleSet(value) => Some(value),
            _ => None,
        })
    {
        sources.push(format!("rule_set:{}", rule_set.slug));
    }
    sources
}

fn matched_trusted_sources_for_item(
    item: &QueuedEdit,
    public_context: &LiveOperatorPublicContextState,
) -> Vec<String> {
    if !item.event.performer.is_registered() {
        return Vec::new();
    }

    let username = item.event.performer.stable_label();
    let mut sources = Vec::new();
    if let Some(team) =
        public_context
            .active_team
            .as_ref()
            .and_then(|resolved| match &resolved.payload {
                PublicStorageDocumentData::Team(value) => Some(value),
                _ => None,
            })
        && team
            .trusted_users
            .iter()
            .any(|candidate| candidate == username)
    {
        sources.push(format!("team:{}", team.slug));
    }
    if let Some(rule_set) = public_context
        .active_rule_set
        .as_ref()
        .and_then(|resolved| match &resolved.payload {
            PublicStorageDocumentData::RuleSet(value) => Some(value),
            _ => None,
        })
        && rule_set
            .trusted_users
            .iter()
            .any(|candidate| candidate == username)
    {
        sources.push(format!("rule_set:{}", rule_set.slug));
    }
    sources
}

fn duplicate_cluster_size_for_item(item: &QueuedEdit) -> Option<u32> {
    item.score
        .contributions
        .iter()
        .find(|entry| matches!(entry.signal, sp42_core::ScoringSignal::DuplicatePattern))
        .and_then(|entry| entry.note.as_deref())
        .and_then(|note| note.rsplit(' ').next())
        .and_then(|value| value.parse::<u32>().ok())
}

fn has_signal(item: &QueuedEdit, signal: &sp42_core::ScoringSignal) -> bool {
    item.score
        .contributions
        .iter()
        .any(|entry| &entry.signal == signal)
}

fn action_reason_note(
    item: &QueuedEdit,
    provenance: Option<&LiveOperatorHeuristicProvenance>,
) -> Option<String> {
    let mut parts = Vec::new();
    if has_signal(item, &sp42_core::ScoringSignal::ObviousVandalism) {
        parts.push("obvious-vandalism".to_string());
    }
    if let Some(cluster_size) = provenance.and_then(|entry| entry.duplicate_cluster_size) {
        parts.push(format!("duplicate-cluster={cluster_size}"));
    }
    if let Some(provenance) = provenance {
        if !provenance.matched_trusted_sources.is_empty() {
            parts.push(format!(
                "trusted-source={}",
                provenance.matched_trusted_sources.join("+")
            ));
        }
        if !provenance.applied_rule_sources.is_empty() {
            parts.push(format!(
                "rules={}",
                provenance.applied_rule_sources.join("+")
            ));
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(format!("SP42 rationale: {}", parts.join("; ")))
    }
}

pub(crate) fn build_live_operator_products(
    wiki_id: &str,
    queue: &[QueuedEdit],
    selected: Option<&QueuedEdit>,
    selected_review: &SelectedReviewState,
    public_context: &LiveOperatorPublicContextState,
    context: &LiveOperatorProductContext<'_>,
) -> LiveOperatorProducts {
    let scenario_report = build_patrol_scenario_report(&PatrolScenarioReportInputs {
        queue,
        selected,
        scoring_context: selected_review.scoring_context.as_ref(),
        diff: selected_review.diff.as_ref(),
        review_workbench: selected_review.review_workbench.as_ref(),
        stream_status: Some(context.stream_status),
        backlog_status: Some(context.backlog_status),
        coordination: selected_review.coordination_state.as_ref(),
        wiki_id_hint: Some(wiki_id),
    });
    let session_digest = build_patrol_session_digest(&PatrolSessionDigestInputs {
        report: &scenario_report,
        review_workbench: selected_review.review_workbench.as_ref(),
    });
    let shell_state = build_shell_state_model(&ShellStateInputs {
        report: &scenario_report,
        review_workbench: selected_review.review_workbench.as_ref(),
    });
    let backend = live_operator_backend_status(&selected_review.readiness, context.auth);
    let debug_snapshot = build_debug_snapshot(&DebugSnapshotInputs {
        queue,
        selected,
        scoring_context: selected_review.scoring_context.as_ref(),
        diff: selected_review.diff.as_ref(),
        review_workbench: selected_review.review_workbench.as_ref(),
        stream_status: Some(context.stream_status),
        backlog_status: Some(context.backlog_status),
        coordination: selected_review.coordination_state.as_ref(),
    });
    let heuristic_provenance = build_live_operator_heuristic_provenance(queue, public_context);
    let selected_heuristic_provenance = selected.and_then(|item| {
        heuristic_provenance
            .iter()
            .find(|entry| entry.rev_id == item.event.rev_id)
            .cloned()
    });
    let selected_note =
        selected.and_then(|item| action_reason_note(item, selected_heuristic_provenance.as_ref()));
    let action_preflight = build_live_operator_action_preflight(
        selected,
        context.capabilities,
        context.action_status,
        selected_note.as_deref(),
    );

    LiveOperatorProducts {
        scenario_report,
        session_digest,
        shell_state,
        backend,
        debug_snapshot,
        action_preflight,
        heuristic_provenance,
        selected_heuristic_provenance,
    }
}

pub(crate) fn finalize_live_operator_view(
    state: &AppState,
    wiki_id: &str,
    finalized: LiveOperatorFinalization,
) -> LiveOperatorView {
    let LiveOperatorFinalization {
        queue_state,
        bootstrap,
        selected_review,
        products,
        public_documents,
        telemetry,
        notes,
        ingestion_supervisor,
    } = finalized;

    LiveOperatorView {
        project: sp42_core::branding::PROJECT_NAME.to_string(),
        fetched_at_ms: state.clock.now_ms(),
        wiki_id: wiki_id.to_string(),
        query: queue_state.query,
        queue: queue_state.queue,
        selected_index: queue_state.selected_index,
        scoring_context: selected_review.scoring_context,
        diff: selected_review.diff,
        media_diff: selected_review.media_diff,
        review_workbench: selected_review.review_workbench,
        stream_status: Some(bootstrap.stream_status),
        backlog_status: Some(queue_state.backlog_status),
        scenario_report: products.scenario_report,
        session_digest: products.session_digest,
        shell_state: products.shell_state,
        capabilities: bootstrap.capabilities,
        auth: bootstrap.auth,
        backend: products.backend,
        action_status: bootstrap.action_status,
        action_history: bootstrap.action_history,
        action_preflight: products.action_preflight,
        public_documents,
        heuristic_provenance: products.heuristic_provenance,
        selected_heuristic_provenance: products.selected_heuristic_provenance,
        ingestion_supervisor,
        coordination_room: selected_review.coordination_room,
        coordination_state: selected_review.coordination_state,
        debug_snapshot: products.debug_snapshot,
        telemetry,
        notes,
        next_continue: queue_state.batch.next_continue,
    }
}
