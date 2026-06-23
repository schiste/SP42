# 04 — sp42-cli structure (map for a read-only `verify` citation subcommand)

Research target: how `crates/sp42-cli` is built today, so a new **read-only citation
verify** capability (PRD-0001 Surface + DoD item 8) can be added *in the house style*.

Files read:
- `crates/sp42-cli/src/main.rs` (2547 lines)
- `crates/sp42-cli/Cargo.toml`
- supporting: `sp42-types/src/transport.rs`, `sp42-types/src/traits.rs`,
  `sp42-core/src/dev_auth.rs`, `sp42-core/src/wiki_storage.rs`,
  `sp42-core/src/article_inventory.rs`, `sp42-server/src/runtime_adapters.rs`.

> **Headline reality check:** this CLI is **NOT clap and NOT tokio and does NOT depend on
> `sp42-server`.** It is a hand-rolled flag parser, stdin-driven, with `futures::executor::block_on`
> for async, and a flag-selected "mode" dispatch (no subcommand verbs at all). Any port that
> assumes `clap` / `#[tokio::main]` / a `Command` enum is wrong for this repo. See below.

---

## 0. Cargo.toml — exact dependency set

```toml
[package]
name = "sp42-cli"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[lints]
workspace = true

[dependencies]
futures.workspace = true        # ONLY for futures::executor::block_on
reqwest.workspace = true        # the real HttpClient is built inline here
serde.workspace = true
serde_json.workspace = true
sp42-core = { path = "../sp42-core" }
sp42-devtools = { path = "../sp42-devtools" }
sp42-reporting = { path = "../sp42-reporting" }
sp42-types = { path = "../sp42-types" }
```

**There is NO `clap`. NO `tokio`. NO `sp42-server` dep. NO `async-trait` dep here.**
A `verify` feature that needs to fetch over HTTP will most likely add `sp42-wiki` (for
`WikiConfig`) and whatever new `sp42-core` citation module gets built; it should NOT pull in
`clap`/`tokio` just to match other ecosystems — that would break the established style and the
workspace lint posture.

---

## 1. Arg parsing — HAND-ROLLED, no clap, NO subcommand verbs

`fn main() -> ExitCode` (line 88) is trivial:

```rust
fn main() -> ExitCode {
    match run() {
        Ok(summary) => { println!("{summary}"); ExitCode::SUCCESS }
        Err(error)  => { eprintln!("{error}"); ExitCode::from(1) }
    }
}
```

`run()` (line 101) drives everything:

```rust
fn run() -> Result<String, String> {
    let options = parse_options(std::env::args().skip(1))?;   // hand-rolled
    let input = read_stdin().map_err(|e| e.to_string())?;     // payload comes from STDIN
    let payload = if input.trim().is_empty() { DEV_PREVIEW_SAMPLE_EVENTS } else { input.as_str() };

    let config = parse_default_dev_wiki_config().map_err(|e| e.to_string())?;
    let ranked = load_ranked_queue(&config, payload)?;

    match selected_shell_mode(&options) {
        Some(ShellMode::ParityReport) => return render_parity_report(&config, &ranked, payload, options.format),
        Some(ShellMode::Stream)       => return render_stream_preview(&config, payload, options.format),
        // ... one arm per mode ...
        None => {}
    }
    // fall-through default rendering (workbench / context / ranked queue)
}
```

### There are NO positional subcommands. "Modes" are selected by FLAGS.

The dispatch surface is two parallel enums, not a `clap`-style command tree:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat { Text, Json, Markdown }

// flags accumulate into a set
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum PreviewMode {
    Stream, Backlog, Coordination, SessionDigest, ScenarioReport,
    ServerReport, ParityReport, ActionPreview, ActionExecute,
}

