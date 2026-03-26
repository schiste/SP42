use std::time::Duration;

use sp42_core::{
    build_ranked_queue, BacklogRuntime, BacklogRuntimeConfig, QueuedEdit, StreamRuntimeStatus,
    WikiConfig,
};

use crate::{
    default_limit, persisted_stream_status, resolved_wiki_config, runtime_storage_for, AppState,
    BearerHttpClient, IngestionSupervisorSnapshot,
};

pub(crate) fn supervisor_poll_interval_ms() -> u64 {
    std::env::var("SP42_INGESTION_POLL_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(15_000)
}

pub(crate) fn supervisor_wiki_ids() -> Vec<String> {
    let configured = std::env::var("SP42_SUPERVISOR_WIKIS")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "frwiki".to_string());

    configured
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

pub(crate) fn spawn_ingestion_supervisors(state: &AppState) {
    let poll_interval_ms = supervisor_poll_interval_ms();
    for wiki_id in supervisor_wiki_ids() {
        let state_clone = state.clone();
        tokio::spawn(async move {
            run_ingestion_supervisor_for_wiki(state_clone, wiki_id, poll_interval_ms).await;
        });
    }
}

pub(crate) async fn run_ingestion_supervisor_for_wiki(
    state: AppState,
    wiki_id: String,
    poll_interval_ms: u64,
) {
    loop {
        let started_at_ms = state.clock.now_ms();
        let snapshot =
            supervisor_snapshot_iteration(&state, &wiki_id, poll_interval_ms, started_at_ms).await;
        let sleep_ms = poll_interval_ms.max(1_000);
        {
            let mut guard = state.ingestion_supervisor.write().await;
            guard.insert(wiki_id.clone(), snapshot);
        }
        tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
    }
}

pub(crate) async fn supervisor_snapshot_iteration(
    state: &AppState,
    wiki_id: &str,
    poll_interval_ms: u64,
    started_at_ms: i64,
) -> IngestionSupervisorSnapshot {
    let previous = {
        let guard = state.ingestion_supervisor.read().await;
        guard.get(wiki_id).cloned()
    };
    let previous_run_count = previous
        .as_ref()
        .map_or(0, |snapshot| snapshot.status.run_count);
    let previous_success = previous
        .as_ref()
        .and_then(|snapshot| snapshot.status.last_success_at_ms);

    let Some(access_token) = state.local_oauth.access_token().map(ToString::to_string) else {
        return supervisor_inactive_snapshot(
            wiki_id,
            poll_interval_ms,
            previous_run_count,
            started_at_ms,
            previous_success,
            "No local Wikimedia access token is available.".to_string(),
            "Supervisor is idle until a local token is configured.".to_string(),
        );
    };

    let config = match resolved_wiki_config(state, wiki_id) {
        Ok(config) => config,
        Err(error) => {
            return supervisor_inactive_snapshot(
                wiki_id,
                poll_interval_ms,
                previous_run_count,
                started_at_ms,
                previous_success,
                error,
                "Supervisor could not resolve wiki configuration.".to_string(),
            );
        }
    };

    let poll_result = perform_supervisor_poll(state, wiki_id, &config, access_token).await;

    match poll_result {
        Ok((batch, backlog_status, stream_status, queue)) => {
            let mut notes = vec![format!(
                "Supervisor polled {} edits.",
                backlog_status.last_batch_size
            )];
            if stream_status.last_event_id.is_none() && backlog_status.next_continue.is_some() {
                notes.push(
                    "Stream checkpoint is empty; backlog checkpoint is currently authoritative."
                        .to_string(),
                );
            }
            IngestionSupervisorSnapshot {
                status: sp42_core::LiveIngestionSupervisorStatus {
                    wiki_id: wiki_id.to_string(),
                    active: true,
                    poll_interval_ms,
                    run_count: previous_run_count.saturating_add(1),
                    latest_queue_depth: queue.len(),
                    last_started_at_ms: Some(started_at_ms),
                    last_success_at_ms: Some(state.clock.now_ms()),
                    last_error: None,
                    stream_status: Some(stream_status),
                    backlog_status: Some(backlog_status),
                    notes,
                },
                queue,
                next_continue: batch.next_continue,
            }
        }
        Err(error) => supervisor_inactive_snapshot(
            wiki_id,
            poll_interval_ms,
            previous_run_count,
            started_at_ms,
            previous_success,
            error.to_string(),
            "Supervisor poll failed; request path will continue to fall back.".to_string(),
        ),
    }
}

pub(crate) fn supervisor_inactive_snapshot(
    wiki_id: &str,
    poll_interval_ms: u64,
    previous_run_count: u64,
    started_at_ms: i64,
    previous_success: Option<i64>,
    error: String,
    note: String,
) -> IngestionSupervisorSnapshot {
    IngestionSupervisorSnapshot {
        status: sp42_core::LiveIngestionSupervisorStatus {
            wiki_id: wiki_id.to_string(),
            active: false,
            poll_interval_ms,
            run_count: previous_run_count.saturating_add(1),
            latest_queue_depth: 0,
            last_started_at_ms: Some(started_at_ms),
            last_success_at_ms: previous_success,
            last_error: Some(error),
            stream_status: None,
            backlog_status: None,
            notes: vec![note],
        },
        queue: Vec::new(),
        next_continue: None,
    }
}

pub(crate) async fn perform_supervisor_poll(
    state: &AppState,
    wiki_id: &str,
    config: &WikiConfig,
    access_token: String,
) -> Result<
    (
        sp42_core::RecentChangesBatch,
        sp42_core::BacklogRuntimeStatus,
        StreamRuntimeStatus,
        Vec<QueuedEdit>,
    ),
    sp42_core::BacklogRuntimeError,
> {
    let client = BearerHttpClient::new(state.http_client.clone(), access_token);
    let storage = runtime_storage_for(state);
    let mut backlog_runtime = BacklogRuntime::new(
        config.clone(),
        storage,
        BacklogRuntimeConfig {
            limit: default_limit(),
            include_bots: false,
        },
        format!("recentchanges.rccontinue.{wiki_id}"),
    );
    backlog_runtime.initialize().await?;
    let batch = backlog_runtime.poll(&client).await?;
    let backlog_status = backlog_runtime.status();
    let stream_status = persisted_stream_status(state, wiki_id)
        .await
        .map_err(|message| {
            sp42_core::BacklogRuntimeError::Storage(sp42_core::StorageError::Operation { message })
        })?;
    let queue = build_ranked_queue(batch.events.clone(), &config.scoring).map_err(|error| {
        sp42_core::BacklogRuntimeError::Storage(sp42_core::StorageError::Operation {
            message: error.to_string(),
        })
    })?;
    Ok((batch, backlog_status, stream_status, queue))
}

pub(crate) async fn supervisor_snapshot_for_wiki(
    state: &AppState,
    wiki_id: &str,
) -> Option<IngestionSupervisorSnapshot> {
    let guard = state.ingestion_supervisor.read().await;
    guard.get(wiki_id).cloned()
}
