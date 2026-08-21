# ADR-0006: Using LLMs in SP42 — model panel, measured agreement, and the inference endpoint

**Status:** Accepted
**Date:** 2026-06-08
**Author:** Luis Villa
**Summary:** Every LLM capability reaches models through a provider-agnostic `ModelClient` boundary as a multi-model panel whose surfaced answer is the voted result (skeptical tiebreaker); the only quantitative signal is measured panel agreement, never a model-reported confidence, and every invocation is fingerprinted for audit and replay.

## Context

This decision should be settled **before** the citation-verification mechanics that
follow it (ADR-0007 verdict semantics, ADR-0008 contract + crate placement,
ADR-0009 source-snapshot storage): **whether and how SP42 uses large language models
at all.**

SP42 is **ML-integrated** today — it consumes ORES-successor damage scores from
Wikimedia's LiftWing ML service (ADR-0001 §9) — but has used **no LLM to date**.
Citation verification (PRD-0001) is the first LLM-assisted capability and will not be
the last (book-scan support-checking, manual-of-style detection, a discovery review,
Wikidata enrichment, GLAM disambiguation, and copyedit are anticipated — several
prototyped in Luis Villa's wikiharness as evidence, not committed SP42 scope). So the
choices here are **platform-level**: the **one provider-agnostic interface** every
capability reaches a model through (Decision 7), how SP42 combines model outputs into a
trustworthy signal, and where keys and budget live. *Per-feature model
selection* — which models, how many, how they vote — stays with each feature.

Three findings shape the posture. **Open-weight models are best ensembled:** in
Luis's prior experimentation each open model was middling on a 185-case benchmark
(~48–57% exact, 20–39 fabrications each), but combining them by vote recovers a
usable, honest signal. **The browser can hold no provider key:** SP42 is
browser-native (ADR-0001) and Art. 10.1 forbids persisting tokens, so a key in Wasm
is exposed — a remote model must be reached keylessly. **Credentials and budget are
often a third party's:** a sponsor such as WMF, via its HuggingFace account, may
supply the keys and pay for inference (perhaps the common case). The panel,
keyless-endpoint, and sponsor-credential shapes are validated in Luis's wikiharness /
alex-cite-checker work; they are adopted on SP42's own merits.

## Decision

### 1. Multi-model voting is the default; single-model is also first-class

Open-weight models are best ensembled, so the **default** production configuration
for any model-using capability is a **panel of independent models combined by a pure
vote**, and SP42 **must implement** that path. It is not the only first-class option:
a **single open-weight model** is fully supported wherever it is good enough, and a
**single SOTA (frontier) model** is equally first-class — to use a frontier model, or
to evaluate against the open panel. Voting is the default and must-implement path,
not a hard requirement on every run; which a capability uses (panel vs single, and
which models) is per-feature config.

### 2. The surfaced result is the panel's voted result, with a default skeptical tiebreaker

Each panel model independently produces the capability's categorical output (for
citation, the `CitationVerdict` of ADR-0007); a pure `sp42-core` voting primitive
(`nClassVote`-style) selects the most-voted value. Ties at the maximum are broken by
a tiebreaker whose **default** is *skeptical* — resolve toward the least-consequential
outcome, never up to the most-consequential — so a vote can never *manufacture* a
strong result a plurality did not independently reach. Citation's instance is
tie-toward-reject (`Partial > NotSupported > SourceUnavailable > Supported`; never up
to `Supported`); the precedence is a **per-capability choice**, overridable where
skeptical-toward-reject is the wrong bias. The primitive is deterministic and I/O-free
(Art. 2.1 / 2.3). In **single-model mode** there is no vote — the lone output is the
result, with no `PanelAgreement`; the gate (Decision 6) applies identically.

### 3. Measured agreement is the honest signal — and the only quantitative one

