use std::collections::BTreeMap;
use std::process::ExitCode;

use futures::executor::block_on;
use sp42_core::traits::{MemoryStorage, ReplayEventSource, StubHttpClient};
use sp42_core::{
    Action, ActionBroadcast, BacklogRuntime, BacklogRuntimeConfig, BacklogRuntimeStatus,
    ContextInputs, CoordinationMessage, CoordinationState, CoordinationStateSummary, EditClaim,
    FlaggedEdit, HttpResponse, PatrolOperatorSummary, PatrolScenarioReport,
    PatrolScenarioReportInputs, PatrolSessionDigest, PresenceHeartbeat, QueuedEdit, RaceResolution,
    RecentChangesQuery, ScoreDelta, ServerSentEvent, ShellStateInputs, ShellStateModel,
    StreamIngestor, StreamRuntime, StreamRuntimeStatus, build_liftwing_score_request,
    build_patrol_operator_summary, build_patrol_scenario_report, build_patrol_session_digest,
    build_ranked_queue, build_recent_changes_request, build_review_workbench,
    build_scoring_context, build_shell_state_model, decode_message, diff_lines, encode_message,
    parse_wiki_config, render_patrol_operator_summary_markdown,
    render_patrol_operator_summary_text, render_patrol_scenario_markdown,
    render_patrol_scenario_text, render_patrol_session_digest_markdown,
    render_patrol_session_digest_text, render_shell_state_markdown, render_shell_state_text,
    score_edit_with_context,
};

const DEFAULT_CONFIG: &str = include_str!("../../../configs/frwiki.yaml");
const SAMPLE_EVENTS: &str = include_str!("../../../fixtures/frwiki_recentchanges_batch.jsonl");
const SAMPLE_BACKLOG_RESPONSE: &str = r#"{
  "continue": { "rccontinue": "20260324010202|456" },
  "query": {
    "recentchanges": [
      {
        "type": "edit",
        "title": "Exemple",
        "ns": 0,
        "revid": 123460,
        "old_revid": 123459,
        "user": "192.0.2.11",
        "timestamp": "2026-03-24T01:02:03Z",
        "bot": false,
        "minor": false,
        "new": false,
        "oldlen": 120,
        "newlen": 90,
        "comment": "backlog sample",
        "tags": ["mw-reverted"]
      }
    ]
}
}"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Text,
    Markdown,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DesktopOptions {
    format: OutputFormat,
}

#[derive(Debug, Clone)]
struct DesktopConsoleSnapshot {
    operator_summary: PatrolOperatorSummary,
    session_digest: PatrolSessionDigest,
    report: PatrolScenarioReport,
    shell_state: ShellStateModel,
    transport_lines: Vec<String>,
}

struct DesktopTransportInputs<'a> {
    queue: &'a [QueuedEdit],
    top: &'a QueuedEdit,
    workbench: &'a sp42_core::ReviewWorkbench,
    context: &'a sp42_core::ScoringContext,
    contextual_score: &'a sp42_core::CompositeScore,
    recentchanges_request: &'a sp42_core::HttpRequest,
    liftwing_request: &'a sp42_core::HttpRequest,
    backlog_status: &'a BacklogRuntimeStatus,
    backlog_request: &'a sp42_core::HttpRequest,
    backlog_batch: &'a sp42_core::RecentChangesBatch,
    roundtrips: &'a [String],
    stream_actionable_lines: &'a [String],
    stream_preview: &'a str,
    backlog_preview: &'a str,
    coordination_preview: &'a str,
}

fn main() -> ExitCode {
    match run() {
        Ok(summary) => {
            println!("{summary}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<String, String> {
    let options = parse_options(std::env::args().skip(1))?;
    run_with_format(options.format)
}

fn parse_options(args: impl IntoIterator<Item = String>) -> Result<DesktopOptions, String> {
    let mut args = args.into_iter();
    let mut format = OutputFormat::Text;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--format" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--format requires a value".to_string())?;
                format = match value.as_str() {
                    "text" => OutputFormat::Text,
                    "markdown" => OutputFormat::Markdown,
                    "json" => OutputFormat::Json,
                    _ => return Err(format!("unsupported output format: {value}")),
                };
            }
            _ => return Err(format!("unsupported argument: {arg}")),
        }
    }

    Ok(DesktopOptions { format })
}

