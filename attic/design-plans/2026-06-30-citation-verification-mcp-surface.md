# Citation-Verification Agent Surface (MCP) Design

## Summary

SP42's citation engine — the SSRF-hardened fetch pipeline and the verbatim-grounded model panel — is today reachable only through `sp42-server`'s page-level HTTP routes and `sp42-cli`. This design exposes those same primitives as four atomic MCP tool verbs in a new native Rust crate, `sp42-mcp`, built on the `rmcp` SDK. The crate is an adapter layer, not new verification logic: `probe_source` wraps `source_fetch` + `classify_source_usability`; `verify_claim` wraps `verify_citation_use_site` with its `locate_quote` grounding gate; `verify_wikipedia_page` wraps the existing Parsoid fan-out; and `verify_wikidata_statement` adds a net-new statement→claim renderer before calling the same `verify_claim` path. The verbatim-grounding gate (ADR-0007 §5) and the `Verdict` taxonomy (ADR-0008) are surfaced unchanged as the public contract.

The verb surface is tiered by cost. `probe_source` is model-free and deterministic — it reports reachability, extractability, and the fetched-but-unusable reason so a caller can screen accessibility before paying for a panel call. `verify_claim` and the two convenience verbs run the model panel, declare MCP `taskSupport: "optional"` for the hosted phase, and execute synchronously over stdio in the MVP. The MVP transport is local stdio with in-process `rmcp`-over-`sp42-core` (no double-hop); callers supply their own inference credentials via the existing `SP42_INFERENCE_*` env seam. The hosted streamable-HTTP transport, SSRF hardening for untrusted callers, result caching, and the roadmap verbs (`find_source`, `assess_reliability`, `is_blp`) are out of scope for the MVP.

## Definition of Done

This design covers the **agent-facing verb surface** for SP42's citation engine and a first transport (MCP over stdio). It is split into an MVP (Phases 1–6, the immediate deliverable) and a roadmap (find_source, assess_reliability, hosted transport — direction only, separate plans).

The design exposes what SP42 *uniquely* owns — mechanistic source access and the verbatim-grounded model panel — as a small set of typed verbs an external agent (e.g. a Wikipedia/Wikidata editing agent) can call as tools. It does **not** re-serve community-owned data (source-reliability lists) or run an agent loop; SP42 is the tool, the caller is the agent.

