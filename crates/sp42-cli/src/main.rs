use std::convert::TryFrom;
use std::io::{self, Read};
use std::process::ExitCode;
use std::time::Duration;

use async_trait::async_trait;
use futures::executor::block_on;
use genai::Client;
use genai::adapter::AdapterKind;
use genai::chat::{ChatMessage as GenaiChatMessage, ChatOptions, ChatRequest};
use genai::resolver::{AuthData, Endpoint};
use genai::{Headers, ModelIden, ServiceTarget};
use reqwest::header::COOKIE;
use serde_json::Value;
use sp42_core::routes as route_contracts;
use sp42_core::{
    ChatRole, CitationFinding, CitationVerificationRequest, DevAuthBootstrapRequest,
    DevAuthSessionStatus, EndpointMode, GroundingStatus, ModelClient, ModelClientError,
    ModelCompletion, ModelCompletionRequest, ModelEndpointConfig, ModelRef, QueuedEdit,
    SamplingParams, SessionActionExecutionRequest, SessionActionExecutionResponse,
    SessionActionKind, SystemClock, VerificationOutcome, VerifyOptions as CoreVerifyOptions,
    build_dev_auth_bootstrap_request, check_fetchable_source_url, locate_quote, locate_quote_fuzzy,
    parse_dev_auth_status, verify_citation_use_site,
};
use sp42_devtools::{
    DEV_PREVIEW_SAMPLE_EVENTS, DEV_PREVIEW_WIKI_ID, DevContextOptions, DevWorkbenchOptions,
    build_dev_action_requests, build_dev_backlog_preview, build_dev_context,
    build_dev_context_preview, build_dev_coordination_preview, build_dev_queue,
    build_dev_stream_preview, build_dev_workbench, parse_default_dev_wiki_config,
};
use sp42_reporting::{
    PatrolScenarioReportInputs, ShellStateInputs, build_patrol_scenario_report,
    build_shell_state_model, render_patrol_scenario_markdown, render_patrol_scenario_text,
    render_shell_state_markdown, render_shell_state_text,
};
use sp42_types::{HttpClient, HttpClientError, HttpMethod, HttpRequest, HttpResponse};
use std::collections::{BTreeMap, BTreeSet};

const LOCAL_SERVER_BASE_URL: &str = "http://127.0.0.1:8788";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Text,
    Json,
    Markdown,
}

/// Which bare-URL flag-mode was selected (PRD-0008 CLI surface).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BareUrlCliMode {
    Preview,
    Execute { ordinal: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BareUrlCliOptions {
    mode: BareUrlCliMode,
    wiki_id: String,
    title: String,
    rev_id: u64,
}

/// The MVP's only enabled wiki; overridable with --wiki.
const BARE_URL_DEFAULT_WIKI: &str = "testwiki";

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
    bare_url: Option<BareUrlCliOptions>,
    verify: Option<VerifyCliOptions>,
    verdict_only: bool,
    /// `--locate-probe --quote <q>`: read a source body from STDIN and report whether the
    /// quote locates (offline, no model) — the deterministic locate-replay tool (SP42#25).
    locate_probe: Option<String>,
}

/// Read-only citation-verification request (PRD-0001). The first cut supports the ad-hoc
/// (claim + source URL) mode; article/revision/index modes await the article parser.
#[derive(Debug, Clone, PartialEq, Eq)]
struct VerifyCliOptions {
    claim: String,
    source_url: String,
    include_metadata: bool,
    /// Emit the full `VerificationOutcome` (finding + per-model votes incl. raw claimed
    /// quotes) as JSON, for the deterministic locate-replay harness (SP42#25).
    debug_votes: bool,
    /// Run the bounded repair turn (SP42#25 layer 3); `--no-repair` turns it off (one fewer
    /// model call per unlocated support vote, for cost control and A/B measurement).
    repair: bool,
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
    if let Some(bare_url) = &options.bare_url {
        return render_bare_url_mode(bare_url, &options, options.format);
    }

    // Offline locate probe: read a source body from STDIN, report whether the quote locates.
    // No model, no fetch — the deterministic locate-replay tool (SP42#25).
    if let Some(quote) = &options.locate_probe {
        let source = read_stdin().map_err(|error| error.to_string())?;
        return run_locate_probe(quote, &source);
    }

    // Read-only citation verification is independent of the dev-queue payload.
    if let Some(verify) = &options.verify {
        return render_verify(verify, options.format, options.verdict_only);
    }

    let input = read_stdin().map_err(|error| error.to_string())?;
    let payload = if input.trim().is_empty() {
        DEV_PREVIEW_SAMPLE_EVENTS
    } else {
        input.as_str()
    };

    let config = parse_default_dev_wiki_config().map_err(|error| error.to_string())?;
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
    build_dev_queue(config, payload).map_err(|error| error.to_string())
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
    let mut bare_url_preview = false;
    let mut bare_url_execute = false;
    let mut bare_url_title = None;
    let mut bare_url_rev = None;
    let mut bare_url_ordinal = None;
    let mut bare_url_wiki = BARE_URL_DEFAULT_WIKI.to_string();
    let mut verify_claim = None;
    let mut verify_source_url = None;
    let mut verify_metadata = false;
    let mut verify_debug_votes = false;
    let mut verify_repair = true;
    let mut verdict_only = false;
    let mut probe_quote = None;
    let mut locate_probe_flag = false;

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
            bare_url_preview: &mut bare_url_preview,
            bare_url_execute: &mut bare_url_execute,
            bare_url_title: &mut bare_url_title,
            bare_url_rev: &mut bare_url_rev,
            bare_url_ordinal: &mut bare_url_ordinal,
            bare_url_wiki: &mut bare_url_wiki,
            verify_claim: &mut verify_claim,
            verify_source_url: &mut verify_source_url,
            verify_metadata: &mut verify_metadata,
            verify_debug_votes: &mut verify_debug_votes,
            verify_repair: &mut verify_repair,
            verdict_only: &mut verdict_only,
            probe_quote: &mut probe_quote,
            locate_probe_flag: &mut locate_probe_flag,
        };
        apply_cli_argument(&arg, &mut args, &mut state)?;
    }

    let bare_url = build_bare_url_options(
        bare_url_preview,
        bare_url_execute,
        bare_url_wiki,
        bare_url_title,
        bare_url_rev,
        bare_url_ordinal,
    )?;
    let verify = build_verify_options(
        verify_claim,
        verify_source_url,
        verify_metadata,
        verify_debug_votes,
        verify_repair,
    )?;
    let locate_probe = if locate_probe_flag {
        Some(probe_quote.ok_or_else(|| "--locate-probe requires --quote".to_string())?)
    } else {
        None
    };

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
        bare_url,
        verify,
        verdict_only,
        locate_probe,
    })
}

/// Assemble the verify options, requiring both the claim and the source URL together.
fn build_verify_options(
    claim: Option<String>,
    source_url: Option<String>,
    include_metadata: bool,
    debug_votes: bool,
    repair: bool,
) -> Result<Option<VerifyCliOptions>, String> {
    match (claim, source_url) {
        (Some(claim), Some(source_url)) => Ok(Some(VerifyCliOptions {
            claim,
            source_url,
            include_metadata,
            debug_votes,
            repair,
        })),
        (None, None) => Ok(None),
        _ => Err("citation verification requires both --claim and --source-url".to_string()),
    }
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
    bare_url_preview: &'a mut bool,
    bare_url_execute: &'a mut bool,
    bare_url_title: &'a mut Option<String>,
    bare_url_rev: &'a mut Option<u64>,
    bare_url_ordinal: &'a mut Option<usize>,
    bare_url_wiki: &'a mut String,
    verify_claim: &'a mut Option<String>,
    verify_source_url: &'a mut Option<String>,
    verify_metadata: &'a mut bool,
    verify_debug_votes: &'a mut bool,
    verify_repair: &'a mut bool,
    verdict_only: &'a mut bool,
    probe_quote: &'a mut Option<String>,
    locate_probe_flag: &'a mut bool,
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
        "--bare-url-preview" => {
            *state.bare_url_preview = true;
        }
        "--bare-url-execute" => {
            *state.bare_url_execute = true;
        }
        "--title" => {
            *state.bare_url_title = Some(next_option_value(args, "--title")?);
        }
        "--rev" => {
            let value = next_option_value(args, "--rev")?;
            *state.bare_url_rev = Some(
                value
                    .parse()
                    .map_err(|_| format!("--rev expects a revision id, got: {value}"))?,
            );
        }
        "--ordinal" => {
            let value = next_option_value(args, "--ordinal")?;
            *state.bare_url_ordinal = Some(
                value
                    .parse()
                    .map_err(|_| format!("--ordinal expects a zero-based index, got: {value}"))?,
            );
        }
        "--wiki" => {
            *state.bare_url_wiki = next_option_value(args, "--wiki")?;
        }
        "--claim" => {
            *state.verify_claim = Some(next_option_value(args, "--claim")?);
        }
        "--source-url" => {
            *state.verify_source_url = Some(next_option_value(args, "--source-url")?);
        }
        "--with-metadata" => {
            *state.verify_metadata = true;
        }
        "--debug-votes" => {
            *state.verify_debug_votes = true;
        }
        "--no-repair" => {
            *state.verify_repair = false;
        }
        "--quote" => {
            *state.probe_quote = Some(next_option_value(args, "--quote")?);
        }
        "--locate-probe" => {
            *state.locate_probe_flag = true;
        }
        "--verdict-only" => {
            *state.verdict_only = true;
        }
        _ => return Err(format!("unsupported argument: {arg}")),
    }

    Ok(())
}