fn run_with_format(format: OutputFormat) -> Result<String, String> {
    let snapshot = build_console_snapshot()?;

    match format {
        OutputFormat::Text => Ok(render_text_snapshot(&snapshot)),
        OutputFormat::Markdown => Ok(render_markdown_snapshot(&snapshot)),
        OutputFormat::Json => render_json_snapshot(&snapshot),
    }
}

fn render_text_snapshot(snapshot: &DesktopConsoleSnapshot) -> String {
    [
        format!(
            "{} native operator console",
            sp42_core::branding::PROJECT_NAME
        ),
        render_desktop_summary(&snapshot.operator_summary, &snapshot.session_digest),
        render_patrol_operator_summary_text(&snapshot.operator_summary),
        render_patrol_session_digest_text(&snapshot.session_digest),
        render_shell_state_text(&snapshot.shell_state),
        render_patrol_scenario_text(&snapshot.report),
        render_markdown_section("Transport", &snapshot.transport_lines.join("\n")),
    ]
    .join("\n\n")
}

fn render_markdown_snapshot(snapshot: &DesktopConsoleSnapshot) -> String {
    [
        render_markdown_section(
            "Desktop summary",
            &render_desktop_summary(&snapshot.operator_summary, &snapshot.session_digest),
        ),
        render_patrol_operator_summary_markdown(&snapshot.operator_summary),
        render_patrol_session_digest_markdown(&snapshot.session_digest),
        render_markdown_section(
            "Shell state",
            &render_shell_state_markdown(&snapshot.shell_state),
        ),
        render_patrol_scenario_markdown(&snapshot.report),
        render_markdown_section("Transport", &snapshot.transport_lines.join("\n")),
    ]
    .join("\n\n")
}

fn render_json_snapshot(snapshot: &DesktopConsoleSnapshot) -> Result<String, String> {
    serde_json::to_string_pretty(&serde_json::json!({
        "project": sp42_core::branding::PROJECT_NAME,
        "desktop_summary": render_desktop_summary_value(&snapshot.operator_summary, &snapshot.session_digest),
        "operator_summary": &snapshot.operator_summary,
        "session_digest": &snapshot.session_digest,
        "shell_state": &snapshot.shell_state,
        "report": &snapshot.report,
        "transport_lines": &snapshot.transport_lines,
    }))
    .map_err(|error| error.to_string())
}

fn render_desktop_summary(
    operator_summary: &PatrolOperatorSummary,
    session_digest: &PatrolSessionDigest,
) -> String {
    let selected = operator_summary.selected.as_ref().map_or_else(
        || "selected=none".to_string(),
        |selected| {
            format!(
                "selected_rev={} title=\"{}\" score={} signals={}",
                selected.rev_id, selected.title, selected.score, selected.signals
            )
        },
    );
    let workbench = operator_summary.workbench.as_ref().map_or_else(
        || "action_workbench=none".to_string(),
        |workbench| {
            format!(
                "action_workbench_rev={} requests={} training_rows={}",
                workbench.rev_id,
                workbench.request_labels.len(),
                workbench.training_rows
            )
        },
    );
    let severity_counts = operator_summary
        .severity_counts
        .iter()
        .map(|count| format!("{:?}={}", count.severity, count.count))
        .collect::<Vec<_>>()
        .join(" ");
    let available_sections = operator_summary
        .section_overview
        .iter()
        .filter(|section| section.available)
        .count();

    format!(
        "desktop operator surface wiki={} readiness={:?} queue_depth={} sections={}/{} session_sections={} {selected} {workbench} severity_counts=[{}]",
        operator_summary.wiki_id,
        operator_summary.readiness,
        operator_summary.queue_depth,
        available_sections,
        operator_summary.section_overview.len(),
        session_digest.sections.len(),
        severity_counts
    )
}

