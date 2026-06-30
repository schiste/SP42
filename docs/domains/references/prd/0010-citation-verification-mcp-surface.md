# PRD-0010: Citation-verification agent surface (MCP)

**Drafter:** Claude Code (Opus 4.8)
**Editor:** Luis Villa
**Date:** 2026-06-30
**State:** Draft
**Discussion:** none yet
**Spawned ADRs:** none yet (bound by ADR-0007 verification semantics and ADR-0008 verification contract; a transport/threat-model ADR is expected with the hosted phase, not the MVP)

## Problem

SP42 can already do something no general-purpose agent can: fetch a cited source through a hardened pipeline and decide, with verbatim-quote grounding and a multi-model panel, whether the source actually supports a claim. Today that capability is reachable only through SP42's own page-oriented HTTP routes and CLI — i.e. only by SP42 itself.

People are building Wikipedia and Wikidata editing agents *now* (e.g. fuzheado's [Wikipedia-AI-Skills](https://github.com/fuzheado/Wikipedia-AI-Skills)). Those agents do citation *hygiene* (archiving, dead-link repair, bare-URL expansion) and citation *presence* checks ("does this `<ref>` contain a URL?"), but none can verify that a source supports the claim it is attached to — a `<ref>` pointing at an unrelated page passes every check they have. So they insert, audit, and trust citations with no grounding, and defer the hard judgment to a human or skip it. The capability that closes that gap exists in SP42 but is not callable by anyone else.

## Proposal

Expose SP42's verification primitives as a small set of typed tools an external agent can call, over the Model Context Protocol (MCP). An agent building or auditing Wikipedia/Wikidata citations can:

- **Check whether SP42 can use a source at all** — `probe_source(url)` returns, deterministically and without spending any model inference, whether the URL is reachable and whether SP42's pipeline can extract usable text — *and distinguishes the two*, so the agent learns "a human could still read this even though the automated pipeline can't."
- **Verify a claim against a source** — `verify_claim(claim, source)` returns a governed verdict (`Supported | Partial | NotSupported | SourceUnavailable`) plus a quote that has been re-located verbatim in the fetched source. The source may be a URL *or* content the agent already fetched, so an agent that just expanded a bare URL via Citoid (or pulled an archive snapshot) is not forced to re-fetch.
- **Verify a whole Wikipedia article** — `verify_wikipedia_page(title)` decomposes the article's claim/reference use-sites and returns a per-use-site verdict, reporting progress as it works.
- **Verify a Wikidata statement** — `verify_wikidata_statement(ref)` renders a statement into a claim, resolves its reference URL, and verifies it.

The verdict is machine-legible and carries its evidence (the located quote), so the agent gets a *defensible* answer it can branch on or hand to a human reviewer — not a model's say-so. The MVP runs locally: the agent-builder runs the SP42 MCP server, brings their own inference keys, and pays their own inference. A hosted, multi-tenant service and two further verbs — `find_source` (find a source that supports an unsourced claim; continuous with PRD-0009 citation-insertion) and `assess_reliability` (is this source acceptable *for this claim* under the wiki's rules) — are roadmap, not this PRD.

The *how* — crate layout, the `rmcp` SDK, execution/caching model, phase plan — is in the design plan `docs/design-plans/2026-06-30-citation-verification-mcp-surface.md`.

## Definition of Done

