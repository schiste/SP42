use std::process::ExitCode;

use futures::executor::block_on;
use sp42_core::{
    BacklogRuntimeStatus, ContextInputs, DEV_PREVIEW_SAMPLE_EVENTS, DEV_PREVIEW_WIKI_ID,
    DevBacklogPreview, DevCoordinationPreview, DevStreamPreview, PatrolOperatorSummary,
    PatrolScenarioReport, PatrolScenarioReportInputs, PatrolSessionDigest, QueuedEdit,
    RecentChangesQuery, ShellStateInputs, ShellStateModel, StreamIngestor,
    build_dev_backlog_preview, build_dev_coordination_preview, build_dev_stream_preview,
    build_liftwing_score_request, build_patrol_operator_summary, build_patrol_scenario_report,
    build_patrol_session_digest, build_ranked_queue, build_recent_changes_request,
    build_review_workbench, build_scoring_context, build_shell_state_model, diff_lines,
    parse_default_dev_wiki_config, render_patrol_operator_summary_markdown,
    render_patrol_operator_summary_text, render_patrol_scenario_markdown,
    render_patrol_scenario_text, render_patrol_session_digest_markdown,
    render_patrol_session_digest_text, render_shell_state_markdown, render_shell_state_text,
    score_edit_with_context,
};

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
    let config = parse_default_dev_wiki_config().map_err(|error| error.to_string())?;
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
    let stream_snapshot = block_on(build_dev_stream_preview(
        &config,
        DEV_PREVIEW_SAMPLE_EVENTS,
        "desktop-fixture",
    ))
    .map_err(|error| error.to_string())?;
    let backlog_snapshot =
        block_on(build_dev_backlog_preview(&config)).map_err(|error| error.to_string())?;
    let coordination_snapshot =
        build_dev_coordination_preview(DEV_PREVIEW_WIKI_ID).map_err(|error| error.to_string())?;
    let diff = diff_lines("Avant\n", "Avant\nApres\n");
    let stream_actionable_lines = render_stream_actionable_lines(&stream_snapshot);
    let stream_preview = render_stream_preview(&stream_snapshot);
    let backlog_preview = render_backlog_preview(&backlog_snapshot);
    let coordination_preview = render_coordination_preview(&coordination_snapshot);
    let transport_lines = build_desktop_transport_lines(&DesktopTransportInputs {
        queue: &queue,
        top,
        workbench: &workbench,
        context: &context,
        contextual_score: &contextual_score,
        recentchanges_request: &recentchanges_request,
        liftwing_request: &liftwing_request,
        backlog_status: &backlog_snapshot.status,
        backlog_request: &backlog_snapshot.request,
        backlog_batch: &backlog_snapshot.batch,
        roundtrips: &coordination_snapshot.roundtrips,
        stream_actionable_lines: &stream_actionable_lines,
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
        stream_status: Some(&stream_snapshot.status),
        backlog_status: Some(&backlog_snapshot.status),
        coordination: Some(&coordination_snapshot.summary),
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
        .ingest_lines(DEV_PREVIEW_SAMPLE_EVENTS)
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
            include_bots: false.into(),
            unpatrolled_only: false.into(),
            include_minor: true.into(),
            include_anonymous: true.into(),
            include_new_pages: true.into(),
            tag_filter: None,
            namespace_override: None,
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

fn render_stream_actionable_lines(snapshot: &DevStreamPreview) -> Vec<String> {
    snapshot
        .edits
        .iter()
        .map(|edit| format!("stream rev={} title=\"{}\"", edit.rev_id, edit.title))
        .collect()
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

fn render_stream_preview(snapshot: &DevStreamPreview) -> String {
    let status = &snapshot.status;
    format!(
        "stream delivered={} filtered={} reconnects={} checkpoint={}",
        status.delivered_events,
        status.filtered_events,
        status.reconnect_attempts,
        status.last_event_id.as_deref().unwrap_or("none")
    )
}

fn render_backlog_preview(snapshot: &DevBacklogPreview) -> String {
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

fn render_coordination_preview(snapshot: &DevCoordinationPreview) -> String {
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
