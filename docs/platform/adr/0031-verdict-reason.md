# ADR-0031: Verdict reason — a static policy-legible code plus localizable text, so a blocklist never has to match prose

**Status:** Accepted
**Date:** 2026-09-28
**Author:** Christophe Henner (drafted by Claude Code)
**Summary:** A verdict's reasons carry a statically declared, per-ruleset machine-legible code plus text rendered from a per-project, per-language catalog keyed by that same code, so a disallowed-reasons blocklist is decidable at config-lint time and means the same thing on every wiki regardless of the language its reviewers write in.

## Context

ADR-0027 §2 records `reasons: Vec<Reason>` on every `GateVerdict`, and both
ADR-0028 §4 and ADR-0029 §4 carry the same field on their verdicts. None of the
three defines `Reason`; each deferred it, and ADR-0029 flagged the open question
explicitly — how a free-text, and therefore language- and project-specific,
reason payload should be structured to stay meaningful across projects using
different languages for the same `resolution_class` — while asking that whichever
ADR shaped it treat localization as a starting constraint rather than an
afterthought.

The deferred requirement is not cosmetic. ADR-0028 §5 and ADR-0029 §5 both make
a reason a **fail-closed policy gate**: a recorded reason matching
`disallowed_reasons` is rejected rather than logged, encoding AFCSTANDARDS' real
rule that certain decline reasons are simply not valid however plausible they
read. Two things follow, and they pull against each other:

- The check has to be decidable *mechanically*, or "rejected, not log-only" is
  not enforceable. ADR-0028 §6 asks the linter to validate `disallowed_reasons`
  against the ruleset's own `outcomes` — a static check — while §5 requires the
  rejection to happen at evaluation. ADR-0028 flagged the gap between those two
  and left it unresolved.
- The thing being matched is the part of the verdict that is *least* uniform.
  Reason text is authored by humans, in the wiki's language, on projects whose
  policy vocabulary has no equivalent in English. Matching prose would mean a
  blocklist that is unenforceable on exactly the wikis least able to police their
  own decline reasons, and trivially defeated by rewording.

Localization is therefore not a downstream concern here. It is the reason a
code/text split exists at all.

## Decision

### 1. A reason is a code plus parameters, never prose
```
Reason { code: ReasonCode, params: ReasonParams }
ReasonCode  = <a statically declared, per-ruleset identifier>
ReasonParams = <typed data, rendered into the catalog entry's placeholders>
```
`code` is the policy-legible identity: it is what `disallowed_reasons` lists,
what the linter validates, and what any future cross-project comparison would
match on. Rendered text is never stored on the verdict and is never matched
against. A verdict's reason therefore stays meaningful to a machine regardless of
which language, or which project, produced it.

### 2. Codes are declared statically, per outcome arm, and are a closed vocabulary per ruleset
Each `EligibilityOutcomeArm` (ADR-0028 §5) and `QualityOutcomeArm` (ADR-0029 §5)
declares the reason codes it can produce, and those codes are part of the
ruleset's schema — closed *per ruleset*, and open in the sense that a new
ruleset declares its own without editing a platform type. This mirrors how
`ResolutionClass` is central but outcome `key` vocabularies are per-ruleset
(ADR-0027 §2, ADR-0028 §3): one shared vocabulary where the engine has to branch
generically, per-ruleset vocabularies everywhere else.

Codes may not be composed, concatenated, or computed at runtime. A ruleset that
would need a dynamic code is a config the linter rejects, not a case the engine
handles at runtime.

### 3. This closes ADR-0028's deferred eval-time subtlety
Because codes are static and only `params` are runtime, "does this ruleset cite a
disallowed reason?" is decidable from the ruleset file alone. The check moves to
config-lint time — an arm declaring a blocked code is rejected before the ruleset
can go live — and no fail-closed policy check depends on runtime string
construction. ADR-0028's Consequences flagged exactly this as "an implementation
subtlety this ADR does not fully resolve"; it is resolved here, by construction
rather than by a later amendment.

### 4. Text renders from a per-project, per-language catalog keyed by the same code
```
reason-catalog/<wiki_id>/<lang>.yaml   →   { <code>: { template, param_schema } }
```
The catalog is per project *and* per language, because reason wording is policy
wording: a French wiki's decline reasons are its own, and centralizing them
invites exactly the drift this split exists to prevent. A catalog entry missing
for a code a configured ruleset can produce is a **load-time error for that
project**, not a runtime fallback to the bare identifier — a project may not ship
a ruleset whose reasons render as machine codes, because that is the failure mode
where a reviewer sees `insufficient_sources_v2` instead of an explanation.

