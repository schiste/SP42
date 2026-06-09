# ADR-0006: Using LLMs in SP42 — model panel, measured agreement, and the inference endpoint

**Status:** Proposed
**Date:** 2026-06-08
**Author:** Luis Villa

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
choices here are **platform-level**: how SP42 reaches a model, how it combines model
outputs into a trustworthy signal, and where keys and budget live. *Per-feature model
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

### 7. One shared edge today; richer interaction is the future trigger

The modes above serve a **single request/response** per call — the shape most
capabilities need. A **multi-turn, tool-using investigate→verdict loop** (anticipated
for Wikidata-style enrichment; prototyped in Luis's wikidata-SIFT) does not fit, and
is the concrete trigger to adopt a dedicated **`ModelClient` trait** over the bare
`HttpClient` edge — out of scope here; the endpoint modes and credential ownership are
unchanged by it, only the per-call interface grows. A **heterogeneous** panel (mixed
provider formats) is the same trigger. The proxy's budget spans **all** capabilities
at once — which matters most for high-volume consumers like a discovery review.

### 8. Model outputs are attributable to a `ModelRef { provider, model, version }`

Every model output records the model that produced it — its `provider`, `model`, and
`version` (the pinned model id) — so any capability's result is reproducible and
auditable against the exact model used. This is shared terminology across capabilities;
**persisting** it is each capability's storage concern (e.g. citation verification's
verdict record, ADR-0009). Never a key or token (Art. 10).

**Working assumption:** the configured endpoint (Decision 4) serves the version
requested — requested-vs-served drift has not been observed in the prior
citation-checker work, so the two are treated as one identity, recorded once. If such
drift is ever observed, revisit this and record the served id distinctly.

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

Other:

- **No new runtime dependency** for the homogeneous panel — it rides the existing
  reqwest-backed `HttpClient`; a future model SDK is documented per Art. 7.2 and
  license-checked per Art. 5.2 in its own PR.
- **No scoring coupling** — measured agreement does not feed SP42's composite damage
  score in the first cut, which is standalone (PRD-0001).

## Non-Goals

- **No `ModelClient` trait or multi-turn/agentic interface in v1** — single
  request/response; the deferred trigger is a heterogeneous panel or an
  investigate→verdict loop (Decision 7).
- **No dynamic / auto-tuned panel selection** — the panel is configured.
- **Not building the reference sponsor proxy** — a separate deployment artifact (the
  alex-cite-checker `public-ai-proxy` analogue); SP42 need only support pointing at one.
- **No change** to the verification contract (ADR-0008) or verdict value set
  (ADR-0007) — this ADR adds the *vote* over the values, the *agreement* beside them,
  and the *edge* they travel.
