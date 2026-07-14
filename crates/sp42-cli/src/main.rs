use std::convert::TryFrom;
use std::io::{self, Read};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};
use futures::executor::block_on;
use reqwest::header::COOKIE;
use serde_json::Value;
use sp42_citation::{render_page_verification_markdown, render_page_verification_text};
use sp42_core::routes as route_contracts;
use sp42_core::{
    CitationFinding, CitationVerificationRequest, CitoidMetadata, ClaimContext,
    DevAuthBootstrapRequest, DevAuthSessionStatus, FetchedSource, GroundingStatus,
    PageVerificationReport, PageVerificationRequest, QueuedEdit, ReviewAckResponse, ReviewAnchor,
    ReviewEndRequest, ReviewFindingsRequest, ReviewFindingsResponse, ReviewOpenRequest,
    ReviewOpenResponse, ReviewPollRequest, ReviewPollResponse, ReviewPollStatus, ReviewPrompt,
    ReviewPromptKind, ReviewQueueRequest, ReviewQueueResponse, ReviewReplyRequest,
    ReviewSessionSnapshot, ReviewSessionsResponse, SessionActionExecutionRequest,
    SessionActionExecutionResponse, SessionActionKind, SystemClock, VerificationOutcome,
    VerifyOptions as CoreVerifyOptions, build_dev_auth_bootstrap_request, locate_quote,
    locate_quote_fuzzy, parse_dev_auth_status, review_finding_markers, verify_citation_use_site,
};
use sp42_devtools::{
    DEV_PREVIEW_SAMPLE_EVENTS, DEV_PREVIEW_WIKI_ID, DevContextOptions, DevWorkbenchOptions,
    build_dev_action_requests, build_dev_backlog_preview, build_dev_context,
    build_dev_context_preview, build_dev_coordination_preview, build_dev_queue,
    build_dev_stream_preview, build_dev_workbench, parse_default_dev_wiki_config,
};
use sp42_inference::{GenaiModelClient, client_from_env, panel_from_env, panel_from_models};
use sp42_patrol::{
    PatrolScenarioReportInputs, ShellStateInputs, build_patrol_scenario_report,
    build_shell_state_model, render_patrol_scenario_markdown, render_patrol_scenario_text,
    render_shell_state_markdown, render_shell_state_text,
};
use sp42_types::{HttpMethod, HttpRequest, HttpResponse, ModelRef};
use std::collections::BTreeMap;

const LOCAL_SERVER_BASE_URL: &str = "http://127.0.0.1:8788";

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "lowercase")]
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

/// The shared rendering context for the `preview` and `bare-url` views. Each `clap`
/// subcommand handler builds one of these from its parsed arguments and hands it to the
/// existing `render_*` functions, which read whichever fields their mode needs.
#[derive(Debug, Clone, PartialEq)]
struct CliOptions {
    format: OutputFormat,
    workbench: Option<WorkbenchOptions>,
    context_preview: Option<ContextPreviewOptions>,
    shell_mode: Option<ShellMode>,
    action_note: Option<String>,
    action_kind: SessionActionKind,
    bridge_base_url: String,
}

impl CliOptions {
    /// A bare context: just a format and a bridge URL, everything else defaulted. Handlers
    /// set the few extra fields their view consumes.
    fn new(format: OutputFormat, bridge_base_url: String) -> Self {
        CliOptions {
            format,
            workbench: None,
            context_preview: None,
            shell_mode: None,
            action_note: None,
            action_kind: SessionActionKind::Patrol,
            bridge_base_url,
        }
    }
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
    /// The SIDE co-reference context: preceding sentences (`--preceding-sentence`, repeatable),
    /// in the order given. Empty by default, which keeps the verifier on its no-context path.
    preceding_sentences: Vec<String>,
    /// Offline source body source. `--source-file <path>` reads a file; `--source-body -`
    /// reads STDIN; any other `--source-body` value is literal text. When a body is supplied
    /// the verifier uses it instead of fetching `--source-url` — but the URL is still required
    /// and supplies the provenance URL and prompt display, so a snapshot stays attributable.
    source_file: Option<String>,
    source_body: Option<String>,
    /// Per-run model panel override (`--models a,b,c`). Falls back to the env panel
    /// (`SP42_INFERENCE_MODELS`) when unset; the endpoint/token still come from the env.
    models: Option<String>,
    /// SIDE article-title context (`--article-title`); empty keeps the no-context path.
    article_title: Option<String>,
    /// Pre-fetched bibliographic metadata sidecar (`--metadata-json <path>`). When set, the
    /// verifier uses it as prompt context instead of fetching Citoid — reproducible, no network.
    metadata_json: Option<String>,
}

/// Article-level citation verification (`--verify-page`, PRD-0009 / ADR-0011): verify
/// *every* URL-bearing citation on a revision and render the page report. Reuses the
/// shared `--wiki`/`--title`/`--rev` flags. The route is session+CSRF gated, so the CLI
/// bootstraps a bridge session (ADR-0002) before the call.
#[derive(Debug, Clone, PartialEq, Eq)]
struct VerifyPageCliOptions {
    wiki_id: String,
    title: String,
    rev_id: u64,
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

/// The dev / operator queue views, selected as the positional argument to `preview`.
/// `clap` renders the variants in kebab-case (`SessionDigest` → `session-digest`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "kebab-case")]
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

/// Session-action kind for the action views. Maps to [`SessionActionKind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "lowercase")]
enum ActionKind {
    Patrol,
    Rollback,
    Undo,
}

impl From<ActionKind> for SessionActionKind {
    fn from(kind: ActionKind) -> Self {
        match kind {
            ActionKind::Patrol => SessionActionKind::Patrol,
            ActionKind::Rollback => SessionActionKind::Rollback,
            ActionKind::Undo => SessionActionKind::Undo,
        }
    }
}

impl From<VerifyArgs> for VerifyCliOptions {
    fn from(args: VerifyArgs) -> Self {
        VerifyCliOptions {
            claim: args.claim,
            source_url: args.source_url,
            include_metadata: args.output.with_metadata,
            debug_votes: args.output.debug_votes,
            // `--no-repair` turns the bounded repair turn off; on by default.
            repair: !args.no_repair,
            preceding_sentences: args.preceding_sentence,
            source_file: args.source_file,
            source_body: args.source_body,
            models: args.models,
            article_title: args.article_title,
            metadata_json: args.metadata_json,
        }
    }
}

impl From<VerifyPageArgs> for VerifyPageCliOptions {
    fn from(args: VerifyPageArgs) -> Self {
        VerifyPageCliOptions {
            wiki_id: args.wiki,
            title: args.title,
            // No --rev means the latest revision; the server resolves 0 to a concrete id.
            rev_id: args.rev.unwrap_or(0),
        }
    }
}

/// `sp42-cli` — capabilities are subcommands. See `docs/platform/CLI.md` for the full
/// reference; `--help` on any subcommand prints its options.
#[derive(Debug, Parser)]
#[command(
    name = "sp42-cli",
    version,
    about = "SP42 citation-patrol command-line shell"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Verify a single claim against a single source URL (PRD-0001).
    Verify(VerifyArgs),
    /// Verify every URL-bearing citation on a revision via the bridge (PRD-0001).
    VerifyPage(VerifyPageArgs),
    /// Report whether a quote locates in a source body read from STDIN (offline, SP42#25).
    LocateProbe(LocateProbeArgs),
    /// Verify a JSONL batch (one case per line, STDIN or --file), one result per line.
    Batch(BatchArgs),
    /// Preview or apply bare-URL citation repairs (PRD-0008).
    BareUrl(BareUrlArgs),
    /// Interactive review sessions: open a page, wait for operator feedback (PRD-0017).
    Review(ReviewArgs),
    /// Dev / operator queue views; reads the event payload from STDIN.
    Preview(PreviewArgs),
}

#[derive(Debug, Args)]
struct BatchArgs {
    /// JSONL input file; omit to read cases from STDIN.
    #[arg(long)]
    file: Option<String>,
    /// Per-batch model panel override (comma-separated ids); falls back to `SP42_INFERENCE_MODELS`.
    #[arg(long)]
    models: Option<String>,
}

/// Resolved `batch` inputs handed to [`render_batch`]: input path (`None` = STDIN) and the
/// per-batch model panel override.
#[derive(Debug, Clone, PartialEq, Eq)]
struct BatchOptions {
    file: Option<String>,
    models: Option<String>,
}

/// Shared `--format` flag, flattened into each command that renders.
#[derive(Debug, Args)]
struct FormatArg {
    /// Output format.
    #[arg(long, value_enum, default_value = "text")]
    format: OutputFormat,
}

/// What the `verify` command emits. Grouped so neither struct carries more than three
/// bool flags (`clippy::struct_excessive_bools`).
#[derive(Debug, Args)]
struct VerifyOutputArgs {
    /// Include source metadata in the finding.
    #[arg(long)]
    with_metadata: bool,
    /// Emit the full `VerificationOutcome` (per-model votes) as JSON (SP42#25).
    #[arg(long)]
    debug_votes: bool,
    /// Print only the verdict label.
    #[arg(long)]
    verdict_only: bool,
}

#[derive(Debug, Args)]
struct VerifyArgs {
    /// The claim to verify.
    #[arg(long)]
    claim: String,
    /// Source URL to fetch and check the claim against.
    #[arg(long)]
    source_url: String,
    /// Preceding-context sentence (repeatable, in order) — the SIDE co-reference context.
    #[arg(long = "preceding-sentence")]
    preceding_sentence: Vec<String>,
    /// Disable the bounded repair turn (one fewer model call per unlocated support vote).
    #[arg(long)]
    no_repair: bool,
    /// Read the source body from a file instead of fetching `--source-url` (offline).
    #[arg(long, conflicts_with = "source_body")]
    source_file: Option<String>,
    /// Use this source body instead of fetching `--source-url`: `-` reads STDIN, any other
    /// value is literal text (offline). `--source-url` is still required for provenance.
    #[arg(long)]
    source_body: Option<String>,
    /// Per-run model panel override (comma-separated ids); falls back to `SP42_INFERENCE_MODELS`.
    #[arg(long)]
    models: Option<String>,
    /// SIDE article-title context.
    #[arg(long)]
    article_title: Option<String>,
    /// Bibliographic metadata sidecar (JSON file); used as prompt context instead of fetching
    /// Citoid (offline).
    #[arg(long)]
    metadata_json: Option<String>,
    #[command(flatten)]
    output: VerifyOutputArgs,
    #[command(flatten)]
    fmt: FormatArg,
}

