# ADR-0009: Crate boundary for citation verification

**Status:** Proposed
**Date:** 2026-06-07
**Author:** Luis Villa

## Context

PRD-0001 (citation verification — initial implementation, PR #17) spawns ADRs before
implementation. This one decides **where the verification logic lives**. ADR-0007
fixes the verdict and anti-fabrication semantics; ADR-0008 fixes the
request/response contract; ADR-0010 fixes source-snapshot storage; ADR-0006 owns
LLM use — the model panel, the pure vote, measured agreement, and the inference
endpoint. This ADR decides the
*crate boundary* — `sp42-core` versus a new crate — governed by the crate rules in
ADR-0004.

Verification introduces a kind of code SP42 has never carried: an LLM judgment
step. It is the **first LLM dependency in the project** (grep confirms no
`openai`/`anthropic`/`llm`/`ollama` dependency anywhere in the workspace today).
SP42 is **ML-integrated** — it consumes ORES-successor damage / revert-risk
probability scores from Wikimedia's LiftWing ML service (ADR-0001 §9, *LiftWing as
ML scoring provider*) — but it has used no LLM to date; this is its first. That
makes the placement question
load-bearing: the LLM must be confined behind a trait and behind the
anti-fabrication grounding gate (ADR-0007), and any concrete model-client
dependency must satisfy Constitution Art. 7 dependency discipline.

Verification ships as a **multi-model panel** from v1, not as a single-model first
cut, because no single open model is reliable enough to be the sole judge (the
single-model-insufficiency benchmark evidence and the panel/vote/agreement decision
are owned by ADR-0006). This ADR consumes that decision only to place its parts: the
panel's pure logic lands in `sp42-core` and its concrete model client lands in a
shell.

The shape this ADR chooses — a pure, I/O-free domain layer behind
dependency-injected edges, with the model(s) as one more injected edge that a
missing capability simply *skips* rather than fails on — is not speculative: it too
has been built and validated in production in Luis Villa's separate wikiharness
project (evidence the layering works, not justification for it). That layering maps
cleanly onto SP42's crate model and ADR-0004, which is why it is the right structure
here on SP42's own terms.

This ADR defines internal architecture and the LLM-confinement boundary. The
operator-facing intent and definition of done are owned by PRD-0001
(GOVERNANCE.md, `docs/prd/README.md`): the PRD owns user-facing intent, this
ADR owns the structural decision.

## Decision

**Land citation verification as modules inside `sp42-core`, behind module
boundaries, mirroring the existing external-edge precedent (`liftwing.rs`,
`wiki_storage.rs`). Do not pre-split a `sp42-verification` crate.**

### 1. Apply ADR-0004's Extraction Rules — the conditions are only partly met

ADR-0004 (`:165-175`) creates a new domain crate only when *most* of these hold;
its fallback (`:101-103`, `:174-175`): *"When a contract is still unclear, keep
the code in `sp42-core` behind module boundaries and stabilize the API there
first."* For an unproven, CLI-first verification contract:

- **Met:** an ADR/PRD records the intended contract (this set + PRD-0001,
  satisfying ADR-0004 `:166-167`); deterministic test doubles already exist to
  move with any future crate (`StubHttpClient`, ADR-0004 `:170`); `sp42-core` is
  leaf-most, so no dependency cycle is created.
- **Not met:** the public API is **not yet stable** (CLI-first, first-cut); there
  is a current caller (the CLI / on-demand standalone invocation) but **no
  credible second caller yet** (ADR-0004 `:167` wants both); a split would not yet
  "remove duplication or reduce review blast radius" (`:168-169`) — it would only
  "create a new place" for logic, which ADR-0004 says review must push back on
  (`:199-200`) and the Decision flags as the not-justified "only renames modules"
  case (`:30-31`).

So the conditions fail the ADR-0004 gate. Verification stays in `sp42-core` and
stabilizes its API there first. **ADR-0008 capturing the contract is itself the
ADR-0004 precondition (`:166-167`) that would later justify extraction** — once a
second real caller appears and the API has survived it, an `sp42-verification`
crate becomes the right move, following ADR-0004's preferred slice-based order.

### 2. Pure verdict / grounding / body-classifier / voting / fan-out logic lives in `sp42-core` modules