fn render_desktop_summary_value(
    operator_summary: &PatrolOperatorSummary,
    session_digest: &PatrolSessionDigest,
) -> serde_json::Value {
    serde_json::json!({
        "wiki_id": &operator_summary.wiki_id,
        "readiness": format!("{:?}", operator_summary.readiness),
        "queue_depth": operator_summary.queue_depth,
        "selected": operator_summary.selected.as_ref().map(|selected| serde_json::json!({
            "wiki_id": &operator_summary.wiki_id,
            "rev_id": selected.rev_id,
            "title": &selected.title,
            "score": selected.score,
            "signals": selected.signals,
        })),
        "severity_counts": operator_summary
            .severity_counts
            .iter()
            .map(|count| serde_json::json!({
                "severity": format!("{:?}", count.severity),
                "count": count.count,
            }))
            .collect::<Vec<_>>(),
        "available_sections": operator_summary
            .section_overview
            .iter()
            .filter(|section| section.available)
            .count(),
        "section_count": operator_summary.section_overview.len(),
        "session": serde_json::json!({
            "wiki_id": &session_digest.wiki_id,
            "readiness": format!("{:?}", session_digest.readiness),
            "queue_depth": session_digest.queue_depth,
            "findings": session_digest.findings.len(),
            "sections": session_digest.sections.len(),
        }),
    })
}

fn build_console_snapshot() -> Result<DesktopConsoleSnapshot, String> {
    let config = parse_wiki_config(DEFAULT_CONFIG).map_err(|error| error.to_string())?;
    let queue = build_desktop_queue(&config)?;
    let top = queue
        .first()
        .ok_or_else(|| "No local edit available for the desktop shell.".to_string())?;
    let workbench = build_desktop_workbench(&config, top)?;
    let context = build_desktop_context();
    let contextual_score = score_edit_with_context(&top.event, &config.scoring, &context)
        .map_err(|error| error.to_string())?;
    let recentchanges_request = build_desktop_recentchanges_request(&config)?;
    let liftwing_request = build_desktop_liftwing_request(&config, top)?;
    let (stream_status, stream_edits) = build_stream_snapshot(&config)?;
    let (backlog_status, backlog_request, backlog_batch) = build_backlog_snapshot(&config)?;
    let (coordination_summary, roundtrips) = build_coordination_snapshot()?;
    let diff = diff_lines("Avant\n", "Avant\nApres\n");
    let stream_preview = render_stream_preview(&config)?;
    let backlog_preview = render_backlog_preview(&config)?;
    let coordination_preview = render_coordination_preview()?;
    let transport_lines = build_desktop_transport_lines(&DesktopTransportInputs {
        queue: &queue,
        top,
        workbench: &workbench,
        context: &context,
        contextual_score: &contextual_score,
        recentchanges_request: &recentchanges_request,
        liftwing_request: &liftwing_request,
        backlog_status: &backlog_status,
        backlog_request: &backlog_request,
        backlog_batch: &backlog_batch,
        roundtrips: &roundtrips,
        stream_actionable_lines: &stream_edits,
        stream_preview: &stream_preview,
        backlog_preview: &backlog_preview,
        coordination_preview: &coordination_preview,
    });

    let report = build_patrol_scenario_report(&PatrolScenarioReportInputs {
        queue: &queue,
        selected: Some(top),
        scoring_context: Some(&context),
        diff: Some(&diff),
        review_workbench: Some(&workbench),
        stream_status: Some(&stream_status),
        backlog_status: Some(&backlog_status),
        coordination: Some(&coordination_summary),
        wiki_id_hint: Some(&config.wiki_id),
    });
    let operator_summary = build_patrol_operator_summary(&sp42_core::PatrolOperatorSummaryInputs {
        report: &report,
        review_workbench: Some(&workbench),
    });
    let session_digest = build_patrol_session_digest(&sp42_core::PatrolSessionDigestInputs {
        report: &report,
        review_workbench: Some(&workbench),
    });
    let shell_state = build_shell_state_model(&ShellStateInputs {
        report: &report,
        review_workbench: Some(&workbench),
    });

    Ok(DesktopConsoleSnapshot {
        operator_summary,
        session_digest,
        report,
        shell_state,
        transport_lines,
    })
}