#[derive(Debug, Args)]
struct VerifyPageArgs {
    /// Article title.
    #[arg(long)]
    title: String,
    /// Target wiki id.
    #[arg(long, default_value = BARE_URL_DEFAULT_WIKI)]
    wiki: String,
    /// Revision id. Omit for the latest revision (the server resolves it).
    #[arg(long)]
    rev: Option<u64>,
    /// Local server base URL.
    #[arg(long, default_value = LOCAL_SERVER_BASE_URL)]
    bridge_base_url: String,
    #[command(flatten)]
    fmt: FormatArg,
}

#[derive(Debug, Args)]
struct ReviewArgs {
    #[command(subcommand)]
    action: ReviewAction,
}

#[derive(Debug, Subcommand)]
enum ReviewAction {
    /// Open or resume a review session on a page (bare title or pasted wiki URL).
    Open(ReviewOpenArgs),
    /// Wait for operator feedback on an open session; re-arms until feedback or end.
    Poll(ReviewPollArgs),
    /// Queue operator feedback from the command line (dev/test surface).
    Queue(ReviewQueueArgs),
    /// Attach a verify-page report to the session so its findings overlay
    /// the article outline (the report's in-article frontend).
    Findings(ReviewFindingsArgs),
    /// Send an agent chat reply to the operator surface.
    Reply(ReviewReplyArgs),
    /// End a session as the agent (a plain reopen stays allowed).
    End(ReviewEndArgs),
    /// List review sessions on the local server.
    Sessions(ReviewSessionsArgs),
}

#[derive(Debug, Args)]
struct ReviewOpenArgs {
    /// Page target: bare title or pasted wiki URL (oldid URLs pin the revision).
    target: String,
    /// Target wiki id.
    #[arg(long, default_value = BARE_URL_DEFAULT_WIKI)]
    wiki: String,
    /// Revision id. Omit for the latest revision (the server resolves it).
    #[arg(long)]
    rev: Option<u64>,
    /// Resume a session the operator explicitly ended (only when they ask).
    #[arg(long)]
    reopen: bool,
    /// Local server base URL.
    #[arg(long, default_value = LOCAL_SERVER_BASE_URL)]
    bridge_base_url: String,
    #[command(flatten)]
    fmt: FormatArg,
}

#[derive(Debug, Args)]
struct ReviewPollArgs {
    /// Page target: bare title or pasted wiki URL.
    target: String,
    /// Target wiki id.
    #[arg(long, default_value = BARE_URL_DEFAULT_WIKI)]
    wiki: String,
    /// Chat reply shown to the operator before the wait starts.
    #[arg(long)]
    agent_reply: Option<String>,
    /// Return after one bounded server wait instead of re-arming until feedback.
    #[arg(long)]
    once: bool,
    /// Local server base URL.
    #[arg(long, default_value = LOCAL_SERVER_BASE_URL)]
    bridge_base_url: String,
    #[command(flatten)]
    fmt: FormatArg,
}

#[derive(Debug, Args)]
struct ReviewQueueArgs {
    /// Page target: bare title or pasted wiki URL.
    target: String,
    /// The feedback prompt to queue.
    #[arg(long)]
    message: String,
    /// Target wiki id.
    #[arg(long, default_value = BARE_URL_DEFAULT_WIKI)]
    wiki: String,
    /// Anchor the prompt to this block ordinal.
    #[arg(long)]
    block: Option<usize>,
    /// Anchor the prompt to this cite id (e.g. cite_ref-x_2-0).
    #[arg(long)]
    ref_id: Option<String>,
    /// Anchor the prompt to this verbatim selected text.
    #[arg(long)]
    selected_text: Option<String>,
    /// Queue and end the session in one action ("send & end").
    #[arg(long)]
    end: bool,
    /// Local server base URL.
    #[arg(long, default_value = LOCAL_SERVER_BASE_URL)]
    bridge_base_url: String,
    #[command(flatten)]
    fmt: FormatArg,
}

#[derive(Debug, Args)]
struct ReviewFindingsArgs {
    /// Page target: bare title or pasted wiki URL.
    target: String,
    /// Path to a `verify-page --format json` report, or `-` for stdin.
    #[arg(long)]
    report: String,
    /// Target wiki id.
    #[arg(long, default_value = BARE_URL_DEFAULT_WIKI)]
    wiki: String,
    /// Local server base URL.
    #[arg(long, default_value = LOCAL_SERVER_BASE_URL)]
    bridge_base_url: String,
    #[command(flatten)]
    fmt: FormatArg,
}

#[derive(Debug, Args)]
struct ReviewReplyArgs {
    /// Page target: bare title or pasted wiki URL.
    target: String,
    /// The reply text.
    #[arg(long)]
    message: String,
    /// Target wiki id.
    #[arg(long, default_value = BARE_URL_DEFAULT_WIKI)]
    wiki: String,
    /// Local server base URL.
    #[arg(long, default_value = LOCAL_SERVER_BASE_URL)]
    bridge_base_url: String,
    #[command(flatten)]
    fmt: FormatArg,
}

#[derive(Debug, Args)]
struct ReviewEndArgs {
    /// Page target: bare title or pasted wiki URL.
    target: String,
    /// Target wiki id.
    #[arg(long, default_value = BARE_URL_DEFAULT_WIKI)]
    wiki: String,
    /// Local server base URL.
    #[arg(long, default_value = LOCAL_SERVER_BASE_URL)]
    bridge_base_url: String,
    #[command(flatten)]
    fmt: FormatArg,
}

#[derive(Debug, Args)]
struct ReviewSessionsArgs {
    /// Local server base URL.
    #[arg(long, default_value = LOCAL_SERVER_BASE_URL)]
    bridge_base_url: String,
    #[command(flatten)]
    fmt: FormatArg,
}

#[derive(Debug, Args)]
struct LocateProbeArgs {
    /// The quote to locate within the STDIN source body.
    #[arg(long)]
    quote: String,
}

#[derive(Debug, Args)]
struct BareUrlArgs {
    #[command(subcommand)]
    action: BareUrlAction,
}

#[derive(Debug, Subcommand)]
enum BareUrlAction {
    /// Show bare-URL repair proposals for a revision.
    Preview(BareUrlPreviewArgs),
    /// Apply one bare-URL repair proposal.
    Execute(BareUrlExecuteArgs),
}

#[derive(Debug, Args)]
struct BareUrlPreviewArgs {
    /// Article title.
    #[arg(long)]
    title: String,
    /// Revision id.
    #[arg(long)]
    rev: u64,
    /// Target wiki id.
    #[arg(long, default_value = BARE_URL_DEFAULT_WIKI)]
    wiki: String,
    /// Local server base URL (the proposals request is posted to the bridge).
    #[arg(long, default_value = LOCAL_SERVER_BASE_URL)]
    bridge_base_url: String,
    #[command(flatten)]
    fmt: FormatArg,
}

#[derive(Debug, Args)]
struct BareUrlExecuteArgs {
    /// Article title.
    #[arg(long)]
    title: String,
    /// Revision id.
    #[arg(long)]
    rev: u64,
    /// Zero-based index of the proposal to apply.
    #[arg(long)]
    ordinal: usize,
    /// Target wiki id.
    #[arg(long, default_value = BARE_URL_DEFAULT_WIKI)]
    wiki: String,
    /// Edit summary attached to the applied repair.
    #[arg(long)]
    action_note: Option<String>,
    /// Local server base URL.
    #[arg(long, default_value = LOCAL_SERVER_BASE_URL)]
    bridge_base_url: String,
    #[command(flatten)]
    fmt: FormatArg,
}

#[derive(Debug, Args)]
struct PreviewArgs {
    /// View to render. Omit for the default ranked-queue render.
    #[arg(value_enum)]
    mode: Option<ShellMode>,
    /// Local server base URL for server-backed views (server-report, action-execute).
    #[arg(long, default_value = LOCAL_SERVER_BASE_URL)]
    bridge_base_url: String,
    /// Enable workbench submission with this token.
    #[arg(long)]
    workbench_token: Option<String>,
    /// Workbench actor name.
    #[arg(long, default_value = "SP42-cli")]
    workbench_actor: String,
    /// Workbench submission note.
    #[arg(long, default_value = "cli local workbench")]
    workbench_note: String,
    /// Talk-page wikitext for the context preview.
    #[arg(long)]
    context_talk: Option<String>,
    /// `LiftWing` damaging probability (float).
    #[arg(long)]
    context_liftwing: Option<f32>,
    /// Action kind for the action views.
    #[arg(long, value_enum, default_value = "patrol")]
    action_kind: ActionKind,
    /// Note attached to the action.
    #[arg(long)]
    action_note: Option<String>,
    #[command(flatten)]
    fmt: FormatArg,
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
    let mut argv: Vec<String> = std::env::args().collect();
    let program = if argv.is_empty() {
        "sp42-cli".to_string()
    } else {
        argv.remove(0)
    };
    // Backward-compat: a pre-clap flag-form invocation is rewritten to the equivalent
    // subcommand argv (with a deprecation warning) so existing scripts keep working during
    // the transition. New-form argv is passed to clap unchanged.
    let cli = match rewrite_legacy_argv(&argv) {
        Some(Ok(translated)) => {
            eprintln!(
                "warning: this sp42-cli flag form is deprecated and will be removed; it now \
                 maps to `sp42-cli {}`. See docs/platform/CLI.md.",
                translated.join(" ")
            );
            Cli::parse_from(std::iter::once(program).chain(translated))
        }
        // A legacy invocation that was itself invalid (e.g. mutually-exclusive flags); surface
        // the same usage error the old parser produced rather than silently picking one.
        Some(Err(message)) => return Err(message),
        None => Cli::parse_from(std::iter::once(program).chain(argv)),
    };
    dispatch(cli.command)
}