/// Assemble the bare-URL flag-mode options. Both modes need --title and
/// --rev; --bare-url-execute additionally needs --ordinal.
fn build_bare_url_options(
    preview: bool,
    execute: bool,
    wiki_id: String,
    title: Option<String>,
    rev_id: Option<u64>,
    ordinal: Option<usize>,
) -> Result<Option<BareUrlCliOptions>, String> {
    if preview && execute {
        return Err("--bare-url-preview and --bare-url-execute are mutually exclusive".to_string());
    }
    if !preview && !execute {
        return Ok(None);
    }
    let title = title.ok_or_else(|| "bare-url modes require --title".to_string())?;
    let rev_id = rev_id.ok_or_else(|| "bare-url modes require --rev".to_string())?;
    let mode = if execute {
        let ordinal = ordinal.ok_or_else(|| "--bare-url-execute requires --ordinal".to_string())?;
        BareUrlCliMode::Execute { ordinal }
    } else {
        BareUrlCliMode::Preview
    };
    Ok(Some(BareUrlCliOptions {
        mode,
        wiki_id,
        title,
        rev_id,
    }))
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

fn dev_context_options(options: &ContextPreviewOptions) -> DevContextOptions {
    DevContextOptions {
        talk_page_wikitext: options.talk_page.clone(),
        liftwing_probability: options.liftwing_probability,
    }
}

fn dev_workbench_options(options: &WorkbenchOptions) -> DevWorkbenchOptions {
    DevWorkbenchOptions {
        token: options.token.clone(),
        actor: options.actor.clone(),
        note: Some(options.note.clone()),
    }
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

// ----- Read-only citation verification (PRD-0001) -----

/// A reqwest-backed `HttpClient` for the CLI's **source fetch** only. It deliberately holds
/// no inference credential: the model bearer lives solely in [`GenaiModelClient`], so an API
/// key can never leak to a third-party source site through this client.
struct CliHttpClient {
    client: reqwest::Client,
    /// Allow loopback/private source hosts — a dev/test escape hatch for the loopback-serving
    /// benchmark harness (`SP42_FETCH_ALLOW_PRIVATE=1`). Off by default (SP42#34 SSRF floor).
    allow_private: bool,
    /// Source-response body cap, enforced both against `Content-Length` and while streaming
    /// (SP42#43). Production uses [`MAX_SOURCE_BYTES`]; tests inject a small value.
    max_source_bytes: u64,
}

/// Source-response size cap (SP42#43): enforced against `Content-Length` *and* while reading
/// the body stream, so a chunked / no-length response can't return an unbounded body.
const MAX_SOURCE_BYTES: u64 = 8 * 1024 * 1024;

/// How many redirect hops a source fetch will follow. Each hop's destination is re-checked
/// against the SSRF floor before it is fetched (SP42#43).
const MAX_SOURCE_REDIRECTS: u8 = 5;

#[async_trait]
impl HttpClient for CliHttpClient {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, HttpClientError> {
        // Read-only source fetch: GET only.
        let HttpMethod::Get = request.method else {
            return Err(HttpClientError::Transport {
                message: format!("source fetch only allows GET, got {:?}", request.method),
            });
        };
        // Redirects are NOT auto-followed (the client uses `Policy::none()`); we follow them
        // manually so every hop's destination passes the SSRF floor before we connect — a
        // public-looking host must not be able to bounce us to loopback/private infrastructure.
        let mut url = request.url.clone();
        let mut redirects_left = MAX_SOURCE_REDIRECTS;
        let mut response = loop {
            // SSRF floor (SP42#34): refuse a non-http(s) / loopback / private / link-local host
            // — on the initial URL and on every redirect destination — unless the escape hatch
            // is set.
            if !self.allow_private {
                check_fetchable_source_url(&url)
                    .map_err(|message| HttpClientError::Transport { message })?;
            }
            let mut builder = self.client.get(url.clone());
            for (key, value) in &request.headers {
                builder = builder.header(key, value);
            }
            let response = builder
                .send()
                .await
                .map_err(|error| HttpClientError::Transport {
                    message: error.to_string(),
                })?;
            if !response.status().is_redirection() {
                break response;
            }
            let location =
                redirect_location(&response).ok_or_else(|| HttpClientError::Transport {
                    message: format!(
                        "source redirect {} had no usable Location header",
                        response.status().as_u16()
                    ),
                })?;
            let next = url
                .join(&location)
                .map_err(|error| HttpClientError::Transport {
                    message: format!("invalid redirect Location {location:?}: {error}"),
                })?;
            if redirects_left == 0 {
                return Err(HttpClientError::Transport {
                    message: format!("source fetch exceeded {MAX_SOURCE_REDIRECTS} redirects"),
                });
            }
            redirects_left -= 1;
            url = next;
        };
        // Fast-reject an honestly-declared oversized body before reading any of it.
        if response
            .content_length()
            .is_some_and(|len| len > self.max_source_bytes)
        {
            return Err(HttpClientError::Transport {
                message: format!(
                    "source response exceeds {}-byte cap (Content-Length)",
                    self.max_source_bytes
                ),
            });
        }
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (name.as_str().to_lowercase(), value.to_string()))
            })
            .collect();
        // Stream the body so a chunked / no-`Content-Length` response is capped too: stop as
        // soon as the accumulated bytes would exceed the cap.
        let mut body: Vec<u8> = Vec::new();
        while let Some(chunk) =
            response
                .chunk()
                .await
                .map_err(|error| HttpClientError::Transport {
                    message: error.to_string(),
                })?
        {
            if body.len() as u64 + chunk.len() as u64 > self.max_source_bytes {
                return Err(HttpClientError::Transport {
                    message: format!(
                        "source response exceeds {}-byte cap (streamed)",
                        self.max_source_bytes
                    ),
                });
            }
            body.extend_from_slice(&chunk);
        }
        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }
}

/// The `Location` header of a redirect response as a string, if present and valid UTF-8.
fn redirect_location(response: &reqwest::Response) -> Option<String> {
    response
        .headers()
        .get(reqwest::header::LOCATION)?
        .to_str()
        .ok()
        .map(ToString::to_string)
}

/// The transport header a sponsor proxy may gate per-call authorization on (ADR-0006
/// Decision 6). It is authorization metadata only — never part of the model input.
const CAPABILITY_TAG_HEADER: &str = "X-SP42-Capability";

/// A `genai`-backed [`ModelClient`] adapter (ADR-0006 Decision 7). The concrete provider
/// wire format lives here in the CLI shell; feature crates only ever see the neutral
/// boundary, so swapping the adapter touches no domain code.
///
/// Per call it builds a `genai` `ServiceTarget` directly (endpoint + auth + model id),
/// bypassing genai's provider auto-resolution, so the configured OpenAI-compatible endpoint
/// — a local server, a direct provider, or a sponsor proxy — is used verbatim. The
/// `capability_tag`, when set, rides as a transport header a sponsor proxy may gate on;
/// it is never added to the model input.
struct GenaiModelClient {
    client: Client,
    endpoint: ModelEndpointConfig,
}

/// Wall-clock bound on a single model call so a hung inference endpoint can't wedge the CLI
/// (SP42#34). Applied via `tokio::time::timeout` because `genai` pins its own `reqwest`
/// version, so its client can't be built from this crate's `reqwest`.
const MODEL_CALL_TIMEOUT: Duration = Duration::from_mins(1);

impl GenaiModelClient {
    fn new(endpoint: ModelEndpointConfig) -> Self {
        Self {
            client: Client::default(),
            endpoint,
        }
    }
}

