# ADR-0017: Wikidata statement-proposal write contract

**Status:** Proposed
**Date:** 2026-07-01
**Author:** Luis Villa (drafted by Claude Code)

Spawned by PRD-0011 (Q5/Q6). This ADR fixes the **write contract** shape for
proposing referenced Wikidata statements; the concrete fact-extraction workflow that
uses it is the forthcoming citation→facts follow-on PRD. It is drafted now, ahead of
that PRD, so PRD-0011's forward references are real rather than hand-waving.

## Context

PRD-0011 Q5 prioritizes **citation→facts**: when a citation is added to article X,
propose *referenced* Wikidata statements about X's subject, operator-confirmed.

ADR-0010 already established operator-confirmed propose/confirm for **wikitext** edits
— a replayable payload (`WikitextNodeLocator` + `replacement_wikitext` + `baserevid`,
with an anti-drift re-check that refuses rather than clobbers) — and explicitly
invited reuse of its shape ("locator + replacement + verbatim replay") for future
write features. But a Wikidata statement has **no wikitext node**: the edit is a
structured claim applied via `wbsetclaim` / `wbeditentity` (Action API) or the
Wikibase REST API, and drift is measured against the **entity's revision**, not a text
anchor. ADR-0010's *principle* transfers; its *mechanism* does not — the same
relationship ADR-0010 itself had to ADR-0003, which is why it was a thin sibling ADR
rather than a reuse note.

ADR-0007's anti-fabrication gate also applies: a claim of support needs a
verbatim-located passage. Here that means a proposed statement's fact must be
verbatim-locatable in the cited source. SP42 **authors nothing** (the PRD-0008 rule) —
it relays a sourced triple.

## Decision

1. **Reuse ADR-0010's propose/confirm shape; swap the locator and payload.** A
   statement proposal carries:
   - the **target entity id** and its observed **`lastrevid`** (the drift baseline,
     replacing `baserevid`);
   - the **structured statement** — property, value, qualifiers, rank — and its
     **reference snak set** (the citation): URL citations use P854 as one supported
     snak, while book citations may require non-URL reference snaks such as "stated in"
     (P248), page(s), edition/item, or identifier properties;
   - the **ADR-0007 grounding** that justifies the fact (the verbatim source passage +
     source hash).
   Confirm replays exactly that payload; nothing is written during proposal generation.

2. **Drift is detected against the entity revision.** On apply, if the entity's
   current `lastrevid` differs from the proposal's baseline (or the target
   statement/property state has moved), the write **refuses** (zero writes) and offers
   re-proposal, analogous to ADR-0010's `node-drift` refusal. Wikibase's edit API
   accepts a `baserevid` guard, so the refusal is enforced server-side too, not only in
   SP42.

3. **Structured triples, not prose — the safest crossing.** Unlike PRD-0009's
   synthesized description, a statement is property + value + structured reference
   snaks with **no free text to fabricate**. A proposal is offered **only** when the fact
   is verbatim-grounded in the source (ADR-0007); an ungrounded fact is a **structured
   decline**, not a thin or invented statement (ADR-0010 Decision 3).

4. **Per-operator Wikidata write auth.** The write lands under the operator's **own**
   Wikidata session, attributed and reversible — mirroring the per-operator MediaWiki/
   OAuth session posture (PRD-0011 Q1; PRD-0009 Q1). Never a shared/baked-in key.

5. **A sibling apply route, not a new `SessionActionKind` (for now).** Following
   ADR-0010's reasoning about exhaustive, wasm-gated `match` statements across shells,
   the statement-apply is a **sibling** operator-confirmed write surface reusing the
   session + CSRF gates, not a new action verb — foldable into the action lane later if
   the shell wasm-visibility cost ever inverts.

6. **Per-target presence gate.** The workflow opts in per target (only where entity
   writes are enabled — `testwikidatawiki` for the MVP write gate, PRD-0011 Q2), the
   same check guarding both propose and apply, matching ADR-0010 Decision 4. Production
   configs simply omit the key.

7. **Wire contracts live in platform.** Request/response/proposal types are shared
   serde types in `sp42-platform` / `sp42-types` (the ADR-0010 Decision 5 precedent), so
   the server and every shell speak one contract and render a faithful before/after
   without server round-trips.

