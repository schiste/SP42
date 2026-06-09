# ADR-0008: Citation verification request/response contract

**Status:** Proposed
**Date:** 2026-06-07
**Author:** Luis Villa

## Context

PRD-0001 (citation verification — initial implementation, open as PR #17 on the
`prd-0001-citation-verification` branch) adds an operator-facing capability: for
a claim and its cited source, SP42 fetches the source read-only and reports a
categorical verdict, with the supporting passage shown inline. PRD-0001 names
*public contracts or APIs* as a dual-natured trigger and spawns five ADRs —
**ADR-0006** (using LLMs: the model panel, measured agreement, and the inference
endpoint), **ADR-0007** (verdict + anti-fabrication semantics), **ADR-0008 (this
ADR)** (the request/response surface), **ADR-0009** (the crate boundary), and
**ADR-0010** (source-snapshot storage).

This ADR settles **only** the contract surface: the typed request a single
verification takes, and the typed result it returns. GOVERNANCE lists *public
APIs* among the changes that require an ADR; Constitution Art. 9.1 requires every
external interface to have a versioned schema. The verdict values, the
anti-fabrication rule, where the bytes live, the crate the code sits in, and how a
verdict is voted from a model panel are owned by the sibling ADRs; this ADR
references them and does not re-decide them.

The hard problem the contract must encode is structural: a verification result is
**informational, not an action**. SP42 has exactly one way to mutate a wiki — the
operator-confirmed action path: an `install_action_effect` click
(`sp42-app/src/pages/patrol/action_controller.rs`) populating a
`SessionActionExecutionRequest` (`sp42-core/src/action_contracts.rs:87`), posted
to `POST /dev/actions/execute` (`DEV_ACTION_EXECUTE_PATH`,
`sp42-core/src/routes.rs:78`), routed through `post_execute_action`
(`sp42-server/src/action_routes.rs:40`) behind session, CSRF, capability, and
audit gates before reaching the one writer `execute_wiki_page_save`
(`sp42-core/src/action_executor.rs:318`) — whose two `replacen` content-edit
sites ADR-0003 hardens into node-anchored edits. A verification result must be
structurally incapable of becoming a write, and any repair it motivates must flow
through that unchanged path. The discipline this contract encodes — *suggest
source-verified improvements, never edit autonomously, never hallucinate* —
follows directly from SP42's own single-write-lane architecture (Art. 2.3) and
its evidence-over-generation stance; the same discipline has been built and
validated in production in Luis Villa's separate wikiharness project, which is
cited below where its operational experience offers concrete evidence for a
contract choice.

The verdict this contract carries is **panel-voted, not single-model** — no
single open model is accurate enough to be trusted alone, so the surfaced verdict
is the result of a model panel combined by a pure vote, with the panel's
**measured agreement** carried as a first-class observed signal. The reason a
single model is insufficient, and the panel/vote/agreement decision itself, are
owned by **ADR-0006**; this contract is the request/response *shape* that carries
the voted verdict and the agreement counts.

## Decision

Introduce a **read-only, Finding-style verification contract** — a typed,
versioned request/response pair that is structurally distinct from SP42's
write-bearing `SessionActionExecutionRequest`. It is the only surface a
verification result is exposed through. The surfaced verdict is the **voted**
result of a model panel (ADR-0006), and the response carries the **measured
agreement** among the panel's independent votes.

### 1. The request — claim, source, revision context

```rust
pub struct CitationVerificationRequest {
    pub wiki_id: String,
    pub rev_id: u64,
    pub title: String,
    pub claim: String,
    pub source_url: Url,
}
```

The request carries the claim text, the cited source URL, and the revision
context (`wiki_id` / `rev_id` / `title`) keyed exactly as the review surface
already keys diff loading (`selected_edit.event.{wiki_id, rev_id, title}` in
`sp42-app/src/pages/patrol/revision_artifacts.rs`), so it can later attach to the
review flow without reshaping. It is **on-demand against a specified target** — a
particular citation, or an article for which the operator requests a whole-article
report (PRD-0001 builds this standalone first and wires it into revision review only
after testing — PRD-0001 open question 3), not necessarily a separate queue. The request
carries **no token** (tokens are a write-path concern; cf.
`SessionActionExecutionRequest`, which is itself tokenless —
`action_contracts.rs:153`) and no editor-identity field of any kind (ADR-0007's
identity-blind rule; nothing on this struct can carry account age / edit count /
anon / group / IP into a verdict). The request is panel-agnostic: which models
verify it is a config concern (ADR-0006), not a request field.