#[async_trait]
impl ModelClient for GenaiModelClient {
    async fn complete(
        &self,
        request: &ModelCompletionRequest,
    ) -> Result<ModelCompletion, ModelClientError> {
        let messages = request
            .messages
            .iter()
            .map(|message| match message.role {
                ChatRole::System => GenaiChatMessage::system(message.content.clone()),
                ChatRole::User => GenaiChatMessage::user(message.content.clone()),
                ChatRole::Assistant => GenaiChatMessage::assistant(message.content.clone()),
            })
            .collect::<Vec<_>>();
        let chat_request = ChatRequest::new(messages);

        let target = ServiceTarget {
            endpoint: Endpoint::from_owned(normalize_base_url(&self.endpoint.base_url)),
            auth: genai_auth_for(&self.endpoint),
            model: ModelIden::new(AdapterKind::OpenAI, request.model.model.clone()),
        };
        let options = genai_chat_options(&request.params, self.endpoint.capability_tag.as_deref());

        let response = tokio::time::timeout(
            MODEL_CALL_TIMEOUT,
            self.client.exec_chat(target, chat_request, Some(&options)),
        )
        .await
        .map_err(|_| ModelClientError::Transport {
            message: format!("model request timed out after {MODEL_CALL_TIMEOUT:?}"),
        })?
        .map_err(|error| ModelClientError::Transport {
            message: error.to_string(),
        })?;

        let text = response
            .into_first_text()
            .ok_or_else(|| ModelClientError::InvalidResponse {
                message: "model response contained no text".to_string(),
            })?;
        Ok(ModelCompletion {
            text,
            served_model: None,
        })
    }
}

/// Translate our neutral [`SamplingParams`] into `genai` `ChatOptions`, attaching the
/// capability tag as a transport header when present.
fn genai_chat_options(params: &SamplingParams, capability_tag: Option<&str>) -> ChatOptions {
    let mut options = ChatOptions::default();
    if let Some(temperature) = params.temperature {
        options = options.with_temperature(temperature);
    }
    if let Some(top_p) = params.top_p {
        options = options.with_top_p(top_p);
    }
    if let Some(max_tokens) = params.max_tokens {
        options = options.with_max_tokens(max_tokens);
    }
    if let Some(tag) = capability_tag {
        options =
            options.with_extra_headers([(CAPABILITY_TAG_HEADER.to_string(), tag.to_string())]);
    }
    options
}

/// Build the `genai` `AuthData` for an endpoint, sending **no** `Authorization` header when
/// no token is configured (SP42#44).
///
/// With a (non-empty) token we use the standard `Authorization: Bearer <token>` path. Without
/// one we cannot simply pass an empty key: `genai` 0.6.5's `OpenAI` chat adapter always emits
/// `Authorization: Bearer {key}` built from the `ServiceTarget` key, so an absent token would
/// otherwise become a literal `Authorization: Bearer ` — which breaks local model servers and
/// sponsor proxies that expect a truly tokenless request. `AuthData::None` is not a way out:
/// it errors inside genai's `get_api_key` before any header is built. The only header-less
/// path is `AuthData::RequestOverride`, which replaces the request URL **and** headers
/// wholesale; we therefore rebuild the chat URL ([`chat_completions_url`]) and re-attach the
/// capability tag here, because the override also bypasses genai's URL construction and the
/// `ChatOptions::extra_headers` merge that normally carries it.
fn genai_auth_for(endpoint: &ModelEndpointConfig) -> AuthData {
    if let Some(token) = endpoint
        .auth_token
        .as_deref()
        .filter(|token| !token.is_empty())
    {
        AuthData::from_single(token.to_string())
    } else {
        let mut headers: Vec<(String, String)> = Vec::new();
        if let Some(tag) = endpoint.capability_tag.as_deref() {
            headers.push((CAPABILITY_TAG_HEADER.to_string(), tag.to_string()));
        }
        AuthData::RequestOverride {
            url: chat_completions_url(&endpoint.base_url),
            headers: Headers::from(headers),
        }
    }
}

/// The full OpenAI-compatible chat-completions URL `genai` would POST to, rebuilt here so the
/// tokenless [`genai_auth_for`] `RequestOverride` path (which bypasses genai's URL build)
/// targets the same endpoint. Mirrors genai's `base.join("chat/completions")` over our
/// normalized base (which already carries the single trailing slash that join requires).
fn chat_completions_url(base_url: &str) -> String {
    format!("{}chat/completions", normalize_base_url(base_url))
}

/// Normalize an OpenAI-compatible base URL so `genai`'s adapter can join its
/// `chat/completions` suffix: drop any trailing slash, tolerate a URL that already points at
/// `.../chat/completions` by stripping that segment, then re-append a single trailing slash
/// (reqwest's URL join requires the trailing slash to preserve the base path).
fn normalize_base_url(raw: &str) -> String {
    let trimmed = raw.trim_end_matches('/');
    let base = trimmed.strip_suffix("/chat/completions").unwrap_or(trimmed);
    format!("{base}/")
}

/// Run the offline locate probe: report whether `quote` locates verbatim in `source` using
/// the real [`locate_quote`], plus the guarded-fuzzy axis when exact locate misses, as JSON
/// `{"located": bool, "offset": <n>|null, "fuzzy": bool, "fuzzy_span": <s>|null,
/// "fuzzy_offset": <n>|null}`. No model, no network — lets a harness replay a frozen corpus
/// of model quotes through the actual Rust matcher to measure locate changes exactly (SP42#25).
fn run_locate_probe(quote: &str, source: &str) -> Result<String, String> {
    let offset = locate_quote(quote, source);
    // The fuzzy axis (SP42#25 layer 5) is reported only when exact locate misses, mirroring
    // the gate's exact-first order, so the harness measures layer 5's marginal recovery.
    let fuzzy = if offset.is_some() {
        None
    } else {
        locate_quote_fuzzy(quote, source)
    };
    serde_json::to_string(&serde_json::json!({
        "located": offset.is_some(),
        "offset": offset,
        "fuzzy": fuzzy.is_some(),
        "fuzzy_span": fuzzy.as_ref().map(|hit| hit.span.as_str()),
        "fuzzy_offset": fuzzy.as_ref().map(|hit| hit.offset),
    }))
    .map_err(|error| error.to_string())
}

/// Render a citation verdict in the requested format (or terse verdict-only).
fn render_verify(
    options: &VerifyCliOptions,
    format: OutputFormat,
    verdict_only: bool,
) -> Result<String, String> {
    // genai (and reqwest) require a Tokio reactor, so the verify path runs on its own
    // runtime rather than the futures executor the dev-preview paths use.
    let runtime = tokio::runtime::Runtime::new()
        .map_err(|error| format!("failed to start async runtime: {error}"))?;
    let outcome = runtime.block_on(run_verify(options))?;
    // --debug-votes emits the full outcome (finding + per-model votes incl. the raw claimed
    // quotes) so the offline locate-replay harness can capture model outputs once (SP42#25).
    if options.debug_votes {
        return serde_json::to_string_pretty(&outcome).map_err(|error| error.to_string());
    }
    let finding = &outcome.finding;
    if verdict_only {
        return Ok(finding.verdict.as_wire().to_string());
    }
    match format {
        OutputFormat::Json => serde_json::to_string_pretty(finding).map_err(|e| e.to_string()),
        OutputFormat::Markdown => Ok(render_markdown_section(
            "Citation verdict",
            &render_verify_text(finding),
        )),
        OutputFormat::Text => Ok(render_verify_text(finding)),
    }
}

/// Execute one ad-hoc (claim, source URL) verification against the configured panel.
///
/// Reads the inference endpoint, model panel, and (optional) bearer token from the
/// environment so no secret is hard-coded; performs only read-only GET/POST requests.
async fn run_verify(options: &VerifyCliOptions) -> Result<VerificationOutcome, String> {
    let base_url = std::env::var("SP42_INFERENCE_URL").map_err(|_| {
        "set SP42_INFERENCE_URL to the model's OpenAI-compatible base URL".to_string()
    })?;
    let models = std::env::var("SP42_INFERENCE_MODELS").map_err(|_| {
        "set SP42_INFERENCE_MODELS to a comma-separated list of model ids".to_string()
    })?;
    let provider =
        std::env::var("SP42_INFERENCE_PROVIDER").unwrap_or_else(|_| "configured".to_string());
    let panel: Vec<ModelRef> = models
        .split(',')
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(|model| ModelRef::new(provider.clone(), model, model))
        .collect();
    if panel.is_empty() {
        return Err("SP42_INFERENCE_MODELS is empty".to_string());
    }
    let auth_token = std::env::var("SP42_INFERENCE_TOKEN").ok();
    let capability_tag = std::env::var("SP42_INFERENCE_CAPABILITY").ok();
    let mode = parse_endpoint_mode(std::env::var("SP42_INFERENCE_MODE").ok().as_deref())?;

    let source_url = options
        .source_url
        .parse()
        .map_err(|_| format!("invalid --source-url: {}", options.source_url))?;
    let request = CitationVerificationRequest {
        wiki_id: String::new(),
        rev_id: 0,
        title: String::new(),
        claim: options.claim.clone(),
        source_url,
    };

    // The source-fetch client carries no inference credential; the bearer is held only by
    // the model adapter, so it can never reach a third-party source host.
    let fetch_client = CliHttpClient {
        client: reqwest::Client::builder()
            .user_agent(sp42_core::branding::USER_AGENT)
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            // No auto-follow: `CliHttpClient::execute` follows redirects manually so each hop
            // is re-checked against the SSRF floor before we connect (SP42#43).
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| format!("http client failed to build: {error}"))?,
        allow_private: std::env::var("SP42_FETCH_ALLOW_PRIVATE").is_ok_and(|value| value == "1"),
        max_source_bytes: MAX_SOURCE_BYTES,
    };
    let model_client = GenaiModelClient::new(ModelEndpointConfig {
        mode,
        base_url,
        auth_token,
        capability_tag,
    });

    let verify_options = CoreVerifyOptions {
        include_metadata: options.include_metadata,
        concurrency: 3,
        repair_turn: options.repair,
        ..Default::default()
    };
    let outcome = verify_citation_use_site(
        &fetch_client,
        &model_client,
        &SystemClock,
        &panel,
        &request,
        0,
        verify_options,
    )
    .await
    .map_err(|error| error.to_string())?;
    Ok(outcome)
}

