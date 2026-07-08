# ADR-0019: Open Library apply contract (operator-confirmed enrichment writes)

**Status:** Proposed
**Date:** 2026-07-08
**Author:** Luis Villa (drafted by Claude Code)

Spawned by PRD-0009 resolved Q6(b): the **apply-contract** ADR for the
enrichment lane (Layer 3). ADR-0018 owns the read side (resolve + grounding);
this ADR owns how a confirmed field-level improvement to an existing Open
Library record actually reaches openlibrary.org — and under what gates it is
allowed to. It transfers ADR-0010's propose/confirm/refuse-on-drift
**discipline** to a non-MediaWiki target whose **mechanism** is entirely
different (no `WikitextNodeLocator`, no `baserevid`, no CSRF-token API).

Grounding for every mechanism claim here is the 2026-07-08 apply-path
research note (`docs/design-plans/2026-07-08-open-library-apply-path-research.md`),
which read the server-side permission code rather than the docs' examples.

## Context

What the research established, in one paragraph: Open Library's machine write
API (`PUT /books/OL…M.json`, `POST /api/save_many`) runs against production
but is **usergroup-gated** — infogami's `can_write()` rejects any account not
in `/usergroup/api` or `/usergroup/admin` with a 403, even for a single-record
save. Ordinary accounts write through the **website form path**
(`POST /books/OL…M/edit`, `SaveBookHelper`), which requires only a logged-in
account with per-record permission, carries the edit comment as `_comment`,
currently has no CSRF token, and does **no server-side conflict refusal** (a
stale `v` revision parameter is loaded, not rejected). Authentication for
either lane is a session cookie from `POST /account/login`, which accepts
per-operator Internet Archive S3 keys. API-usergroup membership is granted by
a human process and is publicly readable at `/usergroup/api.json`.

The contract below has to reconcile three constraints: PRD-0009's posture
(per-operator identity, operator confirms every field, enrich-existing-only),
ADR-0010's discipline (a proposal is inert; apply replays exactly what was
confirmed; drift refuses), and an upstream surface where the only
ordinary-account lane is a **form contract that is not a published API**.

## Decision

### 1. Two apply lanes, selected by derived capability — never configured by hand

- **Form lane (default).** The apply executes as the website edit form does:
  `GET /books/OL…M/edit` (or `/works/OL…W/edit`), parse the form, replay its
  fields with the single confirmed field changed and `_comment` set, `POST`
  back. This is the lane every ordinary operator account has today.
- **REST lane (privileged).** When the operator's account is a member of
  `/usergroup/api`, the apply is a `PUT /books/OL…M.json` of the full current
  record with the confirmed field changed and `_comment` attached — the
  officially recommended machine lane, structurally cleaner than form replay.
- **Capability is derived, not declared:** SP42 reads the public
  `/usergroup/api.json` membership list and checks the operator's key — a
  write-free probe. Members get the REST lane; everyone else gets the form
  lane. There is no configuration knob to force a lane, so a misconfiguration
  can never route a write through a path the account cannot use.

### 2. Per-operator session via S3-key login; no shared identity, ever

- The operator authenticates with **their own** Internet Archive S3 key pair
  (mintable by any account), exchanged at `POST /account/login` for the
  session cookie both lanes ride. S3 keys avoid password storage and are
  independently revocable; this is PRD-0009 resolved Q1 made concrete.
- The Open Library session is held per-operator server-side, alongside the
  operator's wiki session, under the same localhost dev-auth posture as
  ADR-0002 — it is never baked into config, never shared between operators,
  and its absence simply means the enrichment lane is proposal-only for that
  operator.

### 3. ADR-0010's discipline, re-implemented client-side around the record revision

The upstream server refuses nothing on conflict, so SP42 enforces the
discipline itself:

