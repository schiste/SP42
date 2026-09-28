# Intake / Lifecycle / Gates Implementation Plan — Phase 8: adapters, capability discovery, and shell wiring

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** Make intake real: implement the Stream adapter over `sp42-live`'s
existing ingestion, the Tier A/B capability-profile discovery with confirm-on-drift,
and boot-time wiring in all four shells — including resolving D4 (the browser has
no durable store) before touching `sp42-app`.

**Architecture:** A `Stream` adapter in `sp42-live` that emits `IntakeItem`s
instead of `EditEvent`s; capability discovery in `sp42-platform` behind an
injected `&dyn HttpClient`; a thin boot-time initializer per shell.

**Tech Stack:** `serde_json`, `serde_yaml`, the injected `HttpClient`/`Storage`/
`Clock` traits. **No new crate, no new dependency.**

**Scope:** Phase 8 of 8. The only phase that touches existing shipped behavior, so
it is also the only one that needs a behavior-preservation argument.

**Codebase verified:** 2026-09-28.

- `sp42-live/src/stream_ingestor.rs` — `StreamIngestor` (:15-30) holds
  `namespace_allowlist: BTreeSet<i32>`, `ignore_bots: bool`, `ignore_minor: bool`,
  `allowed_change_types: BTreeSet<String>`. Defaults are hardcoded in
  `from_config` (:34-46): `ignore_bots: true`, `ignore_minor: false`, and
  `SUPPORTED_CHANGE_TYPES: [&str; 2] = ["edit", "new"]` (:13). The whole filter is
  `ingest()` (:66-89): wiki match, change type, bot, minor, namespace — five early
  returns. `classify_editor` (:150) maps `~…`→`Temporary`, IP-parseable→
  `Anonymous`, else `Registered`. **There is no rights filter** (see D9).
  `StreamIngestor` is `Clone + PartialEq + Eq` with constructor-injected config, so
  extraction is clean.
- `crates/sp42-live/src/stream_runtime.rs:20` `StreamRuntime<E, S>` is already
  generic over `E: EventSource, S: Storage`, and already counts
  `delivered_events`/`filtered_events` into `StreamRuntimeStatus` (:11-17).
  `next_actionable_event` (:86), `drain_actionable_events` (:173),
  `reconnect_from_checkpoint` (:113).
- `crates/sp42-server/src/ingestion_supervisor.rs:38` `spawn_ingestion_supervisors`,
  called from `crates/sp42-server/src/main.rs:333` — the single best anchor in the
  repo for ADR-0026 §2's "always-on, per-wiki, spawn at boot". `AppState` already
  carries `clock: Arc<dyn Clock>`, `wiki_registry`, `runtime_storage_root`, and an
  `http_client` built at `main.rs:319`.
- `crates/sp42-wiki/src/capabilities.rs` — `WikiCapabilityProfile` (:21) is
  **actor**-scoped (`grant ∧ right ∧ token` per capability, :54-62), answering
  "can this user act on this wiki". ADR-0026 §5's Tier A/B profile is a different
  thing and is entirely greenfield: no namespace map, no content models, no rights
  catalogue, no TTL, no Tier B confirmation, no confirm-on-drift. The nearest
  existing content-model logic is the static table
  `default_namespace_content_model` (`sp42-platform/src/wikibase.rs`), used by
  `stream_ingestor.rs:107`.
- `sp42-fetch`'s `GuardedHttpClient` implements `sp42_types::HttpClient`
  (`client.rs:188`) and **is GET-only** (`client.rs:194-198` rejects non-GET).
  `action=query&meta=siteinfo` and `list=categorymembers` are both GET, so this is
  fine — but a future write path must use `sp42-platform`'s
  `execute_wiki_page_save` (`action_executor.rs`), not the fetch edge.
- Boot paths: `sp42-server` `main.rs:286-336`; `sp42-cli` has **no** boot sequence
  (a clap `Subcommand` dispatcher, `main.rs:652`, ~50 handlers, `block_on` per
  command); `sp42-app` `lib.rs:13` `run_app()` → `install_panic_hook`,
  `warm_browser_platform_symbols`, `collect_browser_bootstrap_snapshot().await`,
  `mount_root_or_body`; `sp42-desktop` `main.rs:222` `build_console_snapshot()` —
  currently a fixture-rendering console, not a live shell.
- Fixtures already on disk and directly reusable:
  `fixtures/frwiki_recentchanges_batch.jsonl`, `fixtures/frwiki_recentchange_edit.json`.
- `sp42-live` has 2 `proptest!` blocks already (`stream_ingestor.rs:598`,
  `recent_changes.rs:941`).
