use std::convert::TryFrom;
use std::io::{self, Read};
use std::process::ExitCode;

use futures::executor::block_on;
use reqwest::header::COOKIE;
use serde_json::Value;
use sp42_core::traits::{MemoryStorage, ReplayEventSource};
use sp42_core::{
    Action, ActionBroadcast, BacklogRuntime, BacklogRuntimeConfig, ContextInputs,
    CoordinationMessage, CoordinationState, DevAuthBootstrapRequest, DevAuthSessionStatus,
    EditClaim, FlaggedEdit, HttpResponse, PatrolScenarioReportInputs, PresenceHeartbeat,
    QueuedEdit, RaceResolution, RecentChangesQuery, ScoreDelta, SessionActionExecutionRequest,
    SessionActionExecutionResponse, SessionActionKind, ShellStateInputs, StreamIngestor,
    build_dev_auth_bootstrap_request, build_liftwing_score_request, build_patrol_scenario_report,
    build_ranked_queue, build_recent_changes_request, build_review_workbench,
    build_scoring_context, build_session_action_execution_requests, build_shell_state_model,
    decode_message, encode_message, parse_dev_auth_status, parse_wiki_config,
    render_patrol_scenario_markdown, render_patrol_scenario_text, render_shell_state_markdown,
    render_shell_state_text, score_edit_with_context,
};
use sp42_core::{ServerSentEvent, StreamRuntime};
use std::collections::{BTreeMap, BTreeSet};

const DEFAULT_CONFIG: &str = include_str!("../../../configs/frwiki.yaml");
const SAMPLE_EVENTS: &str = include_str!("../../../fixtures/frwiki_recentchanges_batch.jsonl");
const LOCAL_SERVER_BASE_URL: &str = "http://127.0.0.1:8788";
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
    Json,
    Markdown,
}