- **Propose:** the proposal is computed against a record read
  (`/books/OL…M.json`) whose `revision` is **pinned into the proposal**. A
  proposal names exactly one field, its current value, its proposed value,
  the value's verbatim source (per PRD-0009's provenance rules), and the edit
  comment.
- **Confirm:** the operator confirms that exact proposal; the confirmation
  token binds to the proposal's content hash (same replay-what-was-confirmed
  rule as ADR-0010 — apply never recomputes the change).
- **Apply:** immediately before writing, SP42 re-reads the record. If
  `revision` moved since the proposal, the apply **refuses** and offers
  re-proposal against the current record. Only an unmoved revision proceeds
  to the lane's submit. This is `baserevid`-by-hand: weaker than a server-side
  conditional write (a race in the read→submit window is possible), and
  accepted as such — the window is milliseconds, the stakes are a wiki-model
  record with full history/revert, and the alternative is no ordinary-account
  lane at all.
- **One field per apply.** A confirmed proposal writes exactly one field; a
  record needing three improvements is three proposals. This keeps every
  Open Library history entry small, legible, and individually revertable,
  and keeps the operator's confirmation meaningful.

### 4. The form lane is a versioned adapter that fails closed

The form contract belongs to Open Library's templates, not to any API
promise. Accordingly:

- The adapter **always GETs the edit form first** and replays every field it
  finds — including any hidden fields that appear in the future (this is also
  the CSRF-proofing: if a token is added upstream, the adapter carries it
  automatically rather than posting blind).
- The adapter validates the fetched form against its **expected contract
  version** (the `edition.*`/`work.*` field families it knows how to edit).
  Any surprise — a missing expected field, an unparseable form, a submit
  response that is not the success shape — is **contract drift**: the apply
  refuses, reports "form contract changed; enrichment is proposal-only until
  the adapter is updated", and *never* submits a best-guess POST. A wrong
  write to a public catalog is strictly worse than no write.
- Adapter behavior is covered by fixture-replay tests (recorded form HTML +
  submit responses; ADR-0009 discipline, no live network in tests), with the
  fixtures refreshed by the enablement spike (Decision 6).

### 5. House transport, etiquette, and audit

- Both lanes are pure `build_*`/`parse_*` pairs over the injected
  `HttpClient`, executed through the guarded `sp42-fetch` source face
  (ADR-0015): identified User-Agent, timeouts, caps, no proxy surprises.
  Writes are POST/PUT, so they use the fetch edge's session-bearing execution
  path (as MediaWiki actions do), not the GET/HEAD-only citation fetcher.
- Pacing needs no new machinery: applies are operator-confirmed
  one-field-at-a-time, so the write rate is human. Reads around the apply
  (capability probe, form GET, drift re-read) stay within the existing ≤ 3
  third-party concurrency posture.
- Every apply — attempted, refused, succeeded — lands in the existing action
  history/audit trail with the proposal it replayed, the lane used, the
  record revision it was applied against, and the resulting response, so an
  Open Library edit is as reconstructable as a wiki edit.

### 6. The write lane ships disabled, behind an explicit enablement gate

Layer 3 remains **proposal-only** until all of the following are recorded in
the enabling PR (this is the PRD-0009 DoD item made operational):

1. **Live form spike (one-time, manual, a test account):** confirm the form
   lane's minimal single-field edit end-to-end against production — the exact
   field set the form requires, `_comment` visible in the record's history,
   and the observed behavior of a stale `v` submit (merge vs overwrite),
   which calibrates how paranoid the drift refusal must stay. Record the
   target record, before/after, and the resulting revision.
2. **Upstream courtesy:** Open Library's team has been told what SP42 does
   (assisted, operator-confirmed, one field at a time) via their documented
   contact channel — the same channel as an API-usergroup request. If they
   object or require a different path, the lane stays disabled (the PRD-0008
   frwiki-gate posture).
3. **Capability report:** the operator-facing capability panel shows which
   lane (if any) the operator's session has, and an operator with no session
   or no usable lane sees proposal-only output with zero write affordances.

