# Bare-URL Repair MVP Implementation Plan — Phase 6: CLI flag-modes

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** Operator-reachable preview/execute from the command line:

```text
--bare-url-preview --title <T> --rev <N> [--wiki <ID>] [--bridge-base-url <URL>] [--format text|json|markdown]
--bare-url-execute --title <T> --rev <N> --ordinal <K> [--wiki <ID>] [--action-note <summary>] [--bridge-base-url <URL>] [--format ...]
```

`--bare-url-execute` re-fetches the proposals, selects ordinal K, and replays that proposal verbatim against the apply route (fresh anchor narrows the TOCTOU window); auth rides the bridge session (ADR-0002). Render functions are pure and unit-tested in all three formats.

**Architecture & verified decisions:**
- House flag-mode pattern: hand-rolled arms in `apply_cli_argument`, a validation helper (`build_verify_options` precedent from the citation branch), early `return` from `run()` **before `read_stdin()`** — bare-url modes need no queue/stdin, and exiting before the stdin read keeps terminal invocations from blocking.
- `--wiki` defaults to `testwiki` — the request bodies require a `wiki_id`, testwiki is the MVP's only enabled wiki, and the design's flag contract omits a wiki flag; the optional override keeps the contract verbatim while staying explicit. (Small additive surface; fold into the PRD CLI-surface section in Phase 7.)
- **CSRF (verified gap + fix):** `post_bare_url_apply` validates the `x-sp42-csrf-token` header. The bootstrap status (`DevAuthSessionStatus.csrf_token`, dev_auth.rs:44, populated by `to_status` at session_runtime.rs:180) carries the token; the new bridge helper sends it. (Note: the existing `execute_bridge_action` for `--action-execute` sends only the session cookie — a pre-existing gap in that path; do **not** fix it in this phase, just don't copy it.) The header name becomes a shared contract: `pub const CSRF_HEADER_NAME` moves to `sp42-core/src/routes.rs`, re-exported in `session_runtime.rs`.
- CLI HTTP is reqwest async driven by `futures::executor::block_on` (no tokio runtime in the CLI); errors are `Result<String, String>` with `eprintln!` + exit 1 at `main()`.

**Tech Stack:** sp42-cli `main.rs` only (plus the two-line CSRF contract refactor in core/server). Wire types come from `sp42_core` (Phase 3).

**Scope:** Phase 6 of 7. Depends on Phases 4–5 (routes exist).

**Codebase verified:** 2026-06-09, branch `louie/bare-url-repair` @ `2ed57b3`. Flag names `--title`, `--rev`, `--ordinal`, `--wiki` confirmed unused.

---

**Working directory for every command:** `/var/home/louie/Projects/Volunteering-Consulting/SP42/.worktrees/bare-url-repair`

### Task 1: Share the CSRF header-name contract

**Files:**
- Modify: `crates/sp42-core/src/routes.rs`
- Modify: `crates/sp42-server/src/session_runtime.rs:18`

**Step 1: Add the constant to core**

In `crates/sp42-core/src/routes.rs`, next to the dev-route constants added in Phase 4:

```rust
/// Header carrying the bridge session's CSRF token on state-changing routes.
pub const CSRF_HEADER_NAME: &str = "x-sp42-csrf-token";
```

**Step 2: Re-export in the server**

In `crates/sp42-server/src/session_runtime.rs`, replace line 18:

```rust
pub(crate) const CSRF_HEADER_NAME: &str = "x-sp42-csrf-token";
```

with:

```rust
pub(crate) use sp42_core::routes::CSRF_HEADER_NAME;
```

(All existing users import it from `session_runtime`, so nothing else changes.)

**Step 3: Verify**

```bash
cargo test -p sp42-server
cargo clippy -p sp42-core -p sp42-server --all-targets --all-features -- -D warnings
```

Expected: green.

**Step 4: Commit**

```bash
git add crates/sp42-core/src/routes.rs crates/sp42-server/src/session_runtime.rs
git commit -m "refactor: move the CSRF header name into the core route contracts"
```

### Task 2: Flag parsing and option assembly (test-driven)

**Files:**
- Modify: `crates/sp42-cli/src/main.rs` (`CliOptions` line 38, `parse_options` lines 172–217, `CliParseState` lines 219–231, `apply_cli_argument` arms near line 275, plus the five test-module `CliOptions` literals)

**Step 1: Write the failing tests**

In the `#[cfg(test)] mod tests` at the bottom of `crates/sp42-cli/src/main.rs`, add (extend the `use super::{...}` import with `BareUrlCliMode, BareUrlCliOptions` as needed):

```rust
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
            parse(&["--bare-url-preview", "--bare-url-execute", "--title", "T", "--rev", "1"])
                .is_err()
        );
        assert!(parse(&["--bare-url-preview", "--rev", "1"]).is_err(), "missing --title");
        assert!(parse(&["--bare-url-preview", "--title", "T"]).is_err(), "missing --rev");
        assert!(
            parse(&["--bare-url-execute", "--title", "T", "--rev", "1"]).is_err(),
            "execute requires --ordinal"
        );
        assert!(parse(&["--bare-url-preview", "--title", "T", "--rev", "abc"]).is_err());
        assert!(parse(&[]).expect("no flags is fine").bare_url.is_none());
    }
```

**Step 2: Run to verify failure**

```bash
cargo test -p sp42-cli bare_url
```

Expected: compile errors — `bare_url` field, `BareUrlCliMode`, `BareUrlCliOptions` not found.

**Step 3: Implement parsing**

(a) New types near `CliOptions` (line 38 area):

```rust
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
```

(b) Add the field to `CliOptions`:

```rust
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
}
```

(c) In `parse_options` (lines 172–217), add locals after `let mut preview_modes = BTreeSet::new();`:

```rust
    let mut bare_url_preview = false;
    let mut bare_url_execute = false;
    let mut bare_url_title = None;
    let mut bare_url_rev = None;
    let mut bare_url_ordinal = None;
    let mut bare_url_wiki = BARE_URL_DEFAULT_WIKI.to_string();
```

add the matching `&mut` fields when constructing `CliParseState` in the loop, and build the option before the final `Ok(...)`:

```rust
    let bare_url = build_bare_url_options(
        bare_url_preview,
        bare_url_execute,
        bare_url_wiki,
        bare_url_title,
        bare_url_rev,
        bare_url_ordinal,
    )?;

    Ok(CliOptions {
        format,
        // ... existing fields unchanged ...
        bridge_base_url,
        bare_url,
    })
```

(d) Extend `CliParseState` (lines 219–231):

```rust
    bare_url_preview: &'a mut bool,
    bare_url_execute: &'a mut bool,
    bare_url_title: &'a mut Option<String>,
    bare_url_rev: &'a mut Option<u64>,
    bare_url_ordinal: &'a mut Option<usize>,
    bare_url_wiki: &'a mut String,
```

(e) Add arms in `apply_cli_argument` after the `"--bridge-base-url"` arm (line 275 area):

```rust
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
            *state.bare_url_rev =
                Some(value.parse().map_err(|_| format!("--rev expects a revision id, got: {value}"))?);
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
```

(f) The validation helper (near `build_verify_options`' eventual home; place after `parse_options`):

```rust
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
        let ordinal =
            ordinal.ok_or_else(|| "--bare-url-execute requires --ordinal".to_string())?;
        BareUrlCliMode::Execute { ordinal }
    } else {
        BareUrlCliMode::Preview
    };
    Ok(Some(BareUrlCliOptions { mode, wiki_id, title, rev_id }))
}
```

(g) **Fix the breakage this causes:** every `CliOptions { ... }` literal in the test module must gain `bare_url: None,`. Verified sites: `crates/sp42-cli/src/main.rs` lines **2043, 2080, 2108, 2136, 2181** (line numbers pre-edit; all are in tests). After editing, `grep -n "CliOptions {" crates/sp42-cli/src/main.rs` must show every literal updated.

**Step 4: Run the tests to verify they pass**

```bash
cargo test -p sp42-cli
```

Expected: the three new tests pass and the whole existing CLI suite stays green.

**Step 5: Commit**

```bash
git add crates/sp42-cli/src/main.rs
git commit -m "feat: parse bare-url preview/execute CLI flag-modes"
```

### Task 3: Pure render functions (test-driven)

**Files:**
- Modify: `crates/sp42-cli/src/main.rs`

**Step 1: Write the failing tests**

Add to the test module (extend `use super::{...}` with the new render functions and `BareUrlExecuteReport`):

```rust
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

        let text =
            render_bare_url_proposals(&options, "http://127.0.0.1:8788", &response, OutputFormat::Text)
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

        let text = render_bare_url_execute(&report, OutputFormat::Text)
            .expect("text render should work");
        assert!(text.contains("bare-url execute"));
        assert!(text.contains("ordinal=0"));
        assert!(text.contains("accepted=true"));

        let markdown = render_bare_url_execute(&report, OutputFormat::Markdown)
            .expect("markdown render should work");
        assert!(markdown.contains("## Bare-URL execute"));
        assert!(markdown.contains("## Apply result"));

        let json = render_bare_url_execute(&report, OutputFormat::Json)
            .expect("json render should work");
        let value: serde_json::Value = serde_json::from_str(&json).expect("json should parse");
        assert_eq!(value["ordinal"], 0);
        assert_eq!(value["response"]["accepted"], true);
    }
```

**Step 2: Run to verify failure**

```bash
cargo test -p sp42-cli renders_bare_url
```

Expected: compile errors — `render_bare_url_proposals`, `render_bare_url_execute`, `BareUrlExecuteReport` not found.

**Step 3: Implement the renders**

Place near the other render functions:

```rust
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
```

**Step 4: Run the tests to verify they pass**

```bash
cargo test -p sp42-cli renders_bare_url
```

Expected: 2 tests pass. (Until Task 4 wires callers, the new functions are test-only; `warnings = deny` flags dead code reachable only from tests — if `cargo test -p sp42-cli` errors with `dead_code` on the new items, proceed directly to Task 4 within the same commit cycle and run the full verification there. Commit after Task 4 in that case.)

**Step 5: Commit (or fold into Task 4's commit if dead-code requires it)**

```bash
git add crates/sp42-cli/src/main.rs
git commit -m "feat: render bare-url proposal and apply reports"
```

### Task 4: Bridge helpers + dispatch

**Files:**
- Modify: `crates/sp42-cli/src/main.rs`

**Step 1: Implement the bridge helpers**

Place beside `execute_bridge_action` (line ~1483). Model: that function, **plus** the CSRF header it omits.

```rust
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

/// Re-fetch proposals, select ordinal K, and replay it against the apply
/// route. The fresh fetch re-anchors the locator, narrowing the TOCTOU
/// window; the server's anti-drift re-check and `baserevid` guard close it.
/// Auth rides the bridge session (ADR-0002): bootstrap, then send the
/// session cookie *and* the bootstrap-reported CSRF token.
async fn execute_bare_url_via_bridge(
    base_url: &str,
    bare_url: &BareUrlCliOptions,
    ordinal: usize,
    note: Option<&str>,
) -> Result<BareUrlExecuteReport, String> {
    let proposals = fetch_bare_url_proposals(base_url, &bare_url_proposals_request(bare_url)).await?;
    let proposal = proposals
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
        })?;

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
    let csrf_token = bootstrap
        .csrf_token
        .clone()
        .ok_or_else(|| "bridge bootstrap did not return a CSRF token".to_string())?;

    let apply_request = sp42_core::BareUrlApplyRequest {
        wiki_id: bare_url.wiki_id.clone(),
        title: bare_url.title.clone(),
        rev_id: bare_url.rev_id,
        locator: proposal.locator.clone(),
        replacement_wikitext: proposal.replacement_wikitext.clone(),
        summary: note.map(ToString::to_string),
    };
    let request_url = format!(
        "{base_url}{}",
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
        bridge_base_url: base_url.to_string(),
        wiki_id: bare_url.wiki_id.clone(),
        title: bare_url.title.clone(),
        rev_id: bare_url.rev_id,
        ordinal,
        proposal,
        response: apply,
    })
}
```

**Step 2: Implement the mode runner and dispatch**

```rust
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
```

In `run()`, dispatch **immediately after parsing, before `read_stdin()`** (so the mode never blocks on a TTY):

```rust
fn run() -> Result<String, String> {
    let options = parse_options(std::env::args().skip(1))?;
    if let Some(bare_url) = &options.bare_url {
        return render_bare_url_mode(bare_url, &options, options.format);
    }
    let input = read_stdin().map_err(|error| error.to_string())?;
```

**Step 3: Full verification**

```bash
cargo test -p sp42-cli
cargo clippy -p sp42-cli --all-targets --all-features -- -D warnings
cargo build -p sp42-cli
```

Expected: all green; the binary builds.

**Step 4: Commit**

```bash
git add crates/sp42-cli/src/main.rs
git commit -m "feat: add --bare-url-preview/--bare-url-execute bridge flag-modes"
```

### Task 5: Documented manual smoke (do NOT execute)

This step is **documentation only** — the live smoke needs an operator with a real session and is the PRD's separately-gated final DoD item. Record the procedure; do not run it.

**Step 1: Verify the procedure below is consistent with what you built** (flag names, route paths, default wiki), then record it in the phase summary / PR description:

```bash
# 1. Server with a real token (local mode), testwiki enabled via fixtures/testwiki.yaml:
SP42_WIKI_CONFIG_DIR=fixtures cargo run -p sp42-server

# 2. Preview proposals for a sandbox revision (no auth needed):
cargo run -p sp42-cli -- --bare-url-preview --wiki testwiki \
  --title "Wikipedia:Sandbox" --rev <REV_ID> --format text </dev/null

# 3. Apply ordinal 0 under the operator's session (bootstrap rides the bridge):
cargo run -p sp42-cli -- --bare-url-execute --wiki testwiki \
  --title "Wikipedia:Sandbox" --rev <REV_ID> --ordinal 0 \
  --action-note "SP42 bare-URL repair smoke" --format text </dev/null
```

(`SP42_WIKI_CONFIG_DIR=fixtures` points the registry at `fixtures/testwiki.yaml`; confirm against `docs/RUNTIME_CONFIGURATION.md` — the variable expects a directory of YAML configs. `WIKIMEDIA_ACCESS_TOKEN` must be available per the dev-auth bridge docs for the bootstrap to succeed.)

**Step 2: Commit** any wording fix to this plan file only if needed; otherwise nothing to commit.