#[derive(Debug, Clone, PartialEq)]
struct CliOptions {
    format: OutputFormat,
    workbench: Option<WorkbenchOptions>,
    context_preview: Option<ContextPreviewOptions>,
    preview_modes: BTreeSet<PreviewMode>,
    shell_mode: Option<ShellMode>,
    action_note: Option<String>,
    action_kind: SessionActionKind,
    bridge_base_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkbenchOptions {
    token: String,
    actor: String,
    note: String,
}

#[derive(Debug, Clone, PartialEq)]
struct ContextPreviewOptions {
    talk_page: Option<String>,
    liftwing_probability: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum PreviewMode {
    Stream,
    Backlog,
    Coordination,
    SessionDigest,
    ScenarioReport,
    ServerReport,
    ParityReport,
    ActionPreview,
    ActionExecute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShellMode {
    Stream,
    Backlog,
    Coordination,
    SessionDigest,
    ScenarioReport,
    ServerReport,
    ParityReport,
    ActionPreview,
    ActionExecute,
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
    let input = read_stdin().map_err(|error| error.to_string())?;
    let payload = if input.trim().is_empty() {
        SAMPLE_EVENTS
    } else {
        input.as_str()
    };

    let config = parse_wiki_config(DEFAULT_CONFIG).map_err(|error| error.to_string())?;
    let ranked = load_ranked_queue(&config, payload)?;

    match selected_shell_mode(&options) {
        Some(ShellMode::ParityReport) => {
            return render_parity_report(&config, &ranked, payload, options.format);
        }
        Some(ShellMode::Stream) => {
            return render_stream_preview(&config, payload, options.format);
        }
        Some(ShellMode::Backlog) => {
            return render_backlog_preview(&config, options.format);
        }
        Some(ShellMode::Coordination) => {
            return render_coordination_preview(options.format);
        }
        Some(ShellMode::SessionDigest) => {
            return render_session_digest(&config, &ranked, &options, options.format);
        }
        Some(ShellMode::ScenarioReport) => {
            return render_scenario_report(&config, &ranked, &options, options.format);
        }
        Some(ShellMode::ServerReport) => {
            return render_server_report(&options.bridge_base_url, options.format);
        }
        Some(ShellMode::ActionPreview) => {
            return render_action_preview(&config, &ranked, &options, options.format);
        }
        Some(ShellMode::ActionExecute) => {
            return render_action_execute(&config, &ranked, &options, options.format);
        }
        None => {}
    }

    if ranked.is_empty() {
        return Ok("No actionable edit from input.".to_string());
    }

    if let Some(workbench) = &options.workbench {
        return render_workbench(&config, &ranked, workbench, options.format);
    }

    if let Some(context_preview) = &options.context_preview {
        return render_context_preview(&config, &ranked, context_preview, options.format);
    }

    render_queue(&ranked, options.format)
}

fn read_stdin() -> Result<String, io::Error> {
    let mut buffer = String::new();
    io::stdin().read_to_string(&mut buffer)?;
    Ok(buffer)
}

fn load_ranked_queue(
    config: &sp42_core::WikiConfig,
    payload: &str,
) -> Result<Vec<QueuedEdit>, String> {
    let ingestor = StreamIngestor::from_config(config);
    let events = ingestor
        .ingest_lines(payload)
        .map_err(|error| error.to_string())?;

    if events.is_empty() {
        return Ok(Vec::new());
    }

    build_ranked_queue(events, &config.scoring).map_err(|error| error.to_string())
}

fn parse_options(args: impl IntoIterator<Item = String>) -> Result<CliOptions, String> {
    let mut args = args.into_iter();
    let mut format = OutputFormat::Text;
    let mut workbench_token = None;
    let mut workbench_actor = "SP42-cli".to_string();
    let mut workbench_note = "cli local workbench".to_string();
    let mut context_talk_page = None;
    let mut context_liftwing = None;
    let mut shell_mode = None;
    let mut action_note = None;
    let mut action_kind = SessionActionKind::Patrol;
    let mut bridge_base_url = LOCAL_SERVER_BASE_URL.to_string();
    let mut preview_modes = BTreeSet::new();

    while let Some(arg) = args.next() {
        let mut state = CliParseState {
            format: &mut format,
            workbench_token: &mut workbench_token,
            workbench_actor: &mut workbench_actor,
            workbench_note: &mut workbench_note,
            context_talk_page: &mut context_talk_page,
            context_liftwing: &mut context_liftwing,
            shell_mode: &mut shell_mode,
            action_note: &mut action_note,
            action_kind: &mut action_kind,
            bridge_base_url: &mut bridge_base_url,
            preview_modes: &mut preview_modes,
        };
        apply_cli_argument(&arg, &mut args, &mut state)?;
    }

    Ok(CliOptions {
        format,
        workbench: workbench_token.map(|token| WorkbenchOptions {
            token,
            actor: workbench_actor,
            note: workbench_note,
        }),
        context_preview: build_context_preview(context_talk_page, context_liftwing),
        preview_modes,
        shell_mode,
        action_note,
        action_kind,
        bridge_base_url,
    })
}

struct CliParseState<'a> {
    format: &'a mut OutputFormat,
    workbench_token: &'a mut Option<String>,
    workbench_actor: &'a mut String,
    workbench_note: &'a mut String,
    context_talk_page: &'a mut Option<String>,
    context_liftwing: &'a mut Option<f32>,
    shell_mode: &'a mut Option<ShellMode>,
    action_note: &'a mut Option<String>,
    action_kind: &'a mut SessionActionKind,
    bridge_base_url: &'a mut String,
    preview_modes: &'a mut BTreeSet<PreviewMode>,
}

fn apply_cli_argument<I>(
    arg: &str,
    args: &mut I,
    state: &mut CliParseState<'_>,
) -> Result<(), String>
where
    I: Iterator<Item = String>,
{
    if let Some(mode) = preview_mode_flag(arg) {
        state.preview_modes.insert(mode);
        return Ok(());
    }

    match arg {
        "--format" => {
            *state.format = parse_output_format(&next_option_value(args, "--format")?)?;
        }
        "--workbench-token" => {
            *state.workbench_token = Some(next_option_value(args, "--workbench-token")?);
        }
        "--workbench-actor" => {
            *state.workbench_actor = next_option_value(args, "--workbench-actor")?;
        }
        "--workbench-note" => {
            *state.workbench_note = next_option_value(args, "--workbench-note")?;
        }
        "--context-talk" => {
            *state.context_talk_page = Some(next_option_value(args, "--context-talk")?);
        }
        "--context-liftwing" => {
            let value = next_option_value(args, "--context-liftwing")?;
            *state.context_liftwing = Some(parse_liftwing_probability(&value)?);
        }
        "--shell" => {
            *state.shell_mode = Some(parse_shell_mode(&next_option_value(args, "--shell")?)?);
        }
        "--action-note" => {
            *state.action_note = Some(next_option_value(args, "--action-note")?);
        }
        "--action-kind" => {
            *state.action_kind = parse_action_kind(&next_option_value(args, "--action-kind")?)?;
        }
        "--bridge-base-url" => {
            *state.bridge_base_url = next_option_value(args, "--bridge-base-url")?;
        }
        _ => return Err(format!("unsupported argument: {arg}")),
    }

    Ok(())
}

fn build_context_preview(
    talk_page: Option<String>,
    liftwing_probability: Option<f32>,
) -> Option<ContextPreviewOptions> {
    (talk_page.is_some() || liftwing_probability.is_some()).then_some(ContextPreviewOptions {
        talk_page,
        liftwing_probability,
    })
}

fn next_option_value<I>(args: &mut I, flag: &str) -> Result<String, String>
where
    I: Iterator<Item = String>,
{
    args.next()
        .ok_or_else(|| format!("{flag} requires a value"))
}

fn parse_output_format(value: &str) -> Result<OutputFormat, String> {
    match value {
        "text" => Ok(OutputFormat::Text),
        "json" => Ok(OutputFormat::Json),
        "markdown" => Ok(OutputFormat::Markdown),
        _ => Err(format!("unsupported output format: {value}")),
    }
}

fn parse_liftwing_probability(value: &str) -> Result<f32, String> {
    value
        .parse::<f32>()
        .map_err(|_| "--context-liftwing must be a valid float".to_string())
}

fn preview_mode_flag(flag: &str) -> Option<PreviewMode> {
    match flag {
        "--stream-preview" | "--stream" => Some(PreviewMode::Stream),
        "--backlog-preview" | "--backlog" => Some(PreviewMode::Backlog),
        "--coordination-preview" | "--coordination" => Some(PreviewMode::Coordination),
        "--scenario-report" | "--patrol-report" => Some(PreviewMode::ScenarioReport),
        "--session-digest" => Some(PreviewMode::SessionDigest),
        "--server-report" | "--operator-report" => Some(PreviewMode::ServerReport),
        "--parity-report" => Some(PreviewMode::ParityReport),
        "--action-preview" | "--action" => Some(PreviewMode::ActionPreview),
        "--action-execute" => Some(PreviewMode::ActionExecute),
        _ => None,
    }
}

fn selected_shell_mode(options: &CliOptions) -> Option<ShellMode> {
    options.shell_mode.or_else(|| {
        if options.preview_modes.contains(&PreviewMode::ParityReport) {
            Some(ShellMode::ParityReport)
        } else if options.preview_modes.contains(&PreviewMode::Stream) {
            Some(ShellMode::Stream)
        } else if options.preview_modes.contains(&PreviewMode::Backlog) {
            Some(ShellMode::Backlog)
        } else if options.preview_modes.contains(&PreviewMode::Coordination) {
            Some(ShellMode::Coordination)
        } else if options.preview_modes.contains(&PreviewMode::SessionDigest) {
            Some(ShellMode::SessionDigest)
        } else if options.preview_modes.contains(&PreviewMode::ScenarioReport) {
            Some(ShellMode::ScenarioReport)
        } else if options.preview_modes.contains(&PreviewMode::ServerReport) {
            Some(ShellMode::ServerReport)
        } else if options.preview_modes.contains(&PreviewMode::ActionExecute) {
            Some(ShellMode::ActionExecute)
        } else if options.preview_modes.contains(&PreviewMode::ActionPreview) {
            Some(ShellMode::ActionPreview)
        } else {
            None
        }
    })
}

fn parse_shell_mode(value: &str) -> Result<ShellMode, String> {
    match value {
        "parity-report" | "operator-report" => Ok(ShellMode::ParityReport),
        "stream-preview" | "stream" => Ok(ShellMode::Stream),
        "backlog-preview" | "backlog" => Ok(ShellMode::Backlog),
        "coordination-preview" | "coordination" => Ok(ShellMode::Coordination),
        "session-digest" => Ok(ShellMode::SessionDigest),
        "scenario-report" | "patrol-report" => Ok(ShellMode::ScenarioReport),
        "server-report" | "live-server-report" => Ok(ShellMode::ServerReport),
        "action-preview" | "action" => Ok(ShellMode::ActionPreview),
        "action-execute" => Ok(ShellMode::ActionExecute),
        _ => Err(format!("unsupported shell mode: {value}")),
    }
}

fn parse_action_kind(value: &str) -> Result<SessionActionKind, String> {
    match value {
        "rollback" => Ok(SessionActionKind::Rollback),
        "patrol" => Ok(SessionActionKind::Patrol),
        "undo" => Ok(SessionActionKind::Undo),
        _ => Err(format!("unsupported action kind: {value}")),
    }
}

fn render_queue(queue: &[QueuedEdit], format: OutputFormat) -> Result<String, String> {
    match format {
        OutputFormat::Text => Ok(queue
            .iter()
            .enumerate()
            .map(|(index, item)| {
                format!(
                    "#{rank} wiki={} rev_id={} title=\"{}\" score={} signals={}",
                    item.event.wiki_id,
                    item.event.rev_id,
                    item.event.title,
                    item.score.total,
                    item.score.contributions.len(),
                    rank = index + 1,
                )
            })
            .collect::<Vec<_>>()
            .join("\n")),
        OutputFormat::Markdown => Ok(render_markdown_section(
            "Ranked queue",
            &queue
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    format!(
                        "#{rank} wiki={} rev_id={} title=\"{}\" score={} signals={}",
                        item.event.wiki_id,
                        item.event.rev_id,
                        item.event.title,
                        item.score.total,
                        item.score.contributions.len(),
                        rank = index + 1,
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
        )),
        OutputFormat::Json => {
            serde_json::to_string_pretty(queue).map_err(|error| error.to_string())
        }
    }
}

fn render_workbench(
    config: &sp42_core::WikiConfig,
    queue: &[QueuedEdit],
    options: &WorkbenchOptions,
    format: OutputFormat,
) -> Result<String, String> {
    let item = queue
        .first()
        .ok_or_else(|| "No queue item available for action workbench mode.".to_string())?;
    let workbench = build_review_workbench(
        config,
        item,
        &options.token,
        &options.actor,
        Some(&options.note),
    )
    .map_err(|error| error.to_string())?;

    match format {
        OutputFormat::Text => Ok([
            format!(
                "action workbench rev={} title=\"{}\"",
                workbench.rev_id, workbench.title
            ),
            workbench
                .requests
                .iter()
                .map(|request| {
                    format!(
                        "{} {:?} {} {}",
                        request.label, request.method, request.url, request.body
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
            format!(
                "training_jsonl={}",
                workbench.training_jsonl.trim_end().replace('\n', " | ")
            ),
            format!(
                "training_csv={}",
                workbench
                    .training_csv
                    .lines()
                    .skip(1)
                    .collect::<Vec<_>>()
                    .join(" | ")
            ),
        ]
        .join("\n")),
        OutputFormat::Markdown => Ok([
            render_markdown_section(
                "Action workbench",
                &format!(
                    "rev={} title=\"{}\" actor={} note=\"{}\"",
                    workbench.rev_id, workbench.title, options.actor, options.note
                ),
            ),
            render_markdown_section(
                "Action requests",
                &workbench
                    .requests
                    .iter()
                    .map(|request| {
                        format!(
                            "- {} {:?} {} {}",
                            request.label, request.method, request.url, request.body
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            render_markdown_section(
                "Training JSONL",
                &render_markdown_code_block("jsonl", workbench.training_jsonl.trim_end()),
            ),
            render_markdown_section(
                "Training CSV",
                &render_markdown_code_block(
                    "csv",
                    &workbench
                        .training_csv
                        .lines()
                        .skip(1)
                        .collect::<Vec<_>>()
                        .join("\n"),
                ),
            ),
        ]
        .join("\n\n")),
        OutputFormat::Json => {
            serde_json::to_string_pretty(&workbench).map_err(|error| error.to_string())
        }
    }
}

fn render_context_preview(
    config: &sp42_core::WikiConfig,
    queue: &[QueuedEdit],
    options: &ContextPreviewOptions,
    format: OutputFormat,
) -> Result<String, String> {
    let item = queue
        .first()
        .ok_or_else(|| "No queue item available for context mode.".to_string())?;
    let recentchanges_request = build_recent_changes_request(
        config,
        &RecentChangesQuery {
            limit: 25,
            rccontinue: None,
            include_bots: false,
            unpatrolled_only: false,
            include_minor: true,
            namespace_override: None,
        },
    )
    .map_err(|error| error.to_string())?;
    let liftwing_request = build_liftwing_score_request(
        config,
        &sp42_core::LiftWingRequest {
            rev_id: item.event.rev_id,
        },
    )
    .map_err(|error| error.to_string())?;
    let context = build_scoring_context(&ContextInputs {
        talk_page_wikitext: options.talk_page.clone(),
        liftwing_probability: options.liftwing_probability,
    });
    let score = score_edit_with_context(&item.event, &config.scoring, &context)
        .map_err(|error| error.to_string())?;

    match format {
        OutputFormat::Text => Ok(render_context_preview_text(
            item,
            &recentchanges_request,
            &liftwing_request,
            &context,
            &score,
            options,
        )),
        OutputFormat::Markdown => Ok(render_context_preview_markdown(
            item,
            &recentchanges_request,
            &liftwing_request,
            &context,
            &score,
            options,
        )),
        OutputFormat::Json => render_context_preview_json(
            item,
            &recentchanges_request,
            &liftwing_request,
            &context,
            &score,
            options,
        ),
    }
}

fn render_context_preview_text(
    item: &QueuedEdit,
    recentchanges_request: &sp42_core::HttpRequest,
    liftwing_request: &sp42_core::HttpRequest,
    context: &sp42_core::ScoringContext,
    score: &sp42_core::CompositeScore,
    options: &ContextPreviewOptions,
) -> String {
    [
        format!(
            "context rev={} title=\"{}\"",
            item.event.rev_id, item.event.title
        ),
        format!(
            "recentchanges {:?} {}",
            recentchanges_request.method, recentchanges_request.url
        ),
        format!(
            "liftwing {:?} {} {}",
            liftwing_request.method,
            liftwing_request.url,
            String::from_utf8_lossy(&liftwing_request.body)
        ),
        format!(
            "context user_risk={} liftwing={}",
            context.user_risk.is_some(),
            options
                .liftwing_probability
                .map_or_else(|| "none".to_string(), |value| format!("{value:.2}"))
        ),
        format!(
            "contextual score={} signals={}",
            score.total,
            score.contributions.len()
        ),
    ]
    .join("\n")
}

fn render_context_preview_markdown(
    item: &QueuedEdit,
    recentchanges_request: &sp42_core::HttpRequest,
    liftwing_request: &sp42_core::HttpRequest,
    context: &sp42_core::ScoringContext,
    score: &sp42_core::CompositeScore,
    options: &ContextPreviewOptions,
) -> String {
    [
        render_markdown_section(
            "Context report",
            &format!("rev={} title=\"{}\"", item.event.rev_id, item.event.title),
        ),
        render_markdown_section(
            "Requests",
            &format!(
                "- recentchanges {:?} {}\n- liftwing {:?} {}\n{}",
                recentchanges_request.method,
                recentchanges_request.url,
                liftwing_request.method,
                liftwing_request.url,
                render_markdown_code_block(
                    "json",
                    &String::from_utf8_lossy(&liftwing_request.body)
                ),
            ),
        ),
        render_markdown_section(
            "Context",
            &format!(
                "- user_risk={}\n- liftwing={}\n- contextual_score={}\n- signals={}",
                context.user_risk.is_some(),
                options
                    .liftwing_probability
                    .map_or_else(|| "none".to_string(), |value| format!("{value:.2}")),
                score.total,
                score.contributions.len()
            ),
        ),
    ]
    .join("\n\n")
}

fn render_context_preview_json(
    item: &QueuedEdit,
    recentchanges_request: &sp42_core::HttpRequest,
    liftwing_request: &sp42_core::HttpRequest,
    context: &sp42_core::ScoringContext,
    score: &sp42_core::CompositeScore,
    options: &ContextPreviewOptions,
) -> Result<String, String> {
    serde_json::to_string_pretty(&serde_json::json!({
        "rev_id": item.event.rev_id,
        "title": item.event.title,
        "recentchanges_url": recentchanges_request.url,
        "liftwing_url": liftwing_request.url,
        "liftwing_body": String::from_utf8_lossy(&liftwing_request.body),
        "user_risk_present": context.user_risk.is_some(),
        "liftwing_probability": options.liftwing_probability,
        "score": score.total,
        "signals": score.contributions.len()
    }))
    .map_err(|error| error.to_string())
}

fn render_stream_preview(
    config: &sp42_core::WikiConfig,
    payload: &str,
    format: OutputFormat,
) -> Result<String, String> {
    let source = ReplayEventSource::new(payload.lines().enumerate().filter_map(|(index, line)| {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }

        Some(ServerSentEvent {
            event_type: Some("message".to_string()),
            id: Some(format!("fixture-{}", index + 1)),
            data: trimmed.to_string(),
            retry_ms: None,
        })
    }));

    let storage = MemoryStorage::default();
    let mut runtime = StreamRuntime::from_config(config, source, storage);

    let (edits, status) = block_on(async {
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

        Ok::<_, String>((edits, runtime.status()))
    })?;

    match format {
        OutputFormat::Text => Ok([
            format!("stream checkpoint_key={}", status.checkpoint_key),
            format!(
                "stream last_event_id={}",
                status.last_event_id.unwrap_or_else(|| "none".to_string())
            ),
            format!(
                "stream delivered={} filtered={} reconnects={}",
                status.delivered_events, status.filtered_events, status.reconnect_attempts
            ),
            edits
                .iter()
                .map(|edit| format!("stream rev={} title=\"{}\"", edit.rev_id, edit.title))
                .collect::<Vec<_>>()
                .join("\n"),
        ]
        .into_iter()
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")),
        OutputFormat::Markdown => Ok([
            render_markdown_section(
                "Stream report",
                &format!(
                    "checkpoint_key={} last_event_id={} delivered={} filtered={} reconnects={}",
                    status.checkpoint_key,
                    status.last_event_id.unwrap_or_else(|| "none".to_string()),
                    status.delivered_events,
                    status.filtered_events,
                    status.reconnect_attempts
                ),
            ),
            render_markdown_section(
                "Live edits",
                &edits
                    .iter()
                    .map(|edit| format!("- stream rev={} title=\"{}\"", edit.rev_id, edit.title))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
        ]
        .join("\n\n")),
        OutputFormat::Json => serde_json::to_string_pretty(&serde_json::json!({
            "checkpoint_key": status.checkpoint_key,
            "last_event_id": status.last_event_id,
            "delivered_events": status.delivered_events,
            "filtered_events": status.filtered_events,
            "reconnect_attempts": status.reconnect_attempts,
            "edits": edits,
        }))
        .map_err(|error| error.to_string()),
    }
}

fn render_backlog_preview(
    config: &sp42_core::WikiConfig,
    format: OutputFormat,
) -> Result<String, String> {
    let storage = MemoryStorage::default();
    let client = sp42_core::traits::StubHttpClient::new([Ok(HttpResponse {
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

    match format {
        OutputFormat::Text => Ok([
            format!("backlog report {:?} {}", request.method, request.url),
            format!(
                "backlog batch={} total={} polls={}",
                batch.events.len(),
                status.total_events,
                status.poll_count
            ),
            format!(
                "backlog checkpoint={} next_continue={}",
                status.checkpoint_key,
                status.next_continue.unwrap_or_else(|| "none".to_string())
            ),
            batch.events.first().map_or_else(
                || "backlog empty".to_string(),
                |event| format!("backlog rev={} title=\"{}\"", event.rev_id, event.title),
            ),
        ]
        .join("\n")),
        OutputFormat::Markdown => Ok([
            render_markdown_section(
                "Backlog report",
                &format!("{:?} {}", request.method, request.url),
            ),
            render_markdown_section(
                "Backlog batch",
                &format!(
                    "events={} total={} polls={} checkpoint={} next_continue={}",
                    batch.events.len(),
                    status.total_events,
                    status.poll_count,
                    status.checkpoint_key,
                    status.next_continue.unwrap_or_else(|| "none".to_string())
                ),
            ),
            render_markdown_section(
                "First event",
                &batch.events.first().map_or_else(
                    || "backlog empty".to_string(),
                    |event| format!("rev={} title=\"{}\"", event.rev_id, event.title),
                ),
            ),
        ]
        .join("\n\n")),
        OutputFormat::Json => serde_json::to_string_pretty(&serde_json::json!({
            "request": {
                "method": format!("{:?}", request.method),
                "url": request.url,
            },
            "batch_size": batch.events.len(),
            "total_events": status.total_events,
            "poll_count": status.poll_count,
            "checkpoint_key": status.checkpoint_key,
            "next_continue": status.next_continue,
            "events": batch.events,
        }))
        .map_err(|error| error.to_string()),
    }
}

fn render_coordination_preview(format: OutputFormat) -> Result<String, String> {
    let messages = coordination_preview_messages();
    let mut state = CoordinationState::new("frwiki");
    let mut roundtrips = Vec::new();

    for message in messages {
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
    match format {
        OutputFormat::Text => Ok([
            format!(
                "coordination report wiki={} claims={} presence={} flags={} deltas={} resolutions={} actions={}",
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
        .join("\n")),
        OutputFormat::Markdown => Ok([
            render_markdown_section(
                "Coordination report",
                &format!(
                    "wiki={} claims={} presence={} flags={} deltas={} resolutions={} actions={}",
                    summary.wiki_id,
                    summary.claims.len(),
                    summary.presence.len(),
                    summary.flagged_edits.len(),
                    summary.score_deltas.len(),
                    summary.race_resolutions.len(),
                    summary.recent_actions.len()
                ),
            ),
            render_markdown_section(
                "Roundtrips",
                &roundtrips
                    .iter()
                    .map(|entry| format!("- {entry}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            render_markdown_section(
                "First claim",
                &summary.claims.first().map_or_else(
                    || "coordination claims unavailable".to_string(),
                    |claim| format!("rev={} actor={}", claim.rev_id, claim.actor),
                ),
            ),
        ]
        .join("\n\n")),
        OutputFormat::Json => serde_json::to_string_pretty(&serde_json::json!({
            "wiki_id": summary.wiki_id,
            "counts": {
                "claims": summary.claims.len(),
                "presence": summary.presence.len(),
                "flagged_edits": summary.flagged_edits.len(),
                "score_deltas": summary.score_deltas.len(),
                "race_resolutions": summary.race_resolutions.len(),
                "recent_actions": summary.recent_actions.len(),
            },
            "roundtrips": roundtrips,
            "summary": summary,
        }))
        .map_err(|error| error.to_string()),
    }
}

fn render_parity_report(
    config: &sp42_core::WikiConfig,
    queue: &[QueuedEdit],
    payload: &str,
    format: OutputFormat,
) -> Result<String, String> {
    let queue_summary = render_queue(queue, OutputFormat::Text)?;
    let top = queue.first();
    let top_wiki = top.map_or("frwiki", |item| item.event.wiki_id.as_str());
    let top_rev = top.map_or(0, |item| item.event.rev_id);
    let top_title = top.map_or("n/a", |item| item.event.title.as_str());
    let top_score = top.map_or(0, |item| item.score.total);
    let workbench_summary = render_workbench(
        config,
        queue,
        &WorkbenchOptions {
            token: "parity-report-token".to_string(),
            actor: "SP42-cli".to_string(),
            note: "parity report".to_string(),
        },
        OutputFormat::Text,
    )?;
    let context_summary = render_context_preview(
        config,
        queue,
        &ContextPreviewOptions {
            talk_page: Some("{{Avertissement niveau 2 pour vandalisme}}".to_string()),
            liftwing_probability: Some(0.72),
        },
        OutputFormat::Text,
    )?;
    let stream_summary = render_stream_preview(config, payload, OutputFormat::Text)?;
    let backlog_summary = render_backlog_preview(config, OutputFormat::Text)?;
    let coordination_summary = render_coordination_preview(OutputFormat::Text)?;

    match format {
        OutputFormat::Text => Ok([
            format!("operator parity report wiki={top_wiki} top_rev={top_rev} title=\"{top_title}\" score={top_score}"),
            queue_summary,
            backlog_summary,
            coordination_summary,
            context_summary,
            workbench_summary,
            stream_summary,
        ]
        .join("\n\n")),
        OutputFormat::Json => serde_json::to_string_pretty(&serde_json::json!({
            "queue": queue,
            "queue_summary": queue_summary,
            "backlog": backlog_summary,
            "coordination": coordination_summary,
            "context": context_summary,
            "workbench": workbench_summary,
            "stream": stream_summary,
        }))
        .map_err(|error| error.to_string()),
        OutputFormat::Markdown => {
            let queue_markdown = render_queue(queue, OutputFormat::Markdown)?;
            let backlog_markdown = render_backlog_preview(config, OutputFormat::Markdown)?;
            let coordination_markdown = render_coordination_preview(OutputFormat::Markdown)?;
            let context_markdown = render_context_preview(
                config,
                queue,
                &ContextPreviewOptions {
                    talk_page: Some("{{Avertissement niveau 2 pour vandalisme}}".to_string()),
                    liftwing_probability: Some(0.72),
                },
                OutputFormat::Markdown,
            )?;
            let workbench_markdown = render_workbench(
                config,
                queue,
                &WorkbenchOptions {
                    token: "parity-report-token".to_string(),
                    actor: "SP42-cli".to_string(),
                    note: "parity report".to_string(),
                },
                OutputFormat::Markdown,
            )?;
            let stream_markdown = render_stream_preview(config, payload, OutputFormat::Markdown)?;

            Ok([
                render_markdown_section(
                    "Parity report",
                    &format!(
                        "wiki={top_wiki} top_rev={top_rev} title=\"{top_title}\" score={top_score}"
                    ),
                ),
                queue_markdown,
                backlog_markdown,
                coordination_markdown,
                context_markdown,
                workbench_markdown,
                stream_markdown,
            ]
            .join("\n\n"))
        }
    }
}

fn render_scenario_report(
    config: &sp42_core::WikiConfig,
    queue: &[QueuedEdit],
    options: &CliOptions,
    format: OutputFormat,
) -> Result<String, String> {
    let selected = queue.first();
    let scoring_context = options.context_preview.as_ref().map(|context| {
        build_scoring_context(&ContextInputs {
            talk_page_wikitext: context.talk_page.clone(),
            liftwing_probability: context.liftwing_probability,
        })
    });
    let review_workbench = match (selected, options.workbench.as_ref()) {
        (Some(item), Some(workbench)) => Some(
            build_review_workbench(
                config,
                item,
                &workbench.token,
                &workbench.actor,
                Some(&workbench.note),
            )
            .map_err(|error| error.to_string())?,
        ),
        _ => None,
    };
    let report = build_patrol_scenario_report(&PatrolScenarioReportInputs {
        queue,
        selected,
        scoring_context: scoring_context.as_ref(),
        review_workbench: review_workbench.as_ref(),
        wiki_id_hint: Some(&config.wiki_id),
        ..PatrolScenarioReportInputs::default()
    });
    let shell_state = build_shell_state_model(&ShellStateInputs {
        report: &report,
        review_workbench: review_workbench.as_ref(),
    });

    match format {
        OutputFormat::Text => Ok([
            render_shell_state_text(&shell_state),
            render_patrol_scenario_text(&report),
        ]
        .join("\n\n")),
        OutputFormat::Markdown => Ok([
            render_markdown_section("Shell state", &render_shell_state_markdown(&shell_state)),
            render_markdown_section("Scenario", &render_patrol_scenario_markdown(&report)),
        ]
        .join("\n\n")),
        OutputFormat::Json => serde_json::to_string_pretty(&serde_json::json!({
            "shell_state": shell_state,
            "scenario": report,
        }))
        .map_err(|error| error.to_string()),
    }
}

fn render_session_digest(
    config: &sp42_core::WikiConfig,
    queue: &[QueuedEdit],
    options: &CliOptions,
    format: OutputFormat,
) -> Result<String, String> {
    let selected = queue
        .first()
        .ok_or_else(|| "No queue item available for session digest.".to_string())?;
    let scoring_context = options.context_preview.as_ref().map(|context| {
        build_scoring_context(&ContextInputs {
            talk_page_wikitext: context.talk_page.clone(),
            liftwing_probability: context.liftwing_probability,
        })
    });
    let workbench = build_cli_session_workbench(config, queue.first(), options)?;
    let scenario = build_patrol_scenario_report(&PatrolScenarioReportInputs {
        queue,
        selected: Some(selected),
        scoring_context: scoring_context.as_ref(),
        review_workbench: workbench.as_ref(),
        wiki_id_hint: Some(&config.wiki_id),
        ..PatrolScenarioReportInputs::default()
    });
    let shell_state = build_shell_state_model(&ShellStateInputs {
        report: &scenario,
        review_workbench: workbench.as_ref(),
    });
    let liftwing = options
        .context_preview
        .as_ref()
        .and_then(|preview| preview.liftwing_probability)
        .map_or_else(|| "none".to_string(), |value| format!("{value:.2}"));
    let selected_summary = format!(
        "selected rev={} title=\"{}\" score={} signals={}",
        selected.event.rev_id,
        selected.event.title,
        selected.score.total,
        selected.score.contributions.len()
    );
    let context_summary = format!(
        "context user_risk={} liftwing={liftwing}",
        scoring_context
            .as_ref()
            .is_some_and(|context| context.user_risk.is_some())
    );
    let workbench_summary = workbench.as_ref().map_or_else(
        || "action_workbench=none".to_string(),
        |report| {
            format!(
                "action_workbench requests={} training_rows={}",
                report.requests.len(),
                report.training_csv.lines().count().saturating_sub(1)
            )
        },
    );
    match format {
        OutputFormat::Text => Ok([
            format!(
                "session wiki={} queue={} {}",
                config.wiki_id,
                queue.len(),
                selected_summary
            ),
            context_summary,
            workbench_summary,
            render_shell_state_text(&shell_state),
            render_patrol_scenario_text(&scenario),
        ]
        .join("\n\n")),
        OutputFormat::Markdown => Ok([
            render_markdown_section(
                "Session digest",
                &format!(
                    "wiki={} queue={} {}\n{}\n{}",
                    config.wiki_id,
                    queue.len(),
                    selected_summary,
                    context_summary,
                    workbench_summary
                ),
            ),
            render_markdown_section("Shell state", &render_shell_state_markdown(&shell_state)),
            render_markdown_section("Scenario", &render_patrol_scenario_markdown(&scenario)),
        ]
        .join("\n\n")),
        OutputFormat::Json => render_session_digest_json(
            config,
            queue,
            selected,
            scoring_context.as_ref(),
            options,
            workbench.as_ref(),
            &SessionDigestArtifacts {
                shell_state: &shell_state,
                scenario: &scenario,
            },
        ),
    }
}

fn build_cli_session_workbench(
    config: &sp42_core::WikiConfig,
    selected: Option<&QueuedEdit>,
    options: &CliOptions,
) -> Result<Option<sp42_core::ReviewWorkbench>, String> {
    match (options.workbench.as_ref(), selected) {
        (Some(workbench), Some(item)) => build_review_workbench(
            config,
            item,
            &workbench.token,
            &workbench.actor,
            Some(&workbench.note),
        )
        .map(Some)
        .map_err(|error| error.to_string()),
        _ => Ok(None),
    }
}

#[derive(Debug, Clone, Copy)]
struct SessionDigestArtifacts<'a> {
    shell_state: &'a sp42_core::ShellStateModel,
    scenario: &'a sp42_core::PatrolScenarioReport,
}

fn render_session_digest_json(
    config: &sp42_core::WikiConfig,
    queue: &[QueuedEdit],
    selected: &QueuedEdit,
    scoring_context: Option<&sp42_core::ScoringContext>,
    options: &CliOptions,
    workbench: Option<&sp42_core::ReviewWorkbench>,
    artifacts: &SessionDigestArtifacts<'_>,
) -> Result<String, String> {
    let workbench_json = workbench
        .map(serde_json::to_value)
        .transpose()
        .map_err(|error| error.to_string())?;

    serde_json::to_string_pretty(&serde_json::json!({
        "wiki_id": config.wiki_id,
        "queue_size": queue.len(),
        "selected": {
            "rev_id": selected.event.rev_id,
            "title": selected.event.title.as_str(),
            "score": selected.score.total,
            "signals": selected.score.contributions.len(),
        },
        "context": {
            "user_risk_present": scoring_context.is_some_and(|context| context.user_risk.is_some()),
            "liftwing_probability": options.context_preview.as_ref().and_then(|preview| preview.liftwing_probability),
        },
        "workbench": workbench_json,
        "shell_state": artifacts.shell_state,
        "scenario": artifacts.scenario,
    }))
    .map_err(|error| error.to_string())
}

fn render_action_preview(
    config: &sp42_core::WikiConfig,
    queue: &[QueuedEdit],
    options: &CliOptions,
    format: OutputFormat,
) -> Result<String, String> {
    let selected = queue
        .first()
        .ok_or_else(|| "No queue item available for action mode.".to_string())?;
    let note = action_note(options);
    let requests = build_session_action_execution_requests(selected, note.as_deref())
        .map_err(|error| error.to_string())?;

    match format {
        OutputFormat::Text => Ok([
            format!(
                "action mode wiki={} queue={} selected_rev={} title=\"{}\"",
                config.wiki_id,
                queue.len(),
                selected.event.rev_id,
                selected.event.title
            ),
            render_action_request_lines(&requests).join("\n"),
        ]
        .join("\n\n")),
        OutputFormat::Markdown => Ok([
            render_markdown_section(
                "Action report",
                &format!(
                    "wiki={} queue={} selected_rev={} title=\"{}\"",
                    config.wiki_id,
                    queue.len(),
                    selected.event.rev_id,
                    selected.event.title
                ),
            ),
            render_markdown_section(
                "Prepared actions",
                &render_action_request_lines(&requests)
                    .into_iter()
                    .map(|line| format!("- {line}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
        ]
        .join("\n\n")),
        OutputFormat::Json => serde_json::to_string_pretty(&serde_json::json!({
            "wiki_id": config.wiki_id,
            "queue_size": queue.len(),
            "selected": {
                "rev_id": selected.event.rev_id,
                "title": selected.event.title.as_str(),
                "score": selected.score.total,
            },
            "action_note": action_note(options),
            "requests": requests,
        }))
        .map_err(|error| error.to_string()),
    }
}

fn render_action_execute(
    config: &sp42_core::WikiConfig,
    queue: &[QueuedEdit],
    options: &CliOptions,
    format: OutputFormat,
) -> Result<String, String> {
    let selected = queue
        .first()
        .ok_or_else(|| "No queue item available for action execution.".to_string())?;
    let note = action_note(options);
    let requests = build_session_action_execution_requests(selected, note.as_deref())
        .map_err(|error| error.to_string())?;
    let request = requests
        .iter()
        .find(|request| request.kind == options.action_kind)
        .cloned()
        .ok_or_else(|| "Requested action kind was not prepared.".to_string())?;
    let report = block_on(execute_bridge_action(&options.bridge_base_url, &request))?;

    match format {
        OutputFormat::Text => Ok([
            format!(
                "action execute bridge={} wiki={} kind={:?} rev_id={}",
                options.bridge_base_url, config.wiki_id, request.kind, request.rev_id
            ),
            format!(
                "bootstrap authenticated={} username={} scopes={} cookie={} message={}",
                report.bootstrap.authenticated,
                report.bootstrap.username.as_deref().unwrap_or("none"),
                if report.bootstrap.scopes.is_empty() {
                    "none".to_string()
                } else {
                    report.bootstrap.scopes.join(",")
                },
                report.session_cookie_present,
                report.response.message.as_deref().unwrap_or("none"),
            ),
        ]
        .join("\n")),
        OutputFormat::Markdown => Ok([
            render_markdown_section(
                "Action execute",
                &format!(
                    "bridge={} wiki={} kind={:?} rev_id={}",
                    options.bridge_base_url, config.wiki_id, request.kind, request.rev_id
                ),
            ),
            render_markdown_section(
                "Bootstrap",
                &format!(
                    "authenticated={} username={} scopes={} cookie={}",
                    report.bootstrap.authenticated,
                    report.bootstrap.username.as_deref().unwrap_or("none"),
                    if report.bootstrap.scopes.is_empty() {
                        "none".to_string()
                    } else {
                        report.bootstrap.scopes.join(",")
                    },
                    report.session_cookie_present,
                ),
            ),
            render_markdown_section(
                "Execution result",
                &format!(
                    "accepted={} actor={} message={}",
                    report.response.accepted,
                    report.response.actor.as_deref().unwrap_or("none"),
                    report.response.message.as_deref().unwrap_or("none")
                ),
            ),
        ]
        .join("\n\n")),
        OutputFormat::Json => serde_json::to_string_pretty(&serde_json::json!({
            "bridge_base_url": options.bridge_base_url,
            "wiki_id": config.wiki_id,
            "bootstrap": {
                "authenticated": report.bootstrap.authenticated,
                "username": report.bootstrap.username,
                "scopes": report.bootstrap.scopes,
                "expires_at_ms": report.bootstrap.expires_at_ms,
                "token_present": report.bootstrap.token_present,
                "bridge_mode": report.bootstrap.bridge_mode,
                "local_token_available": report.bootstrap.local_token_available,
            },
            "session_cookie_present": report.session_cookie_present,
            "request": {
                "wiki_id": report.request.wiki_id,
                "kind": report.request.kind,
                "rev_id": report.request.rev_id,
                "title": report.request.title,
                "target_user": report.request.target_user,
                "undo_after_rev_id": report.request.undo_after_rev_id,
                "summary": report.request.summary,
            },
            "response": {
                "wiki_id": report.response.wiki_id,
                "kind": report.response.kind,
                "rev_id": report.response.rev_id,
                "accepted": report.response.accepted,
                "actor": report.response.actor,
                "message": report.response.message,
            },
        }))
        .map_err(|error| error.to_string()),
    }
}

fn render_server_report(base_url: &str, format: OutputFormat) -> Result<String, String> {
    let report = block_on(fetch_server_report(base_url))?;

    match format {
        OutputFormat::Text => Ok(server_report_lines(&report).join("\n")),
        OutputFormat::Markdown => Ok(render_markdown_section(
            "Localhost operator report",
            &server_report_lines(&report)
                .into_iter()
                .map(|line| format!("- {line}"))
                .collect::<Vec<_>>()
                .join("\n"),
        )),
        OutputFormat::Json => {
            serde_json::to_string_pretty(&report).map_err(|error| error.to_string())
        }
    }
}

async fn fetch_server_report(base_url: &str) -> Result<BTreeMap<String, Value>, String> {
    let client = reqwest::Client::builder()
        .user_agent(sp42_core::branding::USER_AGENT)
        .build()
        .map_err(|error| format!("server report client failed to build: {error}"))?;

    let endpoints = [
        ("healthz", "/healthz"),
        ("operator_report", "/operator/report"),
        ("operator_readiness", "/operator/readiness"),
        ("operator_live", "/operator/live/frwiki?limit=1"),
        ("operator_runtime", "/operator/runtime/frwiki"),
        ("bootstrap_status", "/dev/auth/bootstrap/status"),
        ("capabilities_frwiki", "/dev/auth/capabilities/frwiki"),
        ("action_status", "/dev/actions/status"),
        ("action_history", "/dev/actions/history?limit=1"),
    ];

    let mut report = BTreeMap::new();
    for (label, path) in endpoints {
        let value = fetch_server_json(&client, base_url, path).await?;
        report.insert(label.to_string(), value);
    }

    Ok(report)
}

async fn fetch_server_json(
    client: &reqwest::Client,
    base_url: &str,
    path: &str,
) -> Result<Value, String> {
    let url = format!("{base_url}{path}");
    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|error| format!("server endpoint {path} request failed: {error}"))?;
    let response = response
        .error_for_status()
        .map_err(|error| format!("server endpoint {path} request failed: {error}"))?;

    response
        .json::<Value>()
        .await
        .map_err(|error| format!("server endpoint {path} payload was invalid: {error}"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LocalBridgeActionExecutionReport {
    bridge_base_url: String,
    bootstrap: DevAuthSessionStatus,
    session_cookie_present: bool,
    request: SessionActionExecutionRequest,
    response: SessionActionExecutionResponse,
}

fn action_note(options: &CliOptions) -> Option<String> {
    options.action_note.clone().or_else(|| {
        options
            .workbench
            .as_ref()
            .map(|workbench| workbench.note.clone())
    })
}

fn render_action_request_lines(requests: &[SessionActionExecutionRequest]) -> Vec<String> {
    requests
        .iter()
        .map(|request| {
            format!(
                "kind={:?} wiki={} rev_id={} title={} target_user={} undo_after_rev_id={} summary={}",
                request.kind,
                request.wiki_id,
                request.rev_id,
                request.title.as_deref().unwrap_or("none"),
                request.target_user.as_deref().unwrap_or("none"),
                request
                    .undo_after_rev_id
                    .map_or_else(|| "none".to_string(), |value| value.to_string()),
                request.summary.as_deref().unwrap_or("none"),
            )
        })
        .collect()
}

async fn execute_bridge_action(
    base_url: &str,
    request: &SessionActionExecutionRequest,
) -> Result<LocalBridgeActionExecutionReport, String> {
    let client = reqwest::Client::builder()
        .user_agent(sp42_core::branding::USER_AGENT)
        .build()
        .map_err(|error| format!("bridge client failed to build: {error}"))?;

    let bootstrap_request =
        build_dev_auth_bootstrap_request(base_url, &DevAuthBootstrapRequest::default())
            .map_err(|error| error.to_string())?;
    let bootstrap_response = execute_local_http_request(&client, bootstrap_request).await?;
    let bootstrap =
        parse_dev_auth_status(&bootstrap_response.body).map_err(|error| error.to_string())?;
    if !bootstrap.authenticated {
        return Err("bridge bootstrap did not produce an authenticated session".to_string());
    }

    let session_cookie = session_cookie_from_headers(&bootstrap_response.headers)
        .ok_or_else(|| "bridge bootstrap did not set a session cookie".to_string())?;
    let request_url = format!("{base_url}/dev/actions/execute");
    let response = client
        .post(&request_url)
        .header(COOKIE, session_cookie.as_str())
        .json(request)
        .send()
        .await
        .map_err(|error| format!("bridge action request failed: {error}"))?;
    let response = response
        .error_for_status()
        .map_err(|error| format!("bridge action request failed: {error}"))?;
    let action_response = response
        .json::<SessionActionExecutionResponse>()
        .await
        .map_err(|error| format!("bridge action payload was invalid: {error}"))?;

    Ok(LocalBridgeActionExecutionReport {
        bridge_base_url: base_url.to_string(),
        bootstrap,
        session_cookie_present: !session_cookie.is_empty(),
        request: request.clone(),
        response: action_response,
    })
}

async fn execute_local_http_request(
    client: &reqwest::Client,
    request: sp42_core::HttpRequest,
) -> Result<sp42_core::HttpResponse, String> {
    let mut builder = match request.method {
        sp42_core::HttpMethod::Get => client.get(request.url),
        sp42_core::HttpMethod::Post => client.post(request.url),
        sp42_core::HttpMethod::Put => client.put(request.url),
        sp42_core::HttpMethod::Patch => client.patch(request.url),
        sp42_core::HttpMethod::Delete => client.delete(request.url),
    };

    for (key, value) in request.headers {
        builder = builder.header(&key, value);
    }

    let response = builder
        .body(request.body)
        .send()
        .await
        .map_err(|error| format!("bridge request failed: {error}"))?;
    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect();
    let body = response
        .bytes()
        .await
        .map_err(|error| format!("bridge response body could not be read: {error}"))?
        .to_vec();

    Ok(sp42_core::HttpResponse {
        status,
        headers,
        body,
    })
}

fn session_cookie_from_headers(headers: &BTreeMap<String, String>) -> Option<String> {
    headers.get("set-cookie").and_then(|value| {
        value
            .split(';')
            .next()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
    })
}

fn server_report_lines(report: &BTreeMap<String, Value>) -> Vec<String> {
    let mut lines = server_report_operator_lines(report);
    lines.extend(server_report_live_lines(report));
    lines.extend(server_report_runtime_lines(report));
    lines.extend(server_report_action_status_lines(report));
    lines.extend(server_report_action_history_lines(report));
    lines.extend(server_report_capability_lines(report));
    lines.extend(server_report_bootstrap_lines(report));
    lines
}

fn server_report_operator_lines(report: &BTreeMap<String, Value>) -> Vec<String> {
    let operator_report = report.get("operator_report");
    let operator_readiness = report.get("operator_readiness");
    let report_ready = operator_report
        .and_then(|value| json_bool(value, &["readiness", "ready_for_local_testing"]))
        .unwrap_or(false);
    let bootstrap_ready = operator_report
        .and_then(|value| json_bool(value, &["bootstrap", "bootstrap_ready"]))
        .unwrap_or(false);
    let readiness_issues = operator_report
        .and_then(|value| json_array_len(value, &["readiness", "readiness_issues"]))
        .unwrap_or_default();
    let endpoint_count = operator_report
        .and_then(|value| json_array_len(value, &["endpoints"]))
        .unwrap_or_default();
    let operator_ready = operator_readiness
        .and_then(|value| json_bool(value, &["ready_for_local_testing"]))
        .unwrap_or(report_ready && bootstrap_ready);

    let mut lines = vec![format!(
        "operator report ready_for_local_testing={report_ready} bootstrap_ready={bootstrap_ready} readiness_issues={readiness_issues} endpoints={endpoint_count}"
    )];
    lines.push(format!(
        "operator readiness ready_for_local_testing={} bootstrap_ready={} operator_ready={}",
        operator_readiness
            .and_then(|value| json_bool(value, &["ready_for_local_testing"]))
            .unwrap_or(false),
        operator_readiness
            .and_then(|value| json_bool(value, &["bootstrap_ready"]))
            .unwrap_or(false),
        operator_ready
    ));
    lines
}

fn server_report_capability_lines(report: &BTreeMap<String, Value>) -> Vec<String> {
    let capability_checked = report
        .get("capabilities_frwiki")
        .and_then(|capabilities| json_bool(capabilities, &["checked"]))
        .unwrap_or(false);
    let capability_can_patrol = report
        .get("capabilities_frwiki")
        .and_then(|capabilities| {
            json_bool(capabilities, &["capabilities", "moderation", "can_patrol"])
        })
        .unwrap_or(false);
    let capability_can_rollback = report
        .get("capabilities_frwiki")
        .and_then(|capabilities| {
            json_bool(
                capabilities,
                &["capabilities", "moderation", "can_rollback"],
            )
        })
        .unwrap_or(false);

    vec![format!(
        "capabilities checked={capability_checked} patrol={capability_can_patrol} rollback={capability_can_rollback}"
    )]
}

fn server_report_live_lines(report: &BTreeMap<String, Value>) -> Vec<String> {
    let Some(live) = report.get("operator_live") else {
        return Vec::new();
    };

    let selected_index = live
        .get("selected_index")
        .and_then(Value::as_u64)
        .map_or_else(|| "none".to_string(), |value| value.to_string());
    let queue_len = json_array_len(live, &["queue"]).unwrap_or_default();
    let selected = live
        .get("selected_index")
        .and_then(Value::as_u64)
        .and_then(|index| {
            let index = usize::try_from(index).ok()?;
            live.get("queue").and_then(Value::as_array)?.get(index)
        })
        .map_or_else(
            || "selected=none".to_string(),
            |item| {
                let score = item
                    .get("score")
                    .and_then(|score| score.get("total"))
                    .and_then(Value::as_i64)
                    .unwrap_or_default();
                format!(
                    "selected rev={} title=\"{}\" score={score}",
                    json_u64(item, &["event", "rev_id"]).unwrap_or_default(),
                    json_str(item, &["event", "title"]).unwrap_or("none"),
                )
            },
        );
    let backend_ready = json_bool(live, &["backend", "ready_for_local_testing"]).unwrap_or(false);
    let session_authenticated = json_bool(live, &["backend", "session", "authenticated"])
        .unwrap_or_else(|| json_bool(live, &["auth", "authenticated"]).unwrap_or(false));
    let action_total = json_u64(live, &["action_status", "total_actions"]).unwrap_or_default();
    let history_entries = json_array_len(live, &["action_history", "entries"]).unwrap_or_default();
    let live_notes = json_array_len(live, &["notes"]).unwrap_or_default();

    vec![
        format!(
            "operator live wiki={} queue={} selected_index={} backend_ready={} authenticated={} actions={} history_entries={} notes={}",
            json_str(live, &["wiki_id"]).unwrap_or("none"),
            queue_len,
            selected_index,
            backend_ready,
            session_authenticated,
            action_total,
            history_entries,
            live_notes,
        ),
        selected,
        format!(
            "operator live backend bootstrap_ready={} client_id={} access_token={} cache_present={} cache_fresh={} cache_age_ms={}",
            json_bool(live, &["backend", "bootstrap_ready"]).unwrap_or(false),
            json_bool(live, &["backend", "oauth", "client_id_present"]).unwrap_or(false),
            json_bool(live, &["backend", "oauth", "access_token_present"]).unwrap_or(false),
            json_bool(live, &["backend", "capability_cache_present"]).unwrap_or(false),
            json_bool(live, &["backend", "capability_cache_fresh"]).unwrap_or(false),
            json_u64(live, &["backend", "capability_cache_age_ms"]).unwrap_or_default(),
        ),
    ]
}

fn server_report_action_history_lines(report: &BTreeMap<String, Value>) -> Vec<String> {
    let action_history_count = report.get("action_history").map_or_else(
        || 0,
        |history| json_array_len(history, &["entries"]).unwrap_or_default(),
    );
    let action_history_latest = report
        .get("action_history")
        .and_then(|history| json_value_at(history, &["entries"]))
        .and_then(Value::as_array)
        .and_then(|entries| entries.first())
        .map_or_else(
            || "latest=none".to_string(),
            |entry| {
                format!(
                    "latest kind={} rev_id={} accepted={} title=\"{}\"",
                    json_str(entry, &["kind"]).unwrap_or("none"),
                    json_u64(entry, &["rev_id"]).unwrap_or_default(),
                    json_bool(entry, &["accepted"]).unwrap_or(false),
                    json_str(entry, &["title"]).unwrap_or("none"),
                )
            },
        );

    vec![format!(
        "action history entries={action_history_count} {action_history_latest}"
    )]
}

fn server_report_action_status_lines(report: &BTreeMap<String, Value>) -> Vec<String> {
    let Some(action_status) = report.get("action_status") else {
        return Vec::new();
    };

    let last_execution = action_status.get("last_execution").map_or_else(
        || "latest=none".to_string(),
        |entry| {
            format!(
                "latest kind={} rev_id={} accepted={} title=\"{}\"",
                json_str(entry, &["kind"]).unwrap_or("none"),
                json_u64(entry, &["rev_id"]).unwrap_or_default(),
                json_bool(entry, &["accepted"]).unwrap_or(false),
                json_str(entry, &["title"]).unwrap_or("none"),
            )
        },
    );

    vec![
        format!(
            "action status authenticated={} session_id={} username={} total_actions={} shell_feedback={}",
            json_bool(action_status, &["authenticated"]).unwrap_or(false),
            json_str(action_status, &["session_id"]).unwrap_or("none"),
            json_str(action_status, &["username"]).unwrap_or("none"),
            json_u64(action_status, &["total_actions"]).unwrap_or_default(),
            json_array_len(action_status, &["shell_feedback"]).unwrap_or_default(),
        ),
        last_execution,
    ]
}

fn server_report_runtime_lines(report: &BTreeMap<String, Value>) -> Vec<String> {
    let Some(runtime) = report.get("operator_runtime") else {
        return Vec::new();
    };

    let notes_count = json_array_len(runtime, &["notes"]).unwrap_or_default();
    vec![
        format!(
            "operator runtime wiki={} storage_root={} notes={}",
            json_str(runtime, &["wiki_id"]).unwrap_or("none"),
            json_str(runtime, &["storage_root"]).unwrap_or("none"),
            notes_count,
        ),
        format!(
            "operator runtime backlog limit={} total={} polls={} next_continue={} checkpoint={}",
            json_u64(runtime, &["backlog", "limit"]).unwrap_or_default(),
            json_u64(runtime, &["backlog", "total_events"]).unwrap_or_default(),
            json_u64(runtime, &["backlog", "poll_count"]).unwrap_or_default(),
            json_str(runtime, &["backlog", "next_continue"]).unwrap_or("none"),
            json_str(runtime, &["backlog", "checkpoint_key"]).unwrap_or("none"),
        ),
        format!(
            "operator runtime stream checkpoint={} last_event_id={}",
            json_str(runtime, &["stream_checkpoint_key"]).unwrap_or("none"),
            json_str(runtime, &["stream_last_event_id"]).unwrap_or("none"),
        ),
    ]
}

fn server_report_bootstrap_lines(report: &BTreeMap<String, Value>) -> Vec<String> {
    report
        .get("bootstrap_status")
        .map(|bootstrap| {
            format!(
                "bootstrap source_path={} source_file_present={}",
                json_str(bootstrap, &["source_path"]).unwrap_or("none"),
                json_bool(bootstrap, &["source_report", "loaded_from_source"]).unwrap_or(false),
            )
        })
        .into_iter()
        .collect()
}

fn json_value_at<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter()
        .try_fold(value, |current, key| current.get(*key))
}

fn json_bool(value: &Value, path: &[&str]) -> Option<bool> {
    json_value_at(value, path).and_then(Value::as_bool)
}

fn json_u64(value: &Value, path: &[&str]) -> Option<u64> {
    json_value_at(value, path).and_then(Value::as_u64)
}

fn json_str<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    json_value_at(value, path).and_then(Value::as_str)
}

fn json_array_len(value: &Value, path: &[&str]) -> Option<usize> {
    json_value_at(value, path)
        .and_then(Value::as_array)
        .map(std::vec::Vec::len)
}

fn render_markdown_section(title: &str, body: &str) -> String {
    if body.trim().is_empty() {
        format!("## {title}\n\n_Empty_")
    } else {
        format!("## {title}\n\n{body}")
    }
}

fn render_markdown_code_block(language: &str, body: &str) -> String {
    if body.trim().is_empty() {
        "```text\n_Empty_\n```".to_string()
    } else {
        format!("```{language}\n{body}\n```")
    }
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
    use std::collections::BTreeMap;
    use std::collections::BTreeSet;

    use serde_json::Value;

    use super::{
        CliOptions, ContextPreviewOptions, LOCAL_SERVER_BASE_URL, OutputFormat, PreviewMode,
        ShellMode, WorkbenchOptions, parse_options, render_action_preview, render_backlog_preview,
        render_context_preview, render_coordination_preview, render_parity_report, render_queue,
        render_scenario_report, render_session_digest, render_stream_preview, render_workbench,
        server_report_lines,
    };
    use serde_json::json;
    use sp42_core::{StreamIngestor, build_ranked_queue, parse_wiki_config};

    const DEFAULT_CONFIG: &str = include_str!("../../../configs/frwiki.yaml");
    const SAMPLE_EVENTS: &str = include_str!("../../../fixtures/frwiki_recentchanges_batch.jsonl");

    #[test]
    fn renders_ranked_queue_lines() {
        let config = parse_wiki_config(DEFAULT_CONFIG).expect("config should parse");
        let ingestor = StreamIngestor::from_config(&config);
        let events = ingestor
            .ingest_lines(SAMPLE_EVENTS)
            .expect("events should parse");
        let ranked = build_ranked_queue(events, &config.scoring).expect("queue should build");

        let summary = render_queue(&ranked, OutputFormat::Text).expect("text render should work");

        assert!(summary.contains("#1"));
        assert!(summary.contains("wiki=frwiki"));
        assert!(summary.contains("rev_id=123459"));
    }

    #[test]
    fn renders_ranked_queue_as_json() {
        let config = parse_wiki_config(DEFAULT_CONFIG).expect("config should parse");
        let ingestor = StreamIngestor::from_config(&config);
        let events = ingestor
            .ingest_lines(SAMPLE_EVENTS)
            .expect("events should parse");
        let ranked = build_ranked_queue(events, &config.scoring).expect("queue should build");

        let summary = render_queue(&ranked, OutputFormat::Json).expect("json render should work");

        assert!(summary.contains("\"wiki_id\": \"frwiki\""));
        assert!(summary.contains("\"rev_id\": 123459"));
    }

    #[test]
    fn renders_ranked_queue_as_markdown() {
        let config = parse_wiki_config(DEFAULT_CONFIG).expect("config should parse");
        let ingestor = StreamIngestor::from_config(&config);
        let events = ingestor
            .ingest_lines(SAMPLE_EVENTS)
            .expect("events should parse");
        let ranked = build_ranked_queue(events, &config.scoring).expect("queue should build");

        let summary =
            render_queue(&ranked, OutputFormat::Markdown).expect("markdown render should work");

        assert!(summary.contains("## Ranked queue"));
        assert!(summary.contains("#1 wiki=frwiki"));
        assert!(summary.contains("rev_id=123459"));
    }

    #[test]
    fn parses_requested_output_format() {
        let options = parse_options(["--format".to_string(), "json".to_string()])
            .expect("format should parse");

        assert_eq!(options.format, OutputFormat::Json);
    }

    #[test]
    fn parses_markdown_output_format() {
        let options = parse_options(["--format".to_string(), "markdown".to_string()])
            .expect("markdown format should parse");

        assert_eq!(options.format, OutputFormat::Markdown);
    }

    #[test]
    fn parses_workbench_options() {
        let options = parse_options([
            "--workbench-token".to_string(),
            "token-123".to_string(),
            "--workbench-actor".to_string(),
            "Reviewer".to_string(),
            "--workbench-note".to_string(),
            "local workbench".to_string(),
        ])
        .expect("workbench options should parse");

        assert_eq!(
            options.workbench,
            Some(WorkbenchOptions {
                token: "token-123".to_string(),
                actor: "Reviewer".to_string(),
                note: "local workbench".to_string(),
            })
        );
    }

    #[test]
    fn parses_context_preview_options() {
        let options = parse_options([
            "--context-talk".to_string(),
            "{{Avertissement niveau 1}}".to_string(),
            "--context-liftwing".to_string(),
            "0.42".to_string(),
        ])
        .expect("context options should parse");

        assert_eq!(
            options.context_preview,
            Some(ContextPreviewOptions {
                talk_page: Some("{{Avertissement niveau 1}}".to_string()),
                liftwing_probability: Some(0.42),
            })
        );
    }

    #[test]
    fn parses_stream_preview_flag() {
        let options =
            parse_options(["--stream-preview".to_string()]).expect("stream mode flag should parse");

        assert!(options.preview_modes.contains(&PreviewMode::Stream));
    }

    #[test]
    fn parses_backlog_and_coordination_flags() {
        let options = parse_options([
            "--backlog-preview".to_string(),
            "--coordination-preview".to_string(),
            "--session-digest".to_string(),
            "--action-preview".to_string(),
            "--action-execute".to_string(),
            "--scenario-report".to_string(),
            "--patrol-report".to_string(),
            "--server-report".to_string(),
            "--parity-report".to_string(),
        ])
        .expect("new preview flags should parse");

        assert!(options.preview_modes.contains(&PreviewMode::Backlog));
        assert!(options.preview_modes.contains(&PreviewMode::Coordination));
        assert!(options.preview_modes.contains(&PreviewMode::SessionDigest));
        assert!(options.preview_modes.contains(&PreviewMode::ActionPreview));
        assert!(options.preview_modes.contains(&PreviewMode::ActionExecute));
        assert!(options.preview_modes.contains(&PreviewMode::ScenarioReport));
        assert!(options.preview_modes.contains(&PreviewMode::ServerReport));
        assert!(options.preview_modes.contains(&PreviewMode::ParityReport));
    }

    #[test]
    fn parses_shell_and_action_options() {
        let options = parse_options([
            "--shell".to_string(),
            "action-preview".to_string(),
            "--action-kind".to_string(),
            "undo".to_string(),
            "--action-note".to_string(),
            "inspect".to_string(),
            "--bridge-base-url".to_string(),
            "http://127.0.0.1:9000".to_string(),
        ])
        .expect("shell options should parse");

        assert_eq!(options.shell_mode, Some(ShellMode::ActionPreview));
        assert_eq!(options.action_kind, sp42_core::SessionActionKind::Undo);
        assert_eq!(options.action_note.as_deref(), Some("inspect"));
        assert_eq!(options.bridge_base_url, "http://127.0.0.1:9000");
    }

    #[test]
    fn renders_scenario_report() {
        let config = parse_wiki_config(DEFAULT_CONFIG).expect("config should parse");
        let ingestor = StreamIngestor::from_config(&config);
        let events = ingestor
            .ingest_lines(SAMPLE_EVENTS)
            .expect("events should parse");
        let ranked = build_ranked_queue(events, &config.scoring).expect("queue should build");

        let summary = render_scenario_report(
            &config,
            &ranked,
            &CliOptions {
                format: OutputFormat::Text,
                workbench: Some(WorkbenchOptions {
                    token: "token-123".to_string(),
                    actor: "Reviewer".to_string(),
                    note: "local workbench".to_string(),
                }),
                context_preview: Some(ContextPreviewOptions {
                    talk_page: Some("{{Avertissement niveau 2 pour vandalisme}}".to_string()),
                    liftwing_probability: Some(0.42),
                }),
                preview_modes: BTreeSet::new(),
                shell_mode: None,
                action_note: None,
                action_kind: sp42_core::SessionActionKind::Patrol,
                bridge_base_url: LOCAL_SERVER_BASE_URL.to_string(),
            },
            OutputFormat::Text,
        )
        .expect("scenario report should render");

        assert!(summary.contains("Patrol report"));
        assert!(summary.contains("wiki=frwiki"));
        assert!(summary.contains("selected rev=123459"));
        assert!(summary.contains("Shell state"));
        assert!(summary.contains("Findings"));
        assert!(summary.contains("[Workbench] available="));
    }

    #[test]
    fn renders_scenario_report_as_markdown() {
        let config = parse_wiki_config(DEFAULT_CONFIG).expect("config should parse");
        let ingestor = StreamIngestor::from_config(&config);
        let events = ingestor
            .ingest_lines(SAMPLE_EVENTS)
            .expect("events should parse");
        let ranked = build_ranked_queue(events, &config.scoring).expect("queue should build");

        let summary = render_scenario_report(
            &config,
            &ranked,
            &CliOptions {
                format: OutputFormat::Markdown,
                workbench: None,
                context_preview: None,
                preview_modes: BTreeSet::new(),
                shell_mode: None,
                action_note: None,
                action_kind: sp42_core::SessionActionKind::Patrol,
                bridge_base_url: LOCAL_SERVER_BASE_URL.to_string(),
            },
            OutputFormat::Markdown,
        )
        .expect("scenario report should render");

        assert!(summary.contains("# Patrol report"));
        assert!(summary.contains("## Shell state"));
        assert!(summary.contains("selected rev=123459"));
        assert!(summary.contains("## Findings"));
    }

    #[test]
    fn renders_scenario_report_as_json() {
        let config = parse_wiki_config(DEFAULT_CONFIG).expect("config should parse");
        let ingestor = StreamIngestor::from_config(&config);
        let events = ingestor
            .ingest_lines(SAMPLE_EVENTS)
            .expect("events should parse");
        let ranked = build_ranked_queue(events, &config.scoring).expect("queue should build");

        let summary = render_scenario_report(
            &config,
            &ranked,
            &CliOptions {
                format: OutputFormat::Json,
                workbench: None,
                context_preview: None,
                preview_modes: BTreeSet::new(),
                shell_mode: None,
                action_note: None,
                action_kind: sp42_core::SessionActionKind::Patrol,
                bridge_base_url: LOCAL_SERVER_BASE_URL.to_string(),
            },
            OutputFormat::Json,
        )
        .expect("scenario report should render");

        let value: serde_json::Value =
            serde_json::from_str(&summary).expect("scenario report json should parse");

        assert!(value["shell_state"].is_object());
        assert!(value["scenario"].is_object());
        assert_eq!(value["shell_state"]["wiki_id"], "frwiki");
        assert_eq!(value["scenario"]["queue_depth"], 4);
        assert_eq!(value["scenario"]["selected"]["rev_id"], 123_459);
    }

    #[test]
    fn renders_session_digest_in_all_formats() {
        let config = parse_wiki_config(DEFAULT_CONFIG).expect("config should parse");
        let ingestor = StreamIngestor::from_config(&config);
        let events = ingestor
            .ingest_lines(SAMPLE_EVENTS)
            .expect("events should parse");
        let ranked = build_ranked_queue(events, &config.scoring).expect("queue should build");
        let options = CliOptions {
            format: OutputFormat::Text,
            workbench: Some(WorkbenchOptions {
                token: "token-123".to_string(),
                actor: "Reviewer".to_string(),
                note: "digest".to_string(),
            }),
            context_preview: Some(ContextPreviewOptions {
                talk_page: Some("{{Avertissement niveau 2 pour vandalisme}}".to_string()),
                liftwing_probability: Some(0.42),
            }),
            preview_modes: BTreeSet::new(),
            shell_mode: None,
            action_note: None,
            action_kind: sp42_core::SessionActionKind::Patrol,
            bridge_base_url: LOCAL_SERVER_BASE_URL.to_string(),
        };

        let text = render_session_digest(&config, &ranked, &options, OutputFormat::Text)
            .expect("session digest should render");
        assert!(text.contains("session wiki=frwiki"));
        assert!(text.contains("action_workbench requests="));
        assert!(text.contains("selected rev=123459"));
        assert!(text.contains("Shell state"));

        let markdown = render_session_digest(&config, &ranked, &options, OutputFormat::Markdown)
            .expect("session digest markdown should render");
        assert!(markdown.contains("## Session digest"));
        assert!(markdown.contains("## Shell state"));
        assert!(markdown.contains("## Scenario"));

        let json = render_session_digest(&config, &ranked, &options, OutputFormat::Json)
            .expect("session digest json should render");
        let value: serde_json::Value =
            serde_json::from_str(&json).expect("session digest json should parse");
        assert_eq!(value["shell_state"]["wiki_id"], "frwiki");
        assert_eq!(value["scenario"]["selected"]["rev_id"], 123_459);
        assert!(value["shell_state"]["timeline"].is_array());
        assert!(value["scenario"].is_object());
    }

    #[test]
    fn renders_action_preview_in_all_formats() {
        let config = parse_wiki_config(DEFAULT_CONFIG).expect("config should parse");
        let ingestor = StreamIngestor::from_config(&config);
        let events = ingestor
            .ingest_lines(SAMPLE_EVENTS)
            .expect("events should parse");
        let ranked = build_ranked_queue(events, &config.scoring).expect("queue should build");
        let options = CliOptions {
            format: OutputFormat::Text,
            workbench: None,
            context_preview: None,
            preview_modes: BTreeSet::new(),
            shell_mode: Some(ShellMode::ActionPreview),
            action_note: Some("inspect".to_string()),
            action_kind: sp42_core::SessionActionKind::Patrol,
            bridge_base_url: "http://127.0.0.1:8788".to_string(),
        };

        let text = render_action_preview(&config, &ranked, &options, OutputFormat::Text)
            .expect("action preview should render");
        assert!(text.contains("action mode wiki=frwiki"));
        assert!(text.contains("kind=Patrol"));
        assert!(text.contains("summary=inspect"));

        let markdown = render_action_preview(&config, &ranked, &options, OutputFormat::Markdown)
            .expect("action preview markdown should render");
        assert!(markdown.contains("## Action report"));
        assert!(markdown.contains("## Prepared actions"));

        let json = render_action_preview(&config, &ranked, &options, OutputFormat::Json)
            .expect("action preview json should render");
        let value: serde_json::Value =
            serde_json::from_str(&json).expect("action preview json should parse");
        assert_eq!(value["wiki_id"], "frwiki");
        assert_eq!(value["action_note"], "inspect");
        assert_eq!(
            value["requests"].as_array().map(std::vec::Vec::len),
            Some(3)
        );
    }

    #[test]
    fn renders_workbench_preview() {
        let config = parse_wiki_config(DEFAULT_CONFIG).expect("config should parse");
        let ingestor = StreamIngestor::from_config(&config);
        let events = ingestor
            .ingest_lines(SAMPLE_EVENTS)
            .expect("events should parse");
        let ranked = build_ranked_queue(events, &config.scoring).expect("queue should build");

        let summary = render_workbench(
            &config,
            &ranked,
            &WorkbenchOptions {
                token: "token-123".to_string(),
                actor: "Reviewer".to_string(),
                note: "local workbench".to_string(),
            },
            OutputFormat::Text,
        )
        .expect("workbench render should work");

        assert!(summary.contains("action workbench rev="));
        assert!(summary.contains("rollback"));
        assert!(summary.contains("training_jsonl="));
    }

    #[test]
    fn renders_context_preview() {
        let config = parse_wiki_config(DEFAULT_CONFIG).expect("config should parse");
        let ingestor = StreamIngestor::from_config(&config);
        let events = ingestor
            .ingest_lines(SAMPLE_EVENTS)
            .expect("events should parse");
        let ranked = build_ranked_queue(events, &config.scoring).expect("queue should build");

        let summary = render_context_preview(
            &config,
            &ranked,
            &ContextPreviewOptions {
                talk_page: Some("{{Avertissement niveau 2 pour vandalisme}}".to_string()),
                liftwing_probability: Some(0.42),
            },
            OutputFormat::Text,
        )
        .expect("context preview should render");

        assert!(summary.contains("context rev="));
        assert!(summary.contains("recentchanges"));
        assert!(summary.contains("contextual score="));
    }

    #[test]
    fn renders_stream_preview() {
        let config = parse_wiki_config(DEFAULT_CONFIG).expect("config should parse");

        let summary = render_stream_preview(&config, SAMPLE_EVENTS, OutputFormat::Text)
            .expect("stream mode should render");

        assert!(summary.contains("stream checkpoint_key="));
        assert!(summary.contains("stream delivered="));
        assert!(summary.contains("stream rev=123456"));
    }

    #[test]
    fn renders_backlog_preview() {
        let config = parse_wiki_config(DEFAULT_CONFIG).expect("config should parse");

        let summary = render_backlog_preview(&config, OutputFormat::Text)
            .expect("backlog mode should render");

        assert!(summary.contains("backlog report"));
        assert!(summary.contains("backlog batch="));
        assert!(summary.contains("next_continue="));
    }

    #[test]
    fn renders_coordination_preview() {
        let summary = render_coordination_preview(OutputFormat::Text)
            .expect("coordination mode should render");

        assert!(summary.contains("coordination report wiki=frwiki"));
        assert!(summary.contains("roundtrip"));
        assert!(summary.contains("claim rev=123456"));
    }

    #[test]
    fn renders_parity_report() {
        let config = parse_wiki_config(DEFAULT_CONFIG).expect("config should parse");
        let ingestor = StreamIngestor::from_config(&config);
        let events = ingestor
            .ingest_lines(SAMPLE_EVENTS)
            .expect("events should parse");
        let ranked = build_ranked_queue(events, &config.scoring).expect("queue should build");

        let summary = render_parity_report(&config, &ranked, SAMPLE_EVENTS, OutputFormat::Text)
            .expect("parity report should render");

        assert!(summary.contains("operator parity report wiki=frwiki"));
        assert!(summary.contains("backlog report"));
        assert!(summary.contains("coordination report wiki=frwiki"));
        assert!(summary.contains("action workbench rev="));
        assert!(summary.contains("stream delivered="));
    }

    #[test]
    fn renders_parity_report_as_markdown() {
        let config = parse_wiki_config(DEFAULT_CONFIG).expect("config should parse");
        let ingestor = StreamIngestor::from_config(&config);
        let events = ingestor
            .ingest_lines(SAMPLE_EVENTS)
            .expect("events should parse");
        let ranked = build_ranked_queue(events, &config.scoring).expect("queue should build");

        let summary = render_parity_report(&config, &ranked, SAMPLE_EVENTS, OutputFormat::Markdown)
            .expect("parity report should render");

        assert!(summary.contains("## Parity report"));
        assert!(summary.contains("## Ranked queue"));
        assert!(summary.contains("## Backlog report"));
        assert!(summary.contains("## Coordination report"));
        assert!(summary.contains("## Context report"));
        assert!(summary.contains("## Action workbench"));
        assert!(summary.contains("## Stream report"));
    }

    #[allow(clippy::too_many_lines)]
    fn sample_server_report() -> BTreeMap<String, Value> {
        BTreeMap::from([
            (
                "operator_report".to_string(),
                json!({
                    "project": "SP42",
                    "readiness": {
                        "ready_for_local_testing": true,
                        "readiness_issues": []
                    },
                    "runtime": {},
                    "bootstrap": { "bootstrap_ready": true },
                    "debug_summary": {},
                    "endpoints": [{}, {}]
                }),
            ),
            (
                "operator_readiness".to_string(),
                json!({
                    "ready_for_local_testing": true,
                    "bootstrap_ready": true
                }),
            ),
            (
                "operator_live".to_string(),
                json!({
                    "project": "SP42",
                    "wiki_id": "frwiki",
                    "queue": [
                        {
                            "event": {
                                "wiki_id": "frwiki",
                                "rev_id": 123_459,
                                "title": "Vandalisme"
                            },
                            "score": { "total": 88, "contributions": [{}, {}] }
                        }
                    ],
                    "selected_index": 0,
                    "backend": {
                        "ready_for_local_testing": true,
                        "bootstrap_ready": true,
                        "oauth": {
                            "client_id_present": true,
                            "access_token_present": true
                        },
                        "capability_cache_present": true,
                        "capability_cache_fresh": true,
                        "capability_cache_age_ms": 4
                    },
                    "action_status": {
                        "authenticated": true,
                        "session_id": "session-1",
                        "username": "Tester",
                        "total_actions": 3,
                        "last_execution": {
                            "kind": "Patrol",
                            "rev_id": 123_459,
                            "accepted": true,
                            "title": "Vandalisme"
                        }
                    },
                    "action_history": {
                        "entries": [
                            {
                                "kind": "Patrol",
                                "rev_id": 123_459,
                                "accepted": true,
                                "title": "Vandalisme"
                            }
                        ]
                    },
                    "coordination_room": { "wiki_id": "frwiki" },
                    "coordination_state": { "wiki_id": "frwiki" },
                    "notes": ["live note"]
                }),
            ),
            (
                "operator_runtime".to_string(),
                json!({
                    "wiki_id": "frwiki",
                    "storage_root": "/tmp/sp42",
                    "backlog": {
                        "limit": 15,
                        "total_events": 7,
                        "poll_count": 2,
                        "next_continue": "rccontinue-token",
                        "checkpoint_key": "recentchanges.rccontinue.frwiki"
                    },
                    "stream_checkpoint_key": "stream.last_event_id.frwiki",
                    "stream_last_event_id": "evt-123",
                    "notes": ["runtime note"]
                }),
            ),
            (
                "bootstrap_status".to_string(),
                json!({
                    "source_path": ".env.wikimedia.local",
                    "source_report": { "loaded_from_source": true }
                }),
            ),
            (
                "capabilities_frwiki".to_string(),
                json!({
                    "checked": true,
                    "capabilities": {
                        "editing": { "can_edit": true, "can_undo": false },
                        "moderation": { "can_patrol": true, "can_rollback": false }
                    }
                }),
            ),
            (
                "action_history".to_string(),
                json!({
                    "entries": [
                        {
                            "executed_at_ms": 1_234_567_890,
                            "wiki_id": "frwiki",
                            "kind": "Patrol",
                            "rev_id": 123_459,
                            "title": "Vandalisme",
                            "target_user": "Example",
                            "summary": "approve",
                            "accepted": true,
                            "http_status": 200,
                            "response_preview": "{\"status\":\"ok\"}"
                        }
                    ]
                }),
            ),
            (
                "action_status".to_string(),
                json!({
                    "authenticated": true,
                    "session_id": "session-1",
                    "username": "Tester",
                    "total_actions": 3,
                    "last_execution": {
                        "kind": "Patrol",
                        "rev_id": 123_459,
                        "accepted": true,
                        "title": "Vandalisme"
                    },
                    "shell_feedback": ["ok"]
                }),
            ),
        ])
    }

    #[test]
    fn server_report_lines_extract_key_fields() {
        let report = sample_server_report();

        let lines = server_report_lines(&report);

        assert!(lines.iter().any(|line| {
            line.contains("operator report ready_for_local_testing=true bootstrap_ready=true")
        }));
        assert!(lines.iter().any(|line| {
            line.contains("operator readiness ready_for_local_testing=true bootstrap_ready=true operator_ready=true")
        }));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("operator live wiki=frwiki queue=1 selected_index=0"))
        );
        assert!(
            lines.iter().any(|line| line.contains("operator live backend bootstrap_ready=true client_id=true access_token=true cache_present=true cache_fresh=true cache_age_ms=4"))
        );
        assert!(lines.iter().any(|line| {
            line.contains("action status authenticated=true session_id=session-1 username=Tester total_actions=3 shell_feedback=1")
        }));
        assert!(lines.iter().any(|line| {
            line.contains(
                "action history entries=1 latest kind=Patrol rev_id=123459 accepted=true title=\"Vandalisme\""
            )
        }));
        assert!(lines.iter().any(|line| {
            line.contains("operator runtime wiki=frwiki storage_root=/tmp/sp42 notes=1")
        }));
        assert!(lines.iter().any(|line| {
            line.contains("operator runtime backlog limit=15 total=7 polls=2 next_continue=rccontinue-token checkpoint=recentchanges.rccontinue.frwiki")
        }));
        assert!(lines.iter().any(|line| {
            line.contains("operator runtime stream checkpoint=stream.last_event_id.frwiki last_event_id=evt-123")
        }));
        assert!(
            lines.iter().any(|line| {
                line.contains("capabilities checked=true patrol=true rollback=false")
            })
        );
        assert!(lines.iter().any(|line| {
            line.contains("bootstrap source_path=.env.wikimedia.local source_file_present=true")
        }));
    }

    #[test]
    fn server_report_markdown_renders_pure_lines() {
        let report = sample_server_report();

        let markdown = super::render_markdown_section(
            "Localhost operator report",
            &server_report_lines(&report)
                .into_iter()
                .map(|line| format!("- {line}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );

        assert!(markdown.contains("## Localhost operator report"));
        assert!(markdown.contains("- operator report ready_for_local_testing=true"));
    }
}
