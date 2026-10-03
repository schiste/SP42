# Citation Write Surface (MCP editing shell) Design

## Summary

SP42 today exposes citation *verification* to agents as read-only MCP verbs (PRD-0010)
and, separately, applies citation edits through the browser bridge and CLI under the
operator-confirmed-proposal contract (ADR-0010). This design closes the loop: it lets an
external editing agent, via MCP, **apply** a citation edit — both replace/repair an existing
citation and add a new one (per PRD-0012's insertion flow) — with a human at the MCP client
confirming the specific change before it lands. Both paths land end-to-end in this design's scope;
they are *staged* (replace first, since its edit machinery exists today; add follows once its
blockers clear — PRD-0012's insertion flow and the ADR-0003 insertion extension), not scoped apart.

The approach reuses the existing save machinery through one extracted, injectable **guarded-edit
pipeline** (a platform *mechanism*, per ADR-0013) with pluggable edit backends underneath, so
the same core serves the server bridge, the CLI, and the MCP shell, and so Wikidata (structured
statements) and prose insertion are later backends/operations rather than rewrites. Two MCP verbs
mirror ADR-0010's two routes and PRD-0012's `--cite-preview`/`--cite-execute`: a read-only
`preview_citation_edit` and an authenticated `apply_citation_edit` whose confirmation is a
Model-Context-Protocol **elicitation** shown to the human operator. Edit authority is a Wikimedia
OAuth token obtained through a new **token-acquisition seam** for non-server shells (the gap
ADR-0014 left) — bring-your-own owner-only token via env for the MVP, interactive `elicit_url`
login as fast-follow.

## Definition of Done

- [ ] **Replace lands end-to-end:** on elicitation `accept` with no drift, `apply_citation_edit` produces a new revision whose wikitext carries the replacement cite, verified end-to-end against the stub editor.
- [ ] **Add lands end-to-end:** on elicitation `accept` with no drift, `apply_citation_edit` inserts a `<ref>` on the targeted unsourced claim and produces a new revision carrying it, verified end-to-end against the stub editor. *(Blocked on PRD-0012's insertion flow + the ADR-0003 insertion extension; staged as Phase 7.)*
- [ ] `preview_citation_edit` returns a replayable proposal (ADR-0010 payload: `WikitextNodeLocator` + `replacement_wikitext`) for a replace/repair, computing **zero writes**, verified by a stub-editor test asserting no save call is made.
- [ ] `apply_citation_edit` writes only after an elicitation `accept`; a `decline`/`cancel` returns a structured `Declined` outcome with no save, verified by stub tests across accept/decline/cancel.
- [ ] The verbs advertise accurate annotations — `preview` `read_only_hint`, `apply` `destructive_hint` (not read-only) — verified by a tool-descriptor test; enforcement never depends on them.
- [ ] A client that does not support elicitation causes `apply_citation_edit` to **refuse** (fail-closed) rather than write, verified by a capability-negotiation test.
- [ ] Node drift or `baserevid` conflict yields a structured `Refused`/`Conflict` outcome with **zero wiki writes**, verified by tests replaying a drifted revision.
- [ ] The write path carries a **separate** Wikimedia token and never forwards the MCP client's own token downstream, verified by inspection/test of the bearer construction.
- [ ] The guarded-edit pipeline is callable from both `sp42-server` and `sp42-mcp` with no `sp42-server` dependency in `sp42-mcp`, verified by the crate graph (ADR-0013 direction).
- [ ] A wiki not opted in (no `WikiTemplates` citation key) refuses before any network work, verified by a gate test (mirrors `bare-url-repair-not-enabled`).

## Glossary

- **Guarded-edit pipeline**: the substrate-agnostic core — auth → construct change → confirm → `baserevid`-guarded save → outcome taxonomy — shared across wikitext and (future) Wikidata edits. A platform *mechanism* per ADR-0013.
- **Edit backend**: a substrate-specific implementation the pipeline drives. `WikitextEditor` (Parsoid DOM, node-anchored) exists; a Wikidata `wbeditentity` backend is future.
- **Elicitation**: MCP's server-initiated request for user input during a tool call (form-mode for structured confirmation, URL-mode for out-of-band flows like OAuth). Ratified in the 2025-06-18 / 2025-11-25 spec revisions.
- **Operator-confirmed proposal**: ADR-0010's two-step contract — a read-only proposal computes a replayable edit payload; a separate authenticated apply replays it verbatim with anti-drift + `baserevid`, writing nothing during proposal.
- **Token-acquisition seam**: a new platform contract by which a non-server shell (MCP, CLI) obtains and holds a Wikimedia OAuth token — the piece ADR-0014 established server-side but did not expose for downstream shells.
- **BYO owner-only token**: a Wikimedia OAuth *owner-only* consumer token, sufficient for a single developer's own account (ADR-0014 §4), supplied via env — the MVP auth path, analogous to bring-your-own inference keys.
- **Anti-drift / `baserevid`**: two guards — the editor refuses if the addressed node's normalized anchor text changed (`node-drift`); the save sends `baserevid` so MediaWiki rejects (`editconflict`) if the page advanced. Both produce refusals, never guesses.

## Architecture

Three tiers, following ADR-0013's platform ◄ domains ◄ shells direction.

**Tier 1 — guarded-edit pipeline (`sp42-platform`, a mechanism).** Extract the apply-and-save
orchestration currently inside `sp42-server`'s `execute_bare_url_apply` into a library function:

```
apply_guarded_edit(
    backend: &dyn EditBackend,   // wikitext (Parsoid) today; Wikidata later
    http:    &dyn HttpClient,    // bearer-auth'd → MediaWiki Action API
    config:  &WikiConfig,
    page:    &WikitextPageRef,   // title + rev_id (baserevid)
    edit:    EditOp,             // Replace{locator,wikitext} | Insert{anchor,wikitext} | …
    summary: &str,
) -> Result<EditOutcome, EditError>
```

It fetches the MediaWiki edit-token, invokes the backend (drift/range **refusal before any
write**), then saves with the `baserevid` guard. The `ConfirmEdit` payload and `EditOutcome`
taxonomy (`Applied`/`Refused`/`Declined`/`Conflict`/`Error`) are **substrate-agnostic** — a
human-readable change summary plus optional structured before/after — so they render a Wikidata
statement change as well as a wikitext diff. Server-only concerns (patrol, browser-session CSRF)
stay in each shell's wrapper; browser CSRF is irrelevant to an MCP transport, while MediaWiki's
own edit-token is fetched here for all shells.

**Tier 2 — edit backends.** `WikitextEditor` (ADR-0003) is already a generic backend
(replace/modify by ordinal, anti-drift by anchor). Its Parsoid impl moves to public `sp42-parsoid`
(parallel to the read-path `fetch_page_blocks` extraction), so `sp42-mcp` instantiates it without
depending on `sp42-server`. Citation *rendering* (`render_bare_url_citation`, URL → `{{cite web}}`)
stays in the `sp42-citation` domain. New operations — `Insert` (prose-anchored `<ref>`, per
PRD-0012) and a Wikidata backend — bolt on here without touching Tier 1.

**Tier 3 — the MCP shell (`sp42-mcp`).** Two verbs, mirroring ADR-0010's two routes and
PRD-0012's preview/execute:

- `preview_citation_edit` — **read-only**, no auth. Resolves the target (`ref_id`/`use_site_ordinal`)
  against `rev_id`, renders the citation, and returns the replayable proposal + a human-readable
  diff. `read_only_hint = true`.
- `apply_citation_edit` — takes the proposal (or the same target inputs), issues a **form
  elicitation** showing the concrete change to the human operator, and on `accept` runs Tier 1.
  `read_only_hint = false`; replace → `destructive_hint = true`, add → `false`.

The agent never touches low-level locators/anchors — it addresses citations the way
`verify_wikipedia_page` reports them (`ref_id`/ordinal), and SP42 resolves the `WikitextNodeLocator`
and captures the anti-drift anchor from `rev_id`.

**Auth (the token-acquisition seam).** A new `sp42-platform` contract — `WikimediaTokenSource`,
specified in **ADR-0018** — with two implementations: **BYO owner-only token from env** (MVP; mirrors
BYO inference keys and ADR-0014 §4's "owner-only suffices for a developer's own account"), and
**interactive `elicit_url` login** (fast-follow: `prepare_oauth_launch` + loopback callback + token
exchange + refresh + cached token). The seam returns a **bearer-carrying `HttpClient`, never the raw
token** — so the edit pipeline structurally cannot log or forward the secret; that is the mechanism
behind the **separate downstream credential** guarantee (never the MCP client's token — MCP spec's
confused-deputy rule). The token is reused optionally as a read-throughput lever, and — via ADR-0014's
SiteMatrix resolution — valid for **any** Wikimedia project, including Wikidata. `WikiConfig` is handed
in already resolved; the seam does not re-run project resolution.

## Existing Patterns

This design is almost entirely reuse, following patterns already in the tree:

- **ADR-0010 operator-confirmed proposals** — the propose/confirm two-route contract, replayable
  payloads, anti-drift + `baserevid`, structured declines. Elicitation is the MCP realization of the
  confirm step; nothing here weakens the contract.
- **The bare-URL-repair apply flow** (`execute_bare_url_apply`, `replace_node_or_refuse`,
  `execute_wiki_page_save`) — the near-exact template for Tier 1; the extraction generalizes it.
- **The read-path extraction into `sp42-parsoid`** (this PR's `fetch_page_blocks`) — the precedent
  for exposing the Parsoid editor's write ops the same way.
- **ADR-0014 Wikimedia OAuth** — the PKCE/token machinery (`sp42-platform/oauth.rs`) and multi-project
  `WikiConfig` resolution; this design adds only the downstream-shell token seam ADR-0014 left open.
- **ADR-0013 layering** — mechanism-in-platform, domain-policy-in-domain, shell-composes; the tier
  split above is a direct application.
- **PRD-0012 citation insertion** — owns the "add" flow (grounding gate, prose-anchored insert, its
  own ADR-0003 insertion extension). This design *delivers* it over MCP rather than redefining it.

## Implementation Phases

Staged: replace/repair over MCP first (reusable path), then wire insertion (PRD-0012) and the
interactive/Wikidata extensions. Each phase ends green.

### Phase 1: Guarded-edit pipeline (platform extraction)
**Goal:** substrate-agnostic apply-and-save core in `sp42-platform`.
**Components:** `apply_guarded_edit` + `EditBackend`/`EditOp`/`EditOutcome`/`ConfirmEdit` types,
extracted from `execute_bare_url_apply`; `sp42-server` rewired to call it (patrol/session stay in its
wrapper).
**Done when:** server bare-URL apply flow still green; core unit tests cover token→edit→save and the
drift/conflict refusal paths.

### Phase 2: `sp42-parsoid` write ops
**Goal:** the Parsoid `WikitextEditor` (read + write) reachable outside `sp42-server`.
**Components:** expose the editor publicly in `sp42-parsoid`; `sp42-server` re-exports.
**Done when:** editor round-trip tests (existing mock-Parsoid) pass from `sp42-parsoid`.

### Phase 3: Token-acquisition seam — BYO env
**Goal:** `sp42-mcp` can build a bearer client from a Wikimedia owner-only token.
**Components:** `WikimediaTokenSource` (platform contract) + env implementation; a `whoami`/scope
probe.
**Done when:** bearer construction + scope surfacing tested; missing/invalid token yields a clear error.

### Phase 4: `preview_citation_edit` (read-only)
**Goal:** the read-only proposal verb.
**Components:** target resolution by `ref_id`/ordinal against `rev_id`; citation rendering; proposal +
diff response type.
**Done when:** stub-tested; asserts zero writes and correct anchor capture.

### Phase 5: `apply_citation_edit` (replace) + elicitation
**Goal:** the authenticated write verb with operator confirmation.
**Components:** form elicitation of `ConfirmEdit`; capability negotiation (fail-closed without
elicitation); Tier 1 call; outcome mapping.
**Done when:** stub-tested across accept/decline/cancel/drift/conflict/no-elicitation.

### Phase 6: Interactive login + token cache (fast-follow)
**Goal:** `elicit_url` login so the operator need not pre-provision a token, cached across restarts.
**Components:** `prepare_oauth_launch` + loopback callback + token exchange + `prepare_token_refresh` +
persisted token (keychain/`0600`).
**Done when:** OAuth flow tested against a mock authorize/token endpoint; refresh + reuse verified.

### Phase 7: Add primitive (deliver PRD-0012 over MCP)
**Goal:** the `Insert` operation + `apply_citation_edit` add mode.
**Components:** `insert_reference` op (the ADR-0003 insertion extension PRD-0012 spawned) with
uniqueness-or-refuse anchoring; `attach_to` target; grounding gate reuse. The insertion locator must
carry the **same node-anchor drift refusal** replace has: anchoring on an *unsourced* claim (a span
with no existing `<ref>` to key off), a moved node risks a correct ref on the wrong sentence, so a
changed anchor node refuses rather than retargets — add inherits replace's fail-closed posture and
does **not** fuzzy-match a moved claim.
**Done when:** insertion + ambiguity-refusal + drift-refusal tested; matches PRD-0012's insertion behavior.

## Additional Considerations

**Artifacts.** PRD-0013 (this write surface + auth) and a new ADR-0018 (the Wikimedia token seam for
non-server shells, extending ADR-0014). The insertion editor primitive is PRD-0012's spawned
ADR-0003 extension, not owned here — Phase 7 depends on it.

**Wiki opt-in gate.** Production wikis stay disabled unless the wiki config names the citation
template (ADR-0010 §4, PRD-0012 §MVP). The MCP verbs honor the same gate; the MVP targets
`test.wikipedia.org`.

**Wikidata extensibility.** A Wikidata write is a *sibling backend* (`wbeditentity`) reusing Tier 1,
the same Wikimedia token (ADR-0014 resolves Wikidata config for free), and the same elicitation
confirmation — additive, not a rewrite. Its read counterpart (`verify_wikidata_statement`) already
exists.

**Hosted boundary.** For a hosted HTTP `sp42-mcp`, per-connection interactive login (Phase 6) and
MCP's own OAuth 2.1 client→server auth apply; server-side token validation is unnecessary because SP42
is a broker (Wikimedia is the resource that authorizes at edit time), so the design does not add a
resource-server layer. Inference/hosting auth remains an orthogonal, SP42/provider-scoped concern,
moot under BYO-key.

**Threat model / annotations.** `apply_citation_edit` is annotated `destructive`; but annotations are
untrusted hints per spec, so the server-side elicitation confirmation — not the client's
annotation-driven prompt — is the load-bearing gate. Fail-closed when a client cannot elicit.

**Confirm legibility.** The elicitation carries a human-readable before/after, but a raw wikitext diff
is hard to evaluate at the confirm — especially for add, where the meaningful change is *where* the
`<ref>` attaches, not just the inserted text. The confirm should show the anchor claim/sentence in
context with the change highlighted rather than a bare wikitext diff (PRD-0013 Open Question #5);
concrete representation is deferred but constrains the `ConfirmEdit` payload's before/after fields.