/// Parse the optional `SP42_INFERENCE_MODE` env value. Defaults to `local`; the mode is
/// recorded on the endpoint config as advisory metadata (the adapter sends the bearer
/// whenever a token is present, regardless of mode, in this CLI MVP).
fn parse_endpoint_mode(value: Option<&str>) -> Result<EndpointMode, String> {
    match value {
        None | Some("local") => Ok(EndpointMode::Local),
        Some("direct") => Ok(EndpointMode::Direct),
        Some("sponsor_proxy" | "sponsor-proxy") => Ok(EndpointMode::SponsorProxy),
        Some(other) => Err(format!("unsupported SP42_INFERENCE_MODE: {other}")),
    }
}

/// Human-readable verdict block.
fn render_verify_text(finding: &CitationFinding) -> String {
    let mut lines = vec![
        format!("verdict: {}", finding.verdict.as_wire()),
        format!("source: {}", finding.provenance.url),
    ];
    match finding.grounding_status {
        GroundingStatus::Located => {
            lines.push("verification: quote located in source".to_string());
        }
        GroundingStatus::LocatedFuzzy => lines.push(
            "verification: passage located by guarded fuzzy match (shown text is the source's own) — please confirm"
                .to_string(),
        ),
        GroundingStatus::Unlocated => lines.push(
            "verification: UNVERIFIED — model claims support but its quote was not found in the source"
                .to_string(),
        ),
        GroundingStatus::NotApplicable => {}
    }
    if finding.agreement.is_meaningful() {
        lines.push(format!(
            "agreement: {}/{} models",
            finding.agreement.winner_votes, finding.agreement.panel_size
        ));
    }
    match &finding.passage {
        Some(passage) => lines.push(format!("supporting passage: \"{}\"", passage.quote)),
        None => lines.push("supporting passage: (none located)".to_string()),
    }
    lines.push(format!(
        "source content hash: {}",
        finding.provenance.content_hash
    ));
    lines.join("\n")
}

#[cfg(test)]
mod verify_tests {
    use sp42_core::{
        CitationFinding, CitationFindingKind, CitationVerdict, GroundingAssertion, GroundingStatus,
        LocatedPassage, PanelAgreement, SourceProvenance, SupportLevel,
    };

    use super::{
        EndpointMode, ModelEndpointConfig, SamplingParams, VerifyCliOptions, genai_auth_for,
        genai_chat_options, normalize_base_url, parse_endpoint_mode, parse_options,
        render_verify_text, run_locate_probe,
    };
    use genai::resolver::AuthData;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn normalize_base_url_ensures_single_trailing_slash() {
        assert_eq!(
            normalize_base_url("https://openrouter.ai/api/v1"),
            "https://openrouter.ai/api/v1/"
        );
        assert_eq!(
            normalize_base_url("https://openrouter.ai/api/v1/"),
            "https://openrouter.ai/api/v1/"
        );
    }

    #[test]
    fn normalize_base_url_strips_a_chat_completions_suffix() {
        assert_eq!(
            normalize_base_url("https://openrouter.ai/api/v1/chat/completions"),
            "https://openrouter.ai/api/v1/"
        );
        assert_eq!(
            normalize_base_url("http://localhost:8080/v1/chat/completions/"),
            "http://localhost:8080/v1/"
        );
    }

    #[test]
    fn parse_endpoint_mode_maps_known_values() {
        assert_eq!(parse_endpoint_mode(None), Ok(EndpointMode::Local));
        assert_eq!(parse_endpoint_mode(Some("local")), Ok(EndpointMode::Local));
        assert_eq!(
            parse_endpoint_mode(Some("direct")),
            Ok(EndpointMode::Direct)
        );
        assert_eq!(
            parse_endpoint_mode(Some("sponsor_proxy")),
            Ok(EndpointMode::SponsorProxy)
        );
        assert_eq!(
            parse_endpoint_mode(Some("sponsor-proxy")),
            Ok(EndpointMode::SponsorProxy)
        );
        assert!(parse_endpoint_mode(Some("bogus")).is_err());
    }

    #[test]
    fn genai_chat_options_attaches_capability_tag_header() {
        let params = SamplingParams::deterministic();
        let options = genai_chat_options(&params, Some("citation-verify"));
        let headers = options.extra_headers.expect("capability header present");
        assert!(headers.iter().any(
            |(name, value)| name.as_str() == super::CAPABILITY_TAG_HEADER
                && value.as_str() == "citation-verify"
        ));
    }

    #[test]
    fn genai_chat_options_omits_headers_without_a_capability_tag() {
        let options = genai_chat_options(&SamplingParams::deterministic(), None);
        assert!(options.extra_headers.is_none());
    }

    fn endpoint_config(
        auth_token: Option<&str>,
        capability_tag: Option<&str>,
    ) -> ModelEndpointConfig {
        ModelEndpointConfig {
            mode: EndpointMode::Local,
            base_url: "http://localhost:11434/v1".to_string(),
            auth_token: auth_token.map(ToString::to_string),
            capability_tag: capability_tag.map(ToString::to_string),
        }
    }

    #[test]
    fn genai_auth_for_uses_bearer_key_when_token_present() {
        let auth = genai_auth_for(&endpoint_config(Some("secret-token"), None));
        // A present token must keep the standard `Authorization: Bearer <token>` path.
        assert!(matches!(auth, AuthData::Key(_)));
        assert_eq!(
            auth.single_key_value().ok().as_deref(),
            Some("secret-token")
        );
    }

