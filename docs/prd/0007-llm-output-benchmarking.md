# PRD-0007: LLM output-quality benchmarking

**Author:** Luis Villa (drafted with Claude)
**Date:** 2026-06-09
**State:** Draft
**Discussion:** <PR link>
**Spawned ADRs:** none yet

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

A **task-generic output-quality harness**: an in-repo mechanism that runs a
labeled corpus through a real SP42 judgment pipeline and reports measured
quality, with a deterministic replay mode for regression-gating and a
comparison mode for control-vs-treatment decisions.

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
- A **corpus** is a validated JSON file of labeled cases, **committed to the
  repo** with explicit licensing provenance: every text payload carries its
  license and attribution — **CC BY-SA** for Wikipedia-derived text (claims,
  article context; attributed to article + revision), **CC0** for
  Wikidata-derived data, and **fair use** for bounded extractions from cited
  third-party websites (labeled as such, kept to the excerpt needed for
  verification, and individually removable — content-hash ids mean deleting a
  case never disturbs the rest). The harness also accepts an external
  `--corpus <path>` for private or in-progress corpora. A tiny synthetic
  fixture serves the hermetic harness tests.
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
**reproducibility header**: corpus content hash, case count, panel and pinned
sampling parameters, code version. (This is what makes a number quotable in an
issue: today's numbers are unciteable precisely because nothing can reproduce
them.)

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
- [ ] Every report carries the reproducibility header (corpus hash, panel,
      parameter fingerprint, code version) — verified by a report unit test.

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
- **Keep corpora outside the repo.** Considered (the alex corpus contains
  scraped third-party text), rejected: external corpora make every published
  number irreproducible by anyone else and keep the corpus-replay gate out of
  CI permanently. The licensing concern is handled head-on instead — each text
  payload is labeled CC BY-SA (Wikipedia), CC0 (Wikidata), or fair use
  (bounded website extractions), and any case is individually removable
  without disturbing the rest.

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
  opt-in plus keys, and never run in CI.
- **A fair-use claim is a judgment, not a license.** Committed website
  extractions rest on a fair-use rationale that could be challenged.
  Mitigation: extracts are bounded to what verification needs, every payload
  is labeled with its basis and origin URL, a corpus README states the
  posture, and content-hash ids make any case deletable on request without
  renumbering or breaking the rest of the corpus.

## Open questions

1. **Where does it live?** Proposed: pure runner/metrics/compare in a
   dedicated evals crate (or module) with the composition root (real clients,
   keys, corpus path) in `sp42-devtools`/`xtask` — mirroring the wikiharness
   import boundary (runner takes injected clients; no live edge in the eval
   package). Structural decision → spawn an ADR.
2. **When does the gate join CI?** Proposed: the hermetic fixture tests are
   ordinary `cargo test` from day one; wiring a corpus-replay gate into
   `ci-all.sh` (cf. `check-scoring-governance.sh`) waits until the first
   frozen capture is stable enough to gate on.
3. **Corpus layout and licensing presentation.** Proposed: committed corpora
   under a dedicated `corpora/` directory with a README stating the licensing
   posture (CC BY-SA / CC0 / fair use, per-payload labels), produced initially
   by the alex importer; `--corpus <path>` remains for private or in-progress
   corpora. Whether per-payload labels need finer granularity than the
   three-way split (e.g. revision-level attribution strings for CC BY-SA) is
   settled at import time.
4. **Which gates are hard?** Proposed: grounding integrity (an exact-located
   passage must re-locate — machine-checkable) is hard; accuracy/regression
   thresholds are declared per task; identity-invariance is deferred until an
   SP42 task injects editor identity into a prompt (none does today).
5. **On-wiki outcome measurement** (acceptance, reversion durability vs human
   baselines — wikiharness `05`'s external evidence harness). Proposed: out of
   scope for this PRD; a successor PRD once SP42 performs live actions at
   volume.
