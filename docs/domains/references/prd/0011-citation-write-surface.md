# PRD-0011: Citation write surface (MCP editing shell)

**Author:** Luis Villa (drafted with Claude)
**Date:** 2026-07-01
**State:** Draft
**Discussion:** <PR link TBD>
**Spawned ADRs:** ADR-0015 (Wikimedia OAuth token seam for non-server shells). Depends on the
ADR-0003 *insertion* extension spawned by PRD-0009 (used in the add phase, not owned here).

> Design detail lives in `docs/design-plans/2026-07-01-citation-write-surface.md`.

## Problem

An external Wikipedia-editing agent — e.g. one built on fuzheado's Wikipedia-AI-Skills, or a
Wikipedia editor working through such an agent — can now *review* an article's citations over MCP
(PRD-0010) and, with its own tooling, find a candidate source and confirm it supports a claim
(`verify_claim`). But it cannot **act on that**: there is no MCP path to apply the fix. The edit
machinery exists (ADR-0010 operator-confirmed proposals; the bare-URL-repair apply flow) but only
behind the browser bridge and CLI, so the agent's loop — review → discuss → find →
**add/replace the cite** — dead-ends at the last step.

## Proposal

An agent can **propose** a citation fix on a specific revision — "replace this broken cite with this
better source" — and get back exactly what would change, computed without touching the article
(the read-only preview verb, e.g. `preview_citation_edit`). A **human at the agent's client** is then
shown the concrete before/after and **confirms or declines** via an MCP elicitation; only on
confirmation does it land as a real, attributed Wikipedia edit under the operator's own account
(the authenticated apply verb, e.g. `apply_citation_edit`). If the article moved underneath, apply
**refuses** rather than guesses (anti-drift + `baserevid`), returning a structured
refusal/decline/conflict. Adding a citation to an unsourced claim (PRD-0009) rides the same
propose/confirm path once available.

The verbs also carry accurate MCP tool annotations — preview marked read-only, apply marked
destructive — so a cooperative client can render the right UX ahead of the confirm. Those annotations
are advisory labels for well-behaved clients, **not** the gate: the server-side elicitation is what
actually enforces confirm-or-fail-closed (see Risks).

Scope of *this* PRD: the **MCP editing surface**, the **replace/repair** path, and the **auth model**
— how a non-server client authenticates to edit. The **add** flow — inserting a `<ref>` on an
unsourced claim — is PRD-0009's; this PRD *delivers it over MCP* rather than redefining it (add phase).
Edit authority is a Wikimedia OAuth token: bring-your-own owner-only token via env (MVP), interactive
`elicit_url` login (fast-follow); a separate downstream token, never the MCP client's own; valid for
any Wikimedia project.

MVP wiki scope: `test.wikipedia.org`, opt-in by config gate (ADR-0010 §4). Production wikis are not
enabled by this PRD's closing PR.

## Definition of Done

- [ ] **Replace lands end-to-end:** on elicitation `accept` with no drift, apply produces a new
  revision whose wikitext carries the replacement cite, verified end-to-end against the stub editor.
- [ ] **Add lands end-to-end:** on elicitation `accept` with no drift, apply inserts a `<ref>` on the
  targeted unsourced claim and produces a new revision carrying it, verified end-to-end against the
  stub editor. *(Blocked on PRD-0009 insertion flow + the ADR-0003 insertion extension; see Dependencies.)*
- [ ] The preview verb computes a replayable proposal with **zero writes**, verified by a stub-editor test.
- [ ] The apply verb writes only after an elicitation `accept`; `decline`/`cancel` → structured `Declined`, no write — stub-tested across all three.
- [ ] A non-elicitation-capable client → the apply verb **refuses** (fail-closed), verified by capability-negotiation test.
- [ ] The verbs advertise accurate annotations — preview `read_only_hint`, apply `destructive_hint` (not read-only) — verified by a tool-descriptor test; enforcement never depends on them.
- [ ] Node drift / `baserevid` conflict → structured `Refused`/`Conflict`, **zero writes**, verified by a drifted-revision replay test.
- [ ] The edit carries a **separate** Wikimedia token; the MCP client token is never forwarded downstream, verified by construction/test.
- [ ] A non-opted-in wiki refuses before network work, verified by a gate test.

