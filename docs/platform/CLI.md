# SP42 CLI (`sp42-cli`)

`sp42-cli` is the command-line shell over the SP42 platform: ad-hoc and
whole-page citation verification, an offline locate probe, bare-URL repair, and
the dev preview / operator report views. It is the host-side counterpart to the
browser and desktop shells.

Run it from a checkout with:

```sh
cargo run -p sp42-cli -- <command> [options]
```

Capabilities are **subcommands**. The CLI is built on [`clap`], so `--help` and
`--version` work everywhere, and every subcommand prints its own `--help`:

```sh
sp42-cli --help              # list commands
sp42-cli verify --help       # options for one command
sp42-cli bare-url --help     # nested subcommands
```

This document is the canonical, fuller reference; the inline `--help` is the
quick one.

## Commands

| Command | What it does | Reads STDIN |
| --- | --- | --- |
| `verify` | Verify one claim against one source URL (runs the model in-process). | no |
| `verify-page` | Verify every citation on a revision via the bridge. | no |
| `batch` | Verify a JSONL batch (one case per line), one result line out. | JSONL cases (or `--file`) |
| `locate-probe` | Offline check whether a quote locates in a source body. | source body |
| `bare-url preview` / `bare-url execute` | Preview or apply bare-URL repairs. | no |
| `preview [mode]` | Dev / operator queue views; default is the ranked queue. | event payload |

Output goes to **stdout** on success (exit `0`); errors print to **stderr**
(exit `1`). `--help`/`--version` and `clap` usage errors are handled by `clap`
(exit `0` for help/version, `2` for a usage error).

### Legacy flag forms (deprecated)

The CLI previously selected capabilities with top-level flags rather than
subcommands. Those forms still work for now: an old-style invocation is
translated to the equivalent subcommand and a deprecation warning is printed to
stderr. **They will be removed**, so migrate scripts to the subcommand form.

| Old | New |
| --- | --- |
| `--claim … --source-url …` | `verify --claim … --source-url …` |
| `--verify-page …` | `verify-page …` |
| `--locate-probe --quote …` | `locate-probe --quote …` |
| `--batch` / `--batch-file <path>` | `batch` / `batch --file <path>` |
| `--bare-url-preview …` / `--bare-url-execute …` | `bare-url preview …` / `bare-url execute …` |
| `--shell <mode>`, `--<mode>`, `--<mode>-preview` | `preview <mode>` |
| _(no flags)_ | `preview` |

Note the old alias quirk is preserved during translation: the shorthand
`--operator-report` maps to `preview server-report`, but `--shell operator-report`
maps to `preview parity-report`.

## Global

| Flag | Description |
| --- | --- |
| `-h`, `--help` | Print help (top-level or per-command) and exit. |
| `-V`, `--version` | Print version and exit. |

`--format <text\|json\|markdown>` (default `text`) is available on every command
that renders. `--bridge-base-url <URL>` (default `http://127.0.0.1:8788`) is
available on the server-backed commands (`verify-page`, `bare-url execute`,
`preview`).

## `verify` — ad-hoc claim + source (PRD-0001)

Verify a single claim against a single source URL using the configured model
panel. `--claim` and `--source-url` are required.