fn build_desktop_queue(config: &sp42_core::WikiConfig) -> Result<Vec<QueuedEdit>, String> {
    let ingestor = StreamIngestor::from_config(config);
    let events = ingestor
        .ingest_lines(SAMPLE_EVENTS)
        .map_err(|error| error.to_string())?;
    build_ranked_queue(events, &config.scoring).map_err(|error| error.to_string())
}

fn build_desktop_workbench(
    config: &sp42_core::WikiConfig,
    top: &QueuedEdit,
) -> Result<sp42_core::ReviewWorkbench, String> {
    build_review_workbench(
        config,
        top,
        "desktop-local-token",
        "SP42-desktop",
        Some("desktop shell"),
    )
    .map_err(|error| error.to_string())
}

fn build_desktop_context() -> sp42_core::ScoringContext {
    build_scoring_context(&ContextInputs {
        talk_page_wikitext: Some("{{Avertissement niveau 2 pour vandalisme}}".to_string()),
        liftwing_probability: Some(0.72),
    })
}

fn build_desktop_recentchanges_request(
    config: &sp42_core::WikiConfig,
) -> Result<sp42_core::HttpRequest, String> {
    build_recent_changes_request(
        config,
        &RecentChangesQuery {
            limit: 25,
            rccontinue: None,
            include_bots: false,
            include_minor: true,
            namespace_override: None,
            unpatrolled_only: false,
        },
    )
    .map_err(|error| error.to_string())
}

fn build_desktop_liftwing_request(
    config: &sp42_core::WikiConfig,
    top: &QueuedEdit,
) -> Result<sp42_core::HttpRequest, String> {
    build_liftwing_score_request(
        config,
        &sp42_core::LiftWingRequest {
            rev_id: top.event.rev_id,
        },
    )
    .map_err(|error| error.to_string())
}

fn build_desktop_transport_lines(inputs: &DesktopTransportInputs<'_>) -> Vec<String> {
    let mut transport_lines = vec![
        format!(
            "queue wiki={} depth={} top_rev={} title=\"{}\" score={}",
            inputs.top.event.wiki_id,
            inputs.queue.len(),
            inputs.top.event.rev_id,
            inputs.top.event.title,
            inputs.top.score.total
        ),
        render_queue_items(inputs.queue),
        format!(
            "action_workbench requests={} training_rows={}",
            inputs.workbench.requests.len(),
            inputs.workbench.training_csv.lines().skip(1).count()
        ),
        format!(
            "context score={} user_risk={} liftwing={}",
            inputs.contextual_score.total,
            inputs.context.user_risk.is_some(),
            inputs
                .context
                .liftwing_risk
                .map_or_else(|| "none".to_string(), |value| format!("{value:.2}"))
        ),
        format!(
            "recentchanges {:?} {}",
            inputs.recentchanges_request.method, inputs.recentchanges_request.url
        ),
        format!(
            "liftwing {:?} {}",
            inputs.liftwing_request.method, inputs.liftwing_request.url
        ),
        format!(
            "backlog report {:?} {} events={} polls={} next_continue={} checkpoint={}",
            inputs.backlog_request.method,
            inputs.backlog_request.url,
            inputs.backlog_batch.events.len(),
            inputs.backlog_status.poll_count,
            inputs
                .backlog_status
                .next_continue
                .clone()
                .unwrap_or_else(|| "none".to_string()),
            inputs.backlog_status.checkpoint_key,
        ),
    ];

    transport_lines.push("coordination roundtrips".to_string());
    transport_lines.extend(inputs.roundtrips.iter().map(|entry| format!("  {entry}")));
    transport_lines.push("stream actionable edits".to_string());
    transport_lines.extend(
        inputs
            .stream_actionable_lines
            .iter()
            .map(|line| format!("  {line}")),
    );
    transport_lines.push("stream report".to_string());
    transport_lines.extend(
        inputs
            .stream_preview
            .lines()
            .map(|line| format!("  {line}")),
    );
    transport_lines.push("backlog report".to_string());
    transport_lines.extend(
        inputs
            .backlog_preview
            .lines()
            .map(|line| format!("  {line}")),
    );
    transport_lines.push("coordination report".to_string());
    transport_lines.extend(
        inputs
            .coordination_preview
            .lines()
            .map(|line| format!("  {line}")),
    );

    transport_lines
}