    #[test]
    fn genai_auth_for_sends_no_authorization_header_when_token_absent() {
        // genai 0.6.5 always emits `Authorization: Bearer {key}` from a ServiceTarget key, so
        // a tokenless endpoint must use `RequestOverride` to omit the header entirely (#44).
        let auth = genai_auth_for(&endpoint_config(None, None));
        let AuthData::RequestOverride { url, headers } = auth else {
            panic!("tokenless endpoint must use RequestOverride, got {auth:?}");
        };
        assert_eq!(url, "http://localhost:11434/v1/chat/completions");
        assert!(
            !headers
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("authorization")),
            "no Authorization header without a token"
        );
    }

    #[test]
    fn genai_auth_for_treats_an_empty_token_as_absent() {
        // `SP42_INFERENCE_TOKEN=` (set but empty) must not become `Authorization: Bearer `.
        let auth = genai_auth_for(&endpoint_config(Some(""), None));
        assert!(matches!(auth, AuthData::RequestOverride { .. }));
    }

    #[test]
    fn genai_auth_for_carries_capability_tag_in_override_headers() {
        // RequestOverride replaces all headers, so the capability tag must ride here, not via
        // the (discarded) ChatOptions extra_headers.
        let auth = genai_auth_for(&endpoint_config(None, Some("citation-verify")));
        let AuthData::RequestOverride { headers, .. } = auth else {
            panic!("expected RequestOverride");
        };
        assert!(headers.iter().any(
            |(name, value)| name.as_str() == super::CAPABILITY_TAG_HEADER
                && value.as_str() == "citation-verify"
        ));
    }

    #[test]
    fn parses_ad_hoc_verify_flags() {
        let options = parse_options(args(&[
            "--claim",
            "the bridge opened in 1998",
            "--source-url",
            "https://example.com/bridge",
            "--with-metadata",
        ]))
        .expect("parses");
        let verify = options.verify.expect("verify present");
        assert_eq!(
            verify,
            VerifyCliOptions {
                claim: "the bridge opened in 1998".to_string(),
                source_url: "https://example.com/bridge".to_string(),
                include_metadata: true,
                debug_votes: false,
                repair: true,
            }
        );
        assert!(!options.verdict_only);
    }

    #[test]
    fn no_repair_flag_disables_the_repair_turn() {
        let options = parse_options(args(&[
            "--claim",
            "c",
            "--source-url",
            "https://example.com",
            "--no-repair",
        ]))
        .expect("parses");
        assert!(!options.verify.expect("verify present").repair);
    }

    #[test]
    fn debug_votes_flag_is_recognized() {
        let options = parse_options(args(&[
            "--claim",
            "c",
            "--source-url",
            "https://example.com",
            "--debug-votes",
        ]))
        .expect("parses");
        assert!(options.verify.expect("verify present").debug_votes);
    }

    #[test]
    fn verdict_only_flag_is_recognized() {
        let options = parse_options(args(&[
            "--claim",
            "c",
            "--source-url",
            "https://example.com",
            "--verdict-only",
        ]))
        .expect("parses");
        assert!(options.verdict_only);
    }

    #[test]
    fn verify_requires_both_claim_and_source_url() {
        assert!(parse_options(args(&["--claim", "only a claim"])).is_err());
        assert!(parse_options(args(&["--source-url", "https://example.com"])).is_err());
    }

    #[test]
    fn locate_probe_flags_parse_and_carry_the_quote() {
        let options =
            parse_options(args(&["--locate-probe", "--quote", "the Nobel Prize"])).expect("parses");
        assert_eq!(options.locate_probe.as_deref(), Some("the Nobel Prize"));
    }

    #[test]
    fn locate_probe_requires_a_quote() {
        assert!(parse_options(args(&["--locate-probe"])).is_err());
    }

    #[test]
    fn run_locate_probe_reports_found_and_not_found() {
        let hit = run_locate_probe("Nobel Prize", "won the Nobel Prize").expect("ok");
        assert!(hit.contains("\"located\":true"));
        let miss = run_locate_probe("absent span", "a completely different text").expect("ok");
        assert!(miss.contains("\"located\":false"));
    }

    #[test]
    fn run_locate_probe_reports_the_fuzzy_fallback() {
        // Exact locate fails (one reworded token), the guarded fuzzy path recovers: the
        // probe reports both axes so the offline harness can measure layer 5 (SP42#25).
        let source = "In 1985 the Acme Corporation was established in Springfield by a group \
                      of local investors led by John Smith.";
        let quote = "the Acme Corporation was founded in Springfield by a group of local investors";
        let report = run_locate_probe(quote, source).expect("ok");
        assert!(report.contains("\"located\":false"));
        assert!(report.contains("\"fuzzy\":true"));
        assert!(report.contains("established in Springfield"));
        // A fabricated quote is neither located nor fuzzy.
        let miss = run_locate_probe(
            "the museum acquired seventeen paintings from the private collection",
            source,
        )
        .expect("ok");
        assert!(miss.contains("\"located\":false"));
        assert!(miss.contains("\"fuzzy\":false"));
    }

    fn fixture_finding() -> CitationFinding {
        CitationFinding {
            kind: CitationFindingKind::CitationVerdict,
            verdict: CitationVerdict::Judged(SupportLevel::Supported),
            grounding_status: GroundingStatus::Located,
            agreement: PanelAgreement::new(3, 2),
            passage: Some(LocatedPassage {
                quote: "opened in 1998".to_string(),
                offset: 4,
            }),
            provenance: SourceProvenance {
                url: "https://example.com/bridge".parse().expect("url"),
                content_hash: "abc123".to_string(),
                fetched_at: 1,
                http_status: Some(200),
            },
            grounding: GroundingAssertion::LocatedQuote {
                quote: "opened in 1998".to_string(),
                source_hash: "abc123".to_string(),
                offset: 4,
            },
            use_site_ordinal: 0,
            schema_version: 1,
        }
    }

    #[test]
    fn renders_human_verdict_block() {
        let text = render_verify_text(&fixture_finding());
        assert!(text.contains("verdict: supported"));
        assert!(text.contains("agreement: 2/3 models"));
        assert!(text.contains("opened in 1998"));
        assert!(text.contains("https://example.com/bridge"));
    }
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

/// One executed bare-URL repair, for rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
struct BareUrlExecuteReport {
    bridge_base_url: String,
    wiki_id: String,
    title: String,
    rev_id: u64,
    ordinal: usize,
    proposal: sp42_core::BareUrlProposal,
    response: sp42_core::BareUrlApplyResponse,
}

fn bare_url_proposal_lines(response: &sp42_core::BareUrlProposalsResponse) -> Vec<String> {
    response
        .proposals
        .iter()
        .map(|proposal| {
            format!(
                "#{} url={} replacement={}",
                proposal.locator.ordinal, proposal.url, proposal.replacement_wikitext
            )
        })
        .collect()
}

fn bare_url_declined_lines(response: &sp42_core::BareUrlProposalsResponse) -> Vec<String> {
    response
        .declined
        .iter()
        .map(|declined| {
            format!(
                "#{} url={} declined={}",
                declined.ordinal,
                declined.url,
                declined.reason.code()
            )
        })
        .collect()
}

fn render_bare_url_proposals(
    bare_url: &BareUrlCliOptions,
    bridge_base_url: &str,
    response: &sp42_core::BareUrlProposalsResponse,
    format: OutputFormat,
) -> Result<String, String> {
    match format {
        OutputFormat::Text => {
            let mut lines = vec![format!(
                "bare-url preview bridge={bridge_base_url} wiki={} title=\"{}\" rev_id={} proposals={} declined={}",
                bare_url.wiki_id,
                bare_url.title,
                bare_url.rev_id,
                response.proposals.len(),
                response.declined.len(),
            )];
            lines.extend(bare_url_proposal_lines(response));
            lines.extend(bare_url_declined_lines(response));
            Ok(lines.join("\n"))
        }
        OutputFormat::Markdown => {
            let proposals = bare_url_proposal_lines(response);
            let declined = bare_url_declined_lines(response);
            Ok([
                render_markdown_section(
                    "Bare-URL proposals",
                    &if proposals.is_empty() {
                        "(none)".to_string()
                    } else {
                        proposals.join("\n")
                    },
                ),
                render_markdown_section(
                    "Declined references",
                    &if declined.is_empty() {
                        "(none)".to_string()
                    } else {
                        declined.join("\n")
                    },
                ),
            ]
            .join("\n\n"))
        }
        OutputFormat::Json => serde_json::to_string_pretty(&serde_json::json!({
            "bridge_base_url": bridge_base_url,
            "wiki_id": bare_url.wiki_id,
            "title": bare_url.title,
            "rev_id": bare_url.rev_id,
            "proposals": response.proposals,
            "declined": response.declined,
        }))
        .map_err(|error| error.to_string()),
    }
}

fn render_bare_url_execute(
    report: &BareUrlExecuteReport,
    format: OutputFormat,
) -> Result<String, String> {
    let status = report
        .response
        .http_status
        .map_or_else(|| "none".to_string(), |status| status.to_string());
    match format {
        OutputFormat::Text => Ok([
            format!(
                "bare-url execute bridge={} wiki={} title=\"{}\" rev_id={} ordinal={}",
                report.bridge_base_url, report.wiki_id, report.title, report.rev_id, report.ordinal
            ),
            format!(
                "proposal url={} replacement={}",
                report.proposal.url, report.proposal.replacement_wikitext
            ),
            format!(
                "apply accepted={} http_status={status} message={}",
                report.response.accepted,
                report.response.message.as_deref().unwrap_or("none"),
            ),
        ]
        .join("\n")),
        OutputFormat::Markdown => Ok([
            render_markdown_section(
                "Bare-URL execute",
                &format!(
                    "bridge={} wiki={} title=\"{}\" rev_id={} ordinal={}",
                    report.bridge_base_url,
                    report.wiki_id,
                    report.title,
                    report.rev_id,
                    report.ordinal
                ),
            ),
            render_markdown_section(
                "Proposal",
                &format!(
                    "url={} replacement={}",
                    report.proposal.url, report.proposal.replacement_wikitext
                ),
            ),
            render_markdown_section(
                "Apply result",
                &format!(
                    "accepted={} http_status={status} message={}",
                    report.response.accepted,
                    report.response.message.as_deref().unwrap_or("none"),
                ),
            ),
        ]
        .join("\n\n")),
        OutputFormat::Json => serde_json::to_string_pretty(&serde_json::json!({
            "bridge_base_url": report.bridge_base_url,
            "wiki_id": report.wiki_id,
            "title": report.title,
            "rev_id": report.rev_id,
            "ordinal": report.ordinal,
            "proposal": report.proposal,
            "response": report.response,
        }))
        .map_err(|error| error.to_string()),
    }
}

