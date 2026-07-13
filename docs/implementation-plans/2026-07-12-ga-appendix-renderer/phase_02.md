# GA Evidence Appendix Renderer Implementation Plan — Phase 2: CLI surface

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** Expose the appendix on `sp42-cli`: a `ga-appendix` format on `verify-page` (bridge convenience) and a new pure `render-report` subcommand over a saved report JSON (the replay-friendly core surface — no bridge, no session, no network).

**Architecture:** A command-local `PageReportFormat` enum for the two page-report commands only — NOT a variant on the shared `OutputFormat` (which is flattened into `verify`, `bare-url`, `preview` too and would advertise an impossible format there). `render-report` deserializes `PageVerificationReport` (verify-page's `--format json` is exactly `serde_json::to_string_pretty(&report)`, round-trip verified upstream at `page.rs:1262`) and calls the same renderer.

**Tech Stack:** clap 4 derive (`ValueEnum`), serde_json; `SystemClock` already imported in `main.rs:15`.

**Scope:** Phase 2 of 3.

**Codebase verified:** 2026-07-12. Key locations in `crates/sp42-cli/src/main.rs` (4453 lines): `OutputFormat` 36-42; `FormatArg` 261-267; `Command` enum 227-240 (`VerifyPage(VerifyPageArgs)` at 231); `VerifyPageArgs` 321-336 (`fmt: FormatArg` at 334-335); dispatch arm 690-694; `render_verify_page` 2957-2988; legacy-argv translation touches `Command::VerifyPage` at 3598/3616 (do not modify). `sp42-cli/Cargo.toml` already depends on sp42-core/-citation/-types (lines 18/22/25).

---

## Task 1: `PageReportFormat` + `ga-appendix` on `verify-page`

**Files:**
- Modify: `crates/sp42-cli/Cargo.toml` (add `sp42-assessment = { path = "../sp42-assessment" }` in the sp42-* dependency block, alphabetical)
- Modify: `crates/sp42-cli/src/main.rs`

**Step 1: Write the failing test** (in the existing `#[cfg(test)]` module of `main.rs`; find it with `grep -n "mod tests" crates/sp42-cli/src/main.rs` and follow its assertion style):

```rust
#[test]
fn verify_page_accepts_ga_appendix_format() {
    // Parse-level check only: the flag value is accepted and maps to the enum.
    let cli = Cli::try_parse_from([
        "sp42-cli", "verify-page", "--title", "X", "--format", "ga-appendix",
    ])
    .expect("ga-appendix parses");
    let Command::VerifyPage(args) = cli.command else {
        panic!("expected verify-page");
    };
    assert_eq!(args.fmt.format, PageReportFormat::GaAppendix);
}
```

(Match the existing tests' way of getting at `Cli`/`Command` — there are similar parse tests in the file; mirror one.)

Also the negative isolation guard — the whole point of the command-local enum, pinned so a future enum merge cannot pass silently:

```rust
#[test]
fn ga_appendix_is_rejected_on_commands_that_cannot_produce_it() {
    for argv in [
        vec!["sp42-cli", "verify", "--claim", "c", "--source-url", "https://x", "--format", "ga-appendix"],
        vec!["sp42-cli", "bare-url", "preview", "--title", "X", "--format", "ga-appendix"],
        vec!["sp42-cli", "preview", "--format", "ga-appendix"],
    ] {
        assert!(Cli::try_parse_from(argv.clone()).is_err(), "{argv:?} must reject ga-appendix");
    }
}
```

(Adjust each argv's required args to whatever those commands actually demand — the assertion is only that `--format ga-appendix` fails to parse; check the existing parse tests for the minimal valid argv per command and reuse it.)

**Step 2: Run to verify failure** — `cargo test -p sp42-cli verify_page_accepts_ga_appendix_format` → compile error.

**Step 3: Implement**

Directly below `OutputFormat` (after line 42), add:

```rust
/// Output formats for the page-report commands (`verify-page`,
/// `render-report`) only. Command-local on purpose: the shared
/// [`OutputFormat`] is flattened into commands that cannot produce a GA
/// appendix and must not advertise it (PRD-0016).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "lowercase")]
enum PageReportFormat {
    Text,
    Json,
    Markdown,
    /// GA evidence appendix wikitext (PRD-0016).
    #[value(name = "ga-appendix")]
    GaAppendix,
}

/// Shared `--format` flag for the page-report commands.
#[derive(Debug, Args)]
struct PageReportFormatArg {
    /// Output format.
    #[arg(long, value_enum, default_value = "text")]
    format: PageReportFormat,
}
```

Then:
- `VerifyPageArgs` (line 334-335): change `fmt: FormatArg` → `fmt: PageReportFormatArg`.
- `render_verify_page` (line 2957): change the `format: OutputFormat` parameter to `format: PageReportFormat`, adjust the match arms to the new enum, and add:

```rust
        PageReportFormat::GaAppendix => Ok(sp42_assessment::render_ga_appendix(
            &report,
            SystemClock.now_ms(),
            env!("CARGO_PKG_VERSION"),
        )),
```

(`SystemClock` is already imported at line 15; check whether the `Clock` trait itself is in scope for `.now_ms()` — line 997 shows usage as `&SystemClock`, so add `use sp42_types::Clock;` if the method call doesn't resolve. `sp42_assessment` is a new extern; no `use` needed with the fully qualified path.)

- Dispatch arm (line 690-694): no change needed beyond types lining up (`args.fmt.format` is now `PageReportFormat`).
- Legacy-argv translation (3598/3616): builds `VerifyPageCliOptions from args` and never touches `fmt` — verify by reading those two sites, change nothing.

**Step 4: Run tests**

```sh
cargo test -p sp42-cli && cargo clippy -p sp42-cli --all-targets -- -D warnings
```
Expected: PASS, including all existing legacy-translation tests.

**Step 5: Commit**

```sh
git add crates/sp42-cli
git commit -m "feat(cli): ga-appendix output format on verify-page (PRD-0016)"
```

---

## Task 2: `render-report` — pure render of a saved report

**Files:**
- Modify: `crates/sp42-cli/src/main.rs`

**Step 1: Write the failing tests**

```rust
#[test]
fn render_report_renders_a_saved_report_without_any_server() {
    // Minimal saved report: what verify-page --format json emits.
    let report = serde_json::json!({
        "wiki_id": "testwiki",
        "rev_id": 12345,
        "title": "Fixture",
        "findings": [],
        "skipped": [],
        "extraction_failures": [],
        "stats": {
            "refs_seen": 0, "use_sites_verified": 0, "skipped": 0,
            "extraction_failures": 0, "supported": 0, "partial": 0,
            "not_supported": 0, "source_unavailable": 0,
            "source_unavailable_unreachable": 0, "source_unavailable_unusable": 0
        }
    });
    let dir = std::env::temp_dir().join("sp42-cli-render-report-test");
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let path = dir.join("report.json");
    std::fs::write(&path, report.to_string()).expect("write fixture");

    let out = run_render_report(&path, PageReportFormat::GaAppendix).expect("renders");
    assert!(out.contains("Criterion 2"));
    assert!(out.contains("Fixture"));

    // Determinism through the CLI path is covered by the renderer's own
    // pinned-timestamp test; here assert the ga-appendix path threads a real
    // clock without panicking and json round-trips.
    let json_out = run_render_report(&path, PageReportFormat::Json).expect("renders json");
    assert!(json_out.contains("\"rev_id\": 12345"));
}

#[test]
fn render_report_reports_a_readable_error_for_a_bad_file() {
    let err = run_render_report(std::path::Path::new("/nonexistent/report.json"), PageReportFormat::Text)
        .expect_err("missing file errors");
    assert!(err.contains("/nonexistent/report.json"));
}
```

(Check the existing tests for temp-file precedent first — `grep -n "temp_dir\|tempfile" crates/sp42-cli/src/main.rs` — and follow it if one exists.)

**Step 2: Run to verify failure.**

**Step 3: Implement**

- `Command` enum (after `VerifyPage(VerifyPageArgs)`, line 231):

```rust
    /// Render a saved page-verification report (`verify-page --format json`
    /// output) with no bridge, session, or network (PRD-0016).
    RenderReport(RenderReportArgs),
```

- Args struct (near `VerifyPageArgs`):

```rust
/// `render-report`: pure local transform of a saved report file.
#[derive(Debug, Args)]
struct RenderReportArgs {
    /// Path to a saved report JSON (the exact `verify-page --format json` output).
    file: std::path::PathBuf,
    #[command(flatten)]
    fmt: PageReportFormatArg,
}
```

- Runner (near `render_verify_page`; no Tokio runtime — this must stay pure):

```rust
/// Render a saved `PageVerificationReport` from disk. The replay-friendly
/// core surface of PRD-0016: no bridge bootstrap, no session, no network —
/// a stored report renders identically anywhere.
fn run_render_report(
    path: &std::path::Path,
    format: PageReportFormat,
) -> Result<String, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|error| format!("could not read report file {}: {error}", path.display()))?;
    let report: PageVerificationReport = serde_json::from_str(&raw)
        .map_err(|error| format!("{} is not a saved page report: {error}", path.display()))?;
    Ok(match format {
        PageReportFormat::Json => {
            serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?
        }
        PageReportFormat::Markdown => render_page_verification_markdown(&report),
        PageReportFormat::Text => render_page_verification_text(&report),
        PageReportFormat::GaAppendix => sp42_assessment::render_ga_appendix(
            &report,
            SystemClock.now_ms(),
            env!("CARGO_PKG_VERSION"),
        ),
    })
}
```

(`PageVerificationReport` import: check line ~22's `sp42_citation` imports and extend them.)

- Dispatch arm (in the `match` at ~690, after the `VerifyPage` arm):

```rust
        Command::RenderReport(args) => run_render_report(&args.file, args.fmt.format),
```

- Legacy-argv rewriting: untouched — `render-report` has no legacy form. Run the existing legacy tests to confirm no interference.

**DoD bookkeeping note (record in the Phase 3 PRD changelog, verbatim intent):** two DoD readings are satisfied *structurally*, not by a named test — (a) saved-report and verify-page renders are byte-identical because both call the same pure `render_ga_appendix`; byte-level determinism is pinned by the phase-1 pinned-timestamp test, while the CLI surfaces inject a live clock and so differ across wall-clock runs by the footer date alone, by design; (b) "no network/inference in render-report" holds because the renderer takes no client parameters — there is nothing to mock. The closing PR must state these as by-construction guarantees, not claim tests that cannot exist.

**Step 4: Run tests** — `cargo test -p sp42-cli && cargo clippy -p sp42-cli --all-targets -- -D warnings` → PASS.

**Step 5: Commit**

```sh
git add crates/sp42-cli
git commit -m "feat(cli): render-report — pure GA-appendix render of a saved report (PRD-0016)"
```

---

## Task 3: Docs — `docs/platform/CLI.md`

**Files:**
- Modify: `docs/platform/CLI.md`

**Step 1: Edit**

- Commands table: add a row — `| render-report | Render a saved verify-page JSON report locally (no server). | no |`.
- `--format` note in the Global section: state that `verify-page` and `render-report` take `text|json|markdown|ga-appendix` while other commands keep `text|json|markdown`.
- New `## render-report` section after `## verify-page`: one paragraph (pure, no bridge; input is exactly `verify-page --format json` output; `ga-appendix` emits pasteable wikitext per PRD-0016) plus a usage example:

```sh
sp42-cli verify-page --title "Example" --wiki enwiki --format json > report.json
sp42-cli render-report report.json --format ga-appendix
```

**Step 2: Verify** — `grep -n "render-report" docs/platform/CLI.md` shows the three additions; render the table mentally for column alignment.

**Step 3: Commit**

```sh
git add docs/platform/CLI.md
git commit -m "docs(cli): document render-report and the ga-appendix format"
```

---

## Task 4: Phase gate

```sh
cargo build --workspace --all-targets --profile ci
cargo test --workspace --profile ci
cargo clippy --workspace --all-targets --all-features --profile ci -- -D warnings
./scripts/check-layering.sh
```
Expected: green (sp42-cli is a shell; a shell→domain dependency is layering-legal). Commit fallout fixes only if any (`chore(cli): phase-2 gate fixes`).
