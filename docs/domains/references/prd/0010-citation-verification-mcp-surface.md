# PRD-0010: Citation-verification agent surface (MCP)

**Drafter:** Claude Code (Opus 4.8)
**Editor:** Luis Villa
**Date:** 2026-06-30
**State:** Draft
**Discussion:** none yet
**Spawned ADRs:** none yet (bound by ADR-0007 verification semantics and ADR-0008 verification contract; a transport/threat-model ADR is expected with the hosted phase, not the MVP)

**Implementation note (2026-07-10):** The MVP shipped (the `sp42-mcp` crate:
`probe_source`, `verify_claim`, `verify_wikipedia_page`,
`verify_wikidata_statement`; PR #103). The DoD has been audited against the
test suite — see *DoD bindings* below: 5 of 7 items fully bound, 2 partial
(#134: stdio-level round-trip test; progress events, which do not exist yet).
Open questions 1–2 are tracked as issues #130/#131. Remaining before
`Implemented`: close #134's two items and record the question resolutions.

## Problem

SP42 can already do something no general-purpose agent can: fetch a cited source through a hardened pipeline and decide, with verbatim-quote grounding and a multi-model panel, whether the source actually supports a claim. Today that capability is reachable only through SP42's own page-oriented HTTP routes and CLI — i.e. only by SP42 itself.

People are building Wikipedia and Wikidata editing agents *now* (e.g. fuzheado's [Wikipedia-AI-Skills](https://github.com/fuzheado/Wikipedia-AI-Skills)). Those agents do citation *hygiene* (archiving, dead-link repair, bare-URL expansion) and citation *presence* checks ("does this `<ref>` contain a URL?"), but none can verify that a source supports the claim it is attached to — a `<ref>` pointing at an unrelated page passes every check they have. So they insert, audit, and trust citations with no grounding, and defer the hard judgment to a human or skip it. The capability that closes that gap exists in SP42 but is not callable by anyone else.

A second, independent signal: alex-o-748's [citation-checker-script](https://github.com/alex-o-748/citation-checker-script) is a Wikipedia editor tool that already sends a claim plus its source to an LLM and returns the same `Supported / Partial / NotSupported / SourceUnavailable` verdicts — evidence that editors want claim-source verification today, built independently of SP42. It is a human-facing sidebar that calls an LLM directly; rather than a competitor it is a natural future *consumer* of this surface — ported onto it, its verdict step would call `verify_claim` and gain the verbatim-quote grounding, model panel, and hardened fetch pipeline it does not have on its own. The demand for the judgment is already proven; what is missing is a grounded, agent-callable surface for tools like these to build on.

## Proposal

Expose SP42's verification primitives as a small set of typed tools an external agent can call, over the Model Context Protocol (MCP). The verbs and their signatures below are **proposed** — they name the intended capability and shape of the surface; exact parameter and return shapes may be revised during implementation (final contract lives in the design plan and the code). An agent building or auditing Wikipedia/Wikidata citations can:

- **Check whether SP42 can use a source at all** — `probe_source(url)` returns, deterministically and without spending any model inference, whether the URL is reachable and whether SP42's pipeline can extract usable text — *and distinguishes the two*, so the agent learns "a human could still read this even though the automated pipeline can't."
- **Verify a claim against a source** — `verify_claim(claim, source)` returns a governed verdict (`Supported | Partial | NotSupported | SourceUnavailable`) plus a quote that has been re-located verbatim in the fetched source. The source may be a URL *or* content the agent already fetched, so an agent that just expanded a bare URL via Citoid (or pulled an archive snapshot) is not forced to re-fetch.
- **Verify a whole Wikipedia article** — `verify_wikipedia_page(title)` decomposes the article's claim/reference use-sites and returns a per-use-site verdict, reporting progress as it works.
- **Verify a Wikidata statement** — `verify_wikidata_statement(ref)` renders a statement into a claim, resolves its reference URL, and verifies it.

(Signatures proposed; revisable during implementation.) The verdict is machine-legible and carries its evidence (the located quote), so the agent gets a *defensible* answer it can branch on or hand to a human reviewer — not a model's say-so. The MVP runs locally: the agent-builder runs the SP42 MCP server, brings their own inference keys, and pays their own inference. A hosted, multi-tenant service and further verbs — `find_source` (find a source that supports an unsourced claim; continuous with PRD-0009 citation-insertion), `assess_reliability` (is this source acceptable *for this claim* under the wiki's rules, with BLP/MEDRS as policy regimes), and `is_blp` (does BLP apply to this article — a Wikidata subject-classification check, P31→Q5 / P570, escalating to sources only when Wikidata is ambiguous) — are roadmap, not this PRD.

SP42's surface is complementary to the official, read-only [Wikidata MCP](https://www.wikidata.org/wiki/Wikidata:MCP) (Wikimedia Deutschland): that server *reads* statements, references, and the P31/P279 hierarchy but cannot fetch a source or judge whether it supports a claim. An agent runs both — Wikidata MCP to read a statement and its reference URL, SP42 to verify that URL actually supports the statement. Its existence also confirms MCP as the right protocol for this ecosystem.

The *how* — crate layout, the `rmcp` SDK, execution/caching model, phase plan — is in the design plan `attic/design-plans/2026-06-30-citation-verification-mcp-surface.md`.

## Definition of Done

Behavioral acceptance criteria specific to this surface (the Constitution's general guarantees — tested, deterministic, CI-green — are assumed, not restated):

- [ ] An external MCP client connecting over stdio lists exactly four MVP tools (`probe_source`, `verify_claim`, `verify_wikipedia_page`, `verify_wikidata_statement`) with valid schemas, verified by a stdio integration test asserting the tool list and one round-trip per verb.
- [ ] `probe_source` distinguishes **unreachable** from **reachable-but-unextractable** and makes **zero** model-inference calls, verified by unit tests over reachable-clean, reachable-paywalled (`human_readable_hint = true`), and unreachable fixtures.
- [ ] `verify_claim` accepts the source as either a URL or pre-fetched content and produces identical grounding/verdict behavior over the same bytes, verified by a test exercising both input shapes.
- [ ] `verify_claim` returns the existing `Verdict` (ADR-0007/0008) **unchanged**, and any quote it surfaces as supporting evidence is present **verbatim** in the fetched source — verified by a test asserting a fabricated (non-locatable) quote is never returned as evidence. The guarantee is anti-fabrication (no invented quotes); this PRD does **not** fix single-vs-multi-quote grounding semantics — claim support can legitimately span multiple quotes a reader synthesizes, and multi-span grounding is engine-level work tracked separately (cf. the multispan-grounding ADR thread). No new verdict variants.
- [ ] `verify_wikipedia_page(title)` returns a per-use-site verdict for a fixture article and emits at least one progress event during the fan-out, verified by an integration test.
- [ ] `verify_wikidata_statement(ref)` renders a statement with a reference URL into a claim, verifies it, and returns a `Verdict`; a statement with no resolvable reference URL returns `SourceUnavailable` (not an error), verified by unit tests.
- [ ] The MCP server crate is host-only and does not enter or regress the `sp42-app` `wasm32` build, verified by `cargo test` on the new crate plus the existing wasm build.

### DoD bindings (2026-07-10)

Audit of each DoD item against the test suite (all cited tests pass;
`cargo test -p sp42-mcp`, 27 tests, plus the engine suites). Boxes stay
unchecked until every clause is bound; the two gaps are tracked in #134.

| # | Item | Verdict | Binding / gap |
|---|------|---------|---------------|
| 1 | stdio client lists 4 tools + round-trip per verb | PARTIAL | Tool list bound in-process: `server.rs::tool_router_lists_the_mvp_verbs` (exactly four routes); wire shapes: 7 serde round-trips in `lib.rs`. Gap: no stdio-level test (nothing spawns the binary or drives the rmcp stdio transport) and no per-verb `tools/call` round-trip (#134). |
| 2 | `probe_source`: unreachable vs unextractable, zero model calls | BOUND | `probe.rs::reachable_clean_body_is_extractable`, `::reachable_anti_bot_body_is_not_extractable_but_human_readable` (`human_readable_hint`), `::non_2xx_is_unreachable_with_status`, `::transport_failure_is_unreachable_without_status` (+ PDF/short-body/viewer-shell). Zero model calls is structural: `probe_source` takes no `ModelClient`. |
| 3 | `verify_claim`: URL or pre-fetched, same behavior | BOUND | `verify.rs::url_source_supported_with_grounded_quote` and `::text_source_skips_fetch_with_same_grounding` — same body text, same model completion, same verdict + grounded quote; the text path proves the fetch is skipped. |
| 4 | Unchanged `Verdict`; no fabricated quote as evidence | BOUND | `verify.rs::fabricated_quote_is_never_surfaced` (`quote.is_none()` on non-locatable support); engine-level `sp42-citation` `end_to_end_fabricated_quote_is_unverified_not_groundable`. Verdict reuse is by re-export/`From` of `sp42_citation::Verdict` — no new variants exist to drift. |
| 5 | Page verb: per-use-site verdicts + progress event | PARTIAL | Per-use-site verdicts bound: `page.rs::full_run_verifies_use_site_with_grounded_quote`; cap and dry-run: `::fan_out_cap_truncates_and_flags`, `::estimate_only_counts_without_calling_the_panel`. Gap: no progress-event mechanism exists anywhere in the crate (build or amend, #134), and coverage is unit-level over hand-built blocks rather than an integration test over a fixture article. |
| 6 | Wikidata statement → render, verify, `SourceUnavailable` on no ref | BOUND | `wikidata.rs::renders_claim_and_verifies_against_reference_url`, `::statement_without_reference_url_is_source_unavailable` (returns `Ok`, no model call), `::missing_entity_is_an_error`. |
| 7 | Host-only; wasm build unaffected | BOUND | Observable is CI + the dependency graph: the wasm job builds `-p sp42-app` only (`.github/workflows/ci.yml`), `sp42-mcp` is not in `sp42-app`'s graph, and the layer check tags it a shell; crate tests run under the host CI step. |

## Alternatives

- **Keep verification SP42-internal; build our own Wikipedia agent.** Rejected: SP42's value is the verification engine, not an agent loop. External editing agents already exist and have an SP42-shaped hole; meeting them as a tool is higher-leverage than competing with them. (No agent framework — Rig/Flue/AutoAgents — is adopted; SP42 is the tool the agent calls.)
- **Expose a cheap source-reliability lookup verb** (domain → reliable/unreliable). Rejected: reliability is community-owned, enwiki-only (Cite Unseen), and context-sensitive (WP:RSP); a cheap flat verdict misrepresents policy and re-serves data SP42 doesn't own. Context-aware reliability becomes the roadmap verb `assess_reliability`.
- **Expose `check_quote` (verbatim grounding) as its own verb.** Rejected: grounding is an internal guarantee of `verify_claim`, not an agent-facing concept.
- **Ship a hosted multi-tenant service first.** Rejected for the MVP: hosted introduces an untrusted-caller SSRF threat model and an unsolved inference-cost-ownership problem; local BYO-key sidesteps both and is what the first consumers need.

## Risks

- **Over-trust by consumers.** An agent may treat a verdict as ground truth and auto-edit. Mitigation: the verdict is conservative by design (SP42 errs toward abstention, cf. #25) and ships its evidence (located quote, panel agreement) so consumers can route low-agreement cases to humans; integration guidance frames `verify_wikipedia_page` as triage-assist, not a replacement for human review.
- **BLP exposure.** A wrong `Supported` on a contentious claim about a living person carries reputational and policy risk beyond ordinary over-trust. Mitigation: the MVP stays abstention-biased and local (the caller owns the verdict); heightened BLP handling — a Wikidata-based BLP-*applicability* check (`is_blp`) that triggers a stricter reliability/verification regime — is roadmap, not MVP.
- **Verdict-vocabulary mismatch.** Consumers (e.g. article-audit) use neutrality/original-research verdicts SP42 does not produce. Mitigation: document a mapping (`Supported→confirmed`, `NotSupported→contradicted|unverifiable`, `Partial→mixed`) and explicitly emit **no** NPOV verdict; SP42 judges claim-source support only.
- **Cost surprise.** The panel spends real inference per call. Mitigation: MVP is BYO-key (the caller sees and owns the cost); `probe_source` lets the agent screen accessibility before paying, and the `estimate_only` page mode lets it budget. (Result caching to avoid re-paying for repeats is a hosted-phase lever, not MVP — a per-process stdio server rarely sees a repeat.)
- **Wikidata rendering fidelity.** Statement→claim rendering is net-new and could mis-render a statement, producing a misleading claim to verify. Mitigation: scope the MVP renderer to well-formed statements with a reference URL; surface the rendered claim in the result so a consumer can sanity-check it.

## Open questions

1. **Statement→claim rendering scope.** How rich should the MVP Wikidata renderer be — string-value statements only, or also item-valued/quantity/time statements? *Proposed:* MVP handles the common string/URL/item-label cases and returns the rendered claim for inspection; richer datatypes are follow-on. Best practice here is unsettled — worth a light prior-art scan (Wikidata triple→text verbalization; the T-REx triple/text alignment) before Phase 5. *Tracked in #130.*
2. **Page-verb cost ceiling.** Should `verify_wikipedia_page` cap the number of use-sites it will verify in one call (a large article = dozens of panel runs)? *Proposed:* yes — a default limit with an explicit opt-out option, and a logged notice when truncated (no silent cap). This pairs with an `estimate_only` (dry-run) mode on `verify_wikipedia_page` that decomposes the page and returns the use-site count and implied panel-call count (use-sites × panel size) **without** running the panel, so an agent can budget before committing. The dollar figure stays with the caller, who knows their own model pricing; SP42 reports counts, not cost. *Tracked in #131 — the proposed behavior appears already implemented (`fan_out_cap_truncates_and_flags`, `estimate_only_counts_without_calling_the_panel`); the issue records the resolution.*
3. **`find_source` priority.** Two consumer skills demand it (notability, biography-writing). *Proposed:* keep it roadmap but first-in-line after the MVP, given the PRD-0009 lineage. It is a careful leap (search → fetch → verify, with reliability-aware ranking); prior art exists in the SIFT methodology (Caulfield's Stop / Investigate source / Find better coverage / Trace claims) applied to wiki fact-checking, to be mined when the verb is built.