fn build_stream_snapshot(
    config: &sp42_core::WikiConfig,
) -> Result<(StreamRuntimeStatus, Vec<String>), String> {
    let source = ReplayEventSource::new(SAMPLE_EVENTS.lines().enumerate().filter_map(
        |(index, line)| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }

            Some(ServerSentEvent {
                event_type: Some("message".to_string()),
                id: Some(format!("desktop-fixture-{}", index + 1)),
                data: trimmed.to_string(),
                retry_ms: None,
            })
        },
    ));
    let storage = MemoryStorage::default();
    let mut runtime = StreamRuntime::from_config(config, source, storage);
    let edits = block_on(async {
        runtime
            .initialize()
            .await
            .map_err(|error| error.to_string())?;
        let mut edits = Vec::new();
        while let Some(edit) = runtime
            .next_actionable_event()
            .await
            .map_err(|error| error.to_string())?
        {
            edits.push(edit);
        }
        runtime
            .reconnect_from_checkpoint()
            .await
            .map_err(|error| error.to_string())?;
        Ok::<_, String>((runtime.status(), edits))
    })?;

    let (status, edits) = edits;
    Ok((
        status,
        edits
            .into_iter()
            .map(|edit| format!("stream rev={} title=\"{}\"", edit.rev_id, edit.title))
            .collect(),
    ))
}

fn build_backlog_snapshot(
    config: &sp42_core::WikiConfig,
) -> Result<
    (
        BacklogRuntimeStatus,
        sp42_core::HttpRequest,
        sp42_core::RecentChangesBatch,
    ),
    String,
> {
    let storage = MemoryStorage::default();
    let client = StubHttpClient::new([Ok(HttpResponse {
        status: 200,
        headers: BTreeMap::new(),
        body: SAMPLE_BACKLOG_RESPONSE.as_bytes().to_vec(),
    })]);
    let mut runtime = BacklogRuntime::from_config(
        config,
        storage,
        BacklogRuntimeConfig {
            limit: 5,
            include_bots: false,
        },
    );

    block_on(async {
        runtime
            .initialize()
            .await
            .map_err(|error| error.to_string())?;
        let request = runtime
            .build_next_request()
            .map_err(|error| error.to_string())?;
        let batch = runtime
            .poll(&client)
            .await
            .map_err(|error| error.to_string())?;
        Ok::<_, String>((runtime.status(), request, batch))
    })
}

fn build_coordination_snapshot() -> Result<(CoordinationStateSummary, Vec<String>), String> {
    let mut state = CoordinationState::new("frwiki");
    let mut roundtrips = Vec::new();
    for message in coordination_preview_messages() {
        let (byte_len, decoded) = encode_message(&message)
            .and_then(|bytes| {
                let byte_len = bytes.len();
                decode_message(&bytes).map(|decoded| (byte_len, decoded))
            })
            .map_err(|error| error.to_string())?;
        let label = coordination_message_label(&decoded);
        let _ = state.apply(decoded);
        roundtrips.push(format!("roundtrip {label} bytes={byte_len}"));
    }

    Ok((state.summary(), roundtrips))
}