**The MVP is done when:**
- A consumer connects an MCP client to the `sp42-mcp` stdio server, lists tools, and sees exactly the four MVP verbs (`probe_source`, `verify_claim`, `verify_wikipedia_page`, `verify_wikidata_statement`) with their schemas — verified by an integration test that drives the server over stdio and asserts the tool list and one round-trip call per verb.
- `probe_source(url)` returns a result that **distinguishes unreachable from reachable-but-unextractable** (the fetched-but-unusable distinction, cf. `2026-06-25-fetched-but-unusable-source-recovery`), so a consumer learns "a human could still read this even though SP42's pipeline can't" — verified by unit tests over a reachable-but-paywalled fixture, an unreachable URL, and a clean article. Zero model-panel calls (deterministic).
- `verify_claim` accepts the source as **either** a URL **or** pre-fetched content (`{ url } | { text, retrieved_from? }`), so a caller that already fetched (Citoid, archive) is not forced into a re-fetch — verified by tests exercising both input shapes and asserting identical grounding behavior on the same bytes.
- `verify_claim` returns the existing governed `Verdict` (ADR-0007/0008) unchanged — `Supported | Partial | NotSupported | SourceUnavailable` plus the re-located quote — with the **verbatim-grounding gate intact**: a Supported/Partial verdict is surfaced only if its quote re-locates in the fetched bytes (`locate_quote.rs`). No new verdict variants; the anti-fabrication guarantee is the public contract.
- `verify_wikipedia_page(title)` decomposes via the existing Parsoid/extract path and fans out `verify_claim` at the existing `PAGE_VERIFY_CONCURRENCY`, emitting **progress notifications** during the fan-out — verified by a fixture page yielding per-use-site verdicts and at least one observed progress event.
- `verify_wikidata_statement(ref)` renders a statement into a claim sentence, resolves its reference URL (P854), and calls `verify_claim` — verified by a test over a statement with a reference-URL qualifier. This is **net-new** (no Wikidata path exists today); a statement with no resolvable reference URL returns `SourceUnavailable`, not an error.
- Expensive verbs (`verify_claim` and the two convenience verbs) declare `taskSupport: "optional"` and run **synchronously over stdio for the MVP**, with the same handler wrappable as an async MCP task for the hosted phase — no forked logic. (No MVP result cache: a per-session stdio server rarely sees a repeat, and a cache would make `idempotentHint` claim a stability the un-cached panel can't honor; persistent cross-user caching is a hosted-phase lever.)
- `sp42-mcp` is a **native (host-only) crate** like `sp42-server`/`sp42-cli` — it is not part of the `wasm32` build and does not regress it. `cargo test -p sp42-mcp` and the existing `sp42-app` wasm build both pass.

**The roadmap is done when** (tracked here for direction, specified in later plans): `find_source(claim)` locates and verifies candidate sources for an unsourced claim (the {{citation needed}} → suggested-citation flow, continuous with PRD-0009); `assess_reliability(claim, url, wiki)` gives a context-sensitive, policy-aware reliability judgment with Cite Unseen data as an internal input; and a streamable-HTTP transport with a re-reviewed SSRF/abuse threat model and a cost-ownership model (BYO-key passthrough / metering) serves untrusted multi-tenant callers.

## Glossary

- **MCP (Model Context Protocol)**: The protocol (spec 2025-11-25) by which AI agents discover and invoke tools exposed by an MCP server. Defines JSON-RPC wire format over stdio or HTTP, tool-listing and call lifecycle, and extensions such as Tasks for long-running operations. `sp42-mcp` is the server side.
- **rmcp**: The official Rust MCP SDK (`rmcp` on crates.io). Provides the `#[tool]` attribute macro that generates JSON schemas from Rust handler types and the transport layer for stdio and future streamable HTTP.
- **`probe_source`**: The cheap, deterministic MCP verb — no model calls. Checks reachability, HTTP status, and pipeline extractability for a URL; returns the fetched-but-unusable distinction (`unusable_reason`, `human_readable_hint`) so a caller can decide whether to spend a panel on `verify_claim`.
- **`verify_claim`**: The core model-panel MCP verb. Runs fetch→extract→panel→ground on either a URL or caller-supplied content and returns a `Verdict` with re-located quote. The verbatim-grounding gate is internal and non-negotiable.
- **Verdict taxonomy**: The four-value enum governing all verification outcomes — `Supported | Partial | NotSupported | SourceUnavailable` — defined by ADR-0007/0008. `sp42-mcp` surfaces it unchanged; no new variants are introduced.
- **Verbatim grounding / `locate_quote`**: The anti-fabrication gate (`locate_quote.rs`) requiring that a `Supported` or `Partial` verdict cite a quote that re-locates verbatim in the fetched source bytes. A verdict without a re-locatable quote is downgraded. Preserving this gate on the MCP surface is an explicit hard requirement.
- **PanelAgreement**: A summary of how consistently the model panel voted, returned alongside the `Verdict` by `verify_claim`. Lets a caller gauge confidence and route disagreements to human review.
- **Model panel**: The set of language models that independently vote on whether a source supports a claim; votes are tallied by `voting.rs`. Invoked by `verify_claim` and the convenience verbs; bypassed by `probe_source`.
- **Tasks / `taskSupport`**: The MCP 2025-11-25 Tasks extension. A tool declares `execution.taskSupport: "optional"` to signal it can run synchronously or as an async task (returning a `taskId` for polling). The three expensive SP42 verbs declare this so the same handler logic works over both the stdio MVP and the future hosted transport without forked code paths.
- **Parsoid**: Wikipedia's server-side wikitext-to-HTML renderer. `verify_wikipedia_page` decomposes an article's claim/reference structure via the Parsoid + `extract.rs` path, reusing the fan-out and pacing from `citation_routes.rs`.
- **Citoid**: Wikipedia's citation metadata service that expands bare reference URLs to structured citation data. SP42's existing `CITOID_PACE` in `citation_routes.rs` rate-limits requests to it; `verify_wikipedia_page` inherits that pacing.
- **P854**: The Wikidata property for "reference URL" — the qualifier on a statement reference that names the cited source. `verify_wikidata_statement` resolves P854 to pass to `verify_claim`; a statement with no P854 qualifier returns `SourceUnavailable`.
- **Cite Unseen**: A dataset of Wikipedia community source-reliability assessments (enwiki-only, context-sensitive). Named as a future internal input to the roadmap verb `assess_reliability`; not used in the MVP.
- **BYO-key (bring-your-own-key)**: The MVP cost model: callers supply their own inference-API credentials via the existing `SP42_INFERENCE_URL/TOKEN/MODELS` env vars and pay their own model inference. No metering or credential management in `sp42-mcp`.
- **SSRF (Server-Side Request Forgery)**: An attack where a caller-controlled URL causes the server to make requests to internal network addresses. SP42's fetch stack includes an existing SSRF guard (`check_fetchable_source_url`, `urls.rs`). In the MVP (local stdio, trusted caller) this is defense-in-depth; in the hosted phase with untrusted callers it becomes load-bearing — requiring a full re-review before the hosted transport ships.
- **wasm32 / native-crate split**: `sp42-core` compiles to `wasm32-unknown-unknown` for the browser-side `sp42-app`. `sp42-mcp` uses tokio, `rmcp`, and live network I/O, making it host-only — it must not enter the wasm build. The pattern mirrors `sp42-server` and `sp42-cli`.
- **BodyUsabilityReason**: Rust enum from `body_classifier.rs` (introduced in the sibling design `2026-06-25-fetched-but-unusable-source-recovery`) naming why a source body failed the usability gate — e.g., `PdfBody`, `ViewerShell`, `NavChromePaywall`. Surfaced by `probe_source` as `unusable_reason`.
- **PRD-0009**: The product requirements document for citation insertion — the flow from `{{citation needed}}` to a suggested, verified source. The roadmap verb `find_source` (search→fetch→`verify_claim`) is described as PRD-0009-continuous.
- **ADR-0007 / ADR-0008**: Architectural decision records governing verification semantics (ADR-0007: Verdict taxonomy, verbatim-grounding requirement, `SourceUnavailable` routing) and the verification public contract (ADR-0008). `sp42-mcp` is bound by both; no new verdict variants, no changes to grounding logic.

## Architecture

SP42's verification value is two things no external agent can cheaply reproduce: a **hardened fetch/extract pipeline** (`crates/sp42-core/src/citation/source_fetch.rs` + the `classify_source_usability` gate in `body_classifier.rs`, behind the SSRF-guarded client in `crates/sp42-inference/src/lib.rs::guarded_source_client`) and a **verbatim-grounded model panel** (`citation/verify.rs` orchestration, `voting.rs` panel tally, `locate_quote.rs` anti-fabrication gate, `verdict.rs` taxonomy). Today these are reachable only through `sp42-server`'s page-oriented HTTP routes (`crates/sp42-server/src/citation_routes.rs`, `verify_page`) and the `sp42-cli`. This design exposes them as **atomic, agent-callable verbs** behind a new native crate, `sp42-mcp`, using the official Rust MCP SDK (`rmcp`).

**Verb surface (the contract).** Four MVP verbs, two roadmap verbs. Cheap/deterministic verbs run synchronously; the panel verb and its convenience wrappers are MCP-task-capable.

```
// --- MVP ---

// Cheap, deterministic, no model. SP42-owned mechanistic truth.
probe_source(url: string)
  -> { reachable: bool,
       http_status: u16 | null,
       extractable: bool,                     // pipeline can get usable text
       unusable_reason: BodyUsabilityReason | null,   // PdfBody | ViewerShell | NavChromePaywall | ...
       human_readable_hint: bool }            // reachable but not pipeline-extractable -> a person could still read it

// Expensive, model panel, MCP-task-capable. The core.
verify_claim(claim: string,
             source: { url: string } | { text: string, retrieved_from?: string },
             panel?: PanelConfig)
  -> Verdict { support: Supported | Partial | NotSupported | SourceUnavailable,
               quote: string | null,          // re-located verbatim in fetched bytes, or null
               agreement: PanelAgreement | null }

// Convenience; orchestrates the primitives. Wikipedia-native.
verify_wikipedia_page(title: string, options?: PageOptions)
  -> { uses: [ { claim: string, ref_url: string, verdict: Verdict } ] }   // progress notifications during fan-out

// Convenience; net-new statement->claim renderer. Wikidata-native.
verify_wikidata_statement(statement_ref: StatementRef, options?: StatementOptions)
  -> { claim_rendered: string, ref_url: string | null, verdict: Verdict }

// --- Roadmap (contract sketch, not MVP) ---

find_source(claim: string, options?: FindOptions)
  -> [ { url: string, verdict: Verdict } ]    // search -> fetch -> verify_claim; PRD-0009 lineage

assess_reliability(claim: string, url: string, wiki: string, regime?: Default | BLP | MEDRS)
  -> { judgment, rationale, inputs }          // context-sensitive; Cite Unseen data as one internal input

is_blp(article: string)                       // Wikidata subject-classification; triggers the BLP regime
  -> { blp_applies: bool, basis, confidence }  // P31->Q5 / P570 via Wikidata; escalate to sources if ambiguous
```

**Tiering and cost control.** The split exists so the *agent* gates spend. `probe_source` is deterministic and free of model calls; an agent screens accessibility before paying for a panel (a URL `probe_source` reports unreachable would only resolve to `SourceUnavailable` after a panel fetch anyway). This is the concrete form of the "a cheap screen cannot be a confident judge" constraint: the cheap tier judges *nothing* — it reports deterministic facts (reachability, extractability). There is deliberately **no cheap source-reliability verb**: reliability is community-owned, enwiki-only (Cite Unseen), and context-sensitive (WP:RSP), so a cheap domain→verdict lookup would misrepresent policy; reliability becomes the context-aware roadmap verb `assess_reliability`.

**Execution model.** Per MCP 2025-11-25 Tasks, expensive verbs declare `execution.taskSupport: "optional"`. Over stdio (MVP) the `rmcp` handler runs synchronously; the *same* handler is wrapped as an async task (`taskId` + poll) for the hosted phase, so there is one code path. Convenience verbs fan many `verify_claim` calls out and emit `notifications/tasks/status` progress ("18/40 use-sites verified") so even the synchronous MVP isn't opaque. The MVP carries **no result cache**: a per-session stdio server rarely sees a repeat, evals run against `sp42-core` directly (not through this layer), and an un-backed `idempotentHint` would over-promise stability the panel can't give. Persistent, cross-user caching (`cacheScope: public`) — the genuine cost lever at scale — is deferred to the hosted phase.

**Deployment.** MVP is local stdio: the agent-builder runs `sp42-mcp`, brings their own model keys (via the existing `SP42_INFERENCE_URL/TOKEN/MODELS` env seam consumed by `sp42-inference`), and pays their own inference. In-process `rmcp`-over-`sp42-core` (no double-hop). Hosted streamable-HTTP (axum, which `sp42-server` already uses) is a later phase where the SSRF floor stops being defense-in-depth and becomes load-bearing — re-reviewed under "untrusted caller controls the URL."

## Existing Patterns

This design wraps, rather than reworks, the established citation architecture (ADR-0006 LLM use, ADR-0007 verification semantics, ADR-0008 verification contract, ADR-0011 article-level verification):

- **Verb bodies already exist as separable functions.** `probe_source` wraps `source_fetch` + `classify_source_usability` (`body_classifier.rs`); `verify_claim` wraps `verify_citation_use_site` / `execute_citation_verify` (`verify.rs`) with its `locate_quote.rs` grounding and `voting.rs` panel; `verify_wikipedia_page` wraps the `verify_page` fan-out (`citation_routes.rs`). The MCP layer is an adapter over these, not new verification logic.
- **Governed verdict/reason separation.** The surface returns the existing `Verdict` (`verdict.rs`) and `BodyUsabilityReason` (`body_classifier.rs`) unchanged. No parallel taxonomy; the anti-fabrication grounding gate (ADR-0007 §5) is preserved as the public guarantee.
- **Provider-neutral inference seam.** `verify_claim`'s panel runs through the existing `ModelClient` trait (`sp42-types`) and `GenaiModelClient` (`sp42-inference`), configured by the same `SP42_INFERENCE_*` env vars. BYO-key is the existing config path, not new auth code.
- **Native vs wasm crate split.** `sp42-mcp` is host-only (tokio/`rmcp`/axum), exactly like `sp42-server` and `sp42-cli`. `sp42-core` stays wasm-safe; the MCP crate sits outside the `sp42-app` wasm build.
- **Page fan-out + pacing.** `verify_wikipedia_page` reuses `PAGE_VERIFY_CONCURRENCY` and the Citoid pacing (`CITOID_PACE`) from `citation_routes.rs` rather than introducing new concurrency primitives.

New patterns introduced: the `rmcp` dependency and the Tasks/progress execution model (no prior MCP surface), and a **statement→claim renderer** for Wikidata (no Wikidata path exists today — the core verify primitive is general, but every current frontend is Parsoid/wikitext).

## Implementation Phases

### Phase 1: `sp42-mcp` crate + verb contract types
**Goal:** New native crate and the typed request/response contracts, reusing `sp42-types`.

**Components:**
- `crates/sp42-mcp/` — new workspace member (host-only; tokio, `rmcp`). Not in the `sp42-app` wasm build.
- Contract types — `ProbeResult`, the `source: { url } | { text }` input enum, `PanelConfig`, `StatementRef`, re-exporting `Verdict`/`PanelAgreement`/`BodyUsabilityReason` from `sp42-types`/`sp42-core` rather than redefining them.

**Dependencies:** None.

**Done when:** `cargo build -p sp42-mcp` succeeds; contract types serde round-trip; the `sp42-app` wasm build is unaffected.

### Phase 2: `probe_source` verb
**Goal:** Mechanistic accessibility as a deterministic, model-free verb that distinguishes unreachable from reachable-but-unextractable.

**Components:**
- `probe_source` handler in `sp42-mcp` — calls `source_fetch` through `guarded_source_client` then `classify_source_usability`; maps to `ProbeResult` (`reachable`, `http_status`, `extractable`, `unusable_reason`, `human_readable_hint`).

**Dependencies:** Phase 1.

**Done when:** Unit tests over reachable-clean, reachable-but-paywalled (extractable=false, human_readable_hint=true), and unreachable fixtures pass; zero model-client invocations asserted.

### Phase 3: `verify_claim` verb (URL or pre-fetched content)
**Goal:** The panel primitive, accepting either a URL or caller-supplied content, grounding gate intact.

**Components:**
- `verify_claim` handler in `sp42-mcp` — for `{ url }`, runs the existing fetch→extract→panel→ground path (`verify.rs`); for `{ text }`, injects the supplied content at the extract boundary and runs the same panel + `locate_quote` grounding, recording `retrieved_from` as provenance.
- A content-injection entry point on the verify path — the minimum seam to run `execute_citation_verify` over caller-supplied bytes without a fetch.

**Dependencies:** Phase 1.

**Done when:** Tests assert identical grounding/verdict behavior for `{ url }` and `{ text }` over the same bytes; a quote that fails to re-locate yields a non-Supported verdict (anti-fabrication gate holds); `Verdict` enum unchanged.

### Phase 4: `rmcp` stdio server
**Goal:** A runnable MCP server exposing the verbs over stdio, in-process over `sp42-core`.

**Components:**
- `rmcp` server scaffold in `sp42-mcp` — register `probe_source` and `verify_claim` as `#[tool]`s with generated schemas; stdio transport; a binary entry point.

**Dependencies:** Phases 2–3.

**Done when:** An integration test drives the server over stdio, lists tools (asserting the schemas), and round-trips one `probe_source` and one `verify_claim` call.

### Phase 5: Convenience verbs (`verify_wikipedia_page`, `verify_wikidata_statement`)
**Goal:** The two wiki-native orchestration verbs; the Wikidata renderer is net-new.

**Components:**
- `verify_wikipedia_page` handler — decompose via the existing Parsoid/`extract.rs` path, fan out `verify_claim` at `PAGE_VERIFY_CONCURRENCY`, aggregate per-use-site verdicts.
- `verify_wikidata_statement` handler — render a `StatementRef` into a claim sentence, resolve its P854 reference URL, call `verify_claim`; a statement with no resolvable reference URL returns `SourceUnavailable`.
- Statement→claim rendering module in `sp42-core` (or `sp42-wiki`) — net-new; turns (item, property, value) into a natural-language claim.
- Wikidata access: resolve the statement value + P854 reference URL via the **direct Wikidata REST API** for the MVP's narrow, deterministic lookups (simpler, no remote-MCP hop, in-process). The official read-only [Wikidata MCP](https://www.wikidata.org/wiki/Wikidata:MCP) (WMDE) is the richer option — entity/property disambiguation via its hybrid search, and the P31/P279 hierarchy the roadmap `is_blp` needs — but it is not an MVP dependency.

**Dependencies:** Phase 4.

**Done when:** A fixture page returns per-use-site verdicts; a statement with a reference-URL qualifier renders, verifies, and returns a `Verdict`; a reference-less statement returns `SourceUnavailable` (not an error); tests pass.

### Phase 6: Execution model — Tasks-optional + progress
**Goal:** Make expensive verbs task-capable without forking logic; keep long fan-outs legible.

**Components:**
- Task wrapping — declare `taskSupport: "optional"` on the three expensive verbs; the `rmcp` handler runs sync over stdio, wrappable as an async task for hosted.
- Progress — emit `notifications/tasks/status` from the convenience-verb fan-out.
- `estimate_only` (dry-run) mode on `verify_wikipedia_page` — decompose and return the use-site count + implied panel-call count without running the panel, so a caller can budget. (No result cache in the MVP — see Architecture.)

**Dependencies:** Phase 5.

**Done when:** A `verify_wikipedia_page` run emits at least one progress event; `estimate_only` returns a use-site/panel-call count with zero model calls; tests pass.

## Additional Considerations

**Governing PRD.** User-facing intent and acceptance criteria live in PRD-0010 (`docs/prd/0010-citation-verification-mcp-surface.md`); this design owns the *how*.

**Implementation scoping.** Phases 1–6 are the MVP implementation plan. The roadmap verbs (`find_source`, `assess_reliability`, `is_blp`) and the hosted streamable-HTTP transport are future work with their own plans, included here for contract continuity and direction only. `is_blp` (Wikidata BLP-applicability) is the trigger for the BLP/MEDRS regimes of `assess_reliability`, not verification itself. `find_source` is continuous with PRD-0009 (citation-insertion) — it is the generative inverse of `verify_claim` (search → fetch → verify) and the natural home for the {{citation needed}} → suggested-citation flow.

**Reference consumers & integration.** The first external consumer need not be built by SP42: fuzheado's [Wikipedia-AI-Skills](https://github.com/fuzheado/Wikipedia-AI-Skills) implement citation *hygiene* (`wikipedia-citations`: archiving, dead-link, Citoid bare-URL expansion) and citation *presence* (`wikipedia-reference-verifiability`: "does this `<ref>` contain a URL?"), but neither does semantic grounding — a `<ref>` pointing at an unrelated URL passes both. That is exactly `verify_claim`'s gap to fill. Concrete handoffs: `wikipedia-en-article-audit`'s Phase 2 (today "human reads each sentence and cited source, classifies verdicts") → `verify_wikipedia_page` (assist/triage, flag high-confidence verdicts and route disagreements to the human — *not* replace review, since SP42 errs conservative by design, cf. #25); `wikipedia-en-biography-writing` / `wikipedia-citations`, at the moment a `<ref>` is drafted → `verify_claim` before insertion (the "ground before insert" gate, PRD-0009-shaped); `wikidata` → `verify_wikidata_statement`. A second prospective consumer is alex-o-748's [citation-checker-script](https://github.com/alex-o-748/citation-checker-script), an editor-facing Wikipedia sidebar that already returns the same verdict taxonomy via a direct LLM call: ported onto this surface, its verdict step calls `verify_claim` and gains verbatim grounding, the model panel, and the hardened fetch pipeline it lacks.

**Ecosystem position (complementary, not competing).** The official read-only [Wikidata MCP](https://www.wikidata.org/wiki/Wikidata:MCP) (WMDE) reads statements, references, and the P31/P279 hierarchy but cannot fetch a source or judge support. SP42 is the verification capability it structurally lacks; an agent runs both (Wikidata MCP reads a statement + its reference URL, SP42 verifies the URL supports it). Its existence validates the MCP protocol choice and sets the hosting precedent (wmcloud) relevant to SP42's hosted phase. SP42's Wikidata-facing verbs consume the **direct Wikidata REST API** for the MVP's narrow lookups, treating Wikidata MCP as an option for richer disambiguation and the roadmap `is_blp` hierarchy queries — not an MVP dependency.

**Verdict-mapping is a boundary, not a gap.** Consumers (e.g. article-audit) use richer verdict vocabularies that include neutrality/original-research judgments (`npov_or`). SP42 judges *claim-source support*, not neutrality, and must not pretend otherwise. The surface documents a mapping (`Supported → confirmed`, `NotSupported → contradicted | unverifiable`, `Partial → mixed`) and deliberately emits **no NPOV verdict**; consumers obtain neutrality judgments elsewhere.

**Net-value stance (don't compound over-conservatism).** SP42 already errs conservative (#25). The MVP verbs are plumbing over the existing engine and add no new fuzzy gate, so they inherit the engine's tuning rather than stacking caution. The roadmap verbs that *do* introduce fuzzy judgment — `assess_reliability` (context-sensitive policy) and `find_source` (search ranking) — are to be judged on **measured net value**, not zero-false-positive corners, consistent with `2026-06-25-fetched-but-unusable-source-recovery`.

**Cost ownership.** MVP = caller's keys, caller pays, no metering. Per research (2026), hosted multi-tenant MCP cost-passthrough is largely unsolved; the cleanest hosted model is BYO-key passthrough, with free-tier caps + per-call metering as the fallback. A persistent, cross-user result cache (`cacheScope: public`) is the primary cost lever — but only at hosted scale, where popular articles yield cross-user hits; it is deferred with the hosted transport (the MVP has no cache). In the MVP the cost levers are `probe_source` screening and `estimate_only` budgeting.

**Out of scope.** No agent loop / planner / tool-calling framework (Rig, Flue, AutoAgents): SP42 is the tool the agent calls, not the orchestrator. No cheap source-reliability lookup verb (community-owned, enwiki-only, context-sensitive — folds into the roadmap `assess_reliability`). No `check_quote` verb (verbatim grounding is internal to `verify_claim`, not an agent concept). No change to the `Verdict` taxonomy or the ADR-0007/0008 contract.