The deterministic, I/O-free heart of verification is pure platform logic and
belongs in `sp42-core` per Constitution Art. 2.3 (*"`sp42-core` has no dependency
on web-sys, js-sys, or any I/O crate"*) and Art. 2.1 (same input, same output):

- the categorical verdict type + tiebreak (ADR-0007),
- the verbatim-locatability check — the load-bearing anti-fabrication primitive
  (ADR-0007), pure and case-sensitive,
- the deterministic body-usability ("GIGO") gate that short-circuits to the
  abstain verdict *without calling a model* (ADR-0007),
- **the pure panel vote and the pure bounded-concurrency panel fan-out are
  `sp42-core` logic (ADR-0006).** The voting primitive (an `nClassVote`-style
  tally with a skeptical tiebreaker) and the `mapWithConcurrency`-style worker-pool
  combinator are deterministic and I/O-free, so by Art. 2.1 / 2.3 they live in
  `sp42-core` like the rest of the pure platform logic; their semantics (vote,
  tiebreaker precedence, measured agreement) are owned by ADR-0006. The fan-out is
  the one worker-pool the panel reuses (Decision 4).

These are `build_* / parse_*`-style pure functions (plus the two pure combinators
above), with a dedicated `thiserror` domain error enum in
`sp42-core/src/errors.rs` per Art. 6.3 (e.g. `VerificationError`, struct-style
variants with named fields, never `anyhow::Error` in a public interface),
mirroring `LiftWingError` / `WikiStorageError`.

### 3. Source fetching is an injected edge (already exists)

Reading a cited source is I/O and goes through the existing `HttpClient` trait
(`sp42-types/src/traits.rs:19`), exactly as `liftwing.rs` and `wiki_storage.rs`
do — a pure `build_*_request`, a generic `execute_*<C: HttpClient + ?Sized>`,
and a pure `parse_*_response` with a `validate_*` gate. No new fetch edge is
needed.

### 4. The panel rides the existing `HttpClient` edge against a config-driven model panel

The model judgment step is the one genuinely new behavior, but it needs **no new
trait** for the v1 panel. It follows SP42's established external-ML precedent —
LiftWing in `sp42-core/src/liftwing.rs` — exactly, and Constitution Art. 1.3 /
6.2 (all external dependencies behind traits in `sp42-types/src/traits.rs`,
re-exported by `sp42-core`'s facade; *core
never names a concrete implementation* — the network is already behind
`HttpClient`):

- a pure `build_verify_request` (claim + fetched source text → `HttpRequest` for
  the model endpoint, with the panel-member model name varied per request) and a
  pure `parse_verdict_response` (`&[u8]` → `Verdict`, tolerant, with ADR-0007's
  conservative-default-to-not-supported rule), driven in tests by the
  deterministic `StubHttpClient` (`sp42-types/src/traits.rs:51`), network-free per Art. 1.3;
- a generic per-model `execute_*<C: HttpClient + ?Sized>` over the existing
  `HttpClient` trait (`sp42-types/src/traits.rs:19`), exactly as `liftwing.rs` and
  `wiki_storage.rs` reach their services — each panel model is just one more HTTP
  request behind the trait SP42 already abstracts external I/O through, so the
  "first LLM" adds no new edge type;
- **a panel-aware `execute_citation_verify_panel`** that runs the per-model
  `execute_*` across the panel via the Decision-2 bounded-concurrency worker-pool,
  collects N verdicts, applies the pure vote, and produces **one** `CitationFinding`
  — the voted verdict plus the winning verdict's located quote (re-grounded by the
  ADR-0007 gate) together with the measured `PanelAgreement`. The panel's
  configuration, execution semantics, and agreement signal are owned by ADR-0006;
  this ADR's load-bearing crate-boundary fact is only that the **homogeneous panel**
  (the validated shape: open models behind ONE OpenAI-compatible endpoint, e.g. an
  OpenRouter-style gateway) is N requests over the **one** `HttpClient` transport,
  so the bare `HttpClient` edge (the ADR-0008 decision) is sufficient and is kept
  for v1. (A **heterogeneous** panel — mixed provider request/response formats over
  different transports — is the concrete trigger to adopt the deferred `ModelClient`
  trait; see ADR-0006 and Alternative (f).)
- the panel configured as a **config-driven model panel**, generalizing the
  single optional model URL precedent (`WikiConfig.liftwing_url: Option<Url>`,
  `types.rs:401`); the verdict set is ADR-0007's two-axis `CitationVerdict` and the
  panel-config/single-vs-multi-model shape is ADR-0006's. Default-absent, so a wiki
  without a configured panel simply has the capability unmet.