fn render_queue_items(queue: &[QueuedEdit]) -> String {
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
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_markdown_section(title: &str, body: &str) -> String {
    if body.trim().is_empty() {
        format!("## {title}\n\n_Empty_")
    } else {
        format!("## {title}\n\n{body}")
    }
}

fn render_stream_preview(config: &sp42_core::WikiConfig) -> Result<String, String> {
    let source = ReplayEventSource::new(SAMPLE_EVENTS.lines().enumerate().filter_map(
        |(index, line)| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }

            Some(ServerSentEvent {
                event_type: Some("message".to_string()),
                id: Some(format!("desktop-fixture-{}", index + 1)),
                data: trimmed.to_string(),
                retry_ms: None,
            })
        },
    ));
    let storage = MemoryStorage::default();
    let mut runtime = StreamRuntime::from_config(config, source, storage);
    let status = block_on(async {
        runtime
            .initialize()
            .await
            .map_err(|error| error.to_string())?;
        while runtime
            .next_actionable_event()
            .await
            .map_err(|error| error.to_string())?
            .is_some()
        {}
        runtime
            .reconnect_from_checkpoint()
            .await
            .map_err(|error| error.to_string())?;
        Ok::<_, String>(runtime.status())
    })?;

    Ok(format!(
        "stream delivered={} filtered={} reconnects={} checkpoint={}",
        status.delivered_events,
        status.filtered_events,
        status.reconnect_attempts,
        status.last_event_id.unwrap_or_else(|| "none".to_string())
    ))
}

fn render_backlog_preview(config: &sp42_core::WikiConfig) -> Result<String, String> {
    let storage = MemoryStorage::default();
    let client = StubHttpClient::new([Ok(HttpResponse {
        status: 200,
        headers: BTreeMap::new(),
        body: SAMPLE_BACKLOG_RESPONSE.as_bytes().to_vec(),
    })]);
    let mut runtime = BacklogRuntime::from_config(
        config,
        storage,
        BacklogRuntimeConfig {
            limit: 5,
            include_bots: false,
        },
    );

    let (request, batch, status) = block_on(async {
        runtime
            .initialize()
            .await
            .map_err(|error| error.to_string())?;
        let request = runtime
            .build_next_request()
            .map_err(|error| error.to_string())?;
        let batch = runtime
            .poll(&client)
            .await
            .map_err(|error| error.to_string())?;
        Ok::<_, String>((request, batch, runtime.status()))
    })?;

    Ok(format!(
        "backlog report {:?} {}\nbacklog batch={} total={} polls={} checkpoint={} next_continue={} first_rev={}",
        request.method,
        request.url,
        batch.events.len(),
        status.total_events,
        status.poll_count,
        status.checkpoint_key,
        status.next_continue.unwrap_or_else(|| "none".to_string()),
        batch.events.first().map_or(0, |event| event.rev_id)
    ))
}

fn render_coordination_preview() -> Result<String, String> {
    let mut state = CoordinationState::new("frwiki");
    let mut roundtrips = Vec::new();
    for message in coordination_preview_messages() {
        let (byte_len, decoded) = encode_message(&message)
            .and_then(|bytes| {
                let byte_len = bytes.len();
                decode_message(&bytes).map(|decoded| (byte_len, decoded))
            })
            .map_err(|error| error.to_string())?;
        let label = coordination_message_label(&decoded);
        let _ = state.apply(decoded);
        roundtrips.push(format!("roundtrip {label} bytes={byte_len}"));
    }

    let summary = state.summary();
    Ok([
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
        roundtrips.join("\n"),
        summary.claims.first().map_or_else(
            || "coordination claims unavailable".to_string(),
            |claim| format!("coordination claim rev={} actor={}", claim.rev_id, claim.actor),
        ),
    ]
    .join("\n"))
}