The panel's agreement is `PanelAgreement { panel_size, winner_votes }` — **measured**
vote counts (the fraction is derived, never stored), an observed count, **never a
number reported by a model**, meaningful only for `panel_size >= 2`. This is the
carve-out to the no-numeric-confidence rule (ADR-0007), which bans **model-emitted**
numbers: measured agreement is observed by SP42, not asserted by a model, so it is
allowed and shown. **Low agreement is the honest "borderline — needs a human"
signal**, surfaced *alongside* the result, never as a new categorical value.

### 4. The inference endpoint: local, direct, or sponsor/hosted proxy

Models are reached through a **config-driven endpoint** over the existing `HttpClient`
edge (no new edge type) in one of three modes:

- **Local** — a model server on the operator's own machine/network. No provider key
  leaves the machine; the offline / open-default / dev mode.
- **Direct** — SP42 calls a provider directly with a key held by the deployment — the
  operator's **own**, or one **supplied by a third party / sponsor** (e.g. a
  WMF-issued HuggingFace token). Only for shells that can safely hold a secret
  (server, CLI, desktop); **never the browser**.
- **Sponsor / hosted proxy** — SP42 calls a keyless (or proxy-token-gated) proxy that
  owns the provider keys, budget, rate limits, and routing — run by SP42, a **sponsor
  (e.g. WMF)**, or self-hosted. The **only** remote mode the browser may use, the
  default for keyless operators, and the mode in which the funder is swapped by
  re-pointing one config URL.

A homogeneous panel — open models behind one OpenAI-compatible endpoint (an
OpenRouter-style gateway) — is **one endpoint URL + a list of model names**, run with
**bounded concurrency** (a pure `mapWithConcurrency`-style worker-pool in
`sp42-core`). All three modes are deployment + config; `sp42-core` only builds an
`HttpRequest` and parses a response, never branching on the mode.

### 5. Credential ownership follows the mode (Art. 10)

