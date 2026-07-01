# ADR-0007: Citation verification verdict and anti-fabrication semantics

**Status:** Proposed
**Date:** 2026-06-07
**Author:** Luis Villa

## Context

PRD-0001 (*Citation verification — initial implementation*, merged as PR #17)
adds an operator-facing capability: for
a claim and its cited source, SP42 fetches the source read-only and reports a
**categorical verdict** on whether the source supports the claim, with the
supporting passage shown inline. The PRD names this a dual-natured trigger —
*Wikimedia action semantics* — and so spawns four ADRs. The foundational one —
whether and how SP42 uses LLMs at all — is settled first in **ADR-0006**; the three
that follow are the citation-specific mechanics, of which this is the first:

- **ADR-0006 — using LLMs:** the model panel, measured agreement, and the inference
  endpoint (local model, direct provider, or sponsor proxy) — SP42's platform
  posture for model use, reviewed first.
- **ADR-0007 (this) — verdict & action semantics:** the categorical verdict set
  and the rule that there is *no support without a verbatim, in-session-locatable
  passage*.
- **ADR-0008 — verification contract:** the request/response surface a verdict is
  exposed through, and where the verification logic lives (the crate placement,
  folded in as Decision 7).
- **ADR-0009 — source-snapshot storage:** how fetched sources and verdict records
  persist for reproducibility and audit.

The verdict rule is the load-bearing one. It governs whether an LLM-derived
judgment may ever say "the source supports this claim," and is the safeguard that
lets a non-deterministic model sit in a deterministic, governed system at all.
SP42 is ML-integrated (it consumes LiftWing's ORES-successor damage scores,
ADR-0001 §9) but has shipped no LLM to date; this capability is its first, arriving
through the collaboration ADR-0003's Context already records. The semantics below follow
from that load-bearing requirement and SP42's own design laws — and the
anti-fabrication discipline they encode has been built and validated in production
in Luis Villa's separate wikiharness project, concrete evidence the approach holds
up. The rules are nonetheless adopted on SP42's own merits, each bound to a test
under the Constitution.

This ADR honors the Constitution: the verdict type and the locatability check are
**pure `sp42-core` logic** with no I/O (Art. 2.3 — side effects only at the
boundary); the type is **deterministic** (Art. 2.1 — same input, same output);
it gets its own **`thiserror` error enum** (Art. 6.3); it carries **no editor
identity** (a verdict is judgment over a source, never over a person); and every
rule below is bound to a unit, property, or integration test (Art. 1.1 — no
untestable code merges).

## Decision

This ADR settles two things of deliberately different durability, kept separate so
one can change without disturbing the other:

- **The anti-fabrication invariant (§5)** — the load-bearing rule, set in stone:
  no `Supported` without a verbatim, in-session-locatable passage — **as of
  2026-06-26, one or more such passages** (within-source synthesis grounding;
  each span independently located, see Changelog and issue #66). It does not
  depend on the exact category names; it depends only on there being a gated
  "supported"-class outcome and an abstention outcome.
- **The verdict value set (§1)** — the vocabulary the invariant operates over,
  grounded in the validated two-step verification pipeline. It is the more
  revisable of the two: a future ADR may refine the categories (operator-facing
  naming is partly a product call) without reopening the invariant.

### 1. The verdict value set — two orthogonal axes, no numeric confidence

Verification answers two different questions, in order, and the verdict type keeps
them as **separate axes** rather than flattening them into one list:

- **Availability (STEP 1): is there a usable source body to judge at all?** Decided
  availability-first — a deterministic gate (Decision 4) handles the
  mechanically-detectable failures with no model call, and the model's own STEP 1
  is the backstop for semantic unusability a regex cannot catch. A failure here is
  `SourceUnavailable`, and STEP 2 is not attempted.
- **Support (STEP 2): given a usable body, does it support the claim?** Only on a
  usable source does the model render `Supported` / `Partial` / `NotSupported`,
  with `Supported`/`Partial` gated by the Decision 5 locatable-quote invariant.

Modeling these as one Rust type makes "a can't-judge outcome is never a support
judgment" a property of the type, not a convention to remember:

```rust
// sp42-core, illustrative
pub enum CitationVerdict {
    Judged(SupportLevel),  // STEP 1 passed: a usable source was assessed
    SourceUnavailable,     // STEP 1 failed: no usable body — no support judgment possible
}

pub enum SupportLevel {
    Supported,     // the source contains all the claim's assertions   ┐ require ≥1
    Partial,       // some assertions, or only hedged/uncertain support ┘ located span(s)
    NotSupported,  // addresses the topic but contradicts / lacks evidence — also the
                   // home of "support could not be established" (no separate "unclear")
}
```

The surfaced verdict is the panel's voted result (ADR-0006); the value set above is
the vocabulary that vote operates over.

There is deliberately **no "unclear" verdict tier.** The validated two-step design
folds model uncertainty into `NotSupported` — "support could not be established"
*is* not-supported — and reserves abstention for the one availability case the
system can deterministically detect: the source was not usable
(`SourceUnavailable`). An "unclear" *verdict tier* would re-straddle the very axes
this type separates ("can't use it" vs. "can't decide"), so an *unclear*
verdict tier is intentionally excluded as a verdict value, consistent with
PRD-0001's own four-value set. The
genuinely borderline "couldn't determine — needs a human" case is not discarded; it
has a principled, **measured** home as a *signal alongside* the verdict — **low
measured panel agreement**, owned by ADR-0006 — rather than as a verdict tier.

The serde wire form flattens the two-axis type to a single snake_case string for
the four observable outcomes — `supported` / `partial` / `not_supported` /
`source_unavailable` — via a custom (de)serialize, and the read-only finding kind
serializes as `citation_verdict`. The two-axis split is the in-memory guarantee;
the flat wire string is the stable contract the sibling ADRs (0008 contract, 0009
storage) use verbatim, so the one canonical form lives here with the type it names.

There is **no `confidence: f32`, no probability, no percentage** anywhere on the
verdict, its evidence, or its provenance — not in the type, not in the
request/response surface (ADR-0008), not in the operator UI. This is enforced
*structurally*, by the absence of any field that could carry one. A
model-emitted probability is generated text, not a calibrated measurement; it is
false precision and is rejected (PRD-0001 *Alternatives*). The one narrow carve-out
is **measured panel agreement** — observed, not model-reported — which is allowed
and shown; it is owned by ADR-0006 and leaves this structural no-model-confidence
rule fully intact.

The honest signals are: the **graded support level**, **explicit abstention**
(`SourceUnavailable`), and **measured panel agreement** (ADR-0006) — never a number
reported by a model. No agreement number feeds scoring — see Consequences.

### 2. The unit is a (claim, fetched source) pair at one use-site — inputs are those two only, never editor identity

The only inputs to a verdict are the **claim text** and the **fetched source
body**. An editor-identity signal (anon, account age, edit count, group, IP) is
**never** a verdict input. This is "assess the edit, not the editor," and SP42
already holds the discipline elsewhere — but here it is a hard rule of the
verdict type: no field path exists that could route identity into a verdict.
Whatever identity-aware triage SP42 applies to *queue ordering* stays strictly
outside the verdict path.

The verified unit is therefore a single **citation use-site** — one (claim, source)
pair — not a footnote. A source cited at several places in an article (a reused named
ref) backs a *different* claim at each use, so each use-site is verified
**independently** and may receive a different verdict; the footnote number identifies
a *source*, not a claim. An article-level report consequently enumerates **use-sites
in document order** — one result per use-site — and that document-order position,
the **use-site ordinal**, is the stable handle by which a single use-site is
addressed: it is what the *Surface*'s `--ref` drill-down and report rows reference,
and — being the document-order position of the use-site's `<ref>` node — the
article-side anchor a future node-anchored repair (ADR-0003) would resolve an edit
on. The verification result therefore **carries this ordinal** rather than
recomputing it (the field shape is ADR-0008's; this ADR fixes only that the
verdict's subject is a use-site, not a footnote, and that a use-site has a stable
document-order ordinal). How the claim text at each use-site is derived is
**Decision 7**.

### 3. Abstain rather than guess

When there is no usable source to judge, the verdict is `SourceUnavailable` —
**never a support judgment** (PRD-0001 DoD item 4). Within a usable source, an
unparseable or non-committal model response defaults to `NotSupported`, never to
`Supported`: support that cannot be established is *not established*, never a free
pass — there is no separate "unclear" outcome (Decision 1).

A model or panel that cannot be reached — an unreachable endpoint, or a
sponsor-proxy call denied per ADR-0006 Decision 6 — **also** resolves to
`SourceUnavailable`: with no model verdict to ground, abstention is the only
honest outcome, never a guessed support level. This deliberately **reuses the
abstention outcome rather than adding a model-unavailable verdict tier** — the
four-value set stays closed (Decision 1). The verdict value is shared, but the
*cause* (an unusable source body vs. an unreachable model) is distinguished by
the `reason` carried on `CitationVerificationError::SourceUnavailable` (ADR-0008),
so the operator-facing cause is not conflated even though the category is one.

### 4. The availability axis is deterministic-first: a body-usability (GIGO) gate yields `SourceUnavailable` without a model call

Before any model sees a source, a **pure, deterministic** classifier inspects the
fetched body. If the body is structurally unusable — an anti-bot interstitial,
a CSS/JSON-LD leak, an archive-chrome/redirect notice, a stub under a length
floor, or an empty/failed fetch — verification **short-circuits to
`SourceUnavailable` without invoking the model at all**. This is a bounded,
ReDoS-safe, I/O-free function. It pulls the mechanically-determinable availability
decision **out of the model's hands**, so a fetch/scrape failure is never
mis-attributed as a model accuracy error, and the model is never asked to reason
over garbage. The model's own STEP 1 remains a backstop for *semantic* unusability
a regex cannot catch — e.g. a library/catalog landing page that is structurally
fine but carries no article body — so the availability axis is deterministic where
it can be and model-checked where it must be. The classifier lives in `sp42-core`
as pure logic (Art. 2.3) with its own `thiserror` enum (Art. 6.3) and is tuned to
favor false-negatives — let a borderline body through to the model — over
discarding real text.

### 5. The load-bearing anti-fabrication invariant (the firm, isolated rule)

**A `Supported`/`Partial` verdict is never CONFIRMED — and no autonomous path
may ever act on it — unless its supporting passage is locatable VERBATIM in a
source SP42 actually fetched this session.**

This is PRD-0001 DoD item 3, the reason an LLM can be trusted in this loop at all,
and the rule this ADR holds **set in stone** independent of the §1 value set (it
binds to the `Judged(Supported)` / `Judged(Partial)` support levels, but its force
does not depend on what those categories are named). The verdict itself is the
panel's *judgment* and is surfaced honestly either way; what this rule controls is
the orthogonal **grounding axis** — whether that judgment is machine-confirmed in
the fetched bytes (`located`), recovered by the guarded fuzzy match
(`located_fuzzy`), or **unverified** (`unlocated`: a human may weigh it, an
autonomous path never may). Rewriting the verdict on a locate miss is itself a
falsehood (it asserts "the source lacks the evidence" when the truth is "we could
not verify the transcription") and is therefore *not* how the rule is enforced.
The mechanism has two parts:

- **A pure locatability primitive.** A pure `sp42-core` function
  `locate_quote(quote, source) -> Option<usize>` returns a byte offset into the
  source, or `None`. It folds **transcription/extraction artifacts only** —
  Unicode NFC, whitespace-run collapse, curly→straight quotes, case folding,
  dash unification, zero-width-character stripping — **nothing semantic**; an
  ellipsis-elided quote matches fragment-by-fragment, in document order, within
  a bounded window. A reworded or fabricated span therefore still does not
  match. An empty/whitespace quote returns `None` (an empty string would
  otherwise "locate everywhere"). Two bounded recovery mechanisms sit on top,
  neither able to weaken the guarantee: a **repair turn** (one extra model call
  asking for the exact shortest verbatim span or `NO_SPAN` — transcription
  only, it never re-litigates the verdict, and its span re-locates through this
  same primitive) and a **guarded fuzzy fallback** (`locate_quote_fuzzy`:
  anchor-token candidate windows, in-order token match at a high threshold,
  digit-bearing tokens required exactly, short quotes excluded; it surfaces the
  SOURCE's own span and only ever yields the distinct `located_fuzzy` status,
  never the exact tier).

- **An independent re-check, not the model's self-report.** The producing step
  (model call + verdict parse) is *untrusted*. A separate, tool-agnostic
  grounding check re-runs `locate_quote` against the **bytes SP42 actually
  fetched** (content-addressed; see ADR-0009) before any finding is surfaced,
  and records the result as the finding's `grounding_status`. A support verdict
  whose quote does not locate is surfaced marked **unverified** and is never
  **groundable**: `is_groundable_support` — which requires the exact `located`
  tier — is the *only* check an autonomous accept/edit path may consult. The
  contract "no actionable support without a grounded passage" holds *not by
  trusting the model* but by re-verifying against retrieved bytes. Even a
  hallucinating model cannot produce a *groundable* `Supported`, because the
  fabricated quote it offers does not exist in the fetched source and the
  re-check refuses to confirm it.

For a no-quote verdict (`NotSupported` / `SourceUnavailable`) there is
no passage to locate, so it grounds on **provenance**: "the cited source was
actually fetched this session" (a source-fetched grounding variant). This catches
a fabricated "I read it and it doesn't support this" when the source was never
retrieved. Bibliographic metadata (title, author, publication) is **never** part
of the grounded bytes — it is verification *context* only. (This is not
hypothetical: in Luis's wikiharness an early implementation concatenated
bibliographic metadata *into* the grounded/hashed bytes, which let a model
"ground" a quote in a title/author line instead of the source body; the fix was
to keep metadata strictly outside the grounding boundary — see Alternatives.)

This gate **composes with the panel voting of ADR-0006 and is never weakened by
it**: a *voted* `Supported`/`Partial` still requires a winning-class verbatim
located quote to pass this independent re-check before it is CONFIRMED, and the
skeptical tiebreaker can never resolve *up* to `Supported` (ADR-0006). Voting
decides *which* verdict wins; this located-quote gate decides whether a
`Supported`/`Partial` is ever **groundable** — confirmable evidence an
autonomous path could act on — at all.

**What this gate establishes — and what it cannot.** The re-check verifies that
the cited passage **exists in the bytes SP42 fetched this session**. It does not —
and cannot — verify that the passage *caused* the model's verdict. A model with
the source in its context can judge from priors and copy a plausible span post
hoc, and observed behavior confirms the quote field is a *reconstruction*, not a
trace: models emit quotes with markdown emphasis added, encoding damage silently
repaired, and casing normalized. This gate is therefore an **anti-fabrication
check on the model's evidence assertion** ("this text exists and I claim it backs
the claim"), not a faithfulness check on the model's reasoning. What any grounding
mechanism actually grades is **how strongly the located artifact ties back to an
evidence assertion the model made**: an exactly-located quote (the model asserted
that very span) is the strongest tie; recovery mechanisms that locate something
*near* or *implied by* the model's output carry a weaker tie and must be surfaced
as such, never silently promoted to the exact tier; and a mechanism in which code
expands or selects display text beyond what the model asserted (e.g.
anchor-pointing) grounds only the asserted part — the expansion is the code's
choice, and presenting it with the authority of a model-asserted, code-verified
quote would manufacture certainty the system does not have. Design rule: every
weaker-tie mechanism carries its own distinct grounding status, and only the
exact tier may ever satisfy an autonomous-action check.

### 6. A verdict is informational, never a Wikimedia action

A verdict is read-only output. It does not write, it does not patrol, it is not a
`SessionActionKind` (the action contract in `sp42-core/src/action_contracts.rs`;
the action path is owned by ADR-0008). If review leads to a
repair, that edit flows **only** through SP42's existing operator-confirmed action
path — `POST /dev/actions/execute` → `post_execute_action` → `execute_session_action`
→ `execute_wiki_page_save`, behind session + CSRF + capability + audit gates,
initiated by the human action (the operator's click *is* the confirmation). A
verification-driven repair is just another operator-confirmed
`SessionActionExecutionRequest { kind: InlineEdit, … }` on that unchanged lane,
anchored through ADR-0003's node locator. The verdict path owns no writer. (The
write-vs-read separation and the exact action path are detailed in ADR-0008.)

### 7. Claim identification: the deterministic prose span a citation backs

A verdict needs a claim, and the claim must be as reproducible and as ungamed as the
source (Decision 2). So in the article/use-site case the claim is **neither generated
nor chosen by a model** — it is **extracted deterministically from the parsed
article**. The rule: the claim a citation backs is the **contiguous run of rendered,
reader-facing prose from the end of the previous citation marker in the same block up
to this marker** — the text *between adjacent citations*. The first marker in a block
takes the run from the block's start; whitespace is collapsed, and footnote numbers
and maintenance tags (e.g. *[citation needed]*) are stripped. It is a pure function of
the parsed-article structure — same article in, same claims out — with **no sentence
segmentation, no NLP, and no model** in the derivation. The model only ever *judges* an
already-fixed claim; it never selects or writes it.

This *between-markers* rule is chosen over the obvious alternative — *the preceding
sentence* (Alternatives (h)) — on purpose: it is **structure-based and
language-agnostic** (no per-language sentence segmenter, which matters as coverage
grows past the enwiki first cut — issue #23), and it tracks how editors actually place
a `<ref>` directly after the text it supports. Its imperfections are **coverage
imperfections, never safety holes**: a span that is a sentence fragment, that crosses a
sentence boundary, or that runs from a paragraph's start for the first marker can
change *what is asked* — but it can **never manufacture a `Supported`**, because the
anti-fabrication gate (Decision 5) still requires a verbatim, in-session-locatable
quote no matter how the claim was scoped.

Three boundary cases are settled here:

- **Bundled citations** (markers with no prose between them, e.g. `[1][2][3]`) all back
  the **same** claim: the extractor walks back past the zero-text markers to the real
  preceding prose, so each bundled marker is its own use-site (Decision 2) verified
  against that shared claim — not dropped.
- **Non-prose markers** — in tables and infoboxes — are **not verified** (skipped, not
  guessed). Citations in list items and captions are not covered by the first cut's
  paragraph-scoped extraction; widening to them is later work.
- **No fetchable source** — a use-site whose citation carries no fetchable source URL
  is **filtered out before any fetch** (there is nothing to fetch or ground), so it
  produces no verdict. This is distinct from a use-site that *has* a URL whose fetched
  body is empty or unusable — that is a surfaced `SourceUnavailable` (Decision 4); the
  never-guess spirit (Decision 3) covers both.

The base between-markers span extraction is built and validated in both Luis Villa's
wikiharness (over paragraph blocks, against a recorded Parsoid fixture of a real enwiki
article) and alex-citation-checker (over a wider set of block containers) — concrete
evidence the rule holds up on real articles. Two refinements above follow
alex-citation-checker specifically: the **bundled-citation share rule** (wikiharness
instead *drops* the empty-span bundled marker — SP42 deliberately adopts share so every
bundled co-citation is verified) and **maintenance-tag stripping**. The rule is taken
on SP42's own terms, as pure `sp42-core` logic bound to a test (Art. 1, Art. 2.3); the
share behavior gets its own SP42 test, since wikiharness does not validate it.

The CLI builds on this: PRD-0001's single-citation selectors (by a snippet of the
claim, by the article report's index, or by naming a source to check every place it is
used — *Surface*) only *select* a use-site; the verified claim is still this
deterministic extraction at that site, not the snippet the operator typed. The one case
where a claim is **supplied** rather than extracted is the ad-hoc claim + source-URL
mode, where the operator provides both inputs directly.

## Alternatives Considered

- **(a) Score the citation numerically** (a confidence/probability/percentage).
  **Rejected** — and rejected at PRD altitude too. A model number is not a
  calibrated measurement; it is generated text wearing the costume of precision,
  and it invites operators to trust accuracy the system does not have. The
  discipline works without it; the no-number rule is enforced
  *structurally* so it cannot regress by accident. (The measured panel agreement
  of ADR-0006 is **not** an exception: it is an observed vote count, not a
  model-emitted number.)

- **(b) Trust the model's own "I located this quote" claim.** **Rejected.**
  Trusting the producer's self-report re-opens the hallucination hole — a model
  that fabricates a verdict will fabricate the quote that "supports" it. The
  architecture instead converts *trust the model* into *verify against retrieved
  bytes* at an independent re-check (Decision 5). This is the single most
  load-bearing rule in the ADR.

- **(c) Fuzzy / semantic quote matching as the confirmation tier.** **Rejected**
  as the check that CONFIRMS evidence: a loose match would let a reworded
  "quote" pass and defeat the gate, so the `located` tier — the only tier
  `is_groundable_support` accepts — stays verbatim. Two refinements, measured
  against a labeled benchmark (SP42#25): **case folding moved into the verbatim
  normalization** (re-casing proved to be a transcription artifact, not a
  fabrication signal — dashes, zero-width characters, and ellipsis-elided
  fragments likewise), and a **bounded fuzzy fallback was admitted as a
  separate, weaker tier** (`located_fuzzy`: heavily guarded, surfaces the
  source's own span, weighable by a human, can never ground). Paraphrase
  tolerance remains a *model* concern at the `Supported`/`Partial` boundary —
  the source may paraphrase the claim — but a CONFIRMED grounding passage
  itself must be verbatim.

- **(d) Let the model decide availability for the mechanically-detectable cases.**
  **Rejected.** Folding archive chrome, anti-bot pages, and CSS/JSON-LD leaks into
  the model's input both wastes the model on garbage and mis-attributes scrape
  failures as model errors. A deterministic pre-gate (Decision 4) is cheaper, more
  honest, and improves eval attribution; the model's STEP 1 still handles the
  *semantic* unusability a regex cannot detect.

- **(e) Concatenate bibliographic metadata into the grounded source bytes** (so a
  title/author/publication line is searchable as "the source"). **Rejected** —
  and this exact failure mode was observed and fixed in Luis's wikiharness: an
  early implementation once folded metadata *into* the grounded bytes, which let a
  model "ground" a quote in the bibliographic header instead of the source body.
  It was removed. Metadata is kept structurally **outside** the
  grounding boundary — rendered as "context only — do not quote," never
  content-addressed, never passed to `locate_quote`.

- **(f) Couple the verdict into SP42's composite damage score.** **Rejected** for
  the first cut, which is standalone (PRD-0001). Wiring an unproven signal into scoring
  would put the scoring policy at risk before the verdict's reliability is
  established, and would drag this work onto the scoring-policy ADR/PRD surface. The
  verdict stays strictly informational; no scoring-policy ADR is triggered.

- **(g) A flat single-axis verdict enum** (the inherited shape: one list mixing
  `supported` / `partial` / `not_supported` with the abstention case).
  **Rejected.** Flattening the availability and support axes into one list makes
  "a can't-judge outcome is never a support judgment" a convention enforced by
  tests rather than a property of the type. The two-axis `CitationVerdict` /
  `SupportLevel` (Decision 1) makes an abstention structurally unable to masquerade
  as a support level, and matches the two-step pipeline the verification already runs.

- **(h) Identify the claim by sentence segmentation, or let the model choose the claim
  span.** **Rejected.** A per-language sentence segmenter adds a language dependency the
  structural *between-markers* rule (Decision 7) avoids — material as coverage grows
  beyond enwiki (issue #23) — and letting the model pick the span it then judges would
  let it choose an easy-to-support fragment, re-importing the producer-trust hole
  Decision 5 closes. The claim is a deterministic span extraction; the model only
  *judges* it.

## Consequences

The decisions above bind to the following testable invariants. Each maps to a
PRD-0001 Definition-of-Done item.

- **Verdict type is a closed two-axis type with no number** (DoD 1). *Unit test:*
  the verdict type is `CitationVerdict` (`Judged(SupportLevel)` |
  `SourceUnavailable`) with `SupportLevel` = {`Supported`, `Partial`,
  `NotSupported`} and no numeric field; serde
  round-trips each of the four wire values (Art. 9.1 schema, Art. 1.2
  codec-round-trip). *Contract test:* the response surface (ADR-0008) exposes no
  confidence field, and an abstention can never serialize as a support level.

- **The anti-fabrication invariant holds** (DoD 3, the load-bearing one).
  *Property test (`proptest`, Art. 1.2):* for an arbitrary claim and an arbitrary
  source that does **not** contain the model's quote, the surfaced verdict is
  never `Supported`. Plus a focused unit test of `locate_quote`: a quote not
  present → `None`; case-difference → `None`; empty quote → `None`; NFC/NFD and
  curly-quote forms round-trip. The grounding re-check is tested to reject an
  absent quote, a forged offset, and a never-fetched source.

- **Abstention, never a guess** (DoD 4). *Integration test:* against an
  unreachable or unusable source, the verdict is `SourceUnavailable`, never a
  support judgment; the body-usability gate short-circuits with **no model call**.
  The body-usability classifier is tested with one case per unusable pattern plus
  a ReDoS-safety test.

- **No editor identity reaches a verdict.** *Identity-invariance test:* the
  verdict is unchanged when identity metadata is injected alongside the same
  (claim, source). A moved verdict is a failure.

- **Claim identification is deterministic** (Decision 7). *Unit/integration test:* the
  same parsed article yields the same per-use-site claim spans (the rendered prose
  between adjacent citation markers within a block, in document order); a non-prose
  marker (table/infobox) and a use-site whose citation carries no fetchable source URL
  produce no verdict (**filtered out, not guessed**); and bundled markers with no prose
  between them **share** the preceding span (each still its own use-site). The base span
  extraction and the non-prose skip are validated against a recorded Parsoid fixture in
  Luis's wikiharness; the bundled-marker share rule follows alex-citation-checker
  (wikiharness drops it) and gets its own SP42 test.

- **Verification performs no writes** (DoD 5). Covered structurally by Decision 6
  and tested at the contract altitude in ADR-0008 (zero autonomous writes on the
  verification path); recorded here because it is what makes the verdict
  *informational, not an action*.

- **Determinism on replay** (DoD 6) and **observability** (DoD 7) are owned by
  ADR-0009 (source-snapshot storage) and ADR-0008 (the observable surface)
  respectively; this ADR's pure verdict type, locatability primitive, and
  body-usability gate are the deterministic core they depend on — same fetched
  source yields the same verdict category (Art. 2.1). Panel voting and measured
  agreement add their own determinism-on-replay guarantee, owned by ADR-0006.

Cross-cutting:

- **This introduces the first LLM into SP42.** It is confined by construction:
  the model is reached only through the **provider-agnostic `ModelClient` boundary**
  (ADR-0006 Decision 7) against an optional, config-driven model panel (ADR-0006 §4) —
  `sp42-core` depends on the trait, never a concrete client or provider wire format, and
  the concrete adapter (default: OpenAI-compatible over the `HttpClient` edge, mirroring
  the LiftWing `liftwing.rs` / `liftwing_url` precedent generalized from one URL to a
  panel) lives in a shell. So the workspace's "no I/O in core" law (Art. 2.3) and the
  deterministic test-double discipline (Art. 1, a stub `ModelClient`) are unbroken. The
  model is *never* the final authority on `Supported` — the deterministic grounding
  re-check (Decision 5) is. No LLM dependency enters the permissive-licensed dependency
  graph without a `cargo-deny` clearance (Art. 5.2), and no source body or token is
  exported (Art. 10.4 — no telemetry; ADR-0009 governs what a snapshot may contain). The
  model boundary is owned by ADR-0006 (Decision 7); the panel that produces the verdict is
  ADR-0006; the citation crate placement is ADR-0008 (Decision 7).

- **The no-number rule constrains any future routing.** If a verdict is ever
  allowed to influence triage, the gate must be *measured agreement + explicit
  abstention*, never a model-emitted number — and the standalone first cut
  (PRD-0001) keeps the verdict out of scoring entirely.

- **The grounding discipline fails closed on the action axis.** Every failure
  mode — empty quote, missing source, offset mismatch, unfetched source — fails
  *closed* where it matters: the support claim is surfaced as **unverified** and
  is never groundable. The deliberate consequence: SP42 will sometimes decline to
  CONFIRM a real `Supported` rather than ever confirm a fabricated one. For a
  trust-critical capability that is the correct bias — and the measured cost of
  over-firing (SP42#25: ~24% of support votes failed to locate over pure
  transcription noise before the artifact-folding/repair/fuzzy layers) is managed
  by widening what counts as *transcription*, never what counts as *confirmed*.

- **The body-usability pattern set is maintenance.** A curated, ReDoS-safe set of
  unusable-body detectors needs upkeep as the web changes; the cost is accepted in
  exchange for clean abstention attribution.

## Non-Goals

- **No numeric confidence, score, or probability** — anywhere, by structural
  design. The one narrow carve-out — **measured panel agreement** (observed vote
  counts, not a model-reported number) — is owned by ADR-0006.
- **No write path.** This ADR defines a read-only verdict; repairs flow only
  through the existing operator-confirmed action path (ADR-0003, ADR-0008).
- **No scoring coupling** in the first cut, which is standalone (PRD-0001) —
  including no agreement number feeding the composite damage score.
- **No "unclear" / model-indeterminate *verdict tier*.** Model uncertainty folds
  into `NotSupported`; deterministic abstention is reserved for `SourceUnavailable`
  (Decision 1). The genuinely borderline "needs review" case is surfaced as a
  *signal alongside* the verdict — **low measured panel agreement** (ADR-0006) —
  not as a new verdict value.
- **No model-reported confidence.** The surfaced honest signals are the graded
  verdict, explicit abstention, and measured agreement (ADR-0006) — never a number
  a model wrote about its own certainty.
- **No PDF or paywalled source bodies** in the first cut (PRD-0001 *Scope
  decisions*: HTML pages and existing archived snapshots only; PDFs and The Wikipedia
  Library deferred to a follow-up). The body-usability gate and locatability
  primitive are body-format-agnostic, so these extend the input without changing this
  ADR's semantics.
- **No claim *discovery*.** Identifying the claim a citation backs is **in scope** — a
  deterministic span extraction (Decision 7). What stays out of scope is *discovering*
  which uncited statements ought to have a citation, or harvesting claims from
  arbitrary prose not anchored to a citation marker; the first cut judges only claims
  an existing citation already points at.

## Changelog

Per-ADR record of inline changes after merge (the ADR is edited in place; this logs
what changed and when). Reversals still get a new superseding ADR (Constitution §4.1).

- **2026-06-26 — Grounding extended from a single located passage to one or more
  within-source spans ("synthesis grounding").** A claim may be grounded by
  combining multiple verbatim, independently-located passages of the *same* source,
  valid only when the claim is entailed by the union of those spans alone with no
  unstated or outside premise — outside-knowledge inference stays forbidden. The §5
  anti-fabrication invariant is unchanged; it now holds **per-span**. Decision record
  and full rule: issue #66. The substantive prose rewrite of the Decision sections is
  pending discussion with @schiste.
