use std::time::Instant;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};

use crate::{
    access_token_for_request, build_live_operator_notes, build_live_operator_products,
    finalize_live_operator_view, load_live_operator_bootstrap, load_live_queue_state,
    load_selected_review_state, storage_routes, supervisor_snapshot_for_wiki, AppState,
    BearerHttpClient, LiveOperatorAssembly, LiveOperatorFinalization,
    LiveOperatorPhaseTiming, LiveOperatorProductContext, LiveOperatorTelemetry,
    LiveOperatorView, LiveViewFilterParams,
};

pub(crate) fn operator_phase_timing(phase: &str, started_at: Instant) -> LiveOperatorPhaseTiming {
    LiveOperatorPhaseTiming {
        phase: phase.to_string(),
        duration_ms: u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX),
    }
}

pub(crate) async fn get_live_operator_view(
    Path(wiki_id): Path<String>,
    axum::extract::Query(filters): axum::extract::Query<LiveViewFilterParams>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<LiveOperatorView>, (StatusCode, Json<serde_json::Value>)> {
    live_operator_view(&state, &headers, &wiki_id, &filters)
        .await
        .map(Json)
        .map_err(|error| {
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": error })),
            )
        })
}

pub(crate) async fn load_live_operator_assembly(
    state: &AppState,
    headers: &HeaderMap,
    wiki_id: &str,
    filters: &LiveViewFilterParams,
) -> Result<LiveOperatorAssembly, String> {
    let mut phase_timings = Vec::new();

    let phase_started = Instant::now();
    let bootstrap = load_live_operator_bootstrap(state, headers, wiki_id).await?;
    phase_timings.push(operator_phase_timing("bootstrap", phase_started));

    let phase_started = Instant::now();
    let access_token = access_token_for_request(state, headers)
        .await
        .ok_or_else(|| "No local Wikimedia access token is available.".to_string())?;
    let client = BearerHttpClient::new(state.http_client.clone(), access_token.clone());
    let public_context = storage_routes::load_live_operator_public_context(
        state,
        headers,
        wiki_id,
        &bootstrap.auth,
        &bootstrap.capabilities,
        &client,
        &bootstrap.config,
    )
    .await;
    phase_timings.push(operator_phase_timing("public-documents", phase_started));

    let phase_started = Instant::now();
    let queue_state = load_live_queue_state(
        state,
        wiki_id,
        filters,
        &bootstrap.config,
        &client,
        &public_context,
    )
    .await?;
    phase_timings.push(operator_phase_timing("recentchanges", phase_started));

    let phase_started = Instant::now();
    let selected = queue_state
        .selected_index
        .and_then(|index| queue_state.queue.get(index));
    phase_timings.push(operator_phase_timing("queue", phase_started));

    let phase_started = Instant::now();
    let selected_review = load_selected_review_state(
        state,
        headers,
        wiki_id,
        &bootstrap.config,
        &access_token,
        &bootstrap.auth,
        selected,
    )
    .await?;
    phase_timings.push(operator_phase_timing("selection", phase_started));

    Ok(LiveOperatorAssembly {
        bootstrap,
        public_context,
        queue_state,
        selected_review,
        telemetry_phase_timings: phase_timings,
    })
}

pub(crate) fn build_live_operator_finalization(
    wiki_id: &str,
    total_started: Instant,
    assembly: LiveOperatorAssembly,
) -> LiveOperatorFinalization {
    let selected = assembly
        .queue_state
        .selected_index
        .and_then(|index| assembly.queue_state.queue.get(index));
    let products = build_live_operator_products(
        wiki_id,
        &assembly.queue_state.queue,
        selected,
        &assembly.selected_review,
        &LiveOperatorProductContext {
            stream_status: &assembly.bootstrap.stream_status,
            backlog_status: &assembly.queue_state.backlog_status,
            auth: &assembly.bootstrap.auth,
            action_status: &assembly.bootstrap.action_status,
            capabilities: &assembly.bootstrap.capabilities,
        },
    );
    let mut notes = build_live_operator_notes(
        &assembly.queue_state.query,
        &assembly.queue_state.backlog_status,
        &assembly.queue_state.queue,
        assembly.selected_review.scoring_context.as_ref(),
        assembly.selected_review.diff.as_ref(),
        assembly.selected_review.review_workbench.as_ref(),
    );
    notes.extend(assembly.public_context.notes.clone());
    let telemetry = LiveOperatorTelemetry {
        total_duration_ms: u64::try_from(total_started.elapsed().as_millis()).unwrap_or(u64::MAX),
        phase_timings: assembly.telemetry_phase_timings,
    };

    LiveOperatorFinalization {
        queue_state: assembly.queue_state,
        bootstrap: assembly.bootstrap,
        selected_review: assembly.selected_review,
        products,
        public_documents: storage_routes::live_operator_public_documents_model(&assembly.public_context),
        telemetry,
        notes,
        ingestion_supervisor: None,
    }
}

pub(crate) async fn live_operator_view(
    state: &AppState,
    headers: &HeaderMap,
    wiki_id: &str,
    filters: &LiveViewFilterParams,
) -> Result<LiveOperatorView, String> {
    let total_started = Instant::now();
    let assembly = load_live_operator_assembly(state, headers, wiki_id, filters).await?;
    let mut finalization = build_live_operator_finalization(wiki_id, total_started, assembly);
    finalization.ingestion_supervisor = supervisor_snapshot_for_wiki(state, wiki_id)
        .await
        .map(|snapshot| snapshot.status);
    Ok(finalize_live_operator_view(state, wiki_id, finalization))
}