- **capability-gating:** an unconfigured panel means model-dependent verification
  is *skipped*, not errored. The deterministic body gate (Decision 2) already
  removes the mechanically-determinable abstain cases from the model's hands, so a
  missing panel degrades gracefully to "no LLM verdict offered" rather than a
  failure.

The confinement boundary is not a bespoke trait but the conjunction of three
things: the model endpoint is the *only* external call this step adds (the
homogeneous panel is N requests over the one transport); the pure `build_*` /
`parse_*` plus the pure vote and fan-out keep `sp42-core` free of concrete
network; and — load-bearing — each model's output is **never trusted directly**,
the voted result is re-checked by the ADR-0007 grounding gate before any verdict
surfaces. (A dedicated `ModelClient` trait is the deferred option that a
*heterogeneous* panel would trigger, not first-cut scope; see ADR-0006 and
Alternative (f).) The measured-agreement signal surfaced alongside the verdict —
its meaning, its `panel_size >= 2` validity, and its no-numeric-confidence
carve-out — is owned by ADR-0006.

### 5. The concrete model client stays in a shell, never in `sp42-core`

Constitution Art. 7 (every dependency is a liability; Art. 7.2 PR documentation
duty) plus ADR-0004's dependency-direction law (`:59-60`, *"domain crates must
not depend on `sp42-server`, `sp42-app`, `sp42-cli`, `sp42-desktop`"*) mean the
**concrete** model client — the thing that holds an HTTP client and credentials
and pulls in a vendor SDK — lives in a shell, exactly as `BearerHttpClient`
(`sp42-server/src/runtime_adapters.rs:51`) and `BrowserHttpClient`
(`sp42-app/src/platform/runtime.rs:18`) implement `HttpClient` today. `sp42-core`
names no concrete model client; it only builds an `HttpRequest` for each panel
member and parses the response, reaching the network through the `HttpClient`
trait. The v1 panel should adopt **no vendor LLM
SDK at all**: implement the model edge over the existing `HttpClient` against an
open-model HTTP endpoint serving the whole homogeneous panel (open-capable,
open-default),
so the only new transitive surface is the chosen endpoint's HTTP API, documented
per Art. 7.2 in the implementing PR. License compatibility (Art. 5.2, GPL-3.0-only
via `cargo-deny`) is checked for any client/transport dep that does land.

This shell-side concrete client introduces the **first model-API credential** in
SP42 — a bearer token / API key — **but only in the *Direct* endpoint mode**
(server / CLI / desktop). In *Local* mode no provider key exists, and in *Sponsor
proxy* mode the provider keys live in the proxy while SP42 holds at most a proxy
token; the **browser shell cannot hold a provider key at all** (Art. 10.1) and so
uses only those two modes. The three endpoint modes and where keys live are owned
by **ADR-0006**. Where a provider key *is* held (Direct mode), its hygiene boundary
is owned **here**, where the credential is born, per Constitution Art. 10:

- **Art. 10.1 — in memory only, never persisted.** The credential is held in the
  shell adapter only (loaded from env/config at startup), mirroring the
  `BearerHttpClient` `access_token` precedent (`sp42-server/src/runtime_adapters.rs:51`); it
  is never written to disk, logged, or carried into `sp42-core`, which names no
  concrete model client (only the pure `build_*` / `parse_*` over `HttpClient`).