- D4: `sp42-app`'s only `Storage` impls are `LocalStorageBrowserStorage` and
  `VolatileBrowserStorage` (`platform/runtime.rs:120,177`); the latter is
  non-durable. CONSTITUTION §10.1 restricts LocalStorage for tokens, §5.2 caps the
  wasm bundle at <400KB gzipped.

---

## Task 1: Resolve D4 before touching `sp42-app`

**Files:** none — this is a decision gate, not code.

**Step 1: Get the owner's answer on D4** (Phase 0): server-side-only lifecycle
records, or a new IndexedDB `Storage` implementation. The recommendation is
server-side, because it keeps ADR-0027's durability and audit-trail claims honest
and avoids spending wasm budget on a store the browser cannot make authoritative.

**Step 2: Do not start Task 5 until this is answered.** An append-only audit log
that silently loses writes in one of four shells is worse than one that
consistently does not record there — and ADR-0026 §2's "every shell wires intake at
boot" makes the inconsistency the kind of thing that is much cheaper to design
around now than to unpick later.

---

## Task 2: The Stream adapter

**Files:**
- Create: `crates/sp42-live/src/intake_stream_adapter.rs`
- Modify: `crates/sp42-live/src/stream_runtime.rs`
- Create: `fixtures/frwiki_intake_stream_cases.jsonl`

**Step 1: The seam is already clean.** `ingest()` returns
`Result<Option<EditEvent>, _>` where `Ok(None)` means filtered out. The adapter is
"emit `IntakeItem`s instead, and push the five early returns into a baseline
pipeline". No reentrancy, no shared mutable state, all config injected.

**Step 2: The hardcoded defaults become config — and that is constitutionally
required, not tidying.** `ignore_bots: true`, `ignore_minor: false`,
`allowed_change_types: ["edit","new"]` are a live queue cutoff living in Rust,
which SCORING_CONSTITUTION §14.3.2 explicitly forbids. Move them into the embedded
baseline pipeline config (`configs/intake/`), keeping the `Default` impl as the
"safe fallback in code" that §14.3.1 permits.

**Step 3: The baseline must not reference a temporal field.** Per D9, the adapter
yields `created_at: None`, `last_revision_at: None`, `account_created_at: None`, and
`rights: None`. Any baseline rule on those fields evaluates `Unknown` and —
because `on_unknown` defaults to `Drop` — **silently drops real traffic on day
one.** ADR-0026 §2 already restricts the baseline to "intrinsic facts (event type,
bot flag)"; hold that line, and write an acceptance test asserting the baseline
admits a representative frwiki edit event.

**Step 4: The behavior-preservation argument, as a test.** For every case in
`fixtures/frwiki_recentchanges_batch.jsonl`, assert the new path admits exactly
what `StreamIngestor::ingest` admitted before. That is the evidence ADR-0026 §129
promises when it says the migration is "behavior-preserving, but a real refactor".
Note the one intended difference: rights become `None` rather than filtered on,
which is why ADR-0026 §3 types `rights` as `Option`.

**Step 5: `proptest` on the adapter**, matching the two blocks already in the crate.

**Step 6: Commit** — `refactor(live): sp42-live ingestion becomes an intake Stream adapter`

---

## Task 3: Tier A/B capability profiles

**Files:**
- Create: `crates/sp42-platform/src/capability_profile.rs`
- Create: `configs/capabilities/frwiki.yaml`

**Step 1: Tier A — discovered facts, behind an injected client.**
Namespace map, available rights, content models, from
`action=query&meta=siteinfo`. Take `&dyn HttpClient` exactly as
`wiki_storage.rs:427` does (`where C: HttpClient + ?Sized`). **Do not add
`sp42-fetch` to `sp42-platform/Cargo.toml`** — it pulls `reqwest` + `tokio` in,
violating CONSTITUTION §2.3, and counts against the wasm ceiling. The shell
constructs `GuardedHttpClient` and injects it, which is what
`main.rs:319` already does.

**Step 2: Cached with a TTL, refreshed as a background job, never inline in the
intake hot path** (ADR-0026 §5). Diff against the last known-good profile and
surface drift rather than applying it silently.

**Step 3: Tier B — confirmed policy meaning, and the confirm-on-drift rule.**
Which discovered right means "skip human review", which namespace plays "mainspace"
for a given workflow. These are judgments. Activation follows ADR-0010's
propose/confirm pattern, and **any Tier A drift that would change a Tier B mapping
re-requires confirmation rather than auto-updating**. The reviewer-facing surface
for this is a Non-goal (needs its own PRD) — so the *mechanism* is: a confirmation
record that can be satisfied by whatever that PRD eventually builds, with the
absent-surface path failing closed.