### 5. Parameters are data, so translation never reassembles a sentence
A parameter is a page id, a count, a namespace name, a threshold — never a
sentence fragment. Templates own all grammar and word order, so a language with
different syntactic rules needs a translated template and nothing else, and no
translator ever has to stitch meaning back together from fragments authored in
another language's word order.

### 6. Every produced verdict carries at least one reason
Because each arm declares its codes statically, any verdict an arm produces has
a non-empty `reasons`. A `NeedsHuman` handoff therefore records what *was*
determined and what remains outstanding, rather than logging a bare "hand this to
a human" with nothing behind it — an audit trail entry with a blank justification
is not an audit trail entry, which is the gap ADR-0027 §2 was raised to close.

## Alternatives

- **`Reason` as free text only.** Rejected — this is the status quo the
  deferrals left in place, and it makes `disallowed_reasons` a check against
  prose: unenforceable across languages, defeated by rewording, and impossible
  for a linter to validate statically. It would have made ADR-0028 §5's
  "rejected, not log-only" claim untrue in practice.
- **Store the rendered text on the verdict alongside the code.** Rejected — a
  verdict recorded on one wiki's language would be misread by another, and
  fixing a wording bug would mean rewriting entries in the append-only log
  ADR-0027 §2 defines, which is the one thing that log must not do.
- **A closed platform-wide enum of reason codes.** Rejected — repeats the
  closed-enum seam ADR-0021 §5 flags as a real cost and ADR-0028 §3 rejects
  outright. Codes are per-ruleset vocabulary; only `ResolutionClass` is shared.
- **Dynamic or templated reason codes, with the blocklist re-checked at
  evaluation time** — ADR-0028's deferred option. Rejected — it makes a
  fail-closed policy check depend on runtime string construction, so the check
  can only be as reliable as every path that builds a reason string. Static codes
  make it decidable at lint time, which is strictly stronger and simpler.
- **Allow an outcome arm to produce no reason.** Rejected — it makes an empty
  `reasons` a legitimate verdict, and an audit trail with a blank justification
  is not an audit trail.
- **One shared message catalog for the whole platform.** Rejected — reason
  wording is policy wording, and a single catalog would encode one project's
  phrasing as the canonical one for every other project, inviting drift in
  precisely the text that reviewers read when a workflow is refused.
- **Fall back to the bare code when a catalog entry is missing.** Rejected — it
  turns a translation gap into a silently degraded operator experience, in a
  system whose whole thesis is that misconfiguration is loud rather than silent
  (ADR-0026 §6, ADR-0027 §3).
- **Parameters as pre-rendered phrases, so templates stay simple.** Rejected —
  it puts grammar back into the data, forcing every language to reassemble
  meaning from fragments authored in another language's word order (§5).

## Consequences

- Closes the last undefined type in the ADR-0027 → 0028 → 0029 chain: `reasons`
  becomes constructible on all three verdict shapes.
- `disallowed_reasons` becomes a language-independent, statically validated
  blocklist of codes — the difference between a policy gate that holds on a
  French or Japanese wiki and one that only holds on the wiki it was written on.
- ADR-0028's open implementation subtlety is closed by construction rather than
  by a later amendment, and the linter in ADR-0028 §6 / ADR-0029 §6 gains a
  check it could not previously express: every code an arm declares must be a
  member of that ruleset's vocabulary and outside its `disallowed_reasons`.
- Adds a per-project, per-language message catalog to the config surface, and
  makes a missing catalog entry a load-time error. This is real new surface area,
  and it is the price of reasons that survive translation.
- Reason codes become the join key for any future cross-project analysis of why
  items were refused, which is only possible because the code is stable and the
  text is not stored.
- Localization is now a requirement of the config schema rather than a concern
  raised after the fact, which is what ADR-0029 asked for.

## Non-goals

- The reason vocabulary any one ruleset declares. That is policy content —
  `disallowed_reasons` and outcome `key` vocabularies are config, as
  ADR-0028 §3/§6 and ADR-0029 §3/§6 already establish.
- Choosing an i18n library or a template engine. The contract is the
  code/catalog/param split and the load-time failure; the mechanism rendering it
  is an implementation choice.
- `matched_rule_path` (ADR-0027 §2, ADR-0028 §4) — *why* a rule fired, as
  opposed to what the reviewer is told. Replay provenance and reviewer-facing
  explanation are separate concerns and stay separate types.
- A reviewer's own free-text notes on a `ReviewerAction` (ADR-0027 §2). A human's
  words are not a machine verdict's reasons and are not constrained by this
  contract's catalog — a reviewer may write whatever the wiki's conventions
  expect.
- Retro-fitting existing reason strings. No gate has shipped, so there is nothing
  to migrate; this fixes the shape before the first writer exists.