### 2. The response — a Finding, never an action

```rust
pub struct CitationFinding {
    pub kind: CitationFindingKind,      // single value: citation_verdict
    pub verdict: CitationVerdict,       // the VOTED categorical verdict (ADR-0007)
    pub agreement: PanelAgreement,      // measured agreement among the panel's votes
    pub passage: Option<LocatedPassage>,// the winning verdict's located passage, or None
    pub provenance: SourceProvenance,   // the really-fetched source
    pub grounding: GroundingAssertion,  // the machine-checkable assertion
    pub use_site_ordinal: u32,          // document-order position of this use-site (ADR-0007)
    pub schema_version: u32,            // versioned per Art. 9.1
}
```

The response is a **`CitationFinding`** carrying:

- **The voted categorical verdict** owned by ADR-0007 — exactly one value from
  the fixed set, with **no numeric confidence** field (no `f32`/percentage/probability)
  anywhere on the contract or its sub-types (see Alternative (a)). The value
  carried is the panel's **voted** result; how that vote is computed (the pure
  tally and the skeptical tiebreaker) is owned by **ADR-0006**.
- **The measured agreement among the panel's votes.** `agreement:
  PanelAgreement` where `PanelAgreement { panel_size: u8, winner_votes: u8 }` —
  the **measured** vote counts (the fraction `winner_votes / panel_size` is
  derived, never stored). This field is part of the contract surface; its
  **semantics** — that it is an observed count and not a model-reported number,
  that it is the honest substitute for confidence, that it is meaningful only for
  `panel_size >= 2`, and that low agreement is the "borderline — needs human
  review" signal — are owned by **ADR-0006**. It is a signal *alongside* the
  verdict, **not** a new verdict tier (the verdict set stays the two-axis
  `CitationVerdict` from ADR-0007).
- **The winning verdict's located passage, or an explicit record of its
  absence.** `passage: Option<LocatedPassage>` where `LocatedPassage { quote:
  String, offset: usize }` is the verbatim substring located in the fetched
  source, with its byte offset — the quote from the **winning** verdict, not a
  merge across models. `None` is the explicit *absence* record — the
  contract-level manifestation of abstention and of "I fetched it and it does not
  support this".
- **Provenance of the really-fetched source.** `SourceProvenance { url: Url,
  content_hash: String, fetched_at: i64 }` — the source actually fetched this
  session, content-addressed (the hash is owned by ADR-0010's snapshot store);
  `fetched_at` comes from the injected `Clock` (`Clock::now_ms`,
  `sp42-types/src/traits.rs:37`), never wall-clock, per Constitution Art. 2. The
  source is fetched **once** per verification and shared across the panel — every
  panel model verifies against the same content-addressed bytes.
- **A machine-checkable grounding assertion.** `grounding: GroundingAssertion` is
  the load-bearing field — a discriminated enum the gate re-verifies
  independently rather than trusting the producer:

  ```rust
  pub enum GroundingAssertion {
      LocatedQuote { quote: String, source_hash: String, offset: usize },
      SourceFetched { source_hash: String },
  }
  ```

  `LocatedQuote` grounds a `Judged(Supported)` / `Judged(Partial)` verdict on a
  passage string-located in the fetched bytes; `SourceFetched` grounds a *no-quote*
  verdict (`Judged(NotSupported)`, or `SourceUnavailable` when a body was fetched
  but unusable) on "the source was actually fetched this session", so a fabricated
  "I read it and it doesn't support this" is still caught when the cited source was
  never fetched. **Anti-fabrication composes with voting**: a *voted*
  `Judged(Supported)` / `Judged(Partial)` still carries one verbatim located
  quote that the gate re-checks against the fetched bytes — voting changes *which*
  verdict wins and *adds* the agreement signal but never weakens the gate
  (composition owned by ADR-0006). (The enum is left open to additional variants —
  e.g. an in-article-span grounding for future codified-rule findings; only these
  two are in scope for the first cut.)

- **The use-site's document-order ordinal.** `use_site_ordinal: u32` — the
  position of this (claim, source) use-site among the article's citation markers
  in document order; the use-site unit and its ordering are owned by **ADR-0007**
  (Decision 2). It is a **positional index, not a measurement** (like `rev_id`):
  never a verdict input and never an editor-identity signal — a position in the
  text, not a property of a person — so it does not weaken the
  no-numeric-confidence rule. It is the stable handle the *Surface*'s `--ref`
  drill-down and the whole-article report rows refer to (PRD-0001), and — being the
  document-order position of the use-site's `<ref>` node — the article-side anchor a
  future node-anchored repair would resolve an edit on (Decision 5; Consequences).
  Verification already computes it to locate the claim; the contract **carries it
  rather than discarding it**, since it is not re-derivable from the rest of the
  finding.

`CitationFindingKind` is a single-value enum today (`citation_verdict`); it is an
enum, not a bare marker, so the read-only Finding channel can carry future
informational kinds without a breaking change (Art. 9.2).

### 3. Trait-based, per Constitution Art. 6.2; the per-model LLM edge behind an `HttpClient`

Verification is exposed as a pure `build_* / parse_*` split plus an injected
`execute_*`, mirroring the existing external-service edges
`sp42-core/src/liftwing.rs` and `sp42-core/src/wiki_storage.rs`. This contract
owns the **per-model unit** — one model, one verdict — which is the unit the
panel calls:

```rust
pub fn build_citation_verify_request(
    config: &WikiConfig,
    model: &str,
    req: &CitationVerificationRequest,
) -> Result<HttpRequest, CitationVerificationError>;