**Step 4: Registration is explicit, via the Phase 0 `is_registered` predicate —
never `resolve()`.** See Trap 2. An event naming an unconfigured-but-real Wikimedia
dbname must not cause intake to filter on that wiki.

**Step 5: `CapabilityRef` resolution is checked twice, per ADR-0026 §6** — at config
load time (reject outright) and at evaluation time (return `Misconfigured` if a
Tier A drift broke a previously-valid reference). `Misconfigured` is distinct from
`Drop`, and distinct from `Unknown`.

**Step 6: Commit** — `feat(platform): two-tier wiki capability profiles with confirm-on-drift`

---

## Task 4: Boot wiring — server, CLI, desktop

**Files:**
- Modify: `crates/sp42-server/src/main.rs:333` (sibling to `spawn_ingestion_supervisors`)
- Modify: `crates/sp42-server/src/state.rs`
- Modify: `crates/sp42-cli/src/main.rs`
- Modify: `crates/sp42-desktop/src/main.rs:222`

**Step 1: `sp42-server`.** Add a sibling call to `spawn_ingestion_supervisors(&state)`
at `main.rs:333` — that call is already the "always-on, per-wiki, spawn at boot"
shape ADR-0026 §2 describes, so intake startup belongs right beside it rather than
invented elsewhere. Add whatever `AppState` needs: the compiled baseline pipeline,
the capability-profile cache, and the `Storage`-backed lifecycle store.

**Step 2: Resolve the `now_ms` question at the shell edge.** The evaluator takes
`now_ms: i64` (Phase 3), and the shell already holds `clock: Arc<dyn Clock>`.
That is the whole point of the split — pass `clock.now_ms()` in, keep the platform
clock-free per CONSTITUTION §1.4/§2.3.

**Step 3: `sp42-cli`.** There is no boot sequence to hook, so this is a design
choice rather than an insertion point: either eager init in `run()`, or a new
`intake` subcommand, or a shared `ShellContext` factory. A CLI is also the natural
future home for ADR-0026 §3's `IntakeQuerySpec` "topic research" case — but that
surface is an explicit Non-goal, so do not build it here. **Get the owner's
preference**; it is a small fork.

**Step 4: `sp42-desktop`.** `build_console_snapshot()` is currently a
fixture-rendering console, not a live shell. Decide with the owner whether
"intake always-on" means anything for a console that renders fixtures — if not,
say so in the code rather than adding a no-op initializer.

**Step 5: Commit** — `feat(shell): wire intake startup into the server, CLI, and desktop boots`

---

## Task 5: `sp42-app` (wasm) — only after D4

**Files:**
- Modify: `crates/sp42-app/src/lib.rs:13` (`run_app`)
- Possibly: a new `Storage` impl in `sp42-app/src/platform/runtime.rs`

**Step 1: Do not proceed without D4.** See Task 1.

**Step 2: Whatever D4 says, the wiring point is `run_app()`** — after
`install_panic_hook()` and `warm_browser_platform_symbols()`, alongside the
existing `collect_browser_bootstrap_snapshot().await`.

**Step 3: If D4 goes the IndexedDB route**, budget it explicitly: it is a new
`Storage` impl, it costs wasm size against the §5.2 <400KB gzipped ceiling, and
`check-wasm-size.sh` will measure it. The intake engine itself is platform and
compiles to wasm already, so the marginal cost is the store.

**Step 4: Commit** — `feat(app): wire intake into the browser boot` (or omit, per D4)

---

## Phase 8 exit criteria

- [ ] Every case in `fixtures/frwiki_recentchanges_batch.jsonl` is admitted by
      exactly the set the old `ingest()` admitted
- [ ] `ignore_bots` / `ignore_minor` / `allowed_change_types` are config, not Rust constants
- [ ] The baseline pipeline references no temporal field, and admits a representative edit
- [ ] `ActorRights` is `None` (not `Some(vec![])`) for the stream adapter
- [ ] `sp42-platform/Cargo.toml` still has no `tokio` and no `reqwest`
- [ ] Tier A discovery goes through an injected `&dyn HttpClient`, GET-only, off the hot path
- [ ] Tier B drift re-requires confirmation; it never auto-updates
- [ ] Intake skips an unconfigured-but-real wiki id (Trap 2 regression test)
- [ ] `CapabilityRef` failure yields `Misconfigured`, distinct from `Drop` and `Unknown`
- [ ] All four shells wired, or the exceptions are deliberate and commented
- [ ] `./scripts/generate-architecture-map.sh --check` green
- [ ] `./scripts/check-wasm-size.sh` within ceiling