A provider key exists inside SP42 **only in Direct mode**, held in the shell adapter
(the operator's own or a sponsor-supplied token), in memory, never in `sp42-core`,
never persisted. **Local** holds no credential; **Proxy** holds at most a proxy
token, the provider keys living in the proxy. Local and Proxy thus keep provider keys
out of SP42 entirely — which is what makes the browser shell viable. (The concrete
Direct-mode adapter is a shell, never `sp42-core` — ADR-0008, Decision 7.)

### 6. The proxy carries transport and budget, never a feature's judgment

The proxy is a thin transport + budget boundary: keys, budget (rate limit, model
allowlist, token/size caps), routing, optional logging. It **never** runs a feature's
judgment or gating logic and never sees the bytes a feature fetched. A sponsor
therefore **cannot tilt a result** — every gate stays SP42's, run on SP42's side,
wherever the model executed. More broadly, **the LLM is never the final authority on
its own output**: every capability gates it before it has any effect, and the *form*
of the gate is the capability's own — citation re-grounds the quote in the fetched
bytes (ADR-0007), a non-editing discovery pass is confined to suggestions, a content
change is human-confirmed and diff-bounded. The only universal is that **no LLM output
autonomously acts** — not that every capability grounds in citation's sense.

**Per-call authorization (a sponsor may pay for only *certain* calls).** A funder will
not always want to pay for every call — only certain capabilities/prompts, against an
allowlisted set of models, within caps. The proxy may therefore **authorize each call
deterministically** before forwarding: by model allowlist, token/size caps, rate, and an
optional **capability tag** the caller sends (e.g. `citation-verify`) so spend can be
scoped to a capability. This is still the transport + budget boundary — a *policy
enforcement point*, not a judgment point. The decision is deterministic (allow/deny on
request metadata; never an LLM reading the prompt to decide), the proxy still never
inspects the source bytes or runs the feature's gate, and a denied call simply returns
an error the caller treats as a no-result (for citation, `SourceUnavailable`, ADR-0007) —
so the sponsor still **cannot tilt a result**. The capability tag and any audit metadata
(capability name, session id) are **authorization metadata only**: carried for the proxy,
never added to the model input, and never a credential. The config that carries this is
`{ mode: local | direct | sponsor-proxy, base_url, proxy_token?, capability_tag? }` (the
modes are Decision 4); the browser sends a session-scoped **proxy token**, never a
provider key. This shape is already proven in the alex-citation-checker `public-ai-proxy`
(a WMF-funded HuggingFace endpoint enforcing a model allowlist, a per-call token cap, and
`X-HF-Bill-To` attribution) — cited as evidence the pattern holds, not as SP42 scope.

### 7. A provider-agnostic `ModelClient` boundary, adopted now

Every model-using capability reaches a model through one **provider-agnostic `ModelClient`
boundary** — a small trait SP42 owns (`complete(request) -> completion`, over neutral
chat-message / `SamplingParams` / `ModelInvocation` DTOs) — **never** a provider's wire
format. This is settled up front so the LLM-integration surface is clean from the
beginning: a capability builds a neutral request and reads a neutral completion; *which*
provider answered, and *how*, is the adapter's affair. The boundary makes the endpoint
modes (Decision 4), credential ownership (Decision 5), the proxy role (Decision 6), and
the invocation fingerprint (Decision 8) the *same* contract for every capability, and
lets the backend be swapped without touching feature code.

**The boundary — the trait contract feature crates call — is settled now; the *backend
behind it* is the concrete adapter.** That adapter is **`rust-genai`** (jeremychone's
`genai` crate), adopted as an **external, version-pinned dependency — not vendored**
(we revisit vendoring only if we ever need to tweak it). It gives native multi-provider
reach (OpenAI-compatible gateways, Gemini, Claude, OpenRouter) plus the sponsor-proxy
shape (pluggable per-request auth + custom endpoint + arbitrary headers), so feature
crates never touch a provider wire format and v1 is not limited to one wire shape.
(`graniet/llm` and `rig` were evaluated and not chosen — graniet's static per-instance
auth + heavier deps, rig's agent-framework coupling.) The adapter lives in a **shell**
(never pure domain code — ADR-0004's one-way dependency law), so `sp42-core` depends only
on the `ModelClient` trait and gains no vendor dependency. Because the trait *is* the
boundary, the backend stays swappable and invisible to capabilities, and **pinning
contains `genai`'s pre-1.0 churn to the one adapter file**.

The **v1 interaction shape is a single request/response** — what every current capability
needs, served identically in all three endpoint modes. A **multi-turn, tool-using
investigate→verdict loop** (anticipated for Wikidata-style enrichment) and a
**heterogeneous panel** (mixed provider formats) are later *growth* that widens the
trait's method set; they do not change the boundary's existence, only its richness. The
proxy's budget spans **all** capabilities at once — which matters most for high-volume
consumers like a discovery review.

### 8. Every model invocation is fingerprinted — for audit and to enable replay

Every model output records the **invocation** that produced it — not just the model
name — so a result is reproducible and auditable against the exact call. The fingerprint
is `ModelInvocation { model, quant, params, prompt_hash }`:

- `model` — `ModelRef { provider, model, version }`, the model identity (`version` = the
  pinned model id);
- `quant` — quantization when known (e.g. `Q4_K_M`); usually absent for hosted models;
- `params` — the sampling / reasoning parameters actually used (temperature, top_p,
  max_tokens, seed, …), normalized to a stable form;
- `prompt_hash` — a hash of the exact prompt + input sent to the model, so a recorded
  call can be matched and replayed.

This is shared terminology across capabilities; **persisting** it is each capability's
storage concern (e.g. citation verification's verdict record, ADR-0009). It is never a
key or token (Art. 10), and never PII — `prompt_hash` is a digest, not the prompt text
(the prompt and source bytes live in the capability's own content-addressed snapshot
store, ADR-0009).

**Working assumption:** the configured endpoint (Decision 4) serves the `version`
requested — requested-vs-served drift has not been observed in the prior
citation-checker work, so the two are treated as one identity, recorded once. If such
drift is ever observed, revisit this and record the served id distinctly. (Expanding the
attribution from a bare `ModelRef` to the full invocation fingerprint above — provider,
model id, quant, sampling params, and a prompt+input hash — follows PR-#17 review
feedback, so that every turn is auditable and replayable.)

## Alternatives Considered

- **(a) Forcing one fixed configuration on every run — a single model, or a single
  SOTA model.** **Rejected as the default** (open models are middling alone; voting
  is the default and must-implement, Decision 1) — but single-model, open or SOTA, is
  *not* rejected: it is first-class wherever an operator chooses it.
- **(b) Weighted / confidence-weighted voting.** **Rejected** — per-model weights
  from a model's self-reported confidence re-import the model-emitted number ADR-0007
  bans. An unweighted vote plus the default skeptical tiebreaker is the honest
  aggregation.
- **(c) Direct-keyed only, or local-only, as the sole mode.** **Rejected** —
  direct-keyed is impossible for the browser (Art. 10.1); local-only assumes every
  operator can run a model. Both are kept as *modes*, neither as the only one.
- **(d) A single hard-coded proxy / sponsor.** **Rejected** — the endpoint is
  config-driven, so the sponsor is swappable and self-hosters point at their own; no
  lock-in.
- **(e) Let the proxy do verdict / scoring / gating logic.** **Rejected** — that
  would move the gate and the vote out of SP42's control and let a sponsor tilt
  results (Decision 6).

## Consequences

Testable invariants (Art. 1):

- the pure vote selects the plurality value, and the default skeptical tiebreaker
  never resolves a tie up to the strongest support value (a capability may substitute
  its own) — unit test over the tally.
- `PanelAgreement` carries no model-emitted number — serialization/contract test (the
  ADR-0007 carve-out).
- **voting composes with the gate** — a voted strong-support result whose quote does
  not locate in the fetched bytes is suppressed (property test; gate in ADR-0007).
- **determinism over the panel** — replaying the N recorded model responses through
  the pure vote yields the same voted value and the same `PanelAgreement`; network-free
  via `StubHttpClient` (storage in ADR-0009).
- **every invocation is fingerprinted** — each recorded model output carries a
  `ModelInvocation` (`model` / `quant` / `params` / `prompt_hash`); serde/contract test
  that the fingerprint is present and carries no key, token, or raw prompt text
  (`prompt_hash` is a digest) — persistence owned by ADR-0009.

Other:

- **One new runtime dependency, pinned, in the shell only** — the model adapter is
  `rust-genai` (the `genai` crate; MIT/Apache-2.0, ~30 transitive crates — under the
  Art. 7.2 >50 lead-approval threshold), adopted as an **external, version-pinned**
  dependency in the shell adapter (not vendored), documented per Art. 7.2 and
  license-checked per Art. 5.2 in its adopting PR. `sp42-core` stays vendor-free,
  depending only on the `ModelClient` trait; the `genai` dependency never enters a domain
  crate.
- **No scoring coupling** — measured agreement does not feed SP42's composite damage
  score in the first cut, which is standalone (PRD-0001).

## Non-Goals

- **No multi-turn / agentic interface in v1** — the provider-agnostic `ModelClient`
  boundary is **adopted now** (Decision 7) and its v1 shape is a single
  request/response; a heterogeneous panel or an investigate→verdict loop is later
  *growth* that may extend the interface, not a sign the boundary is unsettled.
- **No dynamic / auto-tuned panel selection** — the panel is configured.
- **Not building the reference sponsor proxy** — a separate deployment artifact (the
  alex-cite-checker `public-ai-proxy` analogue); SP42 need only support pointing at one.
- **No change** to the verification contract (ADR-0008) or verdict value set
  (ADR-0007) — this ADR adds the *vote* over the values, the *agreement* beside them,
  and the *edge* they travel.