fn coordination_preview_messages() -> Vec<CoordinationMessage> {
    vec![
        CoordinationMessage::EditClaim(EditClaim {
            wiki_id: "frwiki".to_string(),
            rev_id: 123_456,
            actor: "LocalUser".to_string(),
        }),
        CoordinationMessage::PresenceHeartbeat(PresenceHeartbeat {
            wiki_id: "frwiki".to_string(),
            actor: "LocalUser".to_string(),
            active_edit_count: 1,
        }),
        CoordinationMessage::ScoreDelta(ScoreDelta {
            wiki_id: "frwiki".to_string(),
            rev_id: 123_456,
            delta: 8,
            reason: "LiftWing + warning history".to_string(),
        }),
        CoordinationMessage::FlaggedEdit(FlaggedEdit {
            wiki_id: "frwiki".to_string(),
            rev_id: 123_456,
            score: 95,
            reason: "possible vandalism".to_string(),
        }),
        CoordinationMessage::ActionBroadcast(ActionBroadcast {
            wiki_id: "frwiki".to_string(),
            rev_id: 123_456,
            action: Action::Rollback,
            actor: "LocalUser".to_string(),
        }),
        CoordinationMessage::RaceResolution(RaceResolution {
            wiki_id: "frwiki".to_string(),
            rev_id: 123_456,
            winning_actor: "LocalUser".to_string(),
        }),
    ]
}

fn coordination_message_label(message: &CoordinationMessage) -> &'static str {
    match message {
        CoordinationMessage::ActionBroadcast(_) => "ActionBroadcast",
        CoordinationMessage::EditClaim(_) => "EditClaim",
        CoordinationMessage::ScoreDelta(_) => "ScoreDelta",
        CoordinationMessage::PresenceHeartbeat(_) => "PresenceHeartbeat",
        CoordinationMessage::FlaggedEdit(_) => "FlaggedEdit",
        CoordinationMessage::RaceResolution(_) => "RaceResolution",
    }
}

#[cfg(test)]
mod tests {
    use super::{OutputFormat, parse_options, run_with_format};

    #[test]
    fn desktop_preview_renders_summary() {
        let summary = run_with_format(OutputFormat::Text).expect("desktop shell should render");

        assert!(summary.contains("SP42 native operator console"));
        assert!(summary.contains("desktop operator surface wiki=frwiki"));
        assert!(summary.contains("Patrol operator summary"));
        assert!(summary.contains("Patrol session digest"));
        assert!(summary.contains("Patrol report"));
        assert!(summary.contains("[Queue] available=true"));
        assert!(summary.contains("[Coordination] available=true"));
        assert!(summary.contains("Transport"));
    }

    #[test]
    fn desktop_parses_markdown_format_flag() {
        let options = parse_options(["--format".to_string(), "markdown".to_string()])
            .expect("format should parse");

        assert_eq!(options.format, OutputFormat::Markdown);
    }

    #[test]
    fn desktop_renders_markdown_report() {
        let summary =
            run_with_format(OutputFormat::Markdown).expect("markdown desktop shell should render");

        assert!(summary.contains("## Desktop summary"));
        assert!(summary.contains("# Patrol operator summary"));
        assert!(summary.contains("# Patrol session digest"));
        assert!(summary.contains("# Patrol report"));
        assert!(summary.contains("## Queue"));
        assert!(summary.contains("## Workbench"));
        assert!(summary.contains("## Backlog"));
        assert!(summary.contains("## Coordination"));
        assert!(summary.contains("## Stream"));
        assert!(summary.contains("## Transport"));
    }

    #[test]
    fn desktop_renders_json_report() {
        let summary =
            run_with_format(OutputFormat::Json).expect("json desktop shell should render");

        assert!(summary.contains("\"project\":"));
        assert!(summary.contains("\"desktop_summary\":"));
        assert!(summary.contains("\"operator_summary\":"));
        assert!(summary.contains("\"session_digest\":"));
        assert!(summary.contains("\"report\":"));
        assert!(summary.contains("\"transport_lines\":"));
    }
}