// the resolved single mode actually run
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShellMode {
    Stream, Backlog, Coordination, SessionDigest, ScenarioReport,
    ServerReport, ParityReport, ActionPreview, ActionExecute,
}
```

Top-level options bag:

```rust
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
```

### The parser loop (`parse_options`, line 172) — the pattern to copy

```rust
fn parse_options(args: impl IntoIterator<Item = String>) -> Result<CliOptions, String> {
    let mut args = args.into_iter();
    let mut format = OutputFormat::Text;
    // ... one local mut per option, with defaults ...
    while let Some(arg) = args.next() {
        let mut state = CliParseState { format: &mut format, /* ...borrows of every local... */ };
        apply_cli_argument(&arg, &mut args, &mut state)?;
    }
    Ok(CliOptions { /* assembled from locals */ })
}
```

`apply_cli_argument` (line 233) is the per-flag matcher. **`--flag value` style** (value is a
*separate* following arg, consumed via `next_option_value`). No `--flag=value`, no short flags.
Boolean mode flags are zero-arg and just insert into the set:

```rust
fn apply_cli_argument<I>(arg: &str, args: &mut I, state: &mut CliParseState<'_>) -> Result<(), String>
where I: Iterator<Item = String> {
    if let Some(mode) = preview_mode_flag(arg) {        // zero-arg boolean flags
        state.preview_modes.insert(mode);
        return Ok(());
    }
    match arg {
        "--format"           => *state.format = parse_output_format(&next_option_value(args, "--format")?)?,
        "--workbench-token"  => *state.workbench_token = Some(next_option_value(args, "--workbench-token")?),
        "--context-talk"     => *state.context_talk_page = Some(next_option_value(args, "--context-talk")?),
        "--shell"            => *state.shell_mode = Some(parse_shell_mode(&next_option_value(args, "--shell")?)?),
        "--action-kind"      => *state.action_kind = parse_action_kind(&next_option_value(args, "--action-kind")?)?,
        "--bridge-base-url"  => *state.bridge_base_url = next_option_value(args, "--bridge-base-url")?,
        // ... etc ...
        _ => return Err(format!("unsupported argument: {arg}")),   // unknown flag is a hard error
    }
    Ok(())
}

fn next_option_value<I>(args: &mut I, flag: &str) -> Result<String, String>
where I: Iterator<Item = String> {
    args.next().ok_or_else(|| format!("{flag} requires a value"))
}
```

Helper value parsers are tiny `match` fns returning `Result<_, String>`:

```rust
fn parse_output_format(value: &str) -> Result<OutputFormat, String> {
    match value {
        "text" => Ok(OutputFormat::Text),
        "json" => Ok(OutputFormat::Json),
        "markdown" => Ok(OutputFormat::Markdown),
        _ => Err(format!("unsupported output format: {value}")),
    }
}
```

`selected_shell_mode` (line 347) resolves the *one* mode to run: explicit `--shell` wins,
else a fixed priority over the `preview_modes` set.

> **Implication for `verify`:** there is no "subcommand" idiom to imitate. The repo idiom for "a
> new capability" is **a new mode flag** (e.g. `--verify` / `--verify-citation`) plus its value
> flags (e.g. `--claim`, `--source-url`, `--rev-id`, `--citation-index`), wired through
> `preview_mode_flag` (or a new `parse_*` for `--shell`), `selected_shell_mode`, and a new arm in
> `run()`. The `--format text|json|markdown` flag already exists and should be reused verbatim;
> PRD-0001's `verdict`-only output is best added as a *fourth* `OutputFormat` variant (see §3) OR
> a `--verdict-only` boolean — recommend a new `OutputFormat::Verdict` to keep the single
> `--format` switch.

---

## 2. How an existing capability works end-to-end (the build_/execute/parse split)

The CLI itself has TWO flavors of "real work":

### (a) Pure / in-process (no network) — e.g. ranked queue, scenario report
`run()` calls `load_ranked_queue` → `sp42_devtools::build_dev_queue(config, payload)` (pure,
fed by stdin) → render. No HTTP. Most modes are this.

### (b) Networked — the model to copy for `verify`: `--server-report` and `--action-execute`

The CLI does **NOT** use `sp42-server`'s `BearerHttpClient`. It builds its own `reqwest::Client`
inline, and for the structured path it uses the **`sp42-core` build_request / `client.execute` /
parse_response** trio over the `sp42_types::HttpClient` trait. Two concrete examples:

**Server report** (`fetch_server_report`, line 1373) — builds a `reqwest::Client` with the
project UA and GETs a list of route-contract paths:

```rust
async fn fetch_server_report(base_url: &str) -> Result<BTreeMap<String, Value>, String> {
    let client = reqwest::Client::builder()
        .user_agent(sp42_core::branding::USER_AGENT)   // const = "SP42/0.1.0 (+https://github.com/christophehenner/SP42)"
        .build()
        .map_err(|e| format!("server report client failed to build: {e}"))?;
    // GET each route_contracts::*_PATH, json::<Value>() each
}
```

**Action execute** (`execute_bridge_action`, line 1483) — the canonical
**build_*_request → execute → parse_*** pattern, exactly what `verify` should mirror:

```rust
async fn execute_bridge_action(base_url: &str, request: &SessionActionExecutionRequest)
    -> Result<LocalBridgeActionExecutionReport, String>
{
    let client = reqwest::Client::builder()
        .user_agent(sp42_core::branding::USER_AGENT).build()
        .map_err(|e| format!("bridge client failed to build: {e}"))?;

    // 1. BUILD a typed HttpRequest in sp42-core:
    let bootstrap_request =
        build_dev_auth_bootstrap_request(base_url, &DevAuthBootstrapRequest::default())
            .map_err(|e| e.to_string())?;
    // 2. EXECUTE it through a hand-written request runner (see execute_local_http_request below):
    let bootstrap_response = execute_local_http_request(&client, bootstrap_request).await?;
    // 3. PARSE the bytes back in sp42-core:
    let bootstrap = parse_dev_auth_status(&bootstrap_response.body).map_err(|e| e.to_string())?;
    // ...
}
```

The generic request runner that maps `sp42_types::HttpRequest` → reqwest → `HttpResponse`
(`execute_local_http_request`, line 1529) — **this is reusable as-is for `verify`'s fetches**:

```rust
async fn execute_local_http_request(client: &reqwest::Client, request: HttpRequest)
    -> Result<HttpResponse, String>
{
    let mut builder = match request.method {
        HttpMethod::Get => client.get(request.url),
        HttpMethod::Post => client.post(request.url),
        HttpMethod::Put => client.put(request.url),
        HttpMethod::Patch => client.patch(request.url),
        HttpMethod::Delete => client.delete(request.url),
    };
    for (key, value) in request.headers { builder = builder.header(&key, value); }
    let response = builder.body(request.body).send().await
        .map_err(|e| format!("bridge request failed: {e}"))?;
    let status = response.status().as_u16();
    let headers = response.headers().iter().filter_map(/* to_str -> (String,String) */).collect();
    let body = response.bytes().await.map_err(/* ... */)?.to_vec();
    Ok(HttpResponse { status, headers, body })
}
```

### The `sp42-core` reference for build_/execute/parse over the HttpClient trait

`sp42-core/src/wiki_storage.rs` shows the **canonical core-side three-piece pattern** the
verify port should imitate (and the closest thing to "fetch an article revision" that exists):

```rust
// BUILD: pure, returns sp42_types::HttpRequest
pub fn build_wiki_storage_document_load_request(config: &WikiConfig, title: &str)
    -> Result<HttpRequest, WikiStorageError>
{
    Ok(HttpRequest {
        method: HttpMethod::Get,
        url: build_query_url(&config.api_url, &[
            ("action", "query"), ("prop", "revisions"), ("titles", title),
            ("rvprop", "ids|content"), ("rvslots", "main"),
            ("format", "json"), ("formatversion", "2"),
        ]),
        headers: BTreeMap::default(),
        body: Vec::new(),
    })
}

// EXECUTE+PARSE: generic over the injected client trait, NOT a concrete reqwest type
pub async fn load_wiki_storage_document<C>(client: &C, config: &WikiConfig, title: &str)
    -> Result<WikiStorageLoadedDocument, WikiStorageError>
where C: HttpClient + ?Sized
{
    let request = build_wiki_storage_document_load_request(config, title)?;
    let response = client.execute(request).await
        .map_err(|e| WikiStorageError::Transport { message: e.to_string() })?;
    parse_wiki_storage_document_response(title, &response)   // pure parse
}
```

> The MediaWiki revision-fetch query shape for "give me the current wikitext of a title" is
> **exactly** `action=query&prop=revisions&titles=<T>&rvprop=ids|content&rvslots=main&format=json&formatversion=2`.
> A `verify` "by article title" path reuses this verbatim. A "by rev id" path swaps `titles=` for
> `revids=<rev_id>`. (No existing fn for revids — `verify` would add `build_revision_load_request`.)

### `HttpConfig` / `WikiConfig` and the HttpClient trait (sp42-types)

```rust
// sp42-types/src/traits.rs
#[async_trait]
pub trait HttpClient: Send + Sync {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, HttpClientError>;
}
// Test double also in sp42-types: StubHttpClient::new([Ok(HttpResponse{...})]) — pops queued responses.
```

```rust
// sp42-types/src/transport.rs
pub enum HttpMethod { Get, Post, Put, Patch, Delete }
pub struct HttpRequest  { pub method: HttpMethod, pub url: url::Url,
                          #[serde(default)] pub headers: BTreeMap<String,String>,
                          #[serde(default)] pub body: Vec<u8> }
pub struct HttpResponse { pub status: u16,
                          #[serde(default)] pub headers: BTreeMap<String,String>,
                          #[serde(default)] pub body: Vec<u8> }
```

**Two viable wiring choices for `verify`** (in the CLI's style):

1. **Match the CLI as-is:** build the typed `HttpRequest` in core, run it through the existing
   `execute_local_http_request(&reqwest_client, req)`, parse with a core `parse_*` fn. Simplest;
   no new trait impl in the CLI.
2. **Match `sp42-core`'s generic functions:** define an `impl HttpClient for <a small reqwest
   wrapper>` and call `load_*` generic helpers. Heavier (needs `async-trait`); not how the CLI
   does it today. Prefer (1) unless the new core verify function is written generic-over-`C:
   HttpClient` (then the CLI must provide a trait impl — but note the CLI currently has none of
   its own; `BearerHttpClient`/`BrowserHttpClient` live in server/app, not here).

> **No existing citation-verification code anywhere.** `grep "citation"` finds only incidental
> uses: `article_inventory.rs` (`citation_template_count`, `bare_urls`, `citation_templates`,
> `citation_needed_templates`, `is_citation_template`, `extract_bare_urls`), scoring/diff
> signals. **There is NO `CitationVerdict`, `CitationFinding`, `verify_citation`, no LLM client,
> no `ModelClient`.** The verify port is greenfield in `sp42-core` (or a future
> `sp42-verification` slice per ADR-0008 Decision 7). The CLI just renders/dispatches it.

#### Deterministic bare-URL signal already exists (PRD-0001's cheap signal)
`sp42-core/src/article_inventory.rs`:
```rust
pub fn build_article_inventory(wiki_id: &str, title: &str, wikitext: &str) -> ArticleInventory
// returns ArticleReference { citation_template_count, bare_urls: Vec<String>, ... }, plus
//   bare_urls: Vec<String>, citation_templates, citation_needed_templates
fn extract_bare_urls(value: &str) -> Vec<String>      // private
fn is_citation_template(name: &str) -> bool           // private
```
This is the deterministic half of PRD-0001's "BOTH signals" idea (bare-URL signal) and is
already pure + tested. The LLM "not supported" signal is the part that must be built new.

---

## 3. Output formatting — no formatter abstraction; per-fn `match format` with three arms

There is **no shared trait/formatter object**. Every `render_*` fn takes
`format: OutputFormat` and `match`es three arms. Output is a returned `Result<String, String>`;
`main()` does the single `println!`. The JSON arm uses `serde_json::to_string_pretty`. Examples:

```rust
fn render_queue(queue: &[QueuedEdit], format: OutputFormat) -> Result<String, String> {
    match format {
        OutputFormat::Text => Ok(queue.iter().enumerate().map(|(i, item)| format!(
            "#{rank} wiki={} rev_id={} title=\"{}\" score={} signals={}",
            item.event.wiki_id, item.event.rev_id, item.event.title,
            item.score.total, item.score.contributions.len(), rank = i + 1,
        )).collect::<Vec<_>>().join("\n")),
        OutputFormat::Markdown => Ok(render_markdown_section("Ranked queue", &/* same text body */)),
        OutputFormat::Json => serde_json::to_string_pretty(queue).map_err(|e| e.to_string()),
    }
}
```

JSON often hand-builds with `serde_json::json!{}` (selective fields) rather than serializing the
whole struct — see `render_context_preview_json` (line 652), `render_action_preview` JSON arm
(line 1232). Markdown helpers (line 1846):

```rust
fn render_markdown_section(title: &str, body: &str) -> String { /* "## {title}\n\n{body}" or "_Empty_" */ }
fn render_markdown_code_block(language: &str, body: &str) -> String { /* fenced block or text fallback */ }
```

> **`verify` output plan (PRD-0001 Surface + DoD item 8):**
> - **human** → `OutputFormat::Text` arm: a readable block (claim, source URL, verdict, the
>   anti-fabrication / "supported|not-supported|inconclusive" line, evidence snippet).
> - **json** → `serde_json::to_string_pretty(&CitationFinding)` (or a hand-built `json!{}` if
>   only a subset should surface). Keep `use_site_ordinal` per ADR-0008.
> - **verdict** → PRD-0001 "verdict-only": add a 4th `OutputFormat::Verdict` variant whose arm
>   prints ONLY the categorical verdict token (one line, machine-greppable, no prose). Extend
>   `parse_output_format` with `"verdict" => Ok(OutputFormat::Verdict)`. (Every existing
>   three-arm `render_*` would then need a Verdict arm OR—cleaner—the verify renderer is its own
>   fn and only it knows about Verdict; the other modes can `unreachable!`/reuse Text. Recommend
>   the verify renderer own all four arms and leave other modes 3-arm by never letting `--format
>   verdict` reach them, i.e. validate in the verify path.)

---

## 4. Tests — inline `#[cfg(test)] mod tests` in main.rs, NO `tests/` dir, NO assert_cmd

- All CLI tests live in `crates/sp42-cli/src/main.rs` under `#[cfg(test)] mod tests` (lines
  1862–2547). There is **no `tests/` integration dir** and **no `assert_cmd`/`predicates`** dep.
- Tests call the internal fns directly (imported via `use super::{...}`): they test
  `parse_options(["--flag".to_string(), "val".to_string()])` and each `render_*` fn with a fixed
  fixture, asserting on substrings of the returned `String`.
- Fixtures are built from `sp42_devtools`:
  ```rust
  fn fixture_config() -> sp42_core::WikiConfig { parse_default_dev_wiki_config().expect("...") }
  fn fixture_queue(config: &sp42_core::WikiConfig) -> Vec<sp42_core::QueuedEdit> {
      build_dev_queue(config, DEV_PREVIEW_SAMPLE_EVENTS).expect("...")
  }
  ```
- JSON-arm tests parse the output back: `serde_json::from_str::<Value>(&summary)` then assert on
  fields (e.g. `value["scenario"]["selected"]["rev_id"] == 123_459`).
- Flag-parse tests assert the resulting `CliOptions` fields / `preview_modes.contains(...)`.

> **`verify` test plan:** add inline `#[cfg(test)]` tests in the same module:
> (1) `parse_options(["--verify", "--claim", "...", "--source-url", "..."])` populates the new
> options; (2) a `render_verify(...)` fn over a stubbed/precomputed `CitationFinding` produces the
> right substrings per format; (3) JSON round-trips. **Networked execution stays untested at the
> CLI layer** (the CLI has no HTTP test today) — push fetch/verify unit tests into `sp42-core`
> using `StubHttpClient` (the trait double already in `sp42-types`).

---

## 5. Async runtime — `futures::executor::block_on`, NO tokio

`use futures::executor::block_on;` (line 5). `main()` and `run()` are **sync** (`fn`, not
`async fn`); there is **no `#[tokio::main]`**. Every async core/reqwest call is wrapped at the
mode boundary:

```rust
let preview = block_on(build_dev_stream_preview(config, payload, "fixture"))?;     // line 679
let report  = block_on(execute_bridge_action(&options.bridge_base_url, &request))?; // line 1264
let report  = block_on(fetch_server_report(base_url))?;                             // line 1355
```

The inner async fns (`fetch_server_report`, `execute_bridge_action`,
`execute_local_http_request`) are `async fn`s that internally `.await`. So the pattern is: write
`async fn verify_citation_cli(...)` and call it once from the sync `run()` arm via
`block_on(...)`. (reqwest works under `futures::executor::block_on` here because the existing
networked modes already do exactly this.)

---

## 6. Crate dependency graph used by the CLI

| Dep | What the CLI uses from it |
|---|---|
| `sp42-types` | `HttpMethod`, `HttpRequest`, `HttpResponse` (and the `HttpClient` trait / `StubHttpClient` are here if needed) |
| `sp42-core` | `WikiConfig`, `QueuedEdit`, `SessionActionKind`, `routes as route_contracts`, `branding::USER_AGENT`, the dev-auth `build_*`/`parse_*` fns, action contracts; (would gain the new citation `build_/parse_/verify` module) |
| `sp42-devtools` | `parse_default_dev_wiki_config`, `build_dev_queue`, `DEV_PREVIEW_SAMPLE_EVENTS`, all the `build_dev_*` preview builders |
| `sp42-reporting` | scenario/shell-state report builders + renderers |
| `reqwest` | inline `reqwest::Client::builder().user_agent(branding::USER_AGENT).build()` |
| `futures` | `executor::block_on` only |
| `serde`/`serde_json` | JSON output (`to_string_pretty`, `json!{}`, `Value`) |

NOT present: `clap`, `tokio`, `sp42-server`, `async-trait` (CLI side), `sp42-wiki` (would
likely be added for `WikiConfig`/`api_url` if verify fetches a real article).

The real `HttpClient` reqwest adapters that DO exist (`BearerHttpClient`,
`BrowserHttpClient`) live in `sp42-server`/`sp42-app`, **not** in the CLI — the CLI rolls its
own inline reqwest usage instead.

---

## 7. Concrete shape for the `verify` capability (the deliverable)

PRD-0001 inputs to accept (one of): article **title** · **rev id** · a **single citation**
(claim snippet OR report/use-site index) · ad-hoc **claim + source URL**.

### CLI surface (house style: mode flag + value flags, reuse `--format`)
- `--verify` (or `--shell verify`) → new `ShellMode::VerifyCitation` arm in `run()`.
- Value flags (all `--flag value` style, parsed in `apply_cli_argument` via `next_option_value`):
  - `--title <T>` → fetch current revision wikitext (query `prop=revisions … titles=T …`).
  - `--rev-id <N>` → fetch by `revids=N` (new `build_revision_load_request`).
  - `--claim <snippet>` and `--source-url <url>` → ad-hoc (claim, source) use-site.
  - `--citation-index <k>` → pick the k-th use-site/citation in the fetched article
    (anchor = ADR-0008 `use_site_ordinal`).
- `--format text|json|markdown|verdict` → reuse the existing switch; add `Verdict` variant +
  `parse_output_format` arm `"verdict" => OutputFormat::Verdict`.

### Options bag additions
```rust
struct VerifyOptions {
    title: Option<String>,
    rev_id: Option<u64>,
    claim: Option<String>,
    source_url: Option<url::Url>,   // or String, parsed later
    citation_index: Option<usize>,
}
// add `verify: Option<VerifyOptions>` to CliOptions; build it in parse_options like
// build_context_preview()/build_workbench do (collapse locals -> Option).
```

### Dispatch + execution (mirror `render_action_execute` / `fetch_server_report`)
```rust
fn render_verify(config: &WikiConfig, options: &VerifyOptions, format: OutputFormat)
    -> Result<String, String>
{
    let finding = block_on(run_verify(config, options))?;   // single block_on at the boundary
    match format {
        OutputFormat::Text     => Ok(/* human block */),
        OutputFormat::Markdown => Ok(render_markdown_section("Citation verdict", &/* body */)),
        OutputFormat::Json     => serde_json::to_string_pretty(&finding).map_err(|e| e.to_string()),
        OutputFormat::Verdict  => Ok(verdict_token(&finding).to_string()),   // ONE line only
    }
}

async fn run_verify(config: &WikiConfig, options: &VerifyOptions) -> Result<CitationFinding, String> {
    let client = reqwest::Client::builder()
        .user_agent(sp42_core::branding::USER_AGENT).build().map_err(/* ... */)?;
    // 1. fetch article/revision wikitext when title/rev-id given:
    //    let req = sp42_core::build_revision_load_request(config, ...);          // NEW core fn
    //    let resp = execute_local_http_request(&client, req).await?;             // REUSE existing runner
    //    let wikitext = sp42_core::parse_revision_content(&resp.body)?;          // NEW core fn
    // 2. select the use-site: --citation-index over use-sites, or the ad-hoc (claim, source-url).
    // 3. fetch the source body (build_source_fetch_request -> execute -> parse).   // NEW core fns
    // 4. verify_citation(claim, source_body, ...) -> CitationVerdict/CitationFinding. // NEW core/LLM
}
```

### Where the new types/logic live
**Not in the CLI.** Per ADR-0007/0008/0009, the verdict semantics, request/response contract,
and snapshot storage are `sp42-core` (or a future `sp42-verification`/`sp42-types` slice). The
CLI only: parses flags → calls one async `run_verify` → renders `CitationFinding` in 4 formats.
Keep `use_site_ordinal` on `CitationFinding` (ADR-0008 Decision 7). Anti-fabrication: the
verdict must be grounded in the fetched source bytes (the wikiharness invariant; SP42 ADR-0007).

### Read-only guarantee
`verify` issues only `GET` requests (revision load + source fetch) — no `POST`/save, no action
execution, no auth bootstrap. It does NOT touch `--bridge-base-url`/`execute_bridge_action`. This
satisfies "read-only" naturally; no write path is reachable from the verify arm.

---

## Summary of the load-bearing facts for the port

1. **Hand-rolled flag parser, no clap; no subcommand verbs — capabilities are FLAG-SELECTED
   modes.** Add `--verify` as a mode + value flags; wire through `apply_cli_argument`,
   `selected_shell_mode`, a new `run()` arm.
2. **No tokio.** Sync `main`/`run`; one `futures::executor::block_on(async_fn)` at the mode boundary.
3. **build_request (pure, sp42-core) → execute (reqwest via `execute_local_http_request`, or
   `client.execute` over the `HttpClient` trait) → parse_response (pure, sp42-core).** Reuse
   `wiki_storage`'s `prop=revisions&rvprop=ids|content&rvslots=main&formatversion=2` shape.
4. **Output = per-fn `match OutputFormat { Text|Json|Markdown }` returning `String`; no formatter
   object.** Add `OutputFormat::Verdict` for PRD-0001's verdict-only output.
5. **Tests inline in `main.rs` (`#[cfg(test)] mod tests`); no `tests/` dir, no assert_cmd.**
   Test `parse_options` + `render_*` substrings; push HTTP/verify unit tests into `sp42-core`
   with the existing `StubHttpClient`.
6. **No citation-verification code exists yet** (only deterministic `extract_bare_urls` /
   `is_citation_template` in `article_inventory.rs`). The CLI renders/dispatches a new
   `sp42-core` (or `sp42-verification`) verify module; UA const is
   `sp42_core::branding::USER_AGENT`.