/// Map a pre-clap (flag-only) invocation onto the equivalent subcommand argv, so scripts
/// written against the old surface keep working for a deprecation period.
///
/// Returns `None` when the argv is already in subcommand form — first token is a subcommand
/// (no leading dash) or `--help`/`--version` — in which case clap parses it unchanged.
/// Returns `Some(Err(..))` for a legacy invocation that the old parser itself rejected (e.g.
/// mutually-exclusive flags), so the error is preserved rather than silently resolved.
fn rewrite_legacy_argv(args: &[String]) -> Option<Result<Vec<String>, String>> {
    match args.first().map(String::as_str) {
        Some("-h" | "--help" | "-V" | "--version") => return None,
        Some(token) if !token.starts_with('-') => return None,
        _ => {} // empty, or a leading flag → legacy form
    }

    let present = |flag: &str| args.iter().any(|a| a == flag);
    let without = |drop: &[&str]| -> Vec<String> {
        args.iter()
            .filter(|a| !drop.contains(&a.as_str()))
            .cloned()
            .collect()
    };
    let prepend = |head: &[&str], tail: Vec<String>| -> Vec<String> {
        head.iter().map(|s| (*s).to_string()).chain(tail).collect()
    };

    // Precedence mirrors the pre-clap dispatch order: bare-url, locate, verify, verify-page,
    // then the preview family.
    if present("--bare-url-preview") || present("--bare-url-execute") {
        // The old parser rejected both flags together before dispatch; preserve that so a
        // typo can't turn a usage error into an apply (`execute`).
        if present("--bare-url-preview") && present("--bare-url-execute") {
            return Some(Err(
                "--bare-url-preview and --bare-url-execute are mutually exclusive".to_string(),
            ));
        }
        let action = if present("--bare-url-execute") {
            "execute"
        } else {
            "preview"
        };
        return Some(Ok(prepend(
            &["bare-url", action],
            without(&["--bare-url-preview", "--bare-url-execute"]),
        )));
    }
    if present("--locate-probe") {
        return Some(rewrite_legacy_locate_probe(args));
    }
    if present("--batch") || present("--batch-file") {
        // `--batch` selected batch mode and read STDIN; `--batch-file <p>` set the input path.
        // The subcommand spells the path `--file`; `--models` passes through unchanged. The old
        // global `--format <v>` was accepted but ignored by batch (output is always JSONL), so
        // it is consumed and dropped here rather than forwarded to a subcommand that lacks it.
        let mut out = vec!["batch".to_string()];
        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--batch" => {}
                "--batch-file" => {
                    out.push("--file".to_string());
                    let Some(path) = iter.next() else {
                        return Some(Err("--batch-file requires a value".to_string()));
                    };
                    out.push(path.clone());
                }
                "--format" => {
                    let Some(value) = iter.next() else {
                        return Some(Err("--format requires a value".to_string()));
                    };
                    if let Err(message) = validate_legacy_output_format(value) {
                        return Some(Err(message));
                    }
                }
                _ => out.push(arg.clone()),
            }
        }
        return Some(Ok(out));
    }
    if present("--claim") || present("--source-url") {
        return Some(Ok(prepend(&["verify"], args.to_vec())));
    }
    if present("--verify-page") {
        return Some(Ok(prepend(&["verify-page"], without(&["--verify-page"]))));
    }
    Some(rewrite_legacy_preview(args))
}

fn rewrite_legacy_locate_probe(args: &[String]) -> Result<Vec<String>, String> {
    let mut out = vec!["locate-probe".to_string()];
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--locate-probe" => {}
            "--format" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "--format requires a value".to_string())?;
                validate_legacy_output_format(value)?;
            }
            _ => out.push(arg.clone()),
        }
    }
    Ok(out)
}

fn validate_legacy_output_format(value: &str) -> Result<(), String> {
    match value {
        "text" | "json" | "markdown" => Ok(()),
        _ => Err(format!("unsupported output format: {value}")),
    }
}

/// Translate a legacy preview invocation: `--shell <value>` and the shorthand mode flags
/// (`--parity-report`, `--stream`, …) become the `preview` positional `mode`; all other
/// flags pass through. Reproduces the old alias quirks and selection precedence, and the old
/// usage errors for a missing or unrecognized `--shell` value (so a typo fails loudly rather
/// than silently rendering the default view).
fn rewrite_legacy_preview(args: &[String]) -> Result<Vec<String>, String> {
    let mut shell_mode: Option<&'static str> = None;
    let mut shorthand_mode: Option<&'static str> = None;
    let mut rest: Vec<String> = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--shell" {
            let value = iter
                .next()
                .ok_or_else(|| "--shell requires a value".to_string())?;
            shell_mode = Some(
                legacy_shell_value(value)
                    .ok_or_else(|| format!("unsupported shell mode: {value}"))?,
            );
            continue;
        }
        if let Some(canonical) = legacy_preview_flag(arg) {
            shorthand_mode = Some(match shorthand_mode {
                Some(existing) if preview_mode_rank(existing) <= preview_mode_rank(canonical) => {
                    existing
                }
                _ => canonical,
            });
            continue;
        }
        rest.push(arg.clone());
    }

    let mut out = vec!["preview".to_string()];
    // `--shell` wins over the shorthand flags (old `selected_shell_mode` behavior).
    if let Some(mode) = shell_mode.or(shorthand_mode) {
        out.push(mode.to_string());
    }
    out.extend(rest);
    Ok(out)
}

/// A legacy `--<mode>` shorthand flag mapped to its `preview` mode value (old
/// `preview_mode_flag`). Note `--operator-report` mapped to the server report here.
fn legacy_preview_flag(flag: &str) -> Option<&'static str> {
    match flag {
        "--stream-preview" | "--stream" => Some("stream"),
        "--backlog-preview" | "--backlog" => Some("backlog"),
        "--coordination-preview" | "--coordination" => Some("coordination"),
        "--session-digest" => Some("session-digest"),
        "--scenario-report" | "--patrol-report" => Some("scenario-report"),
        "--server-report" | "--operator-report" => Some("server-report"),
        "--parity-report" => Some("parity-report"),
        "--action-preview" | "--action" => Some("action-preview"),
        "--action-execute" => Some("action-execute"),
        _ => None,
    }
}

/// A legacy `--shell <value>` mapped to its `preview` mode value (old `parse_shell_mode`).
/// Note `operator-report` mapped to the parity report here — the opposite of the shorthand.
fn legacy_shell_value(value: &str) -> Option<&'static str> {
    match value {
        "parity-report" | "operator-report" => Some("parity-report"),
        "stream-preview" | "stream" => Some("stream"),
        "backlog-preview" | "backlog" => Some("backlog"),
        "coordination-preview" | "coordination" => Some("coordination"),
        "session-digest" => Some("session-digest"),
        "scenario-report" | "patrol-report" => Some("scenario-report"),
        "server-report" | "live-server-report" => Some("server-report"),
        "action-preview" | "action" => Some("action-preview"),
        "action-execute" => Some("action-execute"),
        _ => None,
    }
}

/// Selection precedence among multiple legacy shorthand mode flags (old `selected_shell_mode`).
/// Lower rank wins.
fn preview_mode_rank(mode: &str) -> usize {
    [
        "parity-report",
        "stream",
        "backlog",
        "coordination",
        "session-digest",
        "scenario-report",
        "server-report",
        "action-execute",
        "action-preview",
    ]
    .iter()
    .position(|candidate| *candidate == mode)
    .unwrap_or(usize::MAX)
}

/// Route a parsed subcommand to its handler. Each handler returns the text to print on
/// stdout; errors propagate to `main`, which prints them to stderr and exits non-zero.
fn dispatch(command: Command) -> Result<String, String> {
    match command {
        Command::Verify(args) => {
            let format = args.fmt.format;
            let verdict_only = args.output.verdict_only;
            render_verify(&VerifyCliOptions::from(args), format, verdict_only)
        }
        Command::VerifyPage(args) => {
            let format = args.fmt.format;
            let bridge_base_url = args.bridge_base_url.clone();
            render_verify_page(&VerifyPageCliOptions::from(args), &bridge_base_url, format)
        }
        Command::LocateProbe(args) => {
            // Offline locate probe: read a source body from STDIN, report whether the quote
            // locates. No model, no fetch — the deterministic locate-replay tool (SP42#25).
            let source = read_stdin().map_err(|error| error.to_string())?;
            run_locate_probe(&args.quote, &source)
        }
        Command::Batch(args) => render_batch(&BatchOptions {
            file: args.file,
            models: args.models,
        }),
        Command::BareUrl(args) => dispatch_bare_url(args.action),
        Command::Review(args) => dispatch_review(args.action),
        Command::Preview(args) => dispatch_preview(args),
    }
}

fn dispatch_bare_url(action: BareUrlAction) -> Result<String, String> {
    let (bare_url, options) = match action {
        BareUrlAction::Preview(args) => (
            BareUrlCliOptions {
                mode: BareUrlCliMode::Preview,
                wiki_id: args.wiki,
                title: args.title,
                rev_id: args.rev,
            },
            CliOptions::new(args.fmt.format, args.bridge_base_url),
        ),
        BareUrlAction::Execute(args) => {
            let mut options = CliOptions::new(args.fmt.format, args.bridge_base_url);
            options.action_note = args.action_note;
            (
                BareUrlCliOptions {
                    mode: BareUrlCliMode::Execute {
                        ordinal: args.ordinal,
                    },
                    wiki_id: args.wiki,
                    title: args.title,
                    rev_id: args.rev,
                },
                options,
            )
        }
    };
    render_bare_url_mode(&bare_url, &options, options.format)
}