| Flag | Description |
| --- | --- |
| `--claim <TEXT>` | The claim to verify. |
| `--source-url <URL>` | Source URL to fetch and check the claim against. |
| `--preceding-sentence <TEXT>` | Preceding-context sentence; repeatable, in order. The SIDE co-reference context — omit to keep the verifier on its no-context path. |
| `--with-metadata` | Include source metadata in the finding. |
| `--debug-votes` | Emit the full `VerificationOutcome` (finding + per-model votes, incl. raw claimed quotes) as JSON — the locate-replay harness surface (SP42#25). |
| `--no-repair` | Disable the bounded repair turn (one fewer model call per unlocated support vote; for cost control and A/B measurement). |
| `--verdict-only` | Print only the verdict label. |
| `--source-file <PATH>` | Use a file's contents as the source body instead of fetching `--source-url` (offline). Mutually exclusive with `--source-body`. |
| `--source-body <TEXT\|->` | Use this source body instead of fetching: `-` reads STDIN, any other value is literal text. `--source-url` is still required (provenance). |
| `--models <IDS>` | Per-run model panel override (comma-separated ids); falls back to `SP42_INFERENCE_MODELS`. Endpoint/token still come from the env. |
| `--article-title <TITLE>` | SIDE article-title context (with `--preceding-sentence`). |
| `--metadata-json <PATH>` | Bibliographic metadata sidecar (JSON file) used as prompt context instead of fetching Citoid (offline). |
| `--format <FORMAT>` | Output format. |

When `--source-file`/`--source-body` supplies a body, the verifier treats it as a
pre-fetched `200 text/plain` source and never fetches `--source-url` — reproducible,
no network. This runs the model in-process, so it needs the inference environment (see
[Environment](#environment)). It performs only read-only fetches, and the source
fetch holds **no** inference credential.

```sh
export SP42_INFERENCE_URL=...        # OpenAI-compatible base URL
export SP42_INFERENCE_MODELS=...     # comma-separated model ids
sp42-cli verify \
  --claim "The museum opened in 1897." \
  --source-url https://example.org/history \
  --format json --verdict-only
```

## `verify-page` — whole page (PRD-0001)

Verify **every** URL-bearing citation on a revision and render the page report.
The route is session + CSRF gated, so the CLI bootstraps a bridge session
(ADR-0002) before the call — a **running server** is required, and the server
(not the CLI) calls the model.

| Flag | Description |
| --- | --- |
| `--title <TITLE>` | Article title (required). |
| `--wiki <ID>` | Target wiki id. Default `testwiki`. |
| `--rev <REVID>` | Revision id. Omit for the latest revision (the server resolves it). |
| `--bridge-base-url <URL>` | Local server base URL. |
| `--format <FORMAT>` | Output format. |

```sh
sp42-cli verify-page --wiki enwiki --title "Museum" --format markdown
```

## `batch` — JSONL batch verify

Verify many cases in one run: read one JSON object per line (STDIN, or `--file`),
verify each against a single shared model panel, and emit one JSON result line per
case. A line that fails to parse or verify becomes an `error` result and the batch
continues — it never aborts. Each input `id` (if present) is echoed on its result
line, including for schema errors, so the harness can match results to inputs.

| Flag | Description |
| --- | --- |
| `--file <PATH>` | JSONL input file; omit to read from STDIN. |
| `--models <IDS>` | Per-batch model panel override (comma-separated ids); falls back to `SP42_INFERENCE_MODELS`. |

Each input line is a case: `claim` and `source_url` are required; `source_body`
(inline offline text), `metadata` (sidecar object), `preceding`, `article_title`,
`repair` (defaults true), `with_metadata`, and `id` are optional.

```sh
sp42-cli batch --file cases.jsonl > results.jsonl
cat cases.jsonl | sp42-cli batch --models "anthropic/claude-opus-4-8"
```

## `locate-probe` — offline (SP42#25)

A deterministic, model-free check of whether a quote locates within a source
body. Reads the source body from STDIN.

| Flag | Description |
| --- | --- |
| `--quote <TEXT>` | The quote to locate (required). |

```sh
curl -s https://example.org/page | sp42-cli locate-probe --quote "exact phrase to find"
```

## `bare-url` — repair (PRD-0008)

Preview or apply bare-URL → citation-template repairs for a revision.

```sh
sp42-cli bare-url preview --title "Museum" --rev 12345
sp42-cli bare-url execute --title "Museum" --rev 12345 --ordinal 0
```

`bare-url preview`:

| Flag | Description |
| --- | --- |
| `--title <TITLE>` | Article title (required). |
| `--rev <REVID>` | Revision id (required). |
| `--wiki <ID>` | Target wiki id. Default `testwiki` (the MVP's only enabled wiki). |
| `--bridge-base-url <URL>` | Local server base URL (the proposals request is posted to the bridge). |
| `--format <FORMAT>` | Output format. |

`bare-url execute` (same as preview, plus):

| Flag | Description |
| --- | --- |
| `--ordinal <N>` | Zero-based index of the proposal to apply (required). |
| `--action-note <SUMMARY>` | Edit summary attached to the applied repair. |
| `--bridge-base-url <URL>` | Local server base URL. |

## `preview` — dev / operator views

Consumes an event payload from STDIN (or a built-in sample when STDIN is empty)
and renders a queue view. The optional positional **mode** selects the view; omit
it for the default ranked-queue render.

```sh
sp42-cli preview                                  # ranked queue
sp42-cli preview parity-report --format markdown
cat events.json | sp42-cli preview session-digest --format markdown
sp42-cli preview server-report                    # needs a running server
```

| Mode | View |
| --- | --- |
| _(none)_ | Ranked queue (or workbench / context preview, see below). |
| `stream` | Live event stream preview. |
| `backlog` | Backlog preview. |
| `coordination` | Multi-operator coordination preview. |
| `session-digest` | Session digest. |
| `scenario-report` | Patrol scenario report. |
| `server-report` | Localhost server report (needs a running server). |
| `parity-report` | Server/offline parity report. |
| `action-preview` | Reviewer action preview. |
| `action-execute` | Reviewer action execute (needs a running server). |

Shared options on `preview`:

| Flag | Description | Default |
| --- | --- | --- |
| `--format <FORMAT>` | Output format. | `text` |
| `--bridge-base-url <URL>` | Local server base URL for `server-report` / `action-execute`. | `http://127.0.0.1:8788` |
| `--workbench-token <TOKEN>` | With no mode, switches the default render into workbench submission. | |
| `--workbench-actor <NAME>` | Workbench actor name. | `SP42-cli` |
| `--workbench-note <NOTE>` | Workbench submission note. | `cli local workbench` |
| `--context-talk <WIKITEXT>` | With no mode, renders a context preview from this talk-page wikitext. | |
| `--context-liftwing <PROB>` | LiftWing damaging probability (float) for the context preview. | |
| `--action-kind <KIND>` | `patrol` \| `rollback` \| `undo` for the action views. | `patrol` |
| `--action-note <NOTE>` | Note attached to the action. | |

With no mode, the render is selected by which flags are present: `--workbench-token`
→ workbench submission, `--context-talk`/`--context-liftwing` → context preview,
otherwise the plain ranked queue.

## Environment

| Variable | Used by | Meaning |
| --- | --- | --- |
| `SP42_INFERENCE_URL` | `verify` | OpenAI-compatible model base URL (required). |
| `SP42_INFERENCE_MODELS` | `verify` | Comma-separated model ids for the panel (required). |
| `SP42_INFERENCE_PROVIDER` | `verify` | Provider label. Default `configured`. |
| `SP42_INFERENCE_TOKEN` | `verify` | Bearer token for the inference endpoint (optional). |
| `SP42_INFERENCE_CAPABILITY` | `verify` | Capability tag (optional). |
| `SP42_INFERENCE_MODE` | `verify` | Endpoint mode (optional). |
| `SP42_FETCH_ALLOW_PRIVATE` | any source fetch | Set to `1` to allow loopback/private source hosts — a dev/test escape hatch for the loopback-serving benchmark harness. Off by default (SP42#34 SSRF floor). |

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | Success, or `--help`/`--version`. |
| `1` | A command failed (fetch/verify/render error). The message prints to stderr. |
| `2` | A `clap` usage error (unknown command, missing required option, bad value). |

[`clap`]: https://docs.rs/clap
