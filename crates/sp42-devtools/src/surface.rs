//! Deterministic operator-surface builders for local demos and shell previews.

use sp42_core::{
    CompositeScore, ContextInputs, HttpRequest, QueuedEdit, RecentChangesQuery, ReviewWorkbench,
    ScoringContext, SessionActionExecutionRequest, StreamIngestor, WikiConfig,
    build_liftwing_score_request, build_ranked_queue, build_recent_changes_request,
    build_review_workbench, build_scoring_context, build_session_action_execution_requests,
    diff_lines, score_edit_with_context,
};
use sp42_reporting::{
    PatrolOperatorSummary, PatrolOperatorSummaryInputs, PatrolScenarioReport,
    PatrolScenarioReportInputs, PatrolSessionDigest, PatrolSessionDigestInputs, ShellStateInputs,
    ShellStateModel, build_patrol_operator_summary, build_patrol_scenario_report,
    build_patrol_session_digest, build_shell_state_model,
};

use crate::preview::{
    DEV_PREVIEW_SAMPLE_EVENTS, DevBacklogPreview, DevCoordinationPreview, DevStreamPreview,
    build_dev_backlog_preview, build_dev_coordination_preview, build_dev_stream_preview,
    parse_default_dev_wiki_config,
};

#[derive(Debug, thiserror::Error)]
pub enum DevtoolsError {
    #[error(transparent)]
    Backlog(#[from] sp42_core::BacklogRuntimeError),
    #[error(transparent)]
    Codec(#[from] sp42_core::CodecError),
    #[error(transparent)]
    Config(#[from] sp42_core::ConfigError),
    #[error("developer fixture produced no queue item")]
    EmptyQueue,
    #[error(transparent)]
    LiftWing(#[from] sp42_core::LiftWingError),
    #[error(transparent)]
    RecentChanges(#[from] sp42_core::RecentChangesError),
    #[error(transparent)]
    ReviewWorkbench(#[from] sp42_core::ReviewWorkbenchError),
    #[error(transparent)]
    Scoring(#[from] sp42_core::ScoringError),
    #[error(transparent)]
    StreamIngestor(#[from] sp42_core::StreamIngestorError),
    #[error(transparent)]
    StreamRuntime(#[from] sp42_core::StreamRuntimeError),
}

#[derive(Debug, Clone, PartialEq)]
pub struct DevContextOptions {
    pub talk_page_wikitext: Option<String>,
    pub liftwing_probability: Option<f32>,
}

impl Default for DevContextOptions {
    fn default() -> Self {
        Self {
            talk_page_wikitext: Some("{{Avertissement niveau 2 pour vandalisme}}".to_string()),
            liftwing_probability: Some(0.72),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevWorkbenchOptions {
    pub token: String,
    pub actor: String,
    pub note: Option<String>,
}

impl Default for DevWorkbenchOptions {
    fn default() -> Self {
        Self {
            token: "devtools-local-token".to_string(),
            actor: "SP42-devtools".to_string(),
            note: Some("devtools fixture".to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DevOperatorSurfaceOptions {
    pub stream_event_id_prefix: String,
    pub context: Option<DevContextOptions>,
    pub workbench: Option<DevWorkbenchOptions>,
    pub action_note: Option<String>,
}

impl Default for DevOperatorSurfaceOptions {
    fn default() -> Self {
        Self {
            stream_event_id_prefix: "devtools-fixture".to_string(),
            context: Some(DevContextOptions::default()),
            workbench: Some(DevWorkbenchOptions::default()),
            action_note: Some("devtools action preview".to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DevContextPreview {
    pub selected: QueuedEdit,
    pub recentchanges_request: HttpRequest,
    pub liftwing_request: HttpRequest,
    pub context: ScoringContext,
    pub contextual_score: CompositeScore,
    pub liftwing_probability: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DevOperatorSurface {
    pub config: WikiConfig,
    pub queue: Vec<QueuedEdit>,
    pub selected_index: usize,
    pub context: Option<DevContextPreview>,
    pub workbench: Option<ReviewWorkbench>,
    pub action_requests: Vec<SessionActionExecutionRequest>,
    pub stream: DevStreamPreview,
    pub backlog: DevBacklogPreview,
    pub coordination: DevCoordinationPreview,
    pub report: PatrolScenarioReport,
    pub operator_summary: PatrolOperatorSummary,
    pub session_digest: PatrolSessionDigest,
    pub shell_state: ShellStateModel,
}

impl DevOperatorSurface {
    #[must_use]
    pub fn selected(&self) -> &QueuedEdit {
        &self.queue[self.selected_index]
    }
}

/// Build a ranked queue from deterministic recentchange fixture events.
///
/// # Errors
///
/// Returns [`DevtoolsError`] when fixture ingestion or queue scoring fails.
pub fn build_dev_queue(
    config: &WikiConfig,
    payload: &str,
) -> Result<Vec<QueuedEdit>, DevtoolsError> {
    let ingestor = StreamIngestor::from_config(config);
    let events = ingestor.ingest_lines(payload)?;
    Ok(build_ranked_queue(events, &config.scoring)?)
}

#[must_use]
pub fn build_dev_context(options: &DevContextOptions) -> ScoringContext {
    build_scoring_context(&ContextInputs {
        talk_page_wikitext: options.talk_page_wikitext.clone(),
        liftwing_probability: options.liftwing_probability,
    })
}

/// Build deterministic request/context/score artifacts for the selected edit.
///
/// # Errors
///
/// Returns [`DevtoolsError`] when request construction or contextual scoring
/// fails.
pub fn build_dev_context_preview(
    config: &WikiConfig,
    selected: &QueuedEdit,
    options: &DevContextOptions,
) -> Result<DevContextPreview, DevtoolsError> {
    let recentchanges_request = build_dev_recentchanges_request(config)?;
    let liftwing_request = build_dev_liftwing_request(config, selected)?;
    let context = build_dev_context(options);
    let contextual_score = score_edit_with_context(&selected.event, &config.scoring, &context)?;

    Ok(DevContextPreview {
        selected: selected.clone(),
        recentchanges_request,
        liftwing_request,
        context,
        contextual_score,
        liftwing_probability: options.liftwing_probability,
    })
}

/// Build the deterministic recentchanges request used by shell previews.
///
/// # Errors
///
/// Returns [`DevtoolsError`] when request construction fails.
pub fn build_dev_recentchanges_request(config: &WikiConfig) -> Result<HttpRequest, DevtoolsError> {
    Ok(build_recent_changes_request(
        config,
        &RecentChangesQuery {
            limit: 25,
            rccontinue: None,
            include_bots: false.into(),
            unpatrolled_only: false.into(),
            include_minor: true.into(),
            include_anonymous: true.into(),
            include_new_pages: true.into(),
            tag_filter: None,
            namespace_override: None,
        },
    )?)
}

/// Build the deterministic `LiftWing` request used by shell previews.
///
/// # Errors
///
/// Returns [`DevtoolsError`] when request construction fails.
pub fn build_dev_liftwing_request(
    config: &WikiConfig,
    selected: &QueuedEdit,
) -> Result<HttpRequest, DevtoolsError> {
    Ok(build_liftwing_score_request(
        config,
        &sp42_core::LiftWingRequest {
            rev_id: selected.event.rev_id,
        },
    )?)
}

/// Build a deterministic review workbench for the selected edit.
///
/// # Errors
///
/// Returns [`DevtoolsError`] when workbench construction fails.
pub fn build_dev_workbench(
    config: &WikiConfig,
    selected: &QueuedEdit,
    options: &DevWorkbenchOptions,
) -> Result<ReviewWorkbench, DevtoolsError> {
    Ok(build_review_workbench(
        config,
        selected,
        &options.token,
        &options.actor,
        options.note.as_deref(),
    )?)
}

/// Build deterministic action request previews for the selected edit.
///
/// # Errors
///
/// Returns [`DevtoolsError`] when action request construction fails.
pub fn build_dev_action_requests(
    selected: &QueuedEdit,
    note: Option<&str>,
) -> Result<Vec<SessionActionExecutionRequest>, DevtoolsError> {
    Ok(build_session_action_execution_requests(selected, note)?)
}

/// Build the default deterministic operator surface from embedded fixtures.
///
/// # Errors
///
/// Returns [`DevtoolsError`] when any fixture, preview, or report artifact fails
/// to build.
pub async fn build_default_dev_operator_surface(
    options: &DevOperatorSurfaceOptions,
) -> Result<DevOperatorSurface, DevtoolsError> {
    let config = parse_default_dev_wiki_config()?;
    build_dev_operator_surface(&config, DEV_PREVIEW_SAMPLE_EVENTS, options).await
}

/// Build deterministic operator-surface artifacts from a fixture payload.
///
/// # Errors
///
/// Returns [`DevtoolsError`] when any fixture, preview, or report artifact fails
/// to build.
pub async fn build_dev_operator_surface(
    config: &WikiConfig,
    payload: &str,
    options: &DevOperatorSurfaceOptions,
) -> Result<DevOperatorSurface, DevtoolsError> {
    let queue = build_dev_queue(config, payload)?;
    let selected = queue.first().ok_or(DevtoolsError::EmptyQueue)?;
    let context = options
        .context
        .as_ref()
        .map(|context| build_dev_context_preview(config, selected, context))
        .transpose()?;
    let workbench = options
        .workbench
        .as_ref()
        .map(|workbench| build_dev_workbench(config, selected, workbench))
        .transpose()?;
    let action_requests = build_dev_action_requests(selected, options.action_note.as_deref())?;
    let stream = build_dev_stream_preview(config, payload, &options.stream_event_id_prefix).await?;
    let backlog = build_dev_backlog_preview(config).await?;
    let coordination = build_dev_coordination_preview(&config.wiki_id)?;
    let diff = diff_lines("Avant\n", "Avant\nApres\n");
    let report = build_patrol_scenario_report(&PatrolScenarioReportInputs {
        queue: &queue,
        selected: Some(selected),
        scoring_context: context.as_ref().map(|context| &context.context),
        diff: Some(&diff),
        review_workbench: workbench.as_ref(),
        stream_status: Some(&stream.status),
        backlog_status: Some(&backlog.status),
        coordination: Some(&coordination.summary),
        wiki_id_hint: Some(&config.wiki_id),
    });
    let operator_summary = build_patrol_operator_summary(&PatrolOperatorSummaryInputs {
        report: &report,
        review_workbench: workbench.as_ref(),
    });
    let session_digest = build_patrol_session_digest(&PatrolSessionDigestInputs {
        report: &report,
        review_workbench: workbench.as_ref(),
    });
    let shell_state = build_shell_state_model(&ShellStateInputs {
        report: &report,
        review_workbench: workbench.as_ref(),
    });

    Ok(DevOperatorSurface {
        config: config.clone(),
        queue,
        selected_index: 0,
        context,
        workbench,
        action_requests,
        stream,
        backlog,
        coordination,
        report,
        operator_summary,
        session_digest,
        shell_state,
    })
}

#[must_use]
pub fn render_dev_queue_lines(queue: &[QueuedEdit]) -> Vec<String> {
    queue
        .iter()
        .take(4)
        .map(|item| {
            format!(
                "queue rev={} score={} title=\"{}\" signals={}",
                item.event.rev_id,
                item.score.total,
                item.event.title,
                item.score.contributions.len()
            )
        })
        .collect()
}

#[must_use]
pub fn render_dev_stream_actionable_lines(snapshot: &DevStreamPreview) -> Vec<String> {
    snapshot
        .edits
        .iter()
        .map(|edit| format!("stream rev={} title=\"{}\"", edit.rev_id, edit.title))
        .collect()
}

#[must_use]
pub fn render_dev_stream_preview(snapshot: &DevStreamPreview) -> String {
    let status = &snapshot.status;
    format!(
        "stream delivered={} filtered={} reconnects={} checkpoint={}",
        status.delivered_events,
        status.filtered_events,
        status.reconnect_attempts,
        status.last_event_id.as_deref().unwrap_or("none")
    )
}

#[must_use]
pub fn render_dev_backlog_preview(snapshot: &DevBacklogPreview) -> String {
    format!(
        "backlog report {:?} {}\nbacklog batch={} total={} polls={} checkpoint={} next_continue={} first_rev={}",
        snapshot.request.method,
        snapshot.request.url,
        snapshot.batch.events.len(),
        snapshot.status.total_events,
        snapshot.status.poll_count,
        snapshot.status.checkpoint_key,
        snapshot.status.next_continue.as_deref().unwrap_or("none"),
        snapshot
            .batch
            .events
            .first()
            .map_or(0, |event| event.rev_id)
    )
}

#[must_use]
pub fn render_dev_coordination_preview(snapshot: &DevCoordinationPreview) -> String {
    let summary = &snapshot.summary;
    [
        format!(
            "coordination wiki={} claims={} presence={} flags={} deltas={} resolutions={} actions={}",
            summary.wiki_id,
            summary.claims.len(),
            summary.presence.len(),
            summary.flagged_edits.len(),
            summary.score_deltas.len(),
            summary.race_resolutions.len(),
            summary.recent_actions.len()
        ),
        snapshot.roundtrips.join("\n"),
        summary.claims.first().map_or_else(
            || "coordination claims unavailable".to_string(),
            |claim| format!("coordination claim rev={} actor={}", claim.rev_id, claim.actor),
        ),
    ]
    .join("\n")
}

#[must_use]
pub fn render_dev_transport_lines(surface: &DevOperatorSurface) -> Vec<String> {
    let selected = surface.selected();
    let mut transport_lines = vec![
        format!(
            "queue wiki={} depth={} top_rev={} title=\"{}\" score={}",
            selected.event.wiki_id,
            surface.queue.len(),
            selected.event.rev_id,
            selected.event.title,
            selected.score.total
        ),
        render_dev_queue_lines(&surface.queue).join("\n"),
    ];

    if let Some(workbench) = &surface.workbench {
        transport_lines.push(format!(
            "action_workbench requests={} training_rows={}",
            workbench.requests.len(),
            workbench.training_csv.lines().skip(1).count()
        ));
    }
    if let Some(context) = &surface.context {
        transport_lines.extend([
            format!(
                "context score={} user_risk={} liftwing={}",
                context.contextual_score.total,
                context.context.user_risk.is_some(),
                context
                    .context
                    .liftwing_risk
                    .map_or_else(|| "none".to_string(), |value| format!("{value:.2}"))
            ),
            format!(
                "recentchanges {:?} {}",
                context.recentchanges_request.method, context.recentchanges_request.url
            ),
            format!(
                "liftwing {:?} {}",
                context.liftwing_request.method, context.liftwing_request.url
            ),
        ]);
    }

    transport_lines.push(format!(
        "backlog report {:?} {} events={} polls={} next_continue={} checkpoint={}",
        surface.backlog.request.method,
        surface.backlog.request.url,
        surface.backlog.batch.events.len(),
        surface.backlog.status.poll_count,
        surface
            .backlog
            .status
            .next_continue
            .clone()
            .unwrap_or_else(|| "none".to_string()),
        surface.backlog.status.checkpoint_key,
    ));
    transport_lines.push("coordination roundtrips".to_string());
    transport_lines.extend(
        surface
            .coordination
            .roundtrips
            .iter()
            .map(|entry| format!("  {entry}")),
    );
    transport_lines.push("stream actionable edits".to_string());
    transport_lines.extend(
        render_dev_stream_actionable_lines(&surface.stream)
            .into_iter()
            .map(|line| format!("  {line}")),
    );
    transport_lines.push("stream report".to_string());
    transport_lines.extend(
        render_dev_stream_preview(&surface.stream)
            .lines()
            .map(|line| format!("  {line}")),
    );
    transport_lines.push("backlog report".to_string());
    transport_lines.extend(
        render_dev_backlog_preview(&surface.backlog)
            .lines()
            .map(|line| format!("  {line}")),
    );
    transport_lines.push("coordination report".to_string());
    transport_lines.extend(
        render_dev_coordination_preview(&surface.coordination)
            .lines()
            .map(|line| format!("  {line}")),
    );

    transport_lines
}

#[cfg(test)]
mod tests {
    use futures::executor::block_on;

    use super::{
        DevContextOptions, DevOperatorSurfaceOptions, build_default_dev_operator_surface,
        build_dev_context, build_dev_operator_surface, build_dev_queue,
        parse_default_dev_wiki_config, render_dev_transport_lines,
    };
    use crate::DEV_PREVIEW_SAMPLE_EVENTS;

    #[test]
    fn builds_queue_from_default_fixture() {
        let config = parse_default_dev_wiki_config().expect("config should parse");
        let queue =
            build_dev_queue(&config, DEV_PREVIEW_SAMPLE_EVENTS).expect("queue should build");

        assert_eq!(queue.len(), 4);
        assert_eq!(queue[0].event.wiki_id, "frwiki");
    }

    #[test]
    fn builds_context_from_options() {
        let context = build_dev_context(&DevContextOptions {
            talk_page_wikitext: Some("{{Avertissement niveau 2}}".to_string()),
            liftwing_probability: Some(0.42),
        });

        assert!(context.user_risk.is_some());
        assert_eq!(context.liftwing_risk, Some(0.42));
    }

    #[test]
    fn builds_full_operator_surface_from_default_fixture() {
        let surface = block_on(build_default_dev_operator_surface(
            &DevOperatorSurfaceOptions::default(),
        ))
        .expect("surface should build");

        assert_eq!(surface.config.wiki_id, "frwiki");
        assert_eq!(surface.queue.len(), 4);
        assert_eq!(surface.selected().event.rev_id, 123_459);
        assert!(surface.context.is_some());
        assert!(surface.workbench.is_some());
        assert_eq!(surface.action_requests.len(), 3);
        assert_eq!(surface.coordination.roundtrips.len(), 6);
        assert!(surface.report.queue_depth >= 4);
        assert_eq!(surface.operator_summary.wiki_id, "frwiki");
        assert_eq!(surface.session_digest.wiki_id, "frwiki");
        assert_eq!(surface.shell_state.wiki_id, "frwiki");
    }

    #[test]
    fn operator_surface_uses_supplied_wiki_id() {
        let mut config = parse_default_dev_wiki_config().expect("config should parse");
        config.wiki_id = "testwiki".to_string();
        let payload =
            DEV_PREVIEW_SAMPLE_EVENTS.replace("\"wiki\":\"frwiki\"", "\"wiki\":\"testwiki\"");
        let surface = block_on(build_dev_operator_surface(
            &config,
            &payload,
            &DevOperatorSurfaceOptions::default(),
        ))
        .expect("surface should build");

        assert_eq!(surface.selected().event.wiki_id, "testwiki");
        assert_eq!(surface.coordination.summary.wiki_id, "testwiki");
        assert_eq!(surface.report.wiki_id, "testwiki");
    }

    #[test]
    fn renders_transport_lines_from_surface() {
        let surface = block_on(build_default_dev_operator_surface(
            &DevOperatorSurfaceOptions::default(),
        ))
        .expect("surface should build");
        let lines = render_dev_transport_lines(&surface);

        assert!(lines.iter().any(|line| line.contains("queue wiki=frwiki")));
        assert!(lines.iter().any(|line| line.contains("backlog report")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("coordination report"))
        );
    }
}