fn dispatch_preview(args: PreviewArgs) -> Result<String, String> {
    let format = args.fmt.format;
    let mut options = CliOptions::new(format, args.bridge_base_url);
    options.shell_mode = args.mode;
    options.action_kind = args.action_kind.into();
    options.action_note = args.action_note;
    options.workbench = args.workbench_token.map(|token| WorkbenchOptions {
        token,
        actor: args.workbench_actor,
        note: args.workbench_note,
    });
    options.context_preview = (args.context_talk.is_some() || args.context_liftwing.is_some())
        .then_some(ContextPreviewOptions {
            talk_page: args.context_talk,
            liftwing_probability: args.context_liftwing,
        });

    let input = read_stdin().map_err(|error| error.to_string())?;
    let payload = if input.trim().is_empty() {
        DEV_PREVIEW_SAMPLE_EVENTS
    } else {
        input.as_str()
    };

    let config = parse_default_dev_wiki_config().map_err(|error| error.to_string())?;
    let ranked = load_ranked_queue(&config, payload)?;

    match options.shell_mode {
        Some(ShellMode::ParityReport) => {
            return render_parity_report(&config, &ranked, payload, format);
        }
        Some(ShellMode::Stream) => return render_stream_preview(&config, payload, format),
        Some(ShellMode::Backlog) => return render_backlog_preview(&config, format),
        Some(ShellMode::Coordination) => return render_coordination_preview(format),
        Some(ShellMode::SessionDigest) => {
            return render_session_digest(&config, &ranked, &options, format);
        }
        Some(ShellMode::ScenarioReport) => {
            return render_scenario_report(&config, &ranked, &options, format);
        }
        Some(ShellMode::ServerReport) => {
            return render_server_report(&options.bridge_base_url, format);
        }
        Some(ShellMode::ActionPreview) => {
            return render_action_preview(&config, &ranked, &options, format);
        }
        Some(ShellMode::ActionExecute) => {
            return render_action_execute(&config, &ranked, &options, format);
        }
        None => {}
    }

    if ranked.is_empty() {
        return Ok("No actionable edit from input.".to_string());
    }

    if let Some(workbench) = &options.workbench {
        return render_workbench(&config, &ranked, workbench, format);
    }

    if let Some(context_preview) = &options.context_preview {
        return render_context_preview(&config, &ranked, context_preview, format);
    }

    render_queue(&ranked, format)
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

#[allow(clippy::too_many_lines)] // flat arg-parser; refactor tracked in SP42#80
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

// ----- Read-only citation verification (PRD-0001) -----

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

/// Resolve the optional offline source body: `--source-file <path>` reads a file,
/// `--source-body -` reads STDIN, and any other `--source-body` value is literal text.
/// `None` (neither flag) keeps the verifier on its live-fetch path.
fn resolve_source_body(
    source_file: Option<&str>,
    source_body: Option<&str>,
) -> Result<Option<String>, String> {
    match (source_file, source_body) {
        (Some(path), _) => std::fs::read_to_string(path)
            .map(Some)
            .map_err(|error| format!("failed to read --source-file {path:?}: {error}")),
        (None, Some("-")) => read_stdin().map(Some).map_err(|error| error.to_string()),
        (None, Some(text)) => Ok(Some(text.to_string())),
        (None, None) => Ok(None),
    }
}

/// Wrap an offline body as a `FetchedSource` the verifier treats as a 200 `text/plain`
/// response. The eval harness pre-extracts sources to text, so there is no HTML for the
/// usability gate's structured paywall markers — `raw_html` is `None` by design.
fn prefetched_from_body(text: String) -> FetchedSource {
    FetchedSource {
        text,
        status: 200,
        content_type: "text/plain".to_string(),
        raw_html: None,
        book_snippet: false,
    }
}

/// Build the model panel: `--models` (comma-separated) overrides the env panel for this run;
/// the endpoint/token always come from the env (`SP42_INFERENCE_*`).
fn build_panel(models: Option<&str>) -> Result<Vec<ModelRef>, String> {
    match models {
        Some(models) => {
            let provider = std::env::var("SP42_INFERENCE_PROVIDER")
                .unwrap_or_else(|_| "configured".to_string());
            panel_from_models(&provider, models)
        }
        None => panel_from_env(),
    }
}

/// Build the read-only source-fetch HTTP client (the guarded `sp42-fetch` edge,
/// ADR-0015). It carries no inference credential — the bearer is held only by the
/// model adapter, so it can never reach a third-party source host.
fn build_fetch_client() -> Result<sp42_fetch::GuardedHttpClient, String> {
    sp42_fetch::source_client_from_env(sp42_core::branding::USER_AGENT)
}

/// One verification's resolved inputs — shared by the single `verify` path and `batch`.
struct VerifyCase {
    claim: String,
    source_url: String,
    /// Resolved offline body; `None` means live-fetch `source_url`.
    source_body: Option<String>,
    /// Bibliographic sidecar; `None` means fetch-or-skip per `include_metadata`.
    metadata: Option<CitoidMetadata>,
    include_metadata: bool,
    repair: bool,
    preceding_sentences: Vec<String>,
    article_title: Option<String>,
}

/// Verify one case against an already-built panel/clients. An offline body is handed to the
/// verifier as a pre-fetched 200 `text/plain` source (no network for the body); `source_url`
/// still supplies the provenance URL. An empty context stays on the no-context path.
async fn verify_case(
    fetch_client: &sp42_fetch::GuardedHttpClient,
    model_client: &GenaiModelClient,
    panel: &[ModelRef],
    case: &VerifyCase,
) -> Result<VerificationOutcome, String> {
    let source_url = case
        .source_url
        .parse()
        .map_err(|_| format!("invalid source url: {}", case.source_url))?;
    let request = CitationVerificationRequest {
        wiki_id: String::new(),
        rev_id: 0,
        title: String::new(),
        claim: case.claim.clone(),
        source_url,
    };
    let verify_options = CoreVerifyOptions {
        include_metadata: case.include_metadata,
        concurrency: 3,
        repair_turn: case.repair,
        prefetched: case.source_body.clone().map(prefetched_from_body),
        metadata_sidecar: case.metadata.clone(),
        ..Default::default()
    };
    let claim_context = {
        let context = ClaimContext {
            article_title: case.article_title.clone().unwrap_or_default(),
            preceding_sentences: case.preceding_sentences.clone(),
        };
        if context.is_empty() {
            None
        } else {
            Some(context)
        }
    };
    verify_citation_use_site(
        fetch_client,
        model_client,
        &SystemClock,
        panel,
        &request,
        claim_context.as_ref(),
        0,
        verify_options,
    )
    .await
    .map_err(|error| error.to_string())
}

/// A metadata sidecar as supplied on disk / inline: all bibliographic fields optional, and
/// `url` defaults to the case's `source_url` when absent (the harness keys metadata by case).
#[derive(Debug, Clone, serde::Deserialize)]
struct MetadataSidecarFile {
    #[serde(default)]
    publication: Option<String>,
    #[serde(default)]
    published: Option<String>,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    url: Option<String>,
}

impl MetadataSidecarFile {
    fn into_citoid(self, fallback_url: &str) -> CitoidMetadata {
        CitoidMetadata {
            publication: self.publication,
            published: self.published,
            author: self.author,
            title: self.title,
            url: self.url.unwrap_or_else(|| fallback_url.to_string()),
        }
    }
}

/// Load a `--metadata-json` sidecar file, filling a missing `url` from the source URL.
fn load_metadata_sidecar(path: &str, source_url: &str) -> Result<CitoidMetadata, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|error| format!("failed to read --metadata-json {path:?}: {error}"))?;
    let file: MetadataSidecarFile =
        serde_json::from_str(&raw).map_err(|error| format!("invalid --metadata-json: {error}"))?;
    Ok(file.into_citoid(source_url))
}

fn default_repair() -> bool {
    true
}

/// Best-effort recovery of a batch line's `id` when the line is valid JSON but fails typed
/// `BatchCase` deserialization (e.g. missing `claim`, or a wrong-typed field). Keeps a
/// schema-error result attributable to its input case, honoring the contract that ids echo.
fn recover_case_id(raw: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|value| value.get("id")?.as_str().map(str::to_string))
}

/// One input line of a `batch` run. `claim` and `source_url` are required; everything else
/// is optional. `source_body` (inline text) keeps the run offline; `repair` defaults on.
#[derive(Debug, Clone, serde::Deserialize)]
struct BatchCase {
    #[serde(default)]
    id: Option<String>,
    claim: String,
    source_url: String,
    #[serde(default)]
    source_body: Option<String>,
    #[serde(default)]
    metadata: Option<MetadataSidecarFile>,
    #[serde(default)]
    preceding: Vec<String>,
    #[serde(default)]
    article_title: Option<String>,
    #[serde(default = "default_repair")]
    repair: bool,
    #[serde(default)]
    with_metadata: bool,
}

impl BatchCase {
    fn into_verify_case(self) -> VerifyCase {
        let metadata = self.metadata.map(|m| m.into_citoid(&self.source_url));
        VerifyCase {
            claim: self.claim,
            source_url: self.source_url,
            source_body: self.source_body,
            metadata,
            include_metadata: self.with_metadata,
            repair: self.repair,
            preceding_sentences: self.preceding,
            article_title: self.article_title,
        }
    }
}

/// One output line of a `batch` run: the case `id` (echoed when present) plus either the
/// full `VerificationOutcome` or a per-case `error`. A bad case never aborts the batch.
#[derive(Debug, serde::Serialize)]
struct BatchResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    outcome: Option<VerificationOutcome>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Read the batch input (STDIN or `--file`) and verify every line on one Tokio runtime.
fn render_batch(batch: &BatchOptions) -> Result<String, String> {
    let input = match &batch.file {
        Some(path) => std::fs::read_to_string(path)
            .map_err(|error| format!("failed to read --file {path:?}: {error}"))?,
        None => read_stdin().map_err(|error| error.to_string())?,
    };
    let runtime = tokio::runtime::Runtime::new()
        .map_err(|error| format!("failed to start async runtime: {error}"))?;
    runtime.block_on(run_batch(batch.models.as_deref(), &input))
}

/// Verify each non-empty JSONL line against one shared panel/client, emitting one result line.
/// A line that fails to parse or verify becomes an `error` result; the batch continues.
async fn run_batch(models: Option<&str>, input: &str) -> Result<String, String> {
    let panel = build_panel(models)?;
    let model_client = client_from_env()?;
    let fetch_client = build_fetch_client()?;

    let mut lines = Vec::new();
    for (index, raw) in input.lines().enumerate() {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let result = match serde_json::from_str::<BatchCase>(raw) {
            Ok(case) => {
                let id = case.id.clone();
                match verify_case(
                    &fetch_client,
                    &model_client,
                    &panel,
                    &case.into_verify_case(),
                )
                .await
                {
                    Ok(outcome) => BatchResult {
                        id,
                        outcome: Some(outcome),
                        error: None,
                    },
                    Err(error) => BatchResult {
                        id,
                        outcome: None,
                        error: Some(error),
                    },
                }
            }
            Err(error) => BatchResult {
                id: recover_case_id(raw),
                outcome: None,
                error: Some(format!("line {}: could not parse case: {error}", index + 1)),
            },
        };
        lines.push(serde_json::to_string(&result).map_err(|error| error.to_string())?);
    }
    Ok(lines.join("\n"))
}