## Dependencies

This PRD's closing PR is **blocked** on two Draft artifacts landing first — the add path has no edit
machinery without them:

- **PRD-0009 citation insertion** — owns the propose→confirm→insert `<ref>` flow this PRD delivers
  over MCP.
- **The ADR-0003 insertion extension** (spawned by PRD-0009) — node-anchored editing currently does
  replace/modify only; insertion is the planned extension the add path anchors on.

The **replace/repair** path has no such block (it reuses the existing bare-URL-repair apply flow), so
it can land ahead of add if the two above slip. The closing PR's subject line must name these blockers
explicitly.

## Alternatives

- **Proxy to a running `sp42-server`** (reuse its session/OAuth wholesale): rejected — couples the
  standalone MCP binary to a running server and moves confirmation to the browser, breaking the
  BYO-cred stdio posture.
- **One fused write verb** (no separate preview): rejected — ADR-0010 and PRD-0009 are two-step; a
  read-only preview keeps inspection cheap and auth-free.
- **Extend PRD-0009 into one editing PRD**: rejected — bloats 0009's tight test.wiki/CLI insertion MVP.
  This PRD *depends on* 0009's insertion machinery and references it, rather than *absorbing* insertion's
  definition here; depend-on and fold-into are different, and the former keeps each doc's scope tight.
- **Rely on tool annotations for the confirm gate** (`destructive_hint` etc.): rejected — annotations
  are untrusted client-side hints, not a guarantee. We still emit them accurately as advisory labeling,
  but server-side elicitation is the load-bearing confirmation.

## Risks

- **Unconfirmed write** (agent edits without a human seeing it): mitigated by server-side elicitation as
  the load-bearing gate (annotations are untrusted hints), fail-closed without elicitation.
- **Editing under the wrong account / token leakage**: mitigated by a separate downstream token, never
  forwarding the client token; owner-only token scoped to the operator.
- **Stale edit / race**: mitigated by the two guards — node-anchor drift refusal + `baserevid`.
- **Add-path mis-anchoring** (ref inserted on the wrong claim after drift): the add path anchors on an
  *unsourced* claim — a span with no existing `<ref>` to key off — so a moved node risks attaching a
  correct ref to the wrong sentence. Mitigated by applying the same node-anchor drift check to the
  insertion locator: if the anchored node changed, refuse rather than retarget. Add inherits replace's
  fail-closed drift posture; it does **not** fuzzy-match a moved claim. (Constrains the ADR-0003
  insertion extension: its locator must support this drift refusal.)
- **Client elicitation support is uneven**: mitigated by capability negotiation + fail-closed; BYO-env
  token needs no elicitation for auth.

## Open questions

1. **Preview→apply binding.** Does `apply_citation_edit` require the exact proposal object from
   `preview_citation_edit`, or re-resolve from the same inputs? *Proposed:* accept either, but re-run the
   anti-drift re-check at apply regardless (ADR-0010 replays verbatim + re-checks).
2. **Token storage for the interactive path.** Keychain vs `0600` file for the cached login token.
   *Proposed:* OS keychain when available, `0600` file fallback; never committed (mirrors
   `.env.wikimedia.local`).
3. **Verb naming / count.** `preview_citation_edit` + `apply_citation_edit`, or fold the target-resolution
   into `apply` with an `estimate_only`-style preview? *Proposed:* two verbs, matching ADR-0010's routes.
4. **Edit summary provenance.** Auto-generated summary (tool + source) vs agent-supplied. *Proposed:*
   auto-generated with an optional agent note, always marking the tool.
5. **Diff legibility at the confirm.** The elicitation shows a before/after, but wikitext diffs can be
   hard for a human to evaluate — especially the add case, where the meaningful change is *where* the
   `<ref>` attaches, not just the inserted text. What representation makes the confirm a real decision
   rather than a rubber stamp (rendered snippet, highlighted anchor sentence, both)? *Proposed:* show
   the anchor claim/sentence in context with the change highlighted, not a raw wikitext diff; details
   deferred to the design plan.