// The per-model unit: one model, one verdict.
pub async fn execute_citation_verify<C>(
    client: &C,
    config: &WikiConfig,
    model: &str,
    req: &CitationVerificationRequest,
) -> Result<ParsedVerdict, CitationVerificationError>
where
    C: HttpClient + ?Sized;

pub fn parse_citation_verify_response(
    body: &[u8],
) -> Result<ParsedVerdict, CitationVerificationError>;
```

**Panel execution + voting is owned by ADR-0006** (`execute_citation_verify_panel`,
its bounded-concurrency fan-out, the pure vote applied to the N results, and the
panel config shape); this contract's per-model edge `execute_citation_verify` is
the unit it calls. The model endpoint reaches the network **only** through the
`HttpClient` trait (`sp42-types/src/traits.rs:19`); the model endpoint is an
optional, config-driven, default-absent per-wiki field, exactly like
`liftwing_url: Option<Url>` (`sp42-core/src/types.rs:401`) — the panel
generalization of that config is owned by ADR-0006, and *where* the endpoint runs
(local model, direct provider, or sponsor proxy) plus who holds the keys are owned
by ADR-0006; this contract is identical in every mode. Core never names a concrete
model client; the production adapter lives in a shell (`BearerHttpClient`,
`sp42-server/src/runtime_adapters.rs:51`), the deterministic double `StubHttpClient`
(`sp42-types/src/traits.rs:51`) drives tests — for a panel, a queue of N recorded
`HttpResponse`. The pure parser ends in a `validate_*` gate that defaults an
unrecoverable model response to *not supported*, never to a support judgment.

Per Constitution Art. 1.3 (no network in unit tests), **every unit and property
test for the contract runs network-free**, driven by `StubHttpClient` (a queue of
recorded `HttpResponse`); the only network-touching tier is the integration tier
(Art. 1.2, `--features integration` against a mock server). ADR-0007's
anti-fabrication property test — a claim with no matching source text never
yields *supported* — is by construction a no-network test, and holds for the
voted verdict: the tiebreaker (ADR-0006) cannot manufacture a `Supported` the gate
would reject.

### 4. A domain error enum, per Constitution Art. 6.3

A dedicated `thiserror` enum, no `anyhow` in the public interface, struct-style
variants carrying context — matching `LiftWingError` / `WikiStorageError`
(`sp42-core/src/errors.rs`):

```rust
#[derive(Debug, Error)]
pub enum CitationVerificationError {
    #[error("invalid verification request: {message}")]
    InvalidRequest { message: String },
    #[error("source unavailable: {reason}")]
    SourceUnavailable { reason: &'static str },
    #[error("invalid model response: {message}")]
    InvalidResponse { message: String },
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
```

`SourceUnavailable` carries a `&'static str` reason from a fixed deterministic set
(unreachable, anti-bot challenge, archive chrome, short body, …) — the GIGO body
gate that lets the caller short-circuit to a `SourceUnavailable` verdict **without
a model call at all**, removing one nondeterminism source for the mechanically
determinable failures. Because the source is fetched once before the panel runs,
this short-circuit applies to the whole panel: an unusable body yields
`SourceUnavailable` with no model calls and no vote.

### 5. The result is informational; any edit flows through the unchanged action path

A `CitationFinding` is surfaced for display — its natural home is a new optional,
read-only field on `LiveOperatorView`
(`sp42-reporting/src/live_operator_view.rs:19`), peer to `diff` / `media_diff` /
`action_preflight`, all of which are produced by read-only fetches and never enter
the action path. There is **no POST counterpart** to the read surface and **no
decide affordance** on it. If the verification motivates a repair, the operator
confirms it the normal way: it becomes a `SessionActionExecutionRequest { kind:
InlineEdit, selected_text, replacement_text }` on the **unchanged** write lane
(ADR-0003), where session + CSRF + capability + audit gates apply. That repair,
when built, resolves its `selected_text` by anchoring the finding's retained
`use_site_ordinal` to its `<ref>` node (the node-anchored target, ADR-0003) and
supplies its `replacement_text` from a separate repair step — the verdict
`CitationFinding` provides the *anchor* but never authors the edit. Verification
itself owns no write path and cannot reach `execute_wiki_page_save`.

### 6. Versioned serde schema, per Constitution Art. 9.1

All contract types derive `Serialize, Deserialize` and carry an explicit
`schema_version: u32` (matching the `WikiStoragePayloadEnvelope.version: u32`
precedent, `wiki_storage.rs:113`); added fields use `#[serde(default)]` for
one-cycle backward compatibility (Art. 9.2). The `GroundingAssertion` enum uses
`#[serde(rename_all = "snake_case")]`; the verdict flattens to a single snake_case
string (`supported` / `partial` / `not_supported` / `source_unavailable`) via a
custom (de)serialize — the one canonical wire form, owned by ADR-0007.
`PanelAgreement` serializes as the two **measured counts** `panel_size` and
`winner_votes` — never as a confidence float; the derived fraction is computed at
the display layer, not stored on the wire, so the surface carries no
model-emitted or precomputed numeric-confidence value (count semantics owned by
ADR-0006).

## Alternatives Considered

- **(a) A numeric confidence / citation score instead of a category.**
  **Rejected** (decided in ADR-0007; honored here by having no numeric field). A
  model-emitted probability is *false precision* — generated text, not a calibrated
  measurement — that invites the operator to trust precision the system lacks while
  obscuring the one thing that matters: whether the claim is locatable in the
  source. The no-number rule is enforced *structurally* (no field can carry one) so
  it cannot regress by accident. The `PanelAgreement` counts are the carve-out —
  **observed** measurements, not model self-reports (ADR-0006 owns that carve-out);
  they are the only number on the surface, and a count, not a float.

- **(b) Trust the producing tool's "quote located" self-report.** **Rejected** —
  the producer is the untrusted LLM, and trusting its self-report re-opens the
  fabrication hole. The contract instead carries a machine-checkable
  `GroundingAssertion` re-verified by an independent gate against the
  content-addressed fetched bytes (ADR-0007 / ADR-0010): grounded by
  re-verification, not by trust. This holds per-model and for the voted result —
  the winning verdict's quote is re-checked, not taken on the panel's word.

- **(c) Fold source metadata (title / author / publication) into the grounded
  bytes.** **Rejected** — concatenating bibliographic metadata into the hashed,
  grounded bytes lets a quote "ground" in the title / author / publication line
  instead of the source body, defeating the anti-fabrication gate. This exact
  failure was observed and fixed in Luis Villa's wikiharness, where metadata was
  briefly prepended into the grounded bytes and then deleted once the hole was
  found. Metadata, if carried at all, is a sidecar that is **never**
  content-hashed and never passed to the locator; this contract carries only the
  body's `content_hash`.

- **(d) Let verification write a repair directly (a "fix it" verb).**
  **Rejected** (PRD-0001 also rejects this) — it would put unreviewed writes on the
  wiki, violating the operator-confirmed action model (Art. 2.3 side-effects-at-the-
  edges; the single write lane). The contract is deliberately a Finding with no
  action verb.

- **(e) Reuse `SessionActionExecutionRequest` as the result type, with a
  read-only verb.** **Rejected** — every existing `SessionActionKind`
  (`Rollback | Patrol | Undo | TagCitationNeeded | InlineEdit`) is a mutation
  gated by `validate_action_request`. Adding a read-only verb to the write enum
  would invite a verification result onto the write lane. The two surfaces stay
  structurally separate: a Finding for display, an action request for writes.

- **(f) A dedicated `ModelClient` trait/edge for the model call.** **Deferred.**
  The per-model edge here stays on the existing `HttpClient` trait + a config
  endpoint (the `liftwing.rs` shape, no new abstraction, ADR-0004). The concrete
  trigger to adopt a `ModelClient` trait — a **heterogeneous panel** that one
  transport cannot serve uniformly — is owned by **ADR-0006**; until then the
  `HttpClient` edge suffices, and when it is needed the trait's shape is owned by
  ADR-0009 §4.

## Consequences

The contract binds these testable invariants, each tied to a PRD-0001
Definition-of-Done item:

- **No numeric confidence on the surface (DoD 1).** The contract types have no
  `f32`/percentage/probability field; `PanelAgreement` carries only the measured
  `u8` counts. *Unit test* on the verdict type plus a *contract/serialization
  test* asserting the serialized `CitationFinding` carries no numeric-confidence
  key and that `agreement` serializes as `{ panel_size, winner_votes }` (counts,
  not a float). Verdict values owned by ADR-0007; agreement semantics by ADR-0006.

- **Anti-fabrication — load-bearing (DoD 2).** The locator and the gate are owned
  by ADR-0007; this contract is the *shape* that makes them enforceable: a
  *voted* `Judged(Supported)` / `Judged(Partial)` verdict is **unrepresentable**
  without a `LocatedPassage` plus a `LocatedQuote` grounding (the gate
  string-locates the quote verbatim in the fetched bytes — see ADR-0007). The
  property that voting and the skeptical tiebreaker never produce a gate-failing
  `Supported` is owned by ADR-0006; this contract makes the gate-failing state
  unrepresentable on the wire.

- **Measured agreement is observed, never reported (DoD 1, DoD 6).** The
  `PanelAgreement { panel_size, winner_votes }` on a finding equals an
  independently-computed tally of the panel's recorded votes. *Contract/serialization
  test* that no value on the finding is a model-reported number and that
  `winner_votes <= panel_size`; the pure-vote tally test is owned by ADR-0006.

- **Abstain, never guess (DoD 3).** When there is no usable source body, the
  edge short-circuits via `CitationVerificationError::SourceUnavailable` to a
  `SourceUnavailable` verdict with `passage: None` and a `SourceFetched`
  grounding — before any panel model is called, never a support judgment.
  *Integration test* against an unreachable / unusable source (driven by
  `StubHttpClient` returning a non-2xx or a gate-failing body), asserting zero
  model calls were made.

- **No autonomous writes (DoD 4).** The read surface has no POST counterpart and
  no path to `execute_wiki_page_save`; the result type carries no action verb.
  *Integration test* asserting the verification path performs zero wiki writes; any
  repair appears only as an operator-confirmed `SessionActionExecutionRequest` on
  the unchanged action lane (ADR-0003).

- **Deterministic replay (DoD 5).** `build_* / parse_*` are pure (Art. 2.1);
  given the same fetched-source snapshot (ADR-0010) and the same recorded model
  response, `execute_citation_verify` yields the same parsed verdict.
  *Recorded-source replay test* via `StubHttpClient`, network-free per Art. 1.3
  (the determinism story for the LLM is owned by ADR-0010; determinism over the
  full panel + vote is owned by ADR-0006).

- **Observable (DoD 6).** A `CitationFinding` makes the fetched source
  (`SourceProvenance`), the located passage or its absence (`passage`), the voted
  verdict, and the measured `agreement` simultaneously inspectable on
  `LiveOperatorView`. Per Art. 3.1/3.2, a verdict is a decision: the verification
  path emits a `tracing` span recording the verdict, the agreement counts, and the
  grounding outcome (verdict at DEBUG/INFO, cf. the LiftWing round-trip span named
  in Art. 3.5), and `CitationVerificationError` carries full `Display` context with
  no swallowed errors (Art. 3.3).

Cross-cutting effects:

- **The use-site ordinal is retained for a future repair channel
  (forward-compatibility).** A verdict `CitationFinding` deliberately does **not**
  map to a write — coercing a read result onto the write enum is Alternative (e),
  rejected. The one piece of article-side state worth keeping is the
  `use_site_ordinal`: retaining it lets a later repair channel — a distinct
  repair-*proposal* type carrying this ordinal as its node anchor (ADR-0003) plus an
  authored `replacement_text`, the confirmation-gated edit-proposal shape built and
  validated in Luis Villa's wikiharness — produce an operator-confirmed `InlineEdit`
  without re-resolving the use-site. The finding stays purely informational (no
  anchor-to-write path of its own, Decision 5); it simply stops discarding the
  ordinal. Cost: one `u32` on the wire, no new behavior, no scoring impact.

- **First LLM in SP42, behind one `HttpClient` edge — and no new dependency
  (Constitution Art. 7).** This is the first place an LLM enters SP42 — an
  ML-integrated project (it consumes LiftWing's ORES-successor damage scores,
  ADR-0001 §9) that has used no LLM to date (no `openai`/`anthropic`/`llm`/`ollama`
  dependency exists in the workspace today). The contract adds **no new runtime dependency**: the model
  endpoint is reached through the existing reqwest-backed `BearerHttpClient`
  (workspace `reqwest = "0.12.24"`, `rustls-tls`) as just another `HttpClient`
  target — no vendor SDK, so there is no new crate to vet under Art. 7.2. Should a
  future heterogeneous-panel provider adapter (ADR-0006) pull in a model-specific
  crate, that PR must satisfy Art. 7 (documented justification, maintenance status,
  transitive count — >50 needs lead approval) and the Art. 5.2 `cargo-deny`
  GPL-3.0 check in its own right. The dependency is confined to a single
  config-driven edge (pure `build_*`, pure `parse_*` ending in a `validate_*` gate,
  injected `execute_*<C: HttpClient + ?Sized>`, endpoint default-absent like
  `liftwing_url`); `sp42-core` names no model vendor, and the LLM's output is
  **never trusted on its face** — every verdict is re-grounded against retrieved
  bytes, so the model's role is judgment over evidence, not generation of truth.
  Swapping providers or resizing the panel is a config/adapter change, no contract
  change.

- **No telemetry, tokens in memory only (Art. 10); informational scope.** The
  contract carries no token of any kind. The three-way Art. 10 split is deliberate
  and non-overlapping: model-endpoint credential hygiene is owned by ADR-0009 (the
  API key lives in the shell `HttpClient` adapter, in memory only), persisted-
  artifact hygiene — no token or PII in any stored snapshot — by ADR-0010, and this
  contract persists neither. First-cut source types are HTML pages
  and existing archived snapshots only (PDFs deferred); the verdict is **strictly
  informational and does NOT feed scoring** (no scoring-policy ADR is triggered),
  keeping verification off the composite damage score until its reliability is
  established. Crate placement is deferred to ADR-0009 — this ADR fixes the
  contract *shape*; ADR-0009 decides whether it lands as a module in `sp42-core`
  (the ADR-0004 Contract-Stabilization default for an unproven, single-caller,
  CLI-first contract) or a new crate.

## Non-Goals

- **No verdict-value or anti-fabrication-rule decisions** — owned by ADR-0007.
- **No panel/voting/agreement-semantics decisions** — the panel composition, the
  pure vote, the skeptical tiebreaker, the measured-agreement semantics, and panel
  execution are owned by ADR-0006. This ADR carries the `agreement` field on the
  contract; ADR-0006 defines what it means.
- **No source-snapshot storage format or LLM-determinism mechanism** — owned by
  ADR-0010. This ADR references the content hash and the snapshot; it does not
  define how they are persisted.
- **No crate-boundary decision** — owned by ADR-0009.
- **No change to the operator-confirmed action path** (ADR-0003) — verification
  produces a candidate the operator confirms, never a write.
- **No PDF source types and no scoring integration in the first cut** (PRD-0001).