Until the gate passes, everything above exists as mechanism + tests only.

## Consequences

- Every ordinary operator gets a real apply path (the form lane) without
  asking Open Library for anything; operators who obtain API-usergroup
  membership transparently upgrade to the cleaner REST lane. No third
  configuration state exists.
- SP42 takes on a template-coupled adapter with a standing maintenance
  liability: upstream form changes turn the write lane off (fail-closed)
  until the adapter is updated. This is the deliberate price of writing as an
  ordinary account; the drift refusal converts "silently broken" into
  "honestly disabled".
- The refuse-on-drift guarantee is client-side and therefore has a small race
  window neither lane can close (no conditional-write primitive upstream).
  Accepted: wiki-model history bounds the damage, and the one-field rule
  bounds the blast radius.
- The full-record replay in both lanes (form fields / JSON body) means an
  apply rewrites the whole record with one field changed — so the drift
  re-read is load-bearing, not cosmetic: applying over a moved revision would
  silently revert someone else's edit. The refusal exists precisely to make
  that impossible.
- Credential surface grows by one per-operator secret (the S3 key pair),
  handled under the existing dev-auth storage posture; revocation is the
  operator's kill switch.

## Alternatives considered

- **REST lane only (require API-usergroup membership).** Cleanest contract,
  but it makes enrichment unusable for every ordinary operator and turns a
  courtesy process into a hard onboarding dependency. Rejected as the sole
  lane; kept as the privileged lane.
- **Form lane only.** Avoids dual mechanisms, but deliberately ignores the
  officially recommended machine path for accounts that have it, and couples
  *all* writes to the template contract. Rejected; the capability probe makes
  dual lanes cheap.
- **Ask Open Library for a sanctioned SP42-wide bot account.** Violates
  PRD-0009 resolved Q1 (per-operator identity, never a shared key) and
  centralizes attribution away from the human who confirmed the edit.
  Rejected.
- **Browser automation (drive the real edit page headlessly).** Survives
  template drift better than form replay, but imports a browser runtime into
  the server, is far harder to fixture-test deterministically, and still has
  no conflict primitive. Rejected while the form contract stays this simple;
  it is the fallback direction if upstream adds heavy client-side machinery.
- **Skip drift checking (history makes everything revertable).** Rejected:
  full-record replay over a moved revision silently reverts third-party
  edits; "revertable" does not excuse causing the mess (ADR-0010's refusal
  rule exists for exactly this).
- **`/api/import` for missing editions.** Out of scope by PRD-0009 resolved
  Q2 (enrich-existing-only); restated here so the apply lane is never bent
  into an import lane.

## Out of scope / non-goals

- **What is proposable** (field provenance rules, the synthesized-description
  gate and its rich-context requirement) — that is PRD-0009 Layer 3 itself;
  this ADR only carries a confirmed proposal to the site.
- **archive.org item-metadata writes** (IA-S3 JSON-Patch) — excluded by
  PRD-0009's scope boundary.
- **Author-record enrichment** — Open Library's own Wikidata integration owns
  author-level sync (PRD-0009 de-duplication rule).
- **Work/edition creation** — no lane here may create a record; a resolve
  miss stays a miss (ADR-0018 Decision 2).

## References

- PRD-0009 (Layer 3, resolved Q1/Q2/Q6), ADR-0010 (propose/confirm
  discipline), ADR-0018 (read contract), ADR-0015 (fetch edge), ADR-0002
  (per-operator local session posture), ADR-0009 (fixture replay).
- `docs/design-plans/2026-07-08-open-library-apply-path-research.md` — the
  primary-source findings every mechanism claim above rests on (infogami
  `can_write()` gating; `SaveBookHelper` form contract; `/account/login` S3
  login; `/usergroup/api.json` capability probe; upstream rate-limit issues).
