# ADR-0009: Source-snapshot storage for verification reproducibility

**Status:** Proposed
**Date:** 2026-06-07
**Author:** Luis Villa

## Context

PRD-0001 (citation verification, PR #17 on `prd-0001-citation-verification`) requires that
re-running verification on the same claim and the **same fetched source snapshot** yields the
same verdict category (DoD item 5, Constitution Art. 2), and that each verification emits an
observable showing the fetched source, the located passage (or its absence), and the verdict
(DoD item 6, Constitution Art. 3). Both requirements turn on a single artifact this codebase
does not yet have: a persisted record of **the exact bytes a verdict was grounded against**,
plus the verdict it produced.

This ADR is one of PRD-0001's four spawned ADRs. Its siblings settle adjacent surfaces:
**ADR-0006 (using LLMs — model panel, measured agreement, and the inference
endpoint)**, ADR-0007 (verdict & anti-fabrication semantics), and ADR-0008
(request/response contract — which also carries the crate placement, its Decision 7).
This one settles **persistent storage formats** — the GOVERNANCE trigger PRD-0001
names for it.

There is a tension to resolve head-on. PRD-0001's verdict producer is an LLM, and an LLM is
**not bit-deterministic**: the same prompt can yield different tokens on different runs. That
appears to collide with Constitution Art. 2.1 ("Same input, same output"). It does not, once
the boundary is drawn correctly — see the Decision.

The verdict producer is also not a single model: the surfaced verdict is the **voted** result of
a multi-model panel, with the panel's **measured agreement** recorded alongside it. The semantics
of that panel — the vote, the tiebreaker, what agreement means — are owned by **ADR-0006**; what
matters *here* is that the persisted records must capture the panel (the N per-model votes and the
agreement), not just one model's answer.

The design follows directly from SP42's own seams. SP42 already has the matching infrastructure:
the `Storage` trait and its `MemoryStorage` / `FileStorage` doubles (`sp42-types/src/traits.rs`),
the versioned persistence-envelope precedent `WikiStoragePayloadEnvelope` (`wiki_storage.rs:112`),
content-hash provenance via the workspace `sha2` dependency, and the immutable-by-id cache
precedent from ADR-0001 §9 (scores cached by `rev_id`, immutable per revision). The storage
formats below are a straightforward composition of these existing precedents.

## Decision

Persist two content-addressed records — a **source snapshot** and a **verdict record** — behind
a trait, with a versioned serde schema, no secrets or PII, and HTML + archived snapshots only
for the first cut. The verdict record persists the **panel**: the N per-model votes and the
measured panel agreement, alongside the voted verdict (panel semantics owned by ADR-0006).

### 1. Determinism is achieved by fixing the substrate and replaying the panel — not by trusting the model

Constitution Art. 2.1 is honored without claiming the LLM is deterministic. Two mechanisms:

- **(a) The stored snapshot is the fixed grounding substrate.** The verdict is grounded on a
  located passage in *the snapshot bytes*, not on the model's free recall. Given the same
  snapshot, the verdict's anti-fabrication invariant (ADR-0007) bounds the output to a passage
  that is verbatim-locatable in those exact bytes — so the *category* is reproducible even when
  token-level output is not. This is the same property as ADR-0001 §9's immutable-per-revision
  cache: the input is pinned by content, so the result is stable.
- **(b) Tests replay recorded model responses — one per panel member.** The model edge is injected
  behind a trait (the ADR-0008 verification edge over `sp42-core`'s `HttpClient`). The surfaced
  verdict is the panel's **voted** result (vote semantics owned by ADR-0006), so determinism is
  over the whole panel: replaying the **N recorded responses** (one per panel member) through the
  pure vote yields the same voted verdict **and the same measured agreement**. In tests, the
  recorded responses are replayed via a `StubHttpClient`-style double — the exact `liftwing.rs`
  test pattern (`StubHttpClient::new([...])` driven with `block_on`), now seeded with N responses,
  one per panel model. The **recorded-source replay test** that PRD-0001 DoD item 5 demands feeds
  the stored snapshot + the N recorded responses and asserts the same voted verdict category *and*
  the same `PanelAgreement`. The panel is thereby deterministic *under test* without pretending any
  member is deterministic *in production*.

These two together also satisfy DoD item 3 (abstain, never guess): a snapshot of an unusable or
unreachable source replays to `source_unavailable` (ADR-0007's abstention case), never
a support judgment, deterministically.

### 2. Content-addressed snapshot, persisted behind the `Storage` trait

A source snapshot is the **extracted readable text** of one fetched source, addressed by the
SHA-256 hex of its UTF-8 bytes (`sha2` is already a workspace dependency). Content addressing
gives free dedup and tamper-evidence: the same address always names the same bytes, and the
anti-fabrication gate (ADR-0007) locates passages against bytes retrieved by that exact address.

Persistence goes through the existing `Storage` trait (`sp42-types/src/traits.rs`), per Art. 6.2
("all external dependencies via traits") and Art. 2.3 (side effects at the edges). `MemoryStorage`
is the in-crate deterministic double; `FileStorage` (or a server-side store) is the production
adapter — exactly the `wiki_storage.rs` pattern of a pure build/parse split plus an injected
store. `sp42-core` names no concrete store. The verification logic lands as a module inside
`sp42-core` per ADR-0008's Decision 7 (no new crate until a second caller and a stable API exist), so the
snapshot codec lives there beside the verdict type.

### 3. Versioned schema; only the body is grounded; the panel is persisted

Both records are versioned serde envelopes per Art. 9.1 ("every external interface has a schema;
all versioned"), following `WikiStoragePayloadEnvelope { version: u32, ... }` (`version: 1` at
first cut). Breaking changes increment the version with one release cycle of backward compat
(Art. 9.2). Illustrative shapes (these are docs, not doc-tested code):

```rust
// SnapshotEnvelope { version, source_url, fetched_at_ms, content_hash, body_text }
// VerdictEnvelope  { version, claim, snapshot_hash, verdict, located_passage: Option<...>,
//                    panel_votes: Vec<{ invocation: ModelInvocation, verdict, located_passage: Option<...>,
//                                       claimed_quote: Option<String>,        // the model's RAW quote, kept even unlocated
//                                       repaired_quote: Option<String>,       // the repair turn's span, if one ran (ADR-0007 §5)
//                                       repair_invocation: Option<ModelInvocation> }>, // the repair call's fingerprint
//                    agreement: PanelAgreement { panel_size, winner_votes }, panel_ref: Vec<ModelRef> }
// ModelInvocation { model: ModelRef, quant: Option<..>, params, prompt_hash }  // per-call fingerprint (ADR-0006 D8)
// ModelRef { provider, model, version }  // version = the pinned model id used; never a key/token
```

The verdict record persists the **whole panel**, not just the surfaced answer: the N per-model
votes (each tagged with the **`ModelInvocation` fingerprint** — `model` / `quant` / `params` /
`prompt_hash` (ADR-0006 Decision 8) — of the call that cast it, its returned verdict, its raw
**claimed quote** (kept even when it fails to locate — the offline locate-replay record), its
located passage, and — when the bounded repair turn ran (ADR-0007 §5) — the repaired span and
the repair call's own fingerprint, so every model call stays in the audit record) **and** the measured
`PanelAgreement { panel_size, winner_votes }`, alongside the `verdict` that the vote elected. **Persisting** these is storage's job; what the vote, the tiebreaker, and the agreement
*mean* is owned by **ADR-0006**. The panel is captured because the voted verdict is only meaningful
if the inputs that produced it and the observed agreement among them are recoverable from the
record — required for reproducibility and audit. `agreement` is **measured** vote counts (the
fraction `winner_votes / panel_size` is *derived*, not stored), an observed count from independent
votes — never a model-reported number, and meaningful only for `panel_size >= 2` (semantics:
ADR-0006). `panel_ref` records the configured panel members (each a
`ModelRef { provider, model, version }`) only — **never a key or token** (Art. 10.1). Recording the
full **`ModelInvocation` fingerprint** per vote (not just the model id) keeps a verdict attributable
to the exact *call* that produced it and lets a recorded run be replayed and matched: `model.version`
is the pinned model id, `quant`/`params` capture how it was run, and `prompt_hash` is a **digest of
the exact prompt+input** — never the raw prompt text or any PII (the prompt and source bytes live in
the content-addressed snapshot, Decision 2). The `ModelInvocation` / `ModelRef` concepts — and the
working assumption that the endpoint serves the requested version, with the revisit trigger if it
ever drifts — are owned by **ADR-0006 (Decision 8)**; which models form the panel is ADR-0006's call,
and persisting each invocation's fingerprint is storage's.

`fetched_at_ms` comes from the injected `Clock` (`now_ms`, `sp42-types/src/traits.rs`), **never a
direct wall-clock call** — fixed in tests via `FixedClock`, per Art. 2 and PRD-0001's reference
to the Clock-injected fetch time.

The snapshot stores **only the fetched source body**. Any bibliographic metadata (title, author,
publication) is kept structurally **outside** the grounded bytes — it may ride the verdict record
as display context labeled "do not quote", but it is never content-hashed and never fed to the
passage locator, so a model can never "ground" a quote in metadata instead of the source body.
This separation is not theoretical: this exact failure was observed and fixed in Luis's
wikiharness, where a path that concatenated bibliographic metadata into the grounded/hashed bytes
let a quote "ground" in a title/author line instead of the source text — the unsafe path was
deleted, and metadata kept as a never-hashed sidecar.

### 4. The verdict record is immutable and read-only; it never reaches the write path

The verdict record is append-only and read-only — like the LiftWing-by-`rev_id` cache (ADR-0001
§9), it is only ever written once and read. It carries **no status machine, no confirmation
token, and no edit affordance**, because verification performs no wiki writes (PRD-0001 DoD item
4). A snapshot + verdict are informational inputs to the read-only
`LiveOperatorView` field whose shape is owned by ADR-0008 §5
(`sp42-reporting/src/live_operator_view.rs:19`), structurally separate from the single
capability-gated write lane (`SessionActionKind` → `/dev/actions/execute`). If review leads to an
edit, that edit flows through the existing operator-confirmed action path (ADR-0003's content-edit
seam) **unchanged**; the storage layer here cannot emit one.

### 5. The verdict-emission is the Art. 3 observable; SP42's action-audit ledger is left untouched

Storing the snapshot hash, the located passage (or its absence), the voted verdict, the per-model
votes, and the measured agreement *is* the observable PRD-0001 DoD item 6 requires — surfaced in
the operator/debug surface (Art. 3.4 debug panel) and traced via `tracing` at DEBUG (Art. 3.2: "a
developer must be able to reconstruct why any edit appeared"). The persisted snapshot and verdict
records (Decisions 2–3) **are themselves the durable, content-addressed trace** of why a verdict
appeared — including which panel members voted which way — satisfying Art. 3 on their own;
no separate ledger entry is required for the observable.

This is deliberately **not** routed through SP42's existing audit ledger, and it is worth being
explicit about why. SP42's audit ledger is not a `Storage`-trait artifact: it is the on-wiki
public `PublicStorageDocumentData::AuditLedger` document (written via `append_public_audit_entry`
in `sp42-server/src/action_routes.rs:125` → `storage_routes`), backed in-session by
`action_history: Vec<…>` (`action_routes.rs:556`). That ledger records **actions and their
side-effects** — wiki writes. Verification is by construction **read-only with no side effect**
(Decision 4, PRD-0001 DoD item 4), so it produces nothing the action-audit ledger is designed to
record; appending a read-only event to a side-effect ledger would misrepresent what happened and
pollute the write-provenance trail. The first cut therefore **does not append to the existing
audit ledger**.

If a separate, queryable *verification* trace is later wanted (e.g. "show every claim verified
this session"), it is a **new read-only persisted record over the `Storage` trait** — distinct
from the action-audit ledger above — kept in the read-only informational lane. That is a noted
follow-up, not first-cut scope; the snapshot + verdict records already discharge Art. 3.

### 6. No secrets or PII in any persisted record

A snapshot is the **minimal input that exercises verification** — extracted body text — not a
verbatim HTTP wire dump. Identifying request/response headers (cookies, client IP, request-id,
`Authorization`) are stripped **before** persisting, via an allowlist (default-DENY), never a
blocklist. This failure mode is real and was observed and fixed in Luis's wikiharness: a verbatim
header dump once leaked a maintainer's client IP and a tracking cookie; the fix kept only
parser-relevant headers. This honors Art. 10.1 (tokens in memory only — never in a stored record)
and Art. 10.4 (no telemetry; user data stays local).

### 7. Scope: HTML + archived snapshots only (first cut)

Per PRD-0001's resolved open question, the first cut persists snapshots of **HTML pages and
existing archived (Wayback) snapshots only**. PDFs are deferred to a follow-up PRD; the snapshot
codec carries no PDF-specific fields yet (a later version bump adds them under Art. 9.2).

## Alternatives Considered

- **(a) Persist a model-emitted numeric confidence alongside the verdict.** **Rejected** by
  PRD-0001: a model-emitted probability is false precision — generated text, not a calibrated
  measurement — and storing it invites downstream code to trust it. The ban is on
  **model-emitted** numbers (confidence / probability / percentage); the schema carries **no
  model-confidence field anywhere** (Art. 9.1 schema is the enforcement point). The carve-out:
  the persisted `PanelAgreement` (observed `winner_votes` / `panel_size`) **is** a number and **is**
  stored and shown — because it is an *observed* count from independent votes, not a model-reported
  one (the carve-out's rationale is owned by ADR-0006). The honest stored signals are the voted
  graded category, the located-passage presence/absence, and the measured panel agreement.
- **(b) Concatenate bibliographic metadata into the grounded snapshot bytes.** **Rejected:**
  folding metadata into the hashed body lets a quote be "located" in the title/author line instead
  of the source text, silently defeating the anti-fabrication invariant (Decision 3). This is not
  a hypothetical — it was built and then deleted in Luis's wikiharness after exactly this failure
  was observed. Metadata is a never-hashed sidecar (Decision 3).
- **(c) Cache only the verdict, re-fetch the source on replay.** **Rejected:** it makes
  reproducibility depend on a live network and an unchanged remote page — the opposite of Art. 2.
  Pinning the bytes is the whole point; the snapshot *is* the reproducibility guarantee.
- **(d) Store a verbatim HTTP wire dump (headers + body) as the snapshot.** **Rejected** on Art.
  10: it captures cookies, client IP, and possibly tokens, and bloats the record. Store the
  minimal extracted body behind a header allowlist (Decision 6).
- **(e) Make verdict storage mutable / confirmation-gated like an action.** **Rejected:**
  verification never writes (DoD item 4); a mutable, gated verdict store would blur the read-only
  informational lane into the write lane. The record is immutable and read-only (Decision 4).
- **(f) Append verification events to SP42's existing action-audit ledger.** **Rejected** for the
  first cut: that ledger (the on-wiki `PublicStorageDocumentData::AuditLedger` document plus
  in-session `action_history`) records write *side-effects*, and verification has none (Decision
  4, 5). A read-only verification trace, if later wanted, is a *separate* read-only record over the
  `Storage` trait, not an entry in the write-provenance ledger.
- **(g) Persist only the voted verdict, discarding the per-model votes and agreement.**
  **Rejected:** the voted verdict is only meaningful if the inputs that produced it — the N votes
  and the observed agreement among them — are recoverable for reproducibility and audit (Decision
  3). The per-model votes + `PanelAgreement` are persisted. (Whether to surface a single-model
  verdict at all — and why a single open model does not suffice — is ADR-0006's call, not a storage
  decision; the storage record captures whatever panel ADR-0006 configures, including a 1-element
  comparison panel where `agreement` is uninformative.)
- **(h) Extract a `sp42-verification` crate now to own the storage codec.** **Deferred** to
  ADR-0008's Decision 7: the contract is CLI-first and unproven and has no credible second caller, so
  ADR-0004's extraction rules say keep it in `sp42-core` behind module boundaries and stabilize
  the API there first.

## Consequences

Testable invariants this binds (each maps to a PRD-0001 DoD item):

- **Snapshot codec round-trips losslessly** — a `proptest` round-trip-is-identity test
  (Constitution Art. 1.2), the precondition for replay determinism (DoD item 5).
- **Same claim + same snapshot + same N recorded responses ⇒ same voted verdict category and same
  agreement** — the recorded-source replay test (DoD item 5, Art. 2.1): feed the stored snapshot +
  the **N recorded model responses** (one per panel member) through the ADR-0008 verify edge and
  the pure vote (ADR-0006), assert both the voted verdict category and the `PanelAgreement` are
  stable. Built on the `StubHttpClient` / `block_on` pattern from `liftwing.rs`, seeded with N
  responses.
- **No model-confidence field anywhere in either envelope; measured agreement is present** — a
  unit/serde test that the persisted schema has no model-emitted-confidence field but **does** carry
  the observed `PanelAgreement` (DoD item 1; pairs with ADR-0007's verdict-type test).
- **Every panel vote is attributable to an exact invocation** — a unit/serde test that each
  persisted `panel_vote` carries a `ModelInvocation` fingerprint (`model` / `quant` / `params` /
  `prompt_hash`; ADR-0006 Decision 8) (and `panel_ref` the configured members), with no key/token
  and no raw prompt text present — `prompt_hash` is a digest (Art. 10.1) — so a verdict is
  reproducible and auditable against the exact call (model, quant, params, prompt) that produced it.
- **A snapshot of an unusable/unreachable source replays to abstention** — an integration test
  that such a snapshot yields `source_unavailable`, never `supported` (DoD item 3).
- **A metadata-only quote does not ground** — a test that a passage present only in the sidecar
  metadata, not the body, fails to locate (DoD item 2's anti-fabrication invariant; pairs with
  ADR-0007's property test).
- **Stored records contain no identifying headers** — a test asserting cookies / client IP /
  tokens never survive into a persisted snapshot (Art. 10).
- **Verification emits the snapshot + located passage + voted verdict + per-model votes + measured
  agreement as an observable** — checkable in the operator/debug surface (DoD item 6, Art. 3); the
  persisted snapshot + verdict records are the durable trace, not an action-audit-ledger entry.

Cross-cutting effects:

- **The schema is a versioned, protected contract** (Art. 9): future changes (PDF snapshots,
  added fields) increment the version with one release cycle of backward compat. The panel votes +
  `PanelAgreement` are part of the v1 schema, not a later bump.
- **`sp42-core` stays pure and I/O-free** (Art. 2.3, 6.2): the codec and locator are pure, as are
  the vote and worker-pool primitives ADR-0006 owns; all persistence and the per-model edge calls
  are at the edges via injected traits, with `MemoryStorage` as the deterministic double.
- **A dedicated `thiserror` domain-error enum** for snapshot/verdict (de)serialization and
  storage, per Art. 6.3 — no `anyhow` in the public interface; struct-style variants with named
  fields, as in `WikiStorageError`.
- **The action-audit ledger is untouched.** Read-only verification is kept out of SP42's
  write-provenance ledger (the on-wiki `AuditLedger` document + in-session `action_history`); the
  snapshot + verdict records carry the verification trace instead. A dedicated read-only
  verification ledger over the `Storage` trait is a noted follow-up, not first-cut scope.
- **The verdict record never enters the write path** (DoD item 4): it is read-only and immutable;
  any resulting edit is an operator-confirmed `SessionActionExecutionRequest` on the ADR-0003
  path, which this layer cannot produce.
- **Determinism cost:** the gate is "fail-closed" — an empty replay set proves nothing, and a
  missing snapshot or a metadata-only quote is a rejection, biased toward suppressing a real
  finding over surfacing a fabricated one. Replay determinism now binds over the **N panel
  responses → stable voted verdict + agreement** (voting owned by ADR-0006), with the
  snapshot-codec round-trip still its precondition. The recorded-source corpus is a maintained
  artifact: the per-panel-member bytes must be re-recorded when the upstream verify contract or the
  panel composition changes — and a **model-version (or quant/params/prompt) change is one such
  drift, detectable** because each vote's `ModelInvocation` fingerprint
  (`model.version` / `quant` / `params` / `prompt_hash`) is recorded.

## Non-Goals

- **Save-Page-Now / triggering new archive captures.** Out of scope; the first cut reads existing
  archived snapshots only (PRD-0001). Capturing new archives is a separate write axis.
- **A general-purpose persistent cache** beyond verification snapshots/verdicts. This ADR defines
  one feature's records, not a shared store.
- **Appending to SP42's action-audit ledger.** Verification is read-only and produces no
  side-effect for the write-provenance ledger to record (Decision 5); a separate read-only
  verification ledger over the `Storage` trait is a possible follow-up, not first-cut scope.
- **The panel/voting/agreement semantics.** Owned by ADR-0006; this ADR only persists their
  outputs (the N votes + `PanelAgreement`).
- **PDF (or other binary-source) snapshots.** Deferred to a follow-up PRD; a later schema version
  adds them.
- **A storage record that can drive an edit.** The write path is ADR-0003's, unchanged; this
  layer is read-only by construction.