/// Run the selected bare-URL flag-mode against the bridge and render it.
fn render_bare_url_mode(
    bare_url: &BareUrlCliOptions,
    options: &CliOptions,
    format: OutputFormat,
) -> Result<String, String> {
    match bare_url.mode {
        BareUrlCliMode::Preview => {
            let response = block_on(fetch_bare_url_proposals(
                &options.bridge_base_url,
                &bare_url_proposals_request(bare_url),
            ))?;
            render_bare_url_proposals(bare_url, &options.bridge_base_url, &response, format)
        }
        BareUrlCliMode::Execute { ordinal } => {
            let note = action_note(options);
            let report = block_on(execute_bare_url_via_bridge(
                &options.bridge_base_url,
                bare_url,
                ordinal,
                note.as_deref(),
            ))?;
            render_bare_url_execute(&report, format)
        }
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
    let workbench = build_dev_workbench(config, item, &dev_workbench_options(options))
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
    let preview = build_dev_context_preview(config, item, &dev_context_options(options))
        .map_err(|error| error.to_string())?;

    match format {
        OutputFormat::Text => Ok(render_context_preview_text(
            &preview.selected,
            &preview.recentchanges_request,
            &preview.liftwing_request,
            &preview.context,
            &preview.contextual_score,
            options,
        )),
        OutputFormat::Markdown => Ok(render_context_preview_markdown(
            &preview.selected,
            &preview.recentchanges_request,
            &preview.liftwing_request,
            &preview.context,
            &preview.contextual_score,
            options,
        )),
        OutputFormat::Json => render_context_preview_json(
            &preview.selected,
            &preview.recentchanges_request,
            &preview.liftwing_request,
            &preview.context,
            &preview.contextual_score,
            options,
        ),
    }
}

fn render_context_preview_text(
    item: &QueuedEdit,
    recentchanges_request: &HttpRequest,
    liftwing_request: &HttpRequest,
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
    recentchanges_request: &HttpRequest,
    liftwing_request: &HttpRequest,
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
    recentchanges_request: &HttpRequest,
    liftwing_request: &HttpRequest,
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
    let preview = block_on(build_dev_stream_preview(config, payload, "fixture"))
        .map_err(|error| error.to_string())?;
    let edits = preview.edits;
    let status = preview.status;

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
    let preview = block_on(build_dev_backlog_preview(config)).map_err(|error| error.to_string())?;
    let request = preview.request;
    let batch = preview.batch;
    let status = preview.status;

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
    let preview =
        build_dev_coordination_preview(DEV_PREVIEW_WIKI_ID).map_err(|error| error.to_string())?;
    let summary = preview.summary;
    let roundtrips = preview.roundtrips;

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
    let scoring_context = options
        .context_preview
        .as_ref()
        .map(|context| build_dev_context(&dev_context_options(context)));
    let review_workbench = match (selected, options.workbench.as_ref()) {
        (Some(item), Some(workbench)) => Some(
            build_dev_workbench(config, item, &dev_workbench_options(workbench))
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
    let scoring_context = options
        .context_preview
        .as_ref()
        .map(|context| build_dev_context(&dev_context_options(context)));
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
        (Some(workbench), Some(item)) => {
            build_dev_workbench(config, item, &dev_workbench_options(workbench))
                .map(Some)
                .map_err(|error| error.to_string())
        }
        _ => Ok(None),
    }
}

#[derive(Debug, Clone, Copy)]
struct SessionDigestArtifacts<'a> {
    shell_state: &'a sp42_reporting::ShellStateModel,
    scenario: &'a sp42_reporting::PatrolScenarioReport,
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
    let requests =
        build_dev_action_requests(selected, note.as_deref()).map_err(|error| error.to_string())?;

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
    let requests =
        build_dev_action_requests(selected, note.as_deref()).map_err(|error| error.to_string())?;
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
        ("healthz", route_contracts::HEALTHZ_PATH.to_string()),
        (
            "operator_report",
            route_contracts::OPERATOR_REPORT_PATH.to_string(),
        ),
        (
            "operator_readiness",
            route_contracts::OPERATOR_READINESS_PATH.to_string(),
        ),
        (
            "operator_live",
            route_contracts::operator_live_path_with_query("frwiki", "limit=1"),
        ),
        (
            "operator_runtime",
            route_contracts::operator_runtime_path("frwiki"),
        ),
        (
            "bootstrap_status",
            route_contracts::DEV_AUTH_BOOTSTRAP_STATUS_PATH.to_string(),
        ),
        (
            "capabilities_frwiki",
            route_contracts::dev_auth_capabilities_path("frwiki"),
        ),
        (
            "action_status",
            route_contracts::ACTION_STATUS_PATH.to_string(),
        ),
        (
            "action_history",
            route_contracts::dev_action_history_path_with_limit(1),
        ),
    ];

    let mut report = BTreeMap::new();
    for (label, path) in endpoints {
        let value = fetch_server_json(&client, base_url, &path).await?;
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
    let request_url = format!("{base_url}{}", route_contracts::DEV_ACTION_EXECUTE_PATH);
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

fn bare_url_proposals_request(bare_url: &BareUrlCliOptions) -> sp42_core::BareUrlProposalsRequest {
    sp42_core::BareUrlProposalsRequest {
        wiki_id: bare_url.wiki_id.clone(),
        title: bare_url.title.clone(),
        rev_id: bare_url.rev_id,
    }
}

async fn fetch_bare_url_proposals(
    base_url: &str,
    request: &sp42_core::BareUrlProposalsRequest,
) -> Result<sp42_core::BareUrlProposalsResponse, String> {
    let client = reqwest::Client::builder()
        .user_agent(sp42_core::branding::USER_AGENT)
        .build()
        .map_err(|error| format!("bridge client failed to build: {error}"))?;
    let request_url = format!(
        "{base_url}{}",
        route_contracts::DEV_CITATION_BARE_URL_PROPOSALS_PATH
    );
    let response = client
        .post(&request_url)
        .json(request)
        .send()
        .await
        .map_err(|error| format!("bare-url proposals request failed: {error}"))?
        .error_for_status()
        .map_err(|error| format!("bare-url proposals request failed: {error}"))?;
    response
        .json::<sp42_core::BareUrlProposalsResponse>()
        .await
        .map_err(|error| format!("bare-url proposals payload was invalid: {error}"))
}

/// Select a proposal from the response by ordinal. Returns the proposal if
/// found, or an error message listing declined entries.
fn select_bare_url_proposal(
    proposals: &sp42_core::BareUrlProposalsResponse,
    ordinal: usize,
) -> Result<sp42_core::BareUrlProposal, String> {
    proposals
        .proposals
        .iter()
        .find(|proposal| proposal.locator.ordinal == ordinal)
        .cloned()
        .ok_or_else(|| {
            let declined = proposals
                .declined
                .iter()
                .map(|entry| format!("#{} {} ({})", entry.ordinal, entry.url, entry.reason.code()))
                .collect::<Vec<_>>()
                .join(", ");
            format!("no bare-URL proposal for ordinal {ordinal}; declined: [{declined}]")
        })
}

/// Re-fetch proposals, select ordinal K, and replay it against the apply
/// route. The fresh fetch re-anchors the locator, narrowing the TOCTOU
/// window; the server's anti-drift re-check and `baserevid` guard close it.
/// Auth rides the bridge session (ADR-0002): bootstrap, then send the
/// session cookie *and* the bootstrap-reported CSRF token.
async fn execute_bare_url_via_bridge(
    bridge_base_url: &str,
    bare_url_options: &BareUrlCliOptions,
    ordinal: usize,
    note: Option<&str>,
) -> Result<BareUrlExecuteReport, String> {
    let proposals = fetch_bare_url_proposals(
        bridge_base_url,
        &bare_url_proposals_request(bare_url_options),
    )
    .await?;
    let proposal = select_bare_url_proposal(&proposals, ordinal)?;

    let client = reqwest::Client::builder()
        .user_agent(sp42_core::branding::USER_AGENT)
        .build()
        .map_err(|error| format!("bridge client failed to build: {error}"))?;
    let bootstrap_request =
        build_dev_auth_bootstrap_request(bridge_base_url, &DevAuthBootstrapRequest::default())
            .map_err(|error| error.to_string())?;
    let bootstrap_response = execute_local_http_request(&client, bootstrap_request).await?;
    let bootstrap =
        parse_dev_auth_status(&bootstrap_response.body).map_err(|error| error.to_string())?;
    if !bootstrap.authenticated {
        return Err("bridge bootstrap did not produce an authenticated session".to_string());
    }
    let session_cookie = session_cookie_from_headers(&bootstrap_response.headers)
        .ok_or_else(|| "bridge bootstrap did not set a session cookie".to_string())?;
    let csrf_token = bootstrap
        .csrf_token
        .clone()
        .ok_or_else(|| "bridge bootstrap did not return a CSRF token".to_string())?;

    let apply_request = sp42_core::BareUrlApplyRequest {
        wiki_id: bare_url_options.wiki_id.clone(),
        title: bare_url_options.title.clone(),
        rev_id: bare_url_options.rev_id,
        locator: proposal.locator.clone(),
        replacement_wikitext: proposal.replacement_wikitext.clone(),
        summary: note.map(ToString::to_string),
    };
    let request_url = format!(
        "{bridge_base_url}{}",
        route_contracts::DEV_CITATION_BARE_URL_APPLY_PATH
    );
    let response = client
        .post(&request_url)
        .header(COOKIE, session_cookie.as_str())
        .header(route_contracts::CSRF_HEADER_NAME, csrf_token.as_str())
        .json(&apply_request)
        .send()
        .await
        .map_err(|error| format!("bare-url apply request failed: {error}"))?
        .error_for_status()
        .map_err(|error| format!("bare-url apply request failed: {error}"))?;
    let apply = response
        .json::<sp42_core::BareUrlApplyResponse>()
        .await
        .map_err(|error| format!("bare-url apply payload was invalid: {error}"))?;

    Ok(BareUrlExecuteReport {
        bridge_base_url: bridge_base_url.to_string(),
        wiki_id: bare_url_options.wiki_id.clone(),
        title: bare_url_options.title.clone(),
        rev_id: bare_url_options.rev_id,
        ordinal,
        proposal,
        response: apply,
    })
}

async fn execute_local_http_request(
    client: &reqwest::Client,
    request: HttpRequest,
) -> Result<HttpResponse, String> {
    let mut builder = match request.method {
        HttpMethod::Get => client.get(request.url),
        HttpMethod::Post => client.post(request.url),
        HttpMethod::Put => client.put(request.url),
        HttpMethod::Patch => client.patch(request.url),
        HttpMethod::Delete => client.delete(request.url),
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

    Ok(HttpResponse {
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::collections::BTreeSet;

    use serde_json::Value;

    use super::{
        BareUrlCliMode, BareUrlCliOptions, BareUrlExecuteReport, CliOptions, ContextPreviewOptions,
        LOCAL_SERVER_BASE_URL, OutputFormat, PreviewMode, ShellMode, WorkbenchOptions,
        parse_options, render_action_preview, render_backlog_preview, render_bare_url_execute,
        render_bare_url_proposals, render_context_preview, render_coordination_preview,
        render_parity_report, render_queue, render_scenario_report, render_session_digest,
        render_stream_preview, render_workbench, select_bare_url_proposal, server_report_lines,
    };
    use serde_json::json;
    use sp42_devtools::{
        DEV_PREVIEW_SAMPLE_EVENTS, build_dev_queue, parse_default_dev_wiki_config,
    };

    fn fixture_config() -> sp42_core::WikiConfig {
        parse_default_dev_wiki_config().expect("config should parse")
    }

    fn fixture_queue(config: &sp42_core::WikiConfig) -> Vec<sp42_core::QueuedEdit> {
        build_dev_queue(config, DEV_PREVIEW_SAMPLE_EVENTS).expect("queue should build")
    }

    #[test]
    fn renders_ranked_queue_lines() {
        let config = fixture_config();
        let ranked = fixture_queue(&config);

        let summary = render_queue(&ranked, OutputFormat::Text).expect("text render should work");

        assert!(summary.contains("#1"));
        assert!(summary.contains("wiki=frwiki"));
        assert!(summary.contains("rev_id=123459"));
    }

    #[test]
    fn renders_ranked_queue_as_json() {
        let config = fixture_config();
        let ranked = fixture_queue(&config);

        let summary = render_queue(&ranked, OutputFormat::Json).expect("json render should work");

        assert!(summary.contains("\"wiki_id\": \"frwiki\""));
        assert!(summary.contains("\"rev_id\": 123459"));
    }

    #[test]
    fn renders_ranked_queue_as_markdown() {
        let config = fixture_config();
        let ranked = fixture_queue(&config);

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
        let config = fixture_config();
        let ranked = fixture_queue(&config);

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
                bare_url: None,
                verify: None,
                verdict_only: false,
                locate_probe: None,
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
        let config = fixture_config();
        let ranked = fixture_queue(&config);

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
                bare_url: None,
                verify: None,
                verdict_only: false,
                locate_probe: None,
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
        let config = fixture_config();
        let ranked = fixture_queue(&config);

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
                bare_url: None,
                verify: None,
                verdict_only: false,
                locate_probe: None,
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
        let config = fixture_config();
        let ranked = fixture_queue(&config);
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
            bare_url: None,
            verify: None,
            verdict_only: false,
            locate_probe: None,
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
        let config = fixture_config();
        let ranked = fixture_queue(&config);
        let options = CliOptions {
            format: OutputFormat::Text,
            workbench: None,
            context_preview: None,
            preview_modes: BTreeSet::new(),
            shell_mode: Some(ShellMode::ActionPreview),
            action_note: Some("inspect".to_string()),
            action_kind: sp42_core::SessionActionKind::Patrol,
            bridge_base_url: "http://127.0.0.1:8788".to_string(),
            bare_url: None,
            verify: None,
            verdict_only: false,
            locate_probe: None,
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
        let config = fixture_config();
        let ranked = fixture_queue(&config);

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
        let config = fixture_config();
        let ranked = fixture_queue(&config);

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
        let config = fixture_config();

        let summary = render_stream_preview(&config, DEV_PREVIEW_SAMPLE_EVENTS, OutputFormat::Text)
            .expect("stream mode should render");

        assert!(summary.contains("stream checkpoint_key="));
        assert!(summary.contains("stream delivered="));
        assert!(summary.contains("stream rev=123456"));
    }

    #[test]
    fn renders_backlog_preview() {
        let config = fixture_config();

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
        let config = fixture_config();
        let ranked = fixture_queue(&config);

        let summary = render_parity_report(
            &config,
            &ranked,
            DEV_PREVIEW_SAMPLE_EVENTS,
            OutputFormat::Text,
        )
        .expect("parity report should render");

        assert!(summary.contains("operator parity report wiki=frwiki"));
        assert!(summary.contains("backlog report"));
        assert!(summary.contains("coordination report wiki=frwiki"));
        assert!(summary.contains("action workbench rev="));
        assert!(summary.contains("stream delivered="));
    }

    #[test]
    fn renders_parity_report_as_markdown() {
        let config = fixture_config();
        let ranked = fixture_queue(&config);

        let summary = render_parity_report(
            &config,
            &ranked,
            DEV_PREVIEW_SAMPLE_EVENTS,
            OutputFormat::Markdown,
        )
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

    fn parse(arguments: &[&str]) -> Result<CliOptions, String> {
        parse_options(arguments.iter().map(ToString::to_string))
    }

    #[test]
    fn parses_bare_url_preview_flags() {
        let options = parse(&["--bare-url-preview", "--title", "Sandbox", "--rev", "123"])
            .expect("preview flags should parse");
        let bare_url = options.bare_url.expect("bare-url mode should be selected");
        assert_eq!(bare_url.mode, BareUrlCliMode::Preview);
        assert_eq!(bare_url.wiki_id, "testwiki");
        assert_eq!(bare_url.title, "Sandbox");
        assert_eq!(bare_url.rev_id, 123);
    }

    #[test]
    fn parses_bare_url_execute_flags_with_wiki_override() {
        let options = parse(&[
            "--bare-url-execute",
            "--title",
            "Sandbox",
            "--rev",
            "123",
            "--ordinal",
            "2",
            "--wiki",
            "frwiki",
        ])
        .expect("execute flags should parse");
        let bare_url = options.bare_url.expect("bare-url mode should be selected");
        assert_eq!(bare_url.mode, BareUrlCliMode::Execute { ordinal: 2 });
        assert_eq!(bare_url.wiki_id, "frwiki");
    }

    #[test]
    fn bare_url_modes_are_mutually_exclusive_and_validated() {
        assert!(
            parse(&[
                "--bare-url-preview",
                "--bare-url-execute",
                "--title",
                "T",
                "--rev",
                "1"
            ])
            .is_err()
        );
        assert!(
            parse(&["--bare-url-preview", "--rev", "1"]).is_err(),
            "missing --title"
        );
        assert!(
            parse(&["--bare-url-preview", "--title", "T"]).is_err(),
            "missing --rev"
        );
        assert!(
            parse(&["--bare-url-execute", "--title", "T", "--rev", "1"]).is_err(),
            "execute requires --ordinal"
        );
        assert!(parse(&["--bare-url-preview", "--title", "T", "--rev", "abc"]).is_err());
        assert!(parse(&[]).expect("no flags is fine").bare_url.is_none());
    }

    fn fixture_bare_url_options() -> BareUrlCliOptions {
        BareUrlCliOptions {
            mode: BareUrlCliMode::Preview,
            wiki_id: "testwiki".to_string(),
            title: "Sandbox".to_string(),
            rev_id: 123,
        }
    }

    fn fixture_proposals_response() -> sp42_core::BareUrlProposalsResponse {
        sp42_core::BareUrlProposalsResponse {
            proposals: vec![sp42_core::BareUrlProposal {
                locator: sp42_core::WikitextNodeLocator {
                    kind: sp42_core::WikitextNodeKind::Reference,
                    ordinal: 0,
                    expected_text: "https://example.org/article".to_string(),
                },
                url: "https://example.org/article".to_string(),
                current_anchor: "https://example.org/article".to_string(),
                replacement_wikitext:
                    "{{cite web |url=https://example.org/article |title=Headline |access-date=2026-06-09}}"
                        .to_string(),
            }],
            declined: vec![sp42_core::BareUrlDeclined {
                ordinal: 3,
                url: "https://fail.example/b".to_string(),
                reason: sp42_core::BareUrlDeclineReason::MetadataUnavailable,
            }],
        }
    }

    #[test]
    fn renders_bare_url_proposals_in_all_formats() {
        let options = fixture_bare_url_options();
        let response = fixture_proposals_response();

        let text = render_bare_url_proposals(
            &options,
            "http://127.0.0.1:8788",
            &response,
            OutputFormat::Text,
        )
        .expect("text render should work");
        assert!(text.contains("bare-url preview"));
        assert!(text.contains("wiki=testwiki"));
        assert!(text.contains("#0 url=https://example.org/article"));
        assert!(text.contains("|title=Headline"));
        assert!(text.contains("#3 url=https://fail.example/b declined=metadata-unavailable"));

        let markdown = render_bare_url_proposals(
            &options,
            "http://127.0.0.1:8788",
            &response,
            OutputFormat::Markdown,
        )
        .expect("markdown render should work");
        assert!(markdown.contains("## Bare-URL proposals"));
        assert!(markdown.contains("## Declined references"));

        let json = render_bare_url_proposals(
            &options,
            "http://127.0.0.1:8788",
            &response,
            OutputFormat::Json,
        )
        .expect("json render should work");
        let value: serde_json::Value = serde_json::from_str(&json).expect("json should parse");
        assert_eq!(value["wiki_id"], "testwiki");
        assert_eq!(value["proposals"][0]["locator"]["ordinal"], 0);
        assert_eq!(value["declined"][0]["reason"], "metadata-unavailable");
    }

    #[test]
    fn renders_bare_url_execute_report_in_all_formats() {
        let response = fixture_proposals_response();
        let report = BareUrlExecuteReport {
            bridge_base_url: "http://127.0.0.1:8788".to_string(),
            wiki_id: "testwiki".to_string(),
            title: "Sandbox".to_string(),
            rev_id: 123,
            ordinal: 0,
            proposal: response.proposals[0].clone(),
            response: sp42_core::BareUrlApplyResponse {
                wiki_id: "testwiki".to_string(),
                rev_id: 123,
                accepted: true,
                actor: Some("Example".to_string()),
                http_status: Some(200),
                api_code: None,
                retryable: false,
                warnings: Vec::new(),
                result: Some("Success".to_string()),
                message: Some("MediaWiki HTTP 200".to_string()),
            },
        };

        let text =
            render_bare_url_execute(&report, OutputFormat::Text).expect("text render should work");
        assert!(text.contains("bare-url execute"));
        assert!(text.contains("ordinal=0"));
        assert!(text.contains("accepted=true"));

        let markdown = render_bare_url_execute(&report, OutputFormat::Markdown)
            .expect("markdown render should work");
        assert!(markdown.contains("## Bare-URL execute"));
        assert!(markdown.contains("## Apply result"));

        let json =
            render_bare_url_execute(&report, OutputFormat::Json).expect("json render should work");
        let value: serde_json::Value = serde_json::from_str(&json).expect("json should parse");
        assert_eq!(value["ordinal"], 0);
        assert_eq!(value["response"]["accepted"], true);
    }

    #[test]
    fn selects_bare_url_proposal_by_matching_ordinal() {
        let response = fixture_proposals_response();
        let proposal = select_bare_url_proposal(&response, 0).expect("should find ordinal 0");
        assert_eq!(proposal.locator.ordinal, 0);
        assert_eq!(proposal.url, "https://example.org/article");
    }

    #[test]
    fn select_bare_url_proposal_returns_error_for_missing_ordinal() {
        let response = fixture_proposals_response();
        let error =
            select_bare_url_proposal(&response, 99).expect_err("should fail for missing ordinal");
        assert!(error.contains("no bare-URL proposal for ordinal 99"));
        assert!(error.contains("declined: [#3"));
        assert!(error.contains("https://fail.example/b"));
        assert!(error.contains("metadata-unavailable"));
    }

    #[test]
    fn select_bare_url_proposal_handles_empty_declined_list() {
        let response = sp42_core::BareUrlProposalsResponse {
            proposals: vec![sp42_core::BareUrlProposal {
                locator: sp42_core::WikitextNodeLocator {
                    kind: sp42_core::WikitextNodeKind::Reference,
                    ordinal: 0,
                    expected_text: "https://example.org/article".to_string(),
                },
                url: "https://example.org/article".to_string(),
                current_anchor: "https://example.org/article".to_string(),
                replacement_wikitext: "{{cite web |url=https://example.org/article}}".to_string(),
            }],
            declined: vec![],
        };
        let error =
            select_bare_url_proposal(&response, 1).expect_err("should fail for missing ordinal");
        assert!(error.contains("no bare-URL proposal for ordinal 1"));
        assert!(error.contains("declined: []"));
    }
}

#[cfg(test)]
mod fetch_tests {
    use std::io::{Read, Write};
    use std::net::SocketAddr;

    use sp42_types::{HttpClient, HttpMethod, HttpRequest};
    use url::Url;

    use super::CliHttpClient;

    /// Spawn a throwaway HTTP/1.1 server on loopback that replies with `response` to every
    /// connection, returning its address. Runs on a std thread so the test needs no extra
    /// tokio IO features.
    fn spawn_raw_server(response: Vec<u8>) -> SocketAddr {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf); // drain the request line + headers
                let _ = stream.write_all(&response);
                let _ = stream.flush();
            }
        });
        addr
    }

    /// A `CliHttpClient` whose `reqwest` client never auto-follows redirects and resolves
    /// `host` to `addr`, so a floor-passing hostname can reach a loopback test server while the
    /// SSRF floor stays active (`allow_private = false`).
    fn client_resolving(host: &str, addr: SocketAddr, max_source_bytes: u64) -> CliHttpClient {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .resolve(host, addr)
            .build()
            .expect("build client");
        CliHttpClient {
            client,
            allow_private: false,
            max_source_bytes,
        }
    }

    fn get(url: &str) -> HttpRequest {
        HttpRequest {
            method: HttpMethod::Get,
            url: Url::parse(url).expect("valid url"),
            headers: std::collections::BTreeMap::new(),
            body: Vec::new(),
        }
    }

    #[tokio::test]
    async fn source_fetch_blocks_a_redirect_to_a_private_address() {
        // A public-looking host that 302-redirects to loopback must be refused at the redirect
        // hop, not followed (SP42#43).
        let addr = spawn_raw_server(
            b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:9/secret\r\nContent-Length: 0\r\n\r\n"
                .to_vec(),
        );
        let client = client_resolving("source.test", addr, 8 * 1024 * 1024);
        let error = client
            .execute(get(&format!("http://source.test:{}/", addr.port())))
            .await
            .expect_err("redirect to a private address must be blocked");
        let message = format!("{error:?}");
        assert!(
            message.contains("private/loopback"),
            "expected SSRF refusal, got: {message}"
        );
    }

    #[tokio::test]
    async fn source_fetch_enforces_size_cap_on_chunked_response_without_content_length() {
        // No Content-Length (chunked) means the cap must be enforced while reading the stream.
        let addr = spawn_raw_server(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Type: text/plain\r\n\r\n10\r\nAAAAAAAAAAAAAAAA\r\n0\r\n\r\n"
                .to_vec(),
        );
        let client = client_resolving("source.test", addr, 8); // 8-byte cap, 16-byte body
        let error = client
            .execute(get(&format!("http://source.test:{}/", addr.port())))
            .await
            .expect_err("oversized chunked body must be rejected");
        let message = format!("{error:?}");
        assert!(
            message.contains("cap"),
            "expected size-cap error, got: {message}"
        );
    }

    #[tokio::test]
    async fn source_fetch_follows_an_allowed_redirect_and_reads_the_body() {
        // Happy path: follow a redirect to another allowed host and stream a body under the cap.
        let final_addr = spawn_raw_server(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 5\r\n\r\nhello"
                .to_vec(),
        );
        let redirect_addr = spawn_raw_server(
            format!(
                "HTTP/1.1 301 Moved Permanently\r\nLocation: http://dest.test:{}/\r\nContent-Length: 0\r\n\r\n",
                final_addr.port()
            )
            .into_bytes(),
        );
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .resolve("start.test", redirect_addr)
            .resolve("dest.test", final_addr)
            .build()
            .expect("build client");
        let client = CliHttpClient {
            client,
            allow_private: false,
            max_source_bytes: 8 * 1024 * 1024,
        };
        let response = client
            .execute(get(&format!("http://start.test:{}/", redirect_addr.port())))
            .await
            .expect("allowed redirect should be followed");
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"hello");
    }
}
