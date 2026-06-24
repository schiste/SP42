# PRD-0007: LLM output-quality benchmarking

**Author:** Luis Villa (drafted with Claude)
**Date:** 2026-06-09
**State:** Draft
**Discussion:** https://github.com/schiste/SP42/pull/37
**Spawned ADRs:** none yet — the harness's structure (crate placement, the
injected-clients import boundary, the corpus-loader seam to the external data
repo, and where the composition root with real clients and keys lives) is an
ADR decision to spawn before implementation, not a question this PRD answers

## Problem

SP42 is growing a family of LLM-judgment capabilities: citation verification is
implemented (PRD-0001), puffery detection (#30) and weasel-word detection (#31)
are queued, and frwiki extension (#23) is blocked on exactly the question this
PRD answers. For all of them, the maintainer tuning a prompt, swapping a model,
changing a panel, or adjusting a grounding policy has no way to answer the
day-to-day question:

> **Did this change make the outputs better or worse — and on which kinds of
> case?**

The model layer is non-deterministic, so no unit test can catch a quality
regression: a "small wording improvement" to a prompt can silently tank
precision on a cohort (paywalled sources, non-English pairs, short cites) and
CI stays green. The only quality measurements ever made of SP42's citation
verdicts were run this week from ad-hoc scripts in `/tmp`, against a labeled
corpus that lives in a sibling checkout of another project
(`alex-cite-checker`). Those numbers cannot be reproduced by anyone else,
cannot be cited in issues or PRs, and die with the shell session. The repo's
only trace of the entire apparatus is one doc-comment about
`SP42_FETCH_ALLOW_PRIVATE=1`.

This is a solved-in-spirit problem: wikiharness specced and built the most
advanced version of this apparatus (`docs/05-evaluation-and-benchmarking.md`,
ADR-0010 *eval-harness port*, the `packages/evals` runner), itself adapted from
SP42's own deterministic scoring-evaluation harness and alex-cite-checker's
benchmark + `ccs compare`. SP42 should close the loop and port the *design*
back — generalized, because citation verification is only the first customer.

## Proposal

A **task-generic output-quality harness**: an in-repo runner (with metrics and
compare) that runs a labeled corpus — sourced from a separate, version-pinned
data repository — through a real SP42 judgment pipeline and reports measured
quality, with a deterministic replay mode for regression-gating and a
comparison mode for control-vs-treatment decisions. The runner and gate stay in
SP42 (the gate must call SP42's Rust pipeline, even in replay); only the corpus
data lives outside the core repo.

### Concepts (the generality contract)

- A **task** binds: a case-input schema, a pipeline entry point, a fixed
  categorical outcome set, and optional task-specific hard checks (citation
  verification declares a grounding check; a future puffery task may not).
  Citation verification is the first registered task; **registering a second
  task must require no changes to the runner, metrics, or compare contracts.**
- A **labeled case** is: the task input (claim + source for citations; a
  sentence/passage for puffery or weasel-wording), the expected categorical
  outcome (ground truth), optional **cohort tags** (e.g. paywalled,
  non-English, short-cite, PDF), and **provenance**.
- A **corpus** is a validated JSON file of labeled cases living in a separate,
  **public, version-pinned data repository** — not in the core repo. The
  runner reaches it through a single root-path loader seam (the existing
  `--corpus <path>`, elevated from escape-hatch to the standard entry point);
  no other code path hardcodes a corpus location, so relocating the data is a
  config change, not a refactor. Each corpus directory is **self-contained**
  (cases + licensing README + GT corrections map, with no references into the
  rest of the repo), so the whole directory can be lifted out as a unit. The
  data repo is **pinned by commit SHA** for reproducibility (the SHA joins the
  report's reproducibility header). Every text payload carries explicit
  licensing provenance — **CC BY-SA** for Wikipedia-derived text (claims,
  article context; attributed to article + revision), **CC0** for
  Wikidata-derived data, and **fair use** for bounded extractions from cited
  third-party websites (labeled as such, kept to the excerpt needed for
  verification, and individually removable — content-hash ids mean deleting a
  case never disturbs the rest). A tiny synthetic fixture stays **in the core
  repo** to serve the hermetic harness tests (so `cargo test` is offline and
  key-free by construction; see Definition of Done).
- A **run** is corpus × model panel × pinned parameters → per-case, per-model
  outcomes plus the panel vote. A **report** renders a run; a **compare** diffs
  two runs.

### Design spine (carried from wikiharness ADR-0010, kept deliberately)

1. **Gate vs benchmark are distinct deliverables.** A hermetic **gate** replays
   frozen model responses through the real pipeline — deterministic, no keys,
   no network, CI-safe. The **benchmark** runs real models — opt-in, costs
   money, never in CI. Same schema, same metrics, same report.
2. **Purge the accidental-identity anti-patterns.** Case ids are
   **content-hash derived** (stable under insertion — never a row number).
   No version/tranche token. **No model-generated confidence anywhere** — in
   the schema, the pipeline, or the report; the only honest signals are the
   categorical outcome, **measured** panel (dis)agreement, and abstention.
   The corpus loader **rejects** cases carrying banned keys so the purged
   concepts cannot re-enter.
3. **Facet taxonomy.** *Inherent* facets (input provenance) are stored;
   *computed* facets (source type, body usability) are derived at analysis
   time, never stored as truth; *assessed* facets (the GT label) are stored
   **with provenance** (`label_method`, `label_as_of`); *heuristic-assessed*
   (model-labeled) facets may never feed a gate.
4. **GT audit flag, never GT mutation.** The report lists cases where most or
   all of the panel disagrees with ground truth — a re-audit target for a
   human. The harness may never set or change a label (models cannot define
   the truth they are graded against).
5. **Pipeline-attributed vs model-attributed outcomes.** The deterministic
   body-usability gate runs first; an unusable source is recorded as an honest
   pipeline abstention with **zero model calls** — a fetch or extraction
   failure is never scored as a model error.
6. **Two source modes.** *Frozen* (cached source bytes served locally — the
   primary, isolates model quality) and *live* (real fetch spine — adds
   scraper-completeness, opt-in).

### Metrics and report

Per model and per panel: accuracy against GT, per-outcome confusion matrix,
abstention rate, measured agreement. For tasks that declare grounding: the
**grounding-tier rates** (exact-located / fuzzy-located / unlocated), which is
the located-rate measurement used throughout SP42#25. Every report opens with a
**reproducibility header**: corpus content hash, corpus data-repo and commit
SHA, case count, panel and pinned sampling parameters, code version. (This is
what makes a number quotable in an issue: today's numbers are unciteable
precisely because nothing can reproduce them.)

Each run record also captures **cost and latency signals** — model identity,
token counts, model-call count, wall-clock, and estimated cost — reported
alongside the quality metrics but **never blended into the quality verdict**
(these are cost floats, not verdict floats; the no-confidence-on-verdict-paths
rule is unaffected). They make the cost-creep risk observable and enable
constant-quality cost optimization later (see *Compare mode*).

The report carries the ADR-0007 §5 epistemic note: the harness measures verdict
quality against labels and the existence of asserted evidence — it cannot and
does not measure whether a passage *caused* the model's verdict.

### Compare mode (the regression decision)

Ported from alex-cite-checker's `ccs compare`: a two-run control-vs-treatment
diff that classifies every changed cell as **improvement** (wrong→correct),
**regression** (correct→wrong), or **lateral** (wrong→different-wrong);
compares only the **intersection** of cases present in both runs (dropped or
errored cells cannot flatter the delta); and flags aggregate deltas below a
declared noise floor as not-a-signal. The flip taxonomy is the natural answer
to "did my prompt change regress anything."

A compare is trustworthy only under **parity** — control and treatment differ
in exactly one variable. In **replay** mode parity is guaranteed by
determinism, so the baseline is a stored prior replay run and costs nothing to
reuse. In **live** mode, model/API drift breaks parity across time: comparing
against an old live run confounds your change with provider drift, so the
control arm is **co-run in the same batch**. The compare noise floor and pinned
parameters address within-run noise; co-run baseline addresses across-time
drift. Baseline overhead is therefore paid only in live runs, where it is the
sole defense against drift.

### Relationship to constant-quality optimization

A different evaluation style — used for agentic-coding scenarios — optimizes at
*constant quality* and so leans on an LLM comparative judge plus a
quality×cost×duration ratio as the headline rating. SP42's citation eval is the
opposite case, for one root reason: it has **labeled ground truth**. So
comparison is a **deterministic** flip taxonomy against GT (an LLM comparative
judge would reintroduce the non-determinism the gate exists to remove), and the
goal is **improving quality**, so quality is the gate while cost is reported
orthogonally — never traded against precision. The cost/latency signals above
exist to support that constant-quality view when it is wanted, not to fold cost
into the quality verdict.

### No promotion without passing

Measured quality gates are part of the product promise, not optional tooling:
a prompt, model, panel, or policy change that fails a declared hard gate, or
regresses past a task's declared threshold, does not ship. This is a
**mechanism-agnostic invariant** — it holds regardless of *where* the check
runs. The gate/compare decision is produced by an **embeddable verdict
component** returning a structured result (pass / hard-fail /
regression-past-threshold); a thin CLI in CI is the first caller, and an
in-product lifecycle stage is a later caller of the *same* verdict. The check
is therefore callable from **both** the repo/CI path (the maintainer's tuning
loop, today and indefinitely) **and** the product — neither is privileged, and
adding the in-product caller requires no harness change (mirroring the
task-generality contract). The harness's job is to make the check cheap (replay
mode, no keys) and its verdict unambiguous (the flip taxonomy). The full
in-product rule-authoring lifecycle (create / edit / evaluate / validate in the
UI) is a user-facing workflow and belongs to a **successor PRD**; this PRD
commits only to not precluding it. *How* the block is wired into each caller —
CI, hooks, release checklist, in-app stage — is implementation sequencing
tracked outside this PRD, but at least the repo/CI wiring must exist for the
promise to be claimable.

### Future corpora this must already fit

- **Puffery (#30) and weasel words (#31):** single-passage classification
  tasks; corpus sourced by distant supervision from Wikipedia's own inline
  cleanup templates (`{{Peacock term}}`, `{{Weasel inline}}`) plus human
  audit. No grounding check; same runner, metrics, compare.
- **frwiki (#23):** language is a cohort tag; a French corpus targets this
  schema natively rather than inheriting the alex format.
- **Existing corpus:** a one-shot importer maps the alex-cite-checker corpus
  (189 rows) into this schema — applying the wikiharness re-audit learnings
  (content-hash ids, GT corrections map, provenance fields) and attaching the
  licensing labels (CC BY-SA claims, fair-use source extracts) — producing the
  first committed corpus.

## Definition of Done

- [ ] A corpus loader validates on load: duplicate ids, unknown outcome
      values, missing **license/attribution labels** (each text payload must
      declare CC BY-SA, CC0, or fair-use provenance), and **banned keys**
      (`confidence`, `tranche`, `dataset_version`, positional ids) are each
      rejected — verified by schema unit tests.
- [ ] Case ids are content-hash derived and stable under corpus reordering and
      insertion — verified by a property test.
- [ ] A run produces per-model and per-panel accuracy, per-outcome confusion,
      abstention rate, and measured agreement, with model clients and the
      source fetch **injected** — verified hermetically with scripted doubles.
- [ ] For a task declaring grounding, the report includes grounding-tier rates
      (exact / fuzzy / unlocated), and a passage reported as exact-located is
      machine-re-checkable in the case's source bytes — verified by a runner
      test plus the existing locate property tests.
- [ ] An unusable source yields a pipeline-attributed abstention with zero
      model invocations — verified by a runner test asserting no client call.
- [ ] **Replay mode runs a full corpus with no network and no API keys** from
      frozen responses — verified by a hermetic test over the committed
      synthetic fixture (CI-safe by construction).
- [ ] Compare mode classifies flips as improvement / regression / lateral over
      the intersection of two runs and applies a noise floor — verified by
      pure-function unit tests including the dropped-case and below-floor
      cases.
- [ ] The panel-vs-GT disagreement list is emitted as an audit artifact, and
      no code path writes to a corpus file — verified by test + the loader
      being read-only by construction.
- [ ] **Generality:** a second toy task (trivial classifier, synthetic
      fixture) registers and runs through the same runner, metrics, and
      compare with no harness changes — verified by an integration test kept
      permanently as the generality guard.
- [ ] No floating-point confidence value appears in the case schema, run
      record, or report — verified by a structural test (consistent with the
      house no-float-on-verdict-paths rule).
- [ ] Every report carries the reproducibility header (corpus hash, corpus
      data-repo + commit SHA, panel, parameter fingerprint, code version) —
      verified by a report unit test.
- [ ] Each run record captures cost/latency signals (model, token counts,
      model-call count, wall-clock, estimated cost), present in the report and
      **absent from the quality verdict and the gate inputs** — verified by a
      report test plus a structural test that the verdict path reads no cost
      field.
- [ ] **No promotion without passing:** the gate/compare decision is an
      embeddable component returning a **structured verdict** (pass / hard-fail
      / regression-past-threshold) that any caller — CI or in-product — can
      block on; a nonzero CLI exit is one rendering of that verdict. Verified
      by gate/compare unit tests covering the pass, hard-fail, and regression
      cases, asserting on the structured verdict, not only the exit code.

## Alternatives

- **Status quo (ad-hoc `/tmp` scripts).** Free until it isn't: results are
  irreproducible, unciteable, and rebuilt from memory each session. This week
  required reconstructing binary paths and env contracts by archaeology.
- **Reuse wikiharness's `packages/evals` as a sidecar.** The design is right
  but the runtime is TypeScript; SP42 would take a second toolchain and test
  harness, and the runner must call SP42's *Rust* pipeline anyway. Port the
  design, not the code.
- **A bespoke harness per task.** This is the citation-only version of the
  status quo with better hygiene; #30/#31 would each rebuild metrics, compare,
  and governance. The marginal cost of task-genericity is one trait boundary.
- **Commit corpora inside the core repo.** Considered (it is the simplest path
  to a CI-gated, reproducible corpus), not chosen: it couples the evaluation
  data to the core repo's history exactly when a standing goal is to let
  evaluation be managed *outside* the core (and contributed without git). The
  two original objections to externalizing — irreproducibility and keeping the
  gate out of CI — are answered rather than dodged: the data repo is **public
  and pinned by commit SHA** (so any number is reproducible by anyone, and the
  SHA is in the report header), and CI fetches the pinned corpus to run the
  replay gate while the always-on hermetic tests use the in-repo synthetic
  fixture. The licensing concern is handled the same way regardless of host —
  each text payload is labeled CC BY-SA (Wikipedia), CC0 (Wikidata), or fair
  use (bounded website extractions), and any case is individually removable;
  putting the data in its own repo additionally makes wholesale removal trivial
  (rewrite one repo, never the core history).

## Risks

- **Bad ground truth corrupts every number.** Mitigation: label provenance is
  mandatory, the corrections applied at import are recorded, and the
  panel-vs-GT audit flag continuously surfaces suspect labels for human
  re-audit (never auto-correction).
- **Overfitting to a 189-case corpus.** A small set rewards tuning to its
  quirks. Mitigation: cohort tags make per-cohort regressions visible;
  corpus growth (frwiki #23, distant-supervision sets for #30/#31) is part of
  the design; named external benchmarks remain available as later additions.
- **Misreading the numbers as model faithfulness.** The harness grades
  outcomes and evidence existence, not reasoning (ADR-0007 §5). Mitigation:
  the epistemic note is part of the report template, not just documentation.
- **Noise mistaken for signal.** Panel models are non-deterministic even at
  pinned parameters. Mitigation: the compare noise floor, pinned sampling
  parameters in the reproducibility header, and replay mode for the gate.
- **Cost creep.** Real-model runs cost money and invite casual re-running.
  Mitigation: replay is the default mode; live-model runs require explicit
  opt-in plus keys, and never run in CI; and per-run cost/latency capture makes
  spend observable rather than invisible.
- **A fair-use claim is a judgment, not a license.** Committed website
  extractions rest on a fair-use rationale that could be challenged.
  Mitigation: extracts are bounded to what verification needs, every payload
  is labeled with its basis and origin URL, a corpus README states the
  posture, and content-hash ids make any case deletable on request without
  renumbering or breaking the rest of the corpus.

## Open questions

1. **Which gates are hard?** Proposed: grounding integrity (an exact-located
   passage must re-locate — machine-checkable) is hard; accuracy/regression
   thresholds are declared per task; identity-invariance is deferred until an
   SP42 task injects editor identity into a prompt (none does today).
2. **CI wiring sequence** — *implementation tracking, not a design question;
   convert to a tracked issue and link it at acceptance.* Proposed sequencing
   for that issue: the hermetic fixture tests are ordinary `cargo test` from
   day one; the corpus-replay gate joins `ci-all.sh` (cf.
   `check-scoring-governance.sh`) once the first frozen capture is stable
   enough to gate on, fetching the SHA-pinned corpus repo. The *promise* this
   wiring serves is already fixed in Proposal ("No promotion without passing")
   and the Definition of Done.

Resolved:

- **Corpus hosting and layout.** The corpus lives in a separate, **public,
  SHA-pinned data repository**, reached through the single root-path loader
  seam; each corpus directory is self-contained, and a tiny synthetic fixture
  stays in the core repo for hermetic tests. The first corpus is produced by
  the alex importer with per-payload licensing labels (CC BY-SA / CC0 / fair
  use). The only residue, settled at import time, is whether CC BY-SA payloads
  need finer attribution than a single label (e.g. revision-level strings).
- **Promotion enforcement venue.** The gate is an embeddable verdict callable
  from both repo/CI and product; the in-product rule-authoring lifecycle is a
  successor PRD this PRD does not preclude (see *No promotion without passing*).
- **Where does the harness live?** Not a PRD question — structural decision
  (crate placement, import boundary, composition root) deferred to the spawned
  ADR; see the *Spawned ADRs* header. The PRD retains only the requirements
  the structure must satisfy (injected clients; replay without keys), which
  are in the Definition of Done.
- **On-wiki outcome measurement** (acceptance, reversion durability vs human
  baselines — wikiharness `05`'s external evidence harness): out of scope for
  this PRD (owner decision, 2026-06-09); a successor PRD once SP42 performs
  live actions at volume.