8. **Respect the FRBR work/edition split — propose onto the right item.** Wikidata
   models books per FRBR: a **work** item (the abstract creative work) versus **edition**
   items (`instance of` Q3331189, "version, edition or translation"). Edition-specific
   facts — publisher (P123), ISBN (P212/P957), publication date, place, pagination —
   belong on the **edition** item, and WikiProject Books guidance is to **source
   statements on the edition**, not the work. A fact drawn from a *specific* cited book
   is therefore proposed onto the edition that citation identifies (resolved by ISBN,
   the same key PRD-0009/ADR-0016 use); a genuinely work-level fact goes on the work.
   When the citation resolves to an edition that has no Wikidata item yet, the MVP
   **declines** (a structured decline, Decision 3) rather than creating an edition item —
   edition creation is deferred, mirroring PRD-0009 Q2 (enrich existing, don't import).
   This keeps SP42 out of the failure mode that a past bulk import hit — ~40k Open
   Library edition (`…M`) identifiers landed on non-edition Wikidata items — which the
   operator-confirmed, one-statement-at-a-time posture already guards against.

## Relation to prior ADRs

- **ADR-0010 (operator-confirmed proposals):** same propose/confirm discipline and
  structured-decline principle; different locator (entity + `lastrevid`) and payload
  (statement JSON + reference + grounding). This is the ADR-0003 → ADR-0010
  relationship carried one level further.
- **ADR-0007 (anti-fabrication grounding):** governs *which* facts may be proposed — a
  statement without a verbatim-located source passage is declined.
- **ADR-0016 (entity read):** consumed to observe `lastrevid` and current statements
  for the drift baseline and the before/after render — including the shared
  statement/reference parser promoted from PR #103.
- **PRD-0010 / PR #103 (`verify_wikidata_statement`):** the URL-reference subset of
  this contract. #103 *verifies* an existing statement against its P854 reference; this
  ADR *proposes* a new statement carrying a structured reference snak set, with P854 as
  the URL-citation case. They are two ends of one loop when the source is URL-based — a
  statement this lane writes with P854 is precisely what #103's verb would later verify
  — and they share the statement/reference model (ADR-0016) and the ADR-0007 grounding
  gate, so the "fact → referenced statement" and "statement → verified reference"
  directions never diverge for P854 while still allowing richer non-URL references.
- **Existing Open Library ↔ Wikidata data flows (do not duplicate):** OL and Wikidata
  already sync — the P648 "Open Library ID" property links the two, OL runs an official
  Wikidata Integration (pulling author bios/photos/awards *from* Wikidata and pushing OL
  IDs *to* it) and publishes a Wikidata dump, and `cdrini/openlibrary-wikidata-bot` does
  identifier sync/cleanup. SP42 must **not** rebuild identifier sync. Its distinct value
  is the layer none of that provides: a *grounded, referenced* statement (structured
  reference snaks + ADR-0007 verbatim passage) proposed from a source SP42 actually
  fetched and verified.
  That directly answers the standing WikiProject complaint that *most bots add
  statements without reliable sources* — every statement this lane proposes carries
  one. Same positioning as PRD-0010 vs. the official Wikidata MCP: we are the
  verification layer, not another sync bot.
- **ADR-0014 (resolve any project):** the write lands under the operator's project
  session.

## Consequences

- The citation→facts follow-on PRD builds on a defined write contract; the proposal
  payload is self-describing, so any shell renders a faithful before/after.
- SP42 gains a **second** structured write lane — wikitext via ADR-0010, entity
  statements via this ADR — both operator-confirmed, drift-guarded, attributed, and
  reversible.
- The contract is reusable by any future "SP42 proposes a Wikidata statement" feature
  (Wikidata→sources cross-checks, book/entity enrichment).

## Non-Goals

- **The fact-extraction pipeline** (source → candidate statements) — that is the
  citation→facts PRD, not this contract.
- **Entity creation, statement deletion/merge, property modeling depth** — the MVP is
  *adding referenced statements to existing entities*.
- **Batch or automatic application** — every apply is one operator-confirmed payload
  (ADR-0010).
- **Wikidata→sources read mining** — separate, read-only, sequenced second (PRD-0011).