/// Execute one ad-hoc (claim, source URL) verification against the configured panel.
///
/// Reads the inference endpoint, model panel, and (optional) bearer token from the
/// environment so no secret is hard-coded; performs only read-only GET/POST requests.
async fn run_verify(options: &VerifyCliOptions) -> Result<VerificationOutcome, String> {
    let panel = build_panel(options.models.as_deref())?;
    let model_client = client_from_env()?;
    let fetch_client = build_fetch_client()?;

    let source_body = resolve_source_body(
        options.source_file.as_deref(),
        options.source_body.as_deref(),
    )?;
    let metadata = match &options.metadata_json {
        Some(path) => Some(load_metadata_sidecar(path, &options.source_url)?),
        None => None,
    };
    let case = VerifyCase {
        claim: options.claim.clone(),
        source_url: options.source_url.clone(),
        source_body,
        metadata,
        include_metadata: options.include_metadata,
        repair: options.repair,
        preceding_sentences: options.preceding_sentences.clone(),
        article_title: options.article_title.clone(),
    };
    verify_case(&fetch_client, &model_client, &panel, &case).await
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
        BatchCase, Cli, Command, MetadataSidecarFile, VerifyCliOptions, load_metadata_sidecar,
        prefetched_from_body, recover_case_id, render_verify_text, resolve_source_body,
        run_locate_probe,
    };
    use clap::Parser;

    /// Parse a full argv (program name implied) into the top-level subcommand.
    fn command_from(items: &[&str]) -> Result<Command, clap::Error> {
        let argv = std::iter::once("sp42-cli").chain(items.iter().copied());
        Cli::try_parse_from(argv).map(|cli| cli.command)
    }

    fn verify_options(items: &[&str]) -> VerifyCliOptions {
        match command_from(items).expect("parses") {
            Command::Verify(args) => VerifyCliOptions::from(args),
            other => panic!("expected verify, got {other:?}"),
        }
    }

    #[test]
    fn parses_ad_hoc_verify_flags() {
        let verify = verify_options(&[
            "verify",
            "--claim",
            "the bridge opened in 1998",
            "--source-url",
            "https://example.com/bridge",
            "--with-metadata",
        ]);
        assert_eq!(
            verify,
            VerifyCliOptions {
                claim: "the bridge opened in 1998".to_string(),
                source_url: "https://example.com/bridge".to_string(),
                include_metadata: true,
                debug_votes: false,
                repair: true,
                preceding_sentences: Vec::new(),
                source_file: None,
                source_body: None,
                models: None,
                article_title: None,
                metadata_json: None,
            }
        );
    }

    #[test]
    fn parses_offline_body_model_and_article_title_flags() {
        let verify = verify_options(&[
            "verify",
            "--claim",
            "c",
            "--source-url",
            "https://example.com",
            "--source-file",
            "/tmp/body.txt",
            "--models",
            "anthropic/claude-opus-4-8",
            "--article-title",
            "Morrison Bridge",
        ]);
        assert_eq!(verify.source_file.as_deref(), Some("/tmp/body.txt"));
        assert_eq!(verify.models.as_deref(), Some("anthropic/claude-opus-4-8"));
        assert_eq!(verify.article_title.as_deref(), Some("Morrison Bridge"));
    }

    #[test]
    fn source_file_and_source_body_are_mutually_exclusive() {
        assert!(
            command_from(&[
                "verify",
                "--claim",
                "c",
                "--source-url",
                "https://example.com",
                "--source-file",
                "/tmp/body.txt",
                "--source-body",
                "-",
            ])
            .is_err()
        );
    }

    #[test]
    fn resolve_source_body_reads_a_file_and_passes_literal_text() {
        let dir = std::env::temp_dir();
        let path = dir.join("sp42_resolve_source_body_test.txt");
        std::fs::write(&path, "the bridge opened in 2002").expect("write fixture");
        let from_file =
            resolve_source_body(Some(path.to_str().expect("utf8 path")), None).expect("reads file");
        assert_eq!(from_file.as_deref(), Some("the bridge opened in 2002"));
        std::fs::remove_file(&path).ok();

        let literal = resolve_source_body(None, Some("inline body")).expect("literal");
        assert_eq!(literal.as_deref(), Some("inline body"));

        assert_eq!(resolve_source_body(None, None).expect("none"), None);
    }

    #[test]
    fn prefetched_from_body_is_a_200_text_plain_source() {
        let fetched = prefetched_from_body("body text".to_string());
        assert_eq!(fetched.text, "body text");
        assert_eq!(fetched.status, 200);
        assert_eq!(fetched.content_type, "text/plain");
        assert!(fetched.raw_html.is_none());
    }

    #[test]
    fn parses_metadata_json_and_batch_flags() {
        let verify = verify_options(&[
            "verify",
            "--claim",
            "c",
            "--source-url",
            "https://example.com",
            "--metadata-json",
            "/tmp/meta.json",
        ]);
        assert_eq!(verify.metadata_json.as_deref(), Some("/tmp/meta.json"));

        // `batch` with no --file reads STDIN.
        match command_from(&["batch"]).expect("parses") {
            Command::Batch(args) => assert_eq!(args.file, None),
            other => panic!("expected batch, got {other:?}"),
        }
        // `batch --file <path>`.
        match command_from(&["batch", "--file", "/tmp/cases.jsonl"]).expect("parses") {
            Command::Batch(args) => assert_eq!(args.file.as_deref(), Some("/tmp/cases.jsonl")),
            other => panic!("expected batch, got {other:?}"),
        }
    }

    #[test]
    fn metadata_sidecar_file_fills_missing_url_from_source() {
        let file: MetadataSidecarFile =
            serde_json::from_str(r#"{"publication":"The Daily Example","author":"A. Writer"}"#)
                .expect("parses");
        let meta = file.into_citoid("https://example.com/article");
        assert_eq!(meta.publication.as_deref(), Some("The Daily Example"));
        assert_eq!(meta.author.as_deref(), Some("A. Writer"));
        assert_eq!(meta.url, "https://example.com/article");
        assert!(meta.title.is_none());
    }

    #[test]
    fn load_metadata_sidecar_reads_a_file_and_prefers_explicit_url() {
        let path = std::env::temp_dir().join("sp42_metadata_sidecar_test.json");
        std::fs::write(
            &path,
            r#"{"title":"A Title","url":"https://override.example/x"}"#,
        )
        .expect("write fixture");
        let meta = load_metadata_sidecar(path.to_str().expect("utf8 path"), "https://fallback")
            .expect("loads");
        assert_eq!(meta.title.as_deref(), Some("A Title"));
        assert_eq!(meta.url, "https://override.example/x");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn batch_case_parses_with_repair_defaulting_true() {
        let case: BatchCase = serde_json::from_str(
            r#"{"id":"c1","claim":"the bridge opened in 1998","source_url":"https://example.com/b","source_body":"text"}"#,
        )
        .expect("parses");
        assert_eq!(case.id.as_deref(), Some("c1"));
        assert!(case.repair, "repair defaults to true");
        assert!(!case.with_metadata);
        let vc = case.into_verify_case();
        assert_eq!(vc.claim, "the bridge opened in 1998");
        assert_eq!(vc.source_body.as_deref(), Some("text"));
        assert!(vc.metadata.is_none());
    }

    #[test]
    fn recover_case_id_keeps_schema_errors_attributable() {
        // Valid JSON missing the required `claim` still fails BatchCase, but its id is recoverable.
        let schema_error = r#"{"id":"c7","source_url":"https://example.com/b"}"#;
        assert!(serde_json::from_str::<BatchCase>(schema_error).is_err());
        assert_eq!(recover_case_id(schema_error).as_deref(), Some("c7"));

        // No id, non-string id, and non-JSON all recover to None (not attributable).
        assert_eq!(recover_case_id(r#"{"claim":"c"}"#), None);
        assert_eq!(recover_case_id(r#"{"id":42}"#), None);
        assert_eq!(recover_case_id("not json"), None);
    }

    #[test]
    fn batch_case_metadata_inherits_source_url_when_url_absent() {
        let case: BatchCase = serde_json::from_str(
            r#"{"claim":"c","source_url":"https://example.com/b","metadata":{"publication":"P"}}"#,
        )
        .expect("parses");
        let vc = case.into_verify_case();
        let meta = vc.metadata.expect("metadata present");
        assert_eq!(meta.publication.as_deref(), Some("P"));
        assert_eq!(meta.url, "https://example.com/b");
    }

    #[test]
    fn verify_collects_preceding_context() {
        let verify = verify_options(&[
            "verify",
            "--claim",
            "c",
            "--source-url",
            "https://example.com",
            "--preceding-sentence",
            "She joined in 1985.",
            "--preceding-sentence",
            "She scored twice.",
        ]);
        assert_eq!(
            verify.preceding_sentences,
            vec![
                "She joined in 1985.".to_string(),
                "She scored twice.".to_string()
            ]
        );
    }

    #[test]
    fn no_repair_flag_disables_the_repair_turn() {
        let verify = verify_options(&[
            "verify",
            "--claim",
            "c",
            "--source-url",
            "https://example.com",
            "--no-repair",
        ]);
        assert!(!verify.repair);
    }

    #[test]
    fn debug_votes_flag_is_recognized() {
        let verify = verify_options(&[
            "verify",
            "--claim",
            "c",
            "--source-url",
            "https://example.com",
            "--debug-votes",
        ]);
        assert!(verify.debug_votes);
    }

    #[test]
    fn verdict_only_flag_is_recognized() {
        match command_from(&[
            "verify",
            "--claim",
            "c",
            "--source-url",
            "https://example.com",
            "--verdict-only",
        ])
        .expect("parses")
        {
            Command::Verify(args) => assert!(args.output.verdict_only),
            other => panic!("expected verify, got {other:?}"),
        }
    }

    #[test]
    fn verify_requires_both_claim_and_source_url() {
        assert!(command_from(&["verify", "--claim", "only a claim"]).is_err());
        assert!(command_from(&["verify", "--source-url", "https://example.com"]).is_err());
    }

    #[test]
    fn locate_probe_carries_the_quote() {
        match command_from(&["locate-probe", "--quote", "the Nobel Prize"]).expect("parses") {
            Command::LocateProbe(args) => assert_eq!(args.quote, "the Nobel Prize"),
            other => panic!("expected locate-probe, got {other:?}"),
        }
    }

    #[test]
    fn locate_probe_requires_a_quote() {
        assert!(command_from(&["locate-probe"]).is_err());
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
            source_unavailable_reason: None,
            unusable_reason: None,
            provenance: SourceProvenance {
                url: "https://example.com/bridge".parse().expect("url"),
                content_hash: "abc123".to_string(),
                fetched_at: 1,
                http_status: Some(200),
            },
            source_excerpt: None,
            metadata: None,
            grounding: GroundingAssertion::LocatedQuote {
                quote: "opened in 1998".to_string(),
                source_hash: "abc123".to_string(),
                offset: 4,
            },
            use_site_ordinal: 0,
            ref_id: String::new(),
            claim: String::new(),
            preceding_context: Vec::new(),
            archive_of: None,
            is_bare_url_ref: false,
            book_scan: None,
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
    shell_state: &'a sp42_patrol::ShellStateModel,
    scenario: &'a sp42_patrol::PatrolScenarioReport,
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

fn render_verify_page(
    options: &VerifyPageCliOptions,
    bridge_base_url: &str,
    format: OutputFormat,
) -> Result<String, String> {
    // `reqwest` needs a Tokio reactor; drive the bridge calls on a real runtime
    // (mirrors `run_verify`), not `futures::executor::block_on`.
    let runtime = tokio::runtime::Runtime::new()
        .map_err(|error| format!("failed to start async runtime: {error}"))?;
    let report = runtime.block_on(fetch_page_report_via_bridge(bridge_base_url, options))?;
    match format {
        OutputFormat::Json => {
            serde_json::to_string_pretty(&report).map_err(|error| error.to_string())
        }
        // The shared renderer already emits a full markdown document (`# Page citation
        // report`), so it is not wrapped in render_markdown_section.
        OutputFormat::Markdown => Ok(render_page_verification_markdown(&report)),
        OutputFormat::Text => Ok(render_page_verification_text(&report)),
    }
}

/// Verify every citation on a page through the bridge and return the report. The
/// route is session+CSRF gated (it spends the server's inference credentials on a
/// caller-chosen page), so bootstrap a session and send the cookie + CSRF token
/// (ADR-0002), mirroring the bare-url apply path.
async fn fetch_page_report_via_bridge(
    bridge_base_url: &str,
    options: &VerifyPageCliOptions,
) -> Result<PageVerificationReport, String> {
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

    let request = PageVerificationRequest {
        wiki_id: options.wiki_id.clone(),
        title: options.title.clone(),
        rev_id: options.rev_id,
    };
    let request_url = format!(
        "{bridge_base_url}{}",
        route_contracts::DEV_CITATION_VERIFY_PAGE_PATH
    );
    let response = client
        .post(&request_url)
        .header(COOKIE, session_cookie.as_str())
        .header(route_contracts::CSRF_HEADER_NAME, csrf_token.as_str())
        .json(&request)
        .send()
        .await
        .map_err(|error| format!("verify-page request failed: {error}"))?
        .error_for_status()
        .map_err(|error| format!("verify-page request failed: {error}"))?;
    response
        .json::<PageVerificationReport>()
        .await
        .map_err(|error| format!("verify-page payload was invalid: {error}"))
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

/// An authenticated bridge session: the reqwest client plus the cookie and
/// CSRF token every gated review request must carry (ADR-0002).
struct BridgeSession {
    client: reqwest::Client,
    cookie: String,
    csrf_token: String,
}

/// Bootstrap the local dev-auth bridge and capture the session cookie and
/// CSRF token — the shared preamble of every gated bridge call.
async fn bootstrap_bridge_session(base_url: &str) -> Result<BridgeSession, String> {
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
    let cookie = session_cookie_from_headers(&bootstrap_response.headers)
        .ok_or_else(|| "bridge bootstrap did not set a session cookie".to_string())?;
    let csrf_token = bootstrap
        .csrf_token
        .ok_or_else(|| "bridge bootstrap did not return a CSRF token".to_string())?;
    Ok(BridgeSession {
        client,
        cookie,
        csrf_token,
    })
}

/// POST a JSON body to a gated review route and parse the JSON response.
async fn post_review_route<Request, Response>(
    bridge: &BridgeSession,
    base_url: &str,
    path: &str,
    request: &Request,
) -> Result<Response, String>
where
    Request: serde::Serialize,
    Response: serde::de::DeserializeOwned,
{
    let response = bridge
        .client
        .post(format!("{base_url}{path}"))
        .header(COOKIE, bridge.cookie.as_str())
        .header(
            route_contracts::CSRF_HEADER_NAME,
            bridge.csrf_token.as_str(),
        )
        .json(request)
        .send()
        .await
        .map_err(|error| format!("review request to {path} failed: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        // Keep the server's JSON detail: refusals like the operator-ended
        // reopen gate carry their etiquette guidance in the body.
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "review request to {path} failed: HTTP {status}: {body}"
        ));
    }
    response
        .json::<Response>()
        .await
        .map_err(|error| format!("review response from {path} was invalid: {error}"))
}

/// Route a `review` subcommand to its runner on a fresh Tokio runtime
/// (`reqwest` needs a reactor; mirrors `render_verify_page`).
fn dispatch_review(action: ReviewAction) -> Result<String, String> {
    let runtime = tokio::runtime::Runtime::new()
        .map_err(|error| format!("failed to start async runtime: {error}"))?;
    match action {
        ReviewAction::Open(args) => {
            let format = args.fmt.format;
            let response = runtime.block_on(run_review_open(&args))?;
            render_review_output(format, &response, render_review_open_text(&response))
        }
        ReviewAction::Poll(args) => {
            let format = args.fmt.format;
            let response = runtime.block_on(run_review_poll(&args))?;
            render_review_output(format, &response, render_review_poll_text(&response))
        }
        ReviewAction::Queue(args) => {
            let format = args.fmt.format;
            let response = runtime.block_on(run_review_queue(&args))?;
            let text = format!(
                "queued {} prompt(s)\n{}",
                response.queued,
                render_review_snapshot_line(&response.session)
            );
            render_review_output(format, &response, text)
        }
        ReviewAction::Findings(args) => {
            let format = args.fmt.format;
            let response = runtime.block_on(run_review_findings(&args))?;
            let text = format!(
                "attached {} finding marker(s)\n{}",
                response.attached,
                render_review_snapshot_line(&response.session)
            );
            render_review_output(format, &response, text)
        }
        ReviewAction::Reply(args) => {
            let format = args.fmt.format;
            let response = runtime.block_on(run_review_reply(&args))?;
            let text = format!(
                "reply delivered\n{}",
                render_review_snapshot_line(&response.session)
            );
            render_review_output(format, &response, text)
        }
        ReviewAction::End(args) => {
            let format = args.fmt.format;
            let response = runtime.block_on(run_review_end(&args))?;
            let text = format!(
                "session ended by agent\n{}",
                render_review_snapshot_line(&response.session)
            );
            render_review_output(format, &response, text)
        }
        ReviewAction::Sessions(args) => {
            let format = args.fmt.format;
            let response = runtime.block_on(run_review_sessions(&args))?;
            render_review_output(format, &response, render_review_sessions_text(&response))
        }
    }
}

async fn run_review_open(args: &ReviewOpenArgs) -> Result<ReviewOpenResponse, String> {
    let bridge = bootstrap_bridge_session(&args.bridge_base_url).await?;
    let request = ReviewOpenRequest {
        wiki_id: args.wiki.clone(),
        target: args.target.clone(),
        rev_id: args.rev.unwrap_or(0),
        reopen: args.reopen,
    };
    post_review_route(
        &bridge,
        &args.bridge_base_url,
        route_contracts::DEV_REVIEW_OPEN_PATH,
        &request,
    )
    .await
}

/// Wait for operator feedback. Each request is a bounded server-side wait;
/// the loop re-arms until feedback, an end, or a missing session, so from
/// the agent's side this behaves like one long poll. Waiting narration goes
/// to stderr; stdout stays reserved for the final structured response.
async fn run_review_poll(args: &ReviewPollArgs) -> Result<ReviewPollResponse, String> {
    let bridge = bootstrap_bridge_session(&args.bridge_base_url).await?;
    let title = sp42_core::parse_page_target(&args.target).title;
    if let Some(reply) = &args.agent_reply {
        let request = ReviewReplyRequest {
            wiki_id: args.wiki.clone(),
            title: title.clone(),
            text: reply.clone(),
        };
        let _: ReviewAckResponse = post_review_route(
            &bridge,
            &args.bridge_base_url,
            route_contracts::DEV_REVIEW_REPLY_PATH,
            &request,
        )
        .await?;
    }
    let request = ReviewPollRequest {
        wiki_id: args.wiki.clone(),
        title: title.clone(),
        wait_ms: 0,
    };
    loop {
        let response: ReviewPollResponse = post_review_route(
            &bridge,
            &args.bridge_base_url,
            route_contracts::DEV_REVIEW_POLL_PATH,
            &request,
        )
        .await?;
        if args.once || response.status != ReviewPollStatus::Waiting {
            return Ok(response);
        }
        eprintln!(
            "review poll: still waiting for operator feedback on {title} \
             (re-arming; queued feedback is never lost)"
        );
    }
}

/// Build the queue prompt from CLI anchor flags: `--selected-text` makes a
/// text prompt, `--block`/`--ref-id` a block prompt, neither a free-form
/// message. Anchored prompts require `--block` so the anchor is complete.
fn build_review_queue_prompt(args: &ReviewQueueArgs) -> Result<ReviewPrompt, String> {
    let anchored = args.ref_id.is_some() || args.selected_text.is_some();
    let Some(block_ordinal) = args.block else {
        if anchored {
            return Err("--ref-id/--selected-text need --block to anchor the prompt".to_string());
        }
        return Ok(ReviewPrompt {
            kind: ReviewPromptKind::Message,
            prompt: args.message.clone(),
            anchor: None,
        });
    };
    let kind = if args.selected_text.is_some() {
        ReviewPromptKind::Text
    } else {
        ReviewPromptKind::Block
    };
    Ok(ReviewPrompt {
        kind,
        prompt: args.message.clone(),
        anchor: Some(ReviewAnchor {
            block_ordinal,
            ref_id: args.ref_id.clone(),
            selected_text: args.selected_text.clone(),
        }),
    })
}

async fn run_review_queue(args: &ReviewQueueArgs) -> Result<ReviewQueueResponse, String> {
    let bridge = bootstrap_bridge_session(&args.bridge_base_url).await?;
    let request = ReviewQueueRequest {
        wiki_id: args.wiki.clone(),
        title: sp42_core::parse_page_target(&args.target).title,
        prompts: vec![build_review_queue_prompt(args)?],
        end_session: args.end,
    };
    post_review_route(
        &bridge,
        &args.bridge_base_url,
        route_contracts::DEV_REVIEW_PROMPTS_PATH,
        &request,
    )
    .await
}

/// Read a `verify-page --format json` report from a file or stdin (`-`).
fn read_verification_report(path: &str) -> Result<PageVerificationReport, String> {
    let raw = if path == "-" {
        let mut buffer = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buffer)
            .map_err(|error| format!("failed to read report from stdin: {error}"))?;
        buffer
    } else {
        std::fs::read_to_string(path)
            .map_err(|error| format!("failed to read report {path}: {error}"))?
    };
    serde_json::from_str(&raw)
        .map_err(|error| format!("report is not a verify-page JSON report: {error}"))
}

/// Attach a verify-page report to the review session: project its findings
/// onto review anchors (ref ids) and post them, pinned to the report's
/// revision so a stale report cannot overlay a newer session.
async fn run_review_findings(args: &ReviewFindingsArgs) -> Result<ReviewFindingsResponse, String> {
    let report = read_verification_report(&args.report)?;
    let bridge = bootstrap_bridge_session(&args.bridge_base_url).await?;
    let request = ReviewFindingsRequest {
        wiki_id: args.wiki.clone(),
        title: sp42_core::parse_page_target(&args.target).title,
        rev_id: report.rev_id,
        findings: review_finding_markers(&report),
    };
    post_review_route(
        &bridge,
        &args.bridge_base_url,
        route_contracts::DEV_REVIEW_FINDINGS_PATH,
        &request,
    )
    .await
}

async fn run_review_reply(args: &ReviewReplyArgs) -> Result<ReviewAckResponse, String> {
    let bridge = bootstrap_bridge_session(&args.bridge_base_url).await?;
    let request = ReviewReplyRequest {
        wiki_id: args.wiki.clone(),
        title: sp42_core::parse_page_target(&args.target).title,
        text: args.message.clone(),
    };
    post_review_route(
        &bridge,
        &args.bridge_base_url,
        route_contracts::DEV_REVIEW_REPLY_PATH,
        &request,
    )
    .await
}

async fn run_review_end(args: &ReviewEndArgs) -> Result<ReviewAckResponse, String> {
    let bridge = bootstrap_bridge_session(&args.bridge_base_url).await?;
    let request = ReviewEndRequest {
        wiki_id: args.wiki.clone(),
        title: sp42_core::parse_page_target(&args.target).title,
    };
    post_review_route(
        &bridge,
        &args.bridge_base_url,
        route_contracts::DEV_REVIEW_END_PATH,
        &request,
    )
    .await
}

async fn run_review_sessions(args: &ReviewSessionsArgs) -> Result<ReviewSessionsResponse, String> {
    let client = reqwest::Client::builder()
        .user_agent(sp42_core::branding::USER_AGENT)
        .build()
        .map_err(|error| format!("bridge client failed to build: {error}"))?;
    let url = format!(
        "{}{}",
        args.bridge_base_url,
        route_contracts::DEV_REVIEW_SESSIONS_PATH
    );
    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|error| format!("review sessions request failed: {error}"))?
        .error_for_status()
        .map_err(|error| format!("review sessions request failed: {error}"))?;
    response
        .json::<ReviewSessionsResponse>()
        .await
        .map_err(|error| format!("review sessions payload was invalid: {error}"))
}

/// Shared review output shaping: JSON is the machine contract; text and
/// markdown share the same line-oriented rendering.
fn render_review_output<T: serde::Serialize>(
    format: OutputFormat,
    value: &T,
    text: String,
) -> Result<String, String> {
    match format {
        OutputFormat::Json => {
            serde_json::to_string_pretty(value).map_err(|error| error.to_string())
        }
        OutputFormat::Text => Ok(text),
        OutputFormat::Markdown => Ok(format!("## Review session\n\n```\n{text}\n```")),
    }
}

fn render_review_snapshot_line(session: &ReviewSessionSnapshot) -> String {
    let ended = session.ended_by.map_or(String::new(), |by| {
        format!(", ended by {by:?}").to_lowercase()
    });
    format!(
        "session: {} ({}) rev {} — {:?}, {} pending prompt(s){}",
        session.title,
        session.wiki_id,
        session.rev_id,
        session.status,
        session.pending_prompts,
        ended
    )
}

fn render_review_open_text(response: &ReviewOpenResponse) -> String {
    let unanchored = if response.unanchored_findings.is_empty() {
        String::new()
    } else {
        format!(
            "\nunanchored findings: {} (attached but matching no outline block)",
            response.unanchored_findings.len()
        )
    };
    format!(
        "{}\noutline: {} block(s), {} finding(s) attached{}\nnext: {}",
        render_review_snapshot_line(&response.session),
        response.outline.len(),
        response.session.findings,
        unanchored,
        response.next_step
    )
}

fn render_review_prompt_line(prompt: &ReviewPrompt) -> String {
    let anchor = prompt.anchor.as_ref().map_or(String::new(), |anchor| {
        let ref_part = anchor
            .ref_id
            .as_deref()
            .map_or(String::new(), |ref_id| format!(", ref {ref_id}"));
        let text_part = anchor
            .selected_text
            .as_deref()
            .map_or(String::new(), |text| format!(", text \"{text}\""));
        format!(" (block {}{ref_part}{text_part})", anchor.block_ordinal)
    });
    format!("- [{:?}] {}{anchor}", prompt.kind, prompt.prompt).to_string()
}

fn render_review_poll_text(response: &ReviewPollResponse) -> String {
    let mut lines = vec![format!("status: {:?}", response.status).to_lowercase()];
    lines.extend(response.prompts.iter().map(render_review_prompt_line));
    if let Some(ended_by) = response.ended_by {
        lines.push(format!("ended by: {ended_by:?}").to_lowercase());
    }
    lines.push(format!("next: {}", response.next_step));
    lines.join("\n")
}

fn render_review_sessions_text(response: &ReviewSessionsResponse) -> String {
    if response.sessions.is_empty() {
        return "no review sessions".to_string();
    }
    response
        .sessions
        .iter()
        .map(render_review_snapshot_line)
        .collect::<Vec<_>>()
        .join("\n")
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
    use serde_json::Value;
    use std::collections::BTreeMap;

    use super::{
        ActionKind, BareUrlAction, BareUrlCliMode, BareUrlCliOptions, BareUrlExecuteReport, Cli,
        CliOptions, Command, ContextPreviewOptions, FormatArg, LOCAL_SERVER_BASE_URL, OutputFormat,
        ReviewAction, ReviewAnchor, ReviewPollResponse, ReviewPollStatus, ReviewPrompt,
        ReviewPromptKind, ReviewQueueArgs, ShellMode, VerifyPageCliOptions, WorkbenchOptions,
        build_review_queue_prompt, read_verification_report, render_action_preview,
        render_backlog_preview, render_bare_url_execute, render_bare_url_proposals,
        render_context_preview, render_coordination_preview, render_parity_report, render_queue,
        render_review_poll_text, render_scenario_report, render_session_digest,
        render_stream_preview, render_workbench, select_bare_url_proposal, server_report_lines,
    };
    use clap::Parser;
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

    /// Parse a full argv (program name implied) into the top-level subcommand.
    fn command_from(items: &[&str]) -> Result<Command, clap::Error> {
        let argv = std::iter::once("sp42-cli").chain(items.iter().copied());
        Cli::try_parse_from(argv).map(|cli| cli.command)
    }

    fn preview_args(items: &[&str]) -> super::PreviewArgs {
        match command_from(items).expect("parses") {
            Command::Preview(args) => args,
            other => panic!("expected preview, got {other:?}"),
        }
    }

    #[test]
    fn parses_requested_output_format() {
        assert_eq!(
            preview_args(&["preview", "--format", "json"]).fmt.format,
            OutputFormat::Json
        );
    }

    #[test]
    fn parses_markdown_output_format() {
        assert_eq!(
            preview_args(&["preview", "--format", "markdown"])
                .fmt
                .format,
            OutputFormat::Markdown
        );
    }

    #[test]
    fn defaults_to_text_output_format() {
        assert_eq!(preview_args(&["preview"]).fmt.format, OutputFormat::Text);
    }

    #[test]
    fn parses_workbench_options() {
        let args = preview_args(&[
            "preview",
            "--workbench-token",
            "token-123",
            "--workbench-actor",
            "Reviewer",
            "--workbench-note",
            "local workbench",
        ]);
        assert_eq!(args.workbench_token.as_deref(), Some("token-123"));
        assert_eq!(args.workbench_actor, "Reviewer");
        assert_eq!(args.workbench_note, "local workbench");
    }

    #[test]
    fn workbench_actor_and_note_have_defaults() {
        let args = preview_args(&["preview"]);
        assert_eq!(args.workbench_actor, "SP42-cli");
        assert_eq!(args.workbench_note, "cli local workbench");
    }

    #[test]
    fn parses_verify_page_options() {
        let options = match command_from(&[
            "verify-page",
            "--wiki",
            "enwiki",
            "--title",
            "Museum",
            "--rev",
            "12345",
        ])
        .expect("parses")
        {
            Command::VerifyPage(args) => VerifyPageCliOptions::from(args),
            other => panic!("expected verify-page, got {other:?}"),
        };
        assert_eq!(
            options,
            VerifyPageCliOptions {
                wiki_id: "enwiki".to_string(),
                title: "Museum".to_string(),
                rev_id: 12345,
            }
        );
    }

    #[test]
    fn verify_page_without_rev_defaults_to_latest() {
        let options = match command_from(&["verify-page", "--wiki", "enwiki", "--title", "Museum"])
            .expect("parses")
        {
            Command::VerifyPage(args) => VerifyPageCliOptions::from(args),
            other => panic!("expected verify-page, got {other:?}"),
        };
        assert_eq!(options.rev_id, 0);
    }

    #[test]
    fn verify_page_requires_title() {
        assert!(command_from(&["verify-page", "--wiki", "enwiki"]).is_err());
    }

    #[test]
    fn parses_context_preview_options() {
        let args = preview_args(&[
            "preview",
            "--context-talk",
            "{{Avertissement niveau 1}}",
            "--context-liftwing",
            "0.42",
        ]);
        // Compare through the struct's derived PartialEq (avoids a bare float `==`).
        assert_eq!(
            ContextPreviewOptions {
                talk_page: args.context_talk.clone(),
                liftwing_probability: args.context_liftwing,
            },
            ContextPreviewOptions {
                talk_page: Some("{{Avertissement niveau 1}}".to_string()),
                liftwing_probability: Some(0.42),
            }
        );
    }

    #[test]
    fn parses_preview_mode_positional() {
        assert_eq!(
            preview_args(&["preview", "stream"]).mode,
            Some(ShellMode::Stream)
        );
        assert_eq!(preview_args(&["preview"]).mode, None);
    }

    #[test]
    fn every_preview_mode_value_parses() {
        let cases = [
            ("stream", ShellMode::Stream),
            ("backlog", ShellMode::Backlog),
            ("coordination", ShellMode::Coordination),
            ("session-digest", ShellMode::SessionDigest),
            ("scenario-report", ShellMode::ScenarioReport),
            ("server-report", ShellMode::ServerReport),
            ("parity-report", ShellMode::ParityReport),
            ("action-preview", ShellMode::ActionPreview),
            ("action-execute", ShellMode::ActionExecute),
        ];
        for (value, expected) in cases {
            assert_eq!(
                preview_args(&["preview", value]).mode,
                Some(expected),
                "mode {value}"
            );
        }
    }

    #[test]
    fn rejects_unknown_preview_mode() {
        assert!(command_from(&["preview", "bogus-mode"]).is_err());
    }

    #[test]
    fn parses_shell_and_action_options() {
        let args = preview_args(&[
            "preview",
            "action-preview",
            "--action-kind",
            "undo",
            "--action-note",
            "inspect",
            "--bridge-base-url",
            "http://127.0.0.1:9000",
        ]);
        assert_eq!(args.mode, Some(ShellMode::ActionPreview));
        assert_eq!(args.action_kind, ActionKind::Undo);
        assert_eq!(
            sp42_core::SessionActionKind::from(args.action_kind),
            sp42_core::SessionActionKind::Undo
        );
        assert_eq!(args.action_note.as_deref(), Some("inspect"));
        assert_eq!(args.bridge_base_url, "http://127.0.0.1:9000");
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
        let config = fixture_config();
        let ranked = fixture_queue(&config);

        let summary = render_scenario_report(
            &config,
            &ranked,
            &CliOptions {
                format: OutputFormat::Markdown,
                workbench: None,
                context_preview: None,
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
        let config = fixture_config();
        let ranked = fixture_queue(&config);

        let summary = render_scenario_report(
            &config,
            &ranked,
            &CliOptions {
                format: OutputFormat::Json,
                workbench: None,
                context_preview: None,
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
        let config = fixture_config();
        let ranked = fixture_queue(&config);
        let options = CliOptions {
            format: OutputFormat::Text,
            workbench: None,
            context_preview: None,
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

    #[test]
    fn parses_bare_url_preview_flags() {
        match command_from(&["bare-url", "preview", "--title", "Sandbox", "--rev", "123"])
            .expect("parses")
        {
            Command::BareUrl(args) => match args.action {
                BareUrlAction::Preview(preview) => {
                    assert_eq!(preview.wiki, "testwiki");
                    assert_eq!(preview.title, "Sandbox");
                    assert_eq!(preview.rev, 123);
                    // bridge URL defaults to the local server.
                    assert_eq!(preview.bridge_base_url, LOCAL_SERVER_BASE_URL);
                }
                other @ BareUrlAction::Execute(_) => panic!("expected preview, got {other:?}"),
            },
            other => panic!("expected bare-url, got {other:?}"),
        }
    }

    #[test]
    fn bare_url_preview_accepts_a_custom_bridge_url() {
        match command_from(&[
            "bare-url",
            "preview",
            "--title",
            "Sandbox",
            "--rev",
            "1",
            "--bridge-base-url",
            "http://127.0.0.1:9999",
        ])
        .expect("parses")
        {
            Command::BareUrl(args) => match args.action {
                BareUrlAction::Preview(preview) => {
                    assert_eq!(preview.bridge_base_url, "http://127.0.0.1:9999");
                }
                other @ BareUrlAction::Execute(_) => panic!("expected preview, got {other:?}"),
            },
            other => panic!("expected bare-url, got {other:?}"),
        }
    }

    #[test]
    fn parses_bare_url_execute_flags_with_wiki_override() {
        match command_from(&[
            "bare-url",
            "execute",
            "--title",
            "Sandbox",
            "--rev",
            "123",
            "--ordinal",
            "2",
            "--wiki",
            "frwiki",
        ])
        .expect("parses")
        {
            Command::BareUrl(args) => match args.action {
                BareUrlAction::Execute(execute) => {
                    assert_eq!(execute.ordinal, 2);
                    assert_eq!(execute.wiki, "frwiki");
                    assert_eq!(execute.title, "Sandbox");
                    assert_eq!(execute.rev, 123);
                }
                other @ BareUrlAction::Preview(_) => panic!("expected execute, got {other:?}"),
            },
            other => panic!("expected bare-url, got {other:?}"),
        }
    }

    #[test]
    fn bare_url_modes_validate_required_flags() {
        // preview requires --title and --rev
        assert!(command_from(&["bare-url", "preview", "--rev", "1"]).is_err());
        assert!(command_from(&["bare-url", "preview", "--title", "T"]).is_err());
        // --rev must be a number
        assert!(command_from(&["bare-url", "preview", "--title", "T", "--rev", "abc"]).is_err());
        // execute additionally requires --ordinal
        assert!(command_from(&["bare-url", "execute", "--title", "T", "--rev", "1"]).is_err());
        // bare-url requires a preview|execute subcommand
        assert!(command_from(&["bare-url"]).is_err());
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

    #[test]
    fn review_open_args_parse_target_wiki_rev_and_reopen() {
        let cli = Cli::parse_from([
            "sp42-cli",
            "review",
            "open",
            "https://en.wikipedia.org/wiki/Example",
            "--wiki",
            "enwiki",
            "--rev",
            "5",
            "--reopen",
        ]);
        let Command::Review(args) = cli.command else {
            panic!("expected review command");
        };
        let ReviewAction::Open(open) = args.action else {
            panic!("expected open action");
        };
        assert_eq!(open.target, "https://en.wikipedia.org/wiki/Example");
        assert_eq!(open.wiki, "enwiki");
        assert_eq!(open.rev, Some(5));
        assert!(open.reopen);
        assert_eq!(open.bridge_base_url, LOCAL_SERVER_BASE_URL);
    }

    #[test]
    fn parses_review_findings_action() {
        let cli = Cli::parse_from([
            "sp42-cli",
            "review",
            "findings",
            "Exemple",
            "--report",
            "report.json",
            "--wiki",
            "frwiki",
        ]);
        let Command::Review(args) = cli.command else {
            panic!("expected review command");
        };
        let ReviewAction::Findings(findings) = args.action else {
            panic!("expected findings action");
        };
        assert_eq!(findings.target, "Exemple");
        assert_eq!(findings.report, "report.json");
        assert_eq!(findings.wiki, "frwiki");
    }

    #[test]
    fn read_verification_report_rejects_non_report_json() {
        let dir =
            std::env::temp_dir().join(format!("sp42-cli-review-findings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir should create");
        let path = dir.join("not-a-report.json");
        std::fs::write(&path, "{\"outcome\": true}").expect("temp file should write");

        let error = read_verification_report(path.to_str().expect("utf-8 path"))
            .expect_err("non-report JSON should be rejected");
        assert!(error.contains("not a verify-page JSON report"));
    }

    #[test]
    fn review_queue_prompt_builds_each_anchor_shape() {
        let base = ReviewQueueArgs {
            target: "Exemple".to_string(),
            message: "tighten this".to_string(),
            wiki: "frwiki".to_string(),
            block: None,
            ref_id: None,
            selected_text: None,
            end: false,
            bridge_base_url: LOCAL_SERVER_BASE_URL.to_string(),
            fmt: FormatArg {
                format: OutputFormat::Text,
            },
        };

        let message = build_review_queue_prompt(&base).expect("message prompt should build");
        assert_eq!(message.kind, ReviewPromptKind::Message);
        assert!(message.anchor.is_none());

        let block = build_review_queue_prompt(&ReviewQueueArgs {
            block: Some(3),
            ref_id: Some("cite_ref-a_1-0".to_string()),
            ..clone_review_queue_args(&base)
        })
        .expect("block prompt should build");
        assert_eq!(block.kind, ReviewPromptKind::Block);
        let anchor = block.anchor.expect("block prompt should carry an anchor");
        assert_eq!(anchor.block_ordinal, 3);
        assert_eq!(anchor.ref_id.as_deref(), Some("cite_ref-a_1-0"));

        let text = build_review_queue_prompt(&ReviewQueueArgs {
            block: Some(2),
            selected_text: Some("the quote".to_string()),
            ..clone_review_queue_args(&base)
        })
        .expect("text prompt should build");
        assert_eq!(text.kind, ReviewPromptKind::Text);

        let missing_block = build_review_queue_prompt(&ReviewQueueArgs {
            ref_id: Some("cite_ref-a_1-0".to_string()),
            ..clone_review_queue_args(&base)
        })
        .expect_err("an anchored prompt without --block must refuse");
        assert!(missing_block.contains("--block"));
    }

    fn clone_review_queue_args(args: &ReviewQueueArgs) -> ReviewQueueArgs {
        ReviewQueueArgs {
            target: args.target.clone(),
            message: args.message.clone(),
            wiki: args.wiki.clone(),
            block: args.block,
            ref_id: args.ref_id.clone(),
            selected_text: args.selected_text.clone(),
            end: args.end,
            bridge_base_url: args.bridge_base_url.clone(),
            fmt: FormatArg {
                format: OutputFormat::Text,
            },
        }
    }

    #[test]
    fn review_poll_text_lists_prompts_and_next_step() {
        let response = ReviewPollResponse {
            contract_version: 1,
            status: ReviewPollStatus::Feedback,
            prompts: vec![ReviewPrompt {
                kind: ReviewPromptKind::Text,
                prompt: "check this quote".to_string(),
                anchor: Some(ReviewAnchor {
                    block_ordinal: 3,
                    ref_id: Some("cite_ref-x_2-0".to_string()),
                    selected_text: Some("the quote".to_string()),
                }),
            }],
            session_ended: false,
            ended_by: None,
            next_step: "apply the feedback".to_string(),
        };

        let text = render_review_poll_text(&response);

        assert!(text.contains("status: feedback"));
        assert!(text.contains("check this quote"));
        assert!(text.contains("block 3"));
        assert!(text.contains("cite_ref-x_2-0"));
        assert!(text.contains("next: apply the feedback"));
    }
}