- **Art. 10.4 — no telemetry, data stays local.** Each model call sends **only** the
  fetched source text + the claim under verification — nothing else. No editor
  identity, no OAuth/session token, no audit or revision metadata crosses to the
  model endpoint (consistent with ADR-0007's identity-blind verdict). The model
  endpoint is the one external call this ADR adds, and this is the limit of what it
  may carry.

The `tracing` observability (Decision 6 / Consequences) records the fetched source,
located passage, voted verdict, and the measured agreement — never the credential.

### 6. Dependency direction is one-way; verification owns no write path

Per ADR-0004 (`:51-60`), verification logic depends only on traits and data
contracts and never on a shell crate. Verification is **read-only** (PRD-0001):
it produces a verdict for display, never a wiki write. Any resulting repair flows
through the *existing* operator-confirmed action path unchanged — the single
`SessionActionKind` → `POST /dev/actions/execute` → `execute_session_action` →
`execute_wiki_page_save` lane, behind session + CSRF + capability + audit gates,
hardened by ADR-0003. The verification module **does not** own or duplicate that
path; a verification-driven edit is just another operator-confirmed
`SessionActionExecutionRequest { kind: InlineEdit, … }`. The read-only voted
verdict and its agreement surface in the display lane (a new field on
`LiveOperatorView`, `sp42-reporting/src/live_operator_view.rs:19`), which has no
write side effect by construction. Contract details belong to ADR-0008.

## Alternatives Considered

- **(a) Extract `sp42-verification` now.** *Pros:* clean ownership, tests move
  with it. *Cons:* fails ADR-0004's Extraction Rules — unstable API, no second
  caller, no duplication removed — and freezes a first-cut contract prematurely
  (ADR-0004 `:206-207`, "freezes unstable public APIs"). **Rejected** as
  premature; revisit once ADR-0008's contract has survived a second caller.

- **(b) Put the LLM/model concept in the planned `sp42-types` crate.**
  ADR-0004 (`:124-126`, `:128-147`) explicitly says to **defer** broad
  `sp42-types` extraction, starting only with transport/storage slices that have
  multiple real consumers. A model abstraction with one caller does not qualify.
  **Rejected** for now.

- **(c) Embed a concrete LLM client / vendor SDK in `sp42-core`.** Would make
  `sp42-core` name a concrete implementation (violating Art. 6.2) and pull a
  vendor dependency into the leaf domain crate (against Art. 7 and ADR-0004
  dependency direction). **Rejected:** the concrete client lives in a shell
  (Decision 5).

- **(d) A numeric confidence score instead of a categorical verdict.**
  A model-emitted probability is
  false precision — generated text, not a calibrated measurement. **Rejected**
  upstream by ADR-0007 and PRD-0001; recorded here because it shapes the model
  edge: `parse_verdict_response` parses no number, and no `confidence: f32` field
  exists on any verification type, so the no-number rule cannot regress by
  accident. (Measured panel agreement — owned by ADR-0006 — is the carve-out: an
  observed vote count SP42 computes, not a model-emitted number, and so is allowed.)

- **(e) Trust the model's self-reported "I found this quote" / fold source
  metadata into the grounded bytes.** Trusting
  the producer re-opens the hallucination hole; concatenating bibliographic
  metadata into the bytes the quote is located against lets a "quote" drawn from
  a title/author field pass the gate — this exact failure was observed and fixed
  in Luis Villa's wikiharness, where metadata once concatenated into the hashed
  bytes let a quote "ground" in a title/author line. **Rejected:** grounding is
  re-verified independently against only the fetched body bytes (ADR-0007);
  metadata, if used, is a display-only sidecar that is never located against. How
  voting composes with this gate without weakening it is owned by ADR-0006.

- **(f) A dedicated `ModelClient` trait/edge for the model call.** *Pros:* a single
  named LLM boundary and a default-closed
  `NoModelClient`; the clean place to host a panel of mixed-provider clients.
  *Cons:* SP42's own external-ML precedent (`liftwing.rs`) reaches
  its service via the `HttpClient` trait + a config URL, not a bespoke client
  trait, and ADR-0004 warns against abstraction a single edge does not yet need.
  **Deferred:** the v1 panel is **homogeneous** — N requests over the one
  `HttpClient` transport against one OpenAI-compatible endpoint, model name varied
  per request — so it rides the `HttpClient` edge (Decision 4) with no new trait.
  The concrete trigger to adopt this trait is a **heterogeneous panel** (mixed
  provider request/response formats over different transports); that trigger is
  owned by ADR-0006, and matches the same "outgrows one request/response" threshold
  ADR-0008 (f) names. Until that panel is actually wanted, the trait is not built.

## Consequences

- **First LLM in SP42, fully confined, and it ships as a panel.** The models are
  reachable only through the existing `HttpClient` edge against a config-driven
  model panel (Decision 4); the **voted** result never surfaces without passing the
  ADR-0007 grounding gate; the concrete client lives in a shell (Decision 5); the
  capability is default-absent and gracefully skipped when unconfigured. This is
  the notable architectural consequence of this ADR — recorded so a future
  reviewer sees the boundary was deliberate.

- **`sp42-core` grows modules, not a crate.** Verification adds pure modules to
  `sp42-core` (no new trait — the homogeneous panel rides the existing `HttpClient`
  edge; the vote and the bounded fan-out are pure `sp42-core` combinators whose
  semantics ADR-0006 owns),
  keeping it the ADR-0004 compatibility facade. The
  cost is that `sp42-core` carries the new domain until extraction is justified;
  ADR-0008 having captured the contract makes that future extraction a clean,
  slice-based move.

- **Testable invariants this boundary must uphold** (binding PRD-0001's
  Definition of Done; Constitution Art. 1 — no untestable code merges):
  - the verdict type's pure module + a contract/surface test prove a verdict is
    exactly one categorical value with **no model-emitted numeric confidence
    anywhere** (the only number on the surface is the measured `PanelAgreement`
    vote count, which is observed, not model-reported) (DoD #1; Art. 6.1/9.1);
  - a `proptest` over the pure locatability primitive proves a claim with no
    matching source text **never yields supported** — even as a voted result
    (the vote/tiebreaker invariants themselves are owned and tested by ADR-0006)
    (DoD #2, the load-bearing anti-fabrication invariant; Art. 1.2);
  - the pure vote and pure bounded fan-out are tested per ADR-0006 (they live in
    `sp42-core` per Decision 2);
  - an integration test driving the `HttpClient`-injected fetch edge with a
    `StubHttpClient` returning an unreachable/unusable source proves the verdict
    is abstain, never a support judgment (DoD #3; Art. 1.3);
  - an integration test on the verification path proves **zero autonomous wiki
    writes** — verification owns no writer, so the only write lane is the
    unchanged operator-confirmed action path (DoD #4; Decision 6);
  - a recorded-source replay test (a `StubHttpClient` replaying the recorded
    source response **and the N recorded model responses, one per panel member**)
    proves re-running on the same snapshot yields the **same voted verdict category
    and the same measured agreement** (DoD #5; Art. 2.1 — the model endpoints are
    the only nondeterministic actors, made deterministic by replaying their
    recorded responses; the determinism-over-the-panel invariant is detailed in
    ADR-0006);
  - each verification emits a `tracing` observable showing the fetched source,
    the located passage (or its absence), the voted verdict, and the measured
    agreement (DoD #6; Art. 3.2).

- **Dependency discipline (Art. 7).** The v1 panel adds no vendor LLM SDK: the
  model edge rides the existing `HttpClient` against one open-model HTTP endpoint
  serving the homogeneous panel; any new client/transport dep is documented per
  Art. 7.2 and license-checked per Art. 5.2 in the implementing PR, with
  `Cargo.lock` committed (Art. 7.3).

- **Model-credential hygiene owned here (Art. 10).** Because the shell-side model
  client (Decision 5) is where SP42's first model-API credential appears, this ADR
  owns its handling: in memory only, never persisted (Art. 10.1, the
  `BearerHttpClient` precedent), and each model call carries only source text + claim
  — no telemetry, identity, or session token (Art. 10.4). This is a distinct concern
  from snapshot storage below.

- **No new persisted format here.** Whether and how fetched snapshots and verdict
  records are stored (and Art. 10 token/telemetry hygiene *for those persisted
  artifacts*) is ADR-0010's decision, not this one. If verification persists
  anything, it uses the versioned-serde envelope convention (Art. 9.1/9.2) per that
  ADR. The model-client credential above is owned here, not deferred to ADR-0010.

## Non-Goals

- No `sp42-verification` / `sp42-citation` crate in the first cut (Decision 1).
- No `sp42-types` extraction triggered by this work (Alternative b).
- No vendor LLM SDK adopted in `sp42-core` (Decision 5).
- No dedicated `ModelClient` trait in v1 — the **homogeneous** panel rides the
  existing `HttpClient` edge (Decision 4); a bespoke trait is the deferred option a
  **heterogeneous** panel would trigger (ADR-0006 / Alternative f).
- No change to the operator-confirmed action path, ADR-0003's `WikitextEditor`
  seam, or the write gates (Decision 6).
- No verdict (or panel agreement) feeding the composite damage score in the first
  cut — the first cut is standalone (PRD-0001) — kept off the scoring-policy ADR surface
  deliberately; the agreement signal is for human review, not automated routing yet.
- The panel/vote/agreement semantics are not owned here — they are ADR-0006's; this
  ADR places their pure logic in `sp42-core` and their concrete client in a shell.