Behavioral acceptance criteria specific to this surface (the Constitution's general guarantees — tested, deterministic, CI-green — are assumed, not restated):

- [ ] An external MCP client connecting over stdio lists exactly four MVP tools (`probe_source`, `verify_claim`, `verify_wikipedia_page`, `verify_wikidata_statement`) with valid schemas, verified by a stdio integration test asserting the tool list and one round-trip per verb.
- [ ] `probe_source` distinguishes **unreachable** from **reachable-but-unextractable** and makes **zero** model-inference calls, verified by unit tests over reachable-clean, reachable-paywalled (`human_readable_hint = true`), and unreachable fixtures.
- [ ] `verify_claim` accepts the source as either a URL or pre-fetched content and produces identical grounding/verdict behavior over the same bytes, verified by a test exercising both input shapes.
- [ ] `verify_claim` returns the existing `Verdict` (ADR-0007/0008) **unchanged**, and a `Supported`/`Partial` verdict is surfaced only when its quote re-locates verbatim in the fetched source — verified by a test asserting a non-re-locatable quote downgrades the verdict (anti-fabrication gate holds). No new verdict variants.
- [ ] `verify_wikipedia_page(title)` returns a per-use-site verdict for a fixture article and emits at least one progress event during the fan-out, verified by an integration test.
- [ ] `verify_wikidata_statement(ref)` renders a statement with a reference URL into a claim, verifies it, and returns a `Verdict`; a statement with no resolvable reference URL returns `SourceUnavailable` (not an error), verified by unit tests.
- [ ] Identical `verify_claim` inputs return a **stably idempotent** verdict within the cache window (panel nondeterminism absorbed by the cache, not re-rolled), verified by a repeat-call test asserting a cache hit.
- [ ] The MCP server crate is host-only and does not enter or regress the `sp42-app` `wasm32` build, verified by `cargo test` on the new crate plus the existing wasm build.

## Alternatives

- **Keep verification SP42-internal; build our own Wikipedia agent.** Rejected: SP42's value is the verification engine, not an agent loop. External editing agents already exist and have an SP42-shaped hole; meeting them as a tool is higher-leverage than competing with them. (No agent framework — Rig/Flue/AutoAgents — is adopted; SP42 is the tool the agent calls.)
- **Expose a cheap source-reliability lookup verb** (domain → reliable/unreliable). Rejected: reliability is community-owned, enwiki-only (Cite Unseen), and context-sensitive (WP:RSP); a cheap flat verdict misrepresents policy and re-serves data SP42 doesn't own. Context-aware reliability becomes the roadmap verb `assess_reliability`.
- **Expose `check_quote` (verbatim grounding) as its own verb.** Rejected: grounding is an internal guarantee of `verify_claim`, not an agent-facing concept.
- **Ship a hosted multi-tenant service first.** Rejected for the MVP: hosted introduces an untrusted-caller SSRF threat model and an unsolved inference-cost-ownership problem; local BYO-key sidesteps both and is what the first consumers need.

## Risks

- **Over-trust by consumers.** An agent may treat a verdict as ground truth and auto-edit. Mitigation: the verdict is conservative by design (SP42 errs toward abstention, cf. #25) and ships its evidence (located quote, panel agreement) so consumers can route low-agreement cases to humans; integration guidance frames `verify_wikipedia_page` as triage-assist, not a replacement for human review.
- **Verdict-vocabulary mismatch.** Consumers (e.g. article-audit) use neutrality/original-research verdicts SP42 does not produce. Mitigation: document a mapping (`Supported→confirmed`, `NotSupported→contradicted|unverifiable`, `Partial→mixed`) and explicitly emit **no** NPOV verdict; SP42 judges claim-source support only.
- **Cost surprise.** The panel spends real inference per call. Mitigation: MVP is BYO-key (the caller sees and owns the cost); `probe_source` lets the agent screen accessibility before paying; the result cache prevents re-paying for repeats.
- **Wikidata rendering fidelity.** Statement→claim rendering is net-new and could mis-render a statement, producing a misleading claim to verify. Mitigation: scope the MVP renderer to well-formed statements with a reference URL; surface the rendered claim in the result so a consumer can sanity-check it.

## Open questions

1. **Statement→claim rendering scope.** How rich should the MVP Wikidata renderer be — string-value statements only, or also item-valued/quantity/time statements? *Proposed:* MVP handles the common string/URL/item-label cases and returns the rendered claim for inspection; richer datatypes are follow-on.
2. **Page-verb cost ceiling.** Should `verify_wikipedia_page` cap the number of use-sites it will verify in one call (a large article = dozens of panel runs)? *Proposed:* yes — a default limit with an explicit opt-out option, and a logged notice when truncated (no silent cap).
3. **Cache visibility.** Should a cached `verify_claim` result be flagged as cached in the response? *Proposed:* yes — a small `cached` boolean, so a consumer can force-refresh if it suspects source drift.
4. **`find_source` priority.** Two consumer skills demand it (notability, biography-writing). *Proposed:* keep it roadmap but first-in-line after the MVP, given the PRD-0009 lineage.
