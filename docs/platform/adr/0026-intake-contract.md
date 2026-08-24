# ADR-0026: Intake contract — data-driven event filtering, composable rule trees, wiki-relative capability resolution

**Status:** Proposed
**Date:** 2026-08-24
**Author:** Christophe Henner (drafted by Claude Code)
**Summary:** Every workflow needs to narrow a raw stream or a composed query of wiki content down to what it should even consider before any gate or scoring logic runs; intake is that platform mechanism — always loaded with the application behind a safe broad baseline, a data-driven and composable rule engine over a generic item envelope fed by either a live stream or an on-demand query, parameterized per wiki through a confirmed capability profile, that fails closed and loud on misconfiguration.

## Context

Patrolling's live queue filtering (`sp42-live`) currently does this narrowing inline, coupled to patrol's own rules: which namespace, which event type, which actor rights bypass review. As SP42 grows toward a composable workbench — multiple workflows (NPP-equivalent article review, AfC-equivalent draft review, CCI-equivalent contribution investigation, redirect review) each with their own gates — every one of them needs the same shape of decision: *is this item even a candidate for this workflow's queue*. Reimplementing that per domain, the way `sp42-live` does it today, fails the reuse-by-design test (ADR-0013 / `docs/process/adding-a-domain.md`): a second, third, and fourth domain would each rebuild the same filter-pipeline shape against their own hardcoded rules.

The same need shows up outside continuous review too: the tooling survey behind this design (`User:Dreamyshade/Article_workflows`) is full of one-off "find" tools — PetScan-style category/template composition, SDZeroBot sorting pages, custom SQL reports — that answer questions like "every enwiki article lacking sources, unedited for 20 months." That is the same candidate-narrowing problem on a bounded, on-demand query instead of a live stream, and it should not require a second mechanism.

The mechanism also has to be genuinely adaptable across wikis without code changes — different wikis have different namespace numbering, different right names for equivalent permissions, and domains will need to filter on data the generic event envelope doesn't carry (a content model, a tag list, a contribution count). And because a bug here doesn't crash anything — it just silently drops content out of a review queue — the failure behavior has to be as carefully specified as the happy path.

## Decision

### 1. Intake is a platform mechanism, not a domain
The event envelope, the rule-evaluation engine, and the capability-resolution contract live in `sp42-platform` (engine) and `sp42-types` (contracts), on the same footing as `scoring_engine` and `queue_builder`. Per-wiki and per-workflow *configuration* — which pipelines exist, what they check — is data (YAML under `configs/`, schema in `schemas/`), not domain code, following the scoring-policy precedent (ADR-0021 §2).

### 2. Intake is always loaded, not opt-in — a safe, broad baseline ships in code
Every shell wires intake at boot (`sp42-server`, `sp42-cli`, `sp42-app`, `sp42-desktop`), the way `sp42-live` ingestion starts today — it is not a capability that only exists once a workflow configuration summons it. It ships with an embedded default pipeline that is deliberately broad and restricted to *intrinsic* facts (event type, bot flag) that need no wiki-specific resolution — nothing namespace-role-based, since which namespace plays "mainspace" is exactly the kind of thing §5 requires a confirmed capability profile for, and no such profile exists for a wiki nobody has registered yet. Once a wiki is registered and its profile confirmed, sharper wiki-relative pipelines layer on top and narrow the always-on stream further; they never replace it, and workflows/gates never gate whether intake itself runs. This mirrors the "safe fallback in code, tuning in policy" split Constitution §14.3 and ADR-0021 §2 already establish for scoring.

### 3. A generic item envelope, fed by two source modes: continuous stream and composed query
```
IntakeItem { wiki_id, event_type, namespace, page_id, revision_id, created_at: Option<Timestamp>, last_revision_at: Option<Timestamp>, actor, observed_at, payload_ref }
IntakeActor { username: Option<String>, rights: Vec<String>, is_bot: bool, account_created_at: Option<Timestamp> }
```
`observed_at` is when *this event* happened (an edit's timestamp, not necessarily the page's); `created_at` is the subject page's own creation fact, populated by the adapter when known — always for a `Create` event (the two coincide), and always for the `Query` source below, since it fetched the page directly. Conflating the two would make "page age" and "time since this edit" indistinguishable. `last_revision_at` is the subject page's most recent revision timestamp as of the adapter's fetch — always known for `Query` items (fetched directly) and for any `Stream` adapter that polls page state rather than only forwarding an edit's own timestamp; a plain edit-triggered stream event typically has it coincide with `observed_at`, and an adapter that hasn't fetched page history may leave it `None` rather than guess. §4 defines how an absent value here (or in `created_at`) is evaluated, rather than defaulting it silently.

Two kinds of source adapter normalize raw content into this shape:
- **Stream** (default, continuous): EventStreams, backlog polling, a pull-query draft queue, a contribution-history walk. `sp42-live`'s current inline filtering becomes one such adapter instead of doing its own filtering; this ADR authorizes that migration but does not itself perform it (see Non-goals).
- **Query** (on-demand, bounded): a composed spec answering "find X" on request rather than subscribing to a feed — the PetScan/SDZeroBot-style case from Context.
```
IntakeQuerySpec {
    wiki_id, namespace: Option<Vec<i32>>,
    category_all_of: Vec<String>,       // intersection
    has_template: Vec<String>, lacks_template: Vec<String>,
    unedited_since: Option<Duration>,   // resolved against Clock, not wall-clock
    limit: NonZeroU32,                  // bounded and paginated — never an unbounded scan
}
```
A query source fetches through the same guarded `sp42-fetch` edge as capability discovery (§5), returning a bounded, paginated batch of `IntakeItem`s. Both source kinds feed the same downstream pipeline (§4–§6) — the query spec narrows *what's fetched*, the condition tree still narrows *what's admitted*, for criteria the query API can't express natively (a citation-count check needs fetched content, not just category membership).

### 4. Policy is composed, not flat — a boolean condition tree over a typed, extensible field vocabulary
```
IntakeCondition = All(Vec<IntakeCondition>) | Any(Vec<IntakeCondition>) | Not(Box<IntakeCondition>) | Rule(IntakeRule)
IntakeRule { field: IntakeField, op: IntakeOp, value: IntakeValue }
IntakeField  = EventType | Namespace | PageId | RevisionId | CreatedAt | LastRevisionAt
             | ActorUsername | ActorRights | ActorIsBot | ActorAccountCreatedAt
             | ObservedAt | Custom(String)
IntakeOp     = Eq | NotEq | In | NotIn | Has | NotHas | Before | After | OlderThan | YoungerThan
IntakeValue  = Str | Int | StrList | IntList | Bool | Timestamp | Duration | CapabilityRef(String)
```
`All`/`Any`/`Not` compose arbitrarily (real policy needs OR from day one — e.g. a quickfail-equivalent ruleset is inherently a disjunction of independent criteria; an AND-only rule list would force a breaking format change on the first real pipeline). `IntakeField` is a closed, typed enum so configs can be mechanically linted against real fields (§7), with `Custom(String)` as the deliberate escape hatch for **local, wiki- or domain-specific data items** the generic envelope doesn't carry — a content model, a tag list, an edit count in a namespace — resolved through a small per-domain field-resolver registry rather than requiring a platform change for every new filterable fact.

`Before`/`After` compare a field against an absolute `IntakeValue::Timestamp`; `OlderThan`/`YoungerThan` compare it against `Clock::now() ± Duration` — the same non-wall-clock discipline the query spec's `unedited_since` already uses, extended so it applies to any timestamp field, not just a query-time fetch parameter. This is what makes "account younger than 7 days" (`ActorAccountCreatedAt YoungerThan 7d`), "page not touched in 20 months" (`LastRevisionAt OlderThan 20mo`, expressible in stream mode too, not only as a query-fetch optimization), and a review workflow's cooldown window (`CreatedAt YoungerThan 1h`, evaluated at gate-invocation altitude per §8) all the same rule shape — none of them need a bespoke mechanism.

A `Rule` against an `Option`-typed field (`CreatedAt`, `LastRevisionAt`,
`ActorAccountCreatedAt`) evaluates three-valued — `True | False | Unknown`
— not boolean: an absent value is `Unknown`, never coerced to `False`.
`All`/`Any`/`Not` compose with standard Kleene semantics: `All` is `False`
if any child is `False`, else `Unknown` if any child is `Unknown`, else
`True`; `Any` is `True` if any child is `True`, else `Unknown` if any child
is `Unknown`, else `False`; `Not` maps `True↔False` and leaves `Unknown` as
`Unknown` — negating an unknown never manufactures a `True`. A pipeline's
root condition (§6) routes on this three-valued result as `True → on_pass`,
`False | Unknown → on_fail`: an absent field can never itself cause an
admission, closing the exact failure mode where `Not(CreatedAt YoungerThan
1h)` against a missing `created_at` would otherwise read as a confirmed
old-enough page rather than an unknown one.

### 5. Wiki-relative values resolve through a two-tier capability profile
`IntakeValue::CapabilityRef(String)` lets a rule say "the mainspace namespaces for this wiki" instead of a literal namespace number, so one pipeline definition works across wikis with different conventions. Capability profiles split into:
- **Discovered facts** (Tier A): namespace map, available rights, content models — fetched from each wiki's own `action=query&meta=siteinfo` through the already-hardened `sp42-fetch` edge, cached with a TTL, refreshed as a background job (never inline in the intake hot path), diffed against the last known-good profile with drift surfaced rather than silently applied.
- **Confirmed policy meaning** (Tier B): which discovered right counts as "skip human review" for intake purposes, which namespace plays the "mainspace" role for a given workflow. These are judgments, not facts — pattern-matched proposals may be suggested, but activation follows the operator-confirmed propose/confirm pattern already established by ADR-0010: nothing is trusted for filtering until a human confirms it, and any Tier A drift that would change a Tier B mapping re-requires confirmation rather than auto-updating.

Registering a wiki the platform doesn't yet know is always an explicit administrative action in `sp42-wiki`'s registry — intake never auto-onboards a wiki merely because an event referenced it.

### 6. Every decision is a routed, provenanced outcome — and misconfiguration is never silent
```
IntakePipeline { id, condition: IntakeCondition, on_pass: IntakeOutcome, on_fail: IntakeOutcome }
IntakeOutcome  = Admit { route_to: String } | Drop | Reclassify { pipeline: String }
IntakeDecision = { outcome, pipeline_id, matched_rule_path, config_version } | Misconfigured { pipeline_id, unresolved_ref }
```
A `CapabilityRef` that cannot resolve is checked twice: at config **load time**, every reference is resolved against every wiki profile the config could apply to, and an unresolvable config is rejected outright — it never goes live. If a Tier A drift later breaks a previously-valid reference, evaluation returns `Misconfigured`, a state distinct from `Drop`, so a broken filter is never indistinguishable from an intentional one. An `Unknown` three-valued result (§4) is a different situation from `Misconfigured`: the config is valid and every reference resolves, the *data* simply doesn't carry that field for this item — it routes through `on_fail` like an ordinary non-match, never through `Misconfigured`, which is reserved for a config-resolution failure.

### 7. Configs are linted mechanically, the same way layering is
A CI-time check (an `xtask` subcommand run from `ci-all.sh`, alongside `scripts/check-layering.sh` and `scripts/check-scoring-governance.sh`) validates every config in `configs/`: schema conformance, every `CapabilityRef` resolves against the profiles it applies to, every `Custom(String)` field is registered by some domain, every `route_to` and `Reclassify.pipeline` target resolves to a real queue/workflow or pipeline id, the directed graph formed by all `Reclassify` edges across the config set contains no cycle, and no branch of a condition tree is statically unreachable. A `Reclassify.pipeline` that doesn't resolve, or that closes a cycle (including a direct self-reference), is rejected at load/CI time exactly like an unresolvable `CapabilityRef` — never left to loop or fail at runtime.

### 8. The same pipeline mechanism serves two altitudes
A workflow's top-level trigger ("is this page even a candidate for review at all") and a gate's own scoped narrowing ("of this contributor's history, which edits are in scope for this check") are the same `IntakePipeline` mechanism invoked at different points — not two separate systems. Workflow definitions and gates both reference pipelines by id.

## Alternatives

- **Keep filtering inline per source adapter** (status quo). Rejected: fails reuse-by-design the moment a second workflow (AfC-equivalent, CCI-equivalent) needs its own candidacy filter — each would silently reimplement this mechanism against its own hardcoded rules, the same coupling `scoring_engine`'s `EditEvent` input already illustrates as a cost.
- **Make intake opt-in, only active once a workflow config summons it.** Rejected: leaves no baseline signal before any wiki is registered or any workflow authored, and gives the ad hoc topic-research query case (§3) no home in the architecture — it would have to become a separate, duplicate mechanism instead of reusing the same rule/provenance/fail-closed machinery.
- **A separate one-off "query tool" outside intake, for the PetScan-style find case.** Rejected: it is the same candidate-narrowing problem on a bounded source instead of a live one; building it as its own mechanism duplicates §4–§7 for no reason. Modeling it as a second `IntakeSource` kind keeps one rule engine, one provenance model, one misconfiguration story.
- **Flat AND-only rule lists.** Rejected: the first real policy this exists to express is inherently a disjunction (independent quickfail-style criteria), so AND-only would force a breaking config migration almost immediately rather than composing cleanly from the start.
- **Equality/membership operators only, no temporal ordering.** Rejected: age-based criteria (account age, page age, staleness thresholds, review cooldown windows) recur across nearly every real policy this is meant to express and are not expressible as equality or set membership; adding `Before`/`After`/`OlderThan`/`YoungerThan` from the start avoids the same kind of breaking migration the AND-only case above would cause.
- **Free-form string dot-paths instead of a typed `IntakeField` enum.** Rejected: loses the ability to mechanically lint a config against real fields (§7), which is the main safety lever against silent misconfiguration; the `Custom(String)` variant already gives domains an extension path without paying that cost everywhere.
- **Auto-onboard a wiki from an observed event referencing it.** Rejected: turns "send intake traffic mentioning wiki X" into an implicit registration mechanism — an availability/scope-creep risk, not a filtering-correctness one, but avoidable by keeping registration an explicit `sp42-wiki` action.
- **Trust dynamically discovered wiki facts directly as filtering policy.** Rejected: conflates "this wiki has a right named X" (a fact) with "right X means skip human review" (a policy judgment with real consequences if wrong); the Tier A/B split with confirm-on-drift avoids silently promoting a fact into a policy decision.
- **Treat an absent optional field as an ordinary `False` comparison.** Rejected: under `Not`, this silently flips "unknown" into "confirmed clear" — `Not(CreatedAt YoungerThan 1h)` against a missing `created_at` would read as a confirmed old-enough page rather than an unknown one, quietly admitting exactly the item a restrictive rule meant to hold back. Three-valued evaluation (§4) keeps unknown-ness from masquerading as a checked fact.
- **Validate `route_to` targets only, leave `Reclassify.pipeline` unchecked.** Rejected: a typo'd or self-referential/cyclic reclassify target can loop the engine indefinitely on every matching item, with no load-time signal that the config is broken. Validating it exactly like `CapabilityRef` (§7) closes that gap without a second linting mechanism.

## Consequences

- Makes it possible for every future workflow to reuse one filtering mechanism instead of rebuilding candidacy logic per domain, and for a policy change (a new namespace, a right rename, a new quickfail criterion) to be a config edit rather than a Rust change and redeploy.
- Makes ad hoc topic research (the PetScan/SDZeroBot-style "find" case) a first-class, reusable capability instead of a one-off external script — the same rule engine and provenance model apply whether the working set came from a live stream or a composed query.
- Requires building: the `sp42-types` contracts (§3–§4), the `sp42-platform` evaluation engine (§4, §6), the embedded default baseline pipeline (§2), the query-source fetch/pagination path (§3), the capability-profile discovery job and confirm-on-drift flow (§5, needs a review surface that doesn't exist yet), and the config linter (§7) — none of this exists today.
- Requires wiring intake startup into every shell's boot sequence (§2) and migrating `sp42-live`'s inline filtering to a `Stream` adapter (behavior-preserving, but a real refactor) before patrolling itself benefits; until that lands, intake exists as a platform mechanism with no production consumer, the same "mechanism ahead of adoption" state ADR-0021 already documents for scoring's `ScoringSignal` catalogue.
- Query-mode fetches need pagination and a hard result-count bound enforced at the source, not just the pipeline — a maintenance category can have tens of thousands of members, and the guarded `sp42-fetch` edge caps size/redirects but not result-set cardinality on its own.
- Will be pinned by: unit tests on the condition-tree evaluator, including its three-valued `Unknown` propagation through `All`/`Any`/`Not` (§4) — mirroring `scoring_engine`'s tests — fixture-driven regression tests per wiki (mirroring `evals/scoring/fixtures/`), and the CI linter in §7 failing the build on any invalid config, including an unresolved `Reclassify.pipeline` target or a reclassify-graph cycle — none of these exist yet and are tracked as follow-up implementation work, not covered by this ADR.
- Forecloses silently defaulting an unresolvable wiki-relative value to a permissive guess; every misconfiguration path is either a rejected config (load time) or a distinguishable `Misconfigured` decision (eval time), never an ordinary `Drop`.

## Non-goals

- The workflow engine, gate-type marketplace, and per-process workflow definitions (e.g. an `npp.yaml`-equivalent) that would call into intake pipelines — a separate, forward-looking ADR once that design is ready to commit.
- The actual `sp42-live` adapter migration and any other source-adapter implementation — authorized here, implemented separately.
- Any user-facing surface for composing or running a §3 query (a "topic research" UI/CLI command) — this ADR fixes that `IntakeQuerySpec` is a supported source, not how a person builds one.
- The reviewer-facing UI/flow for confirming Tier B capability-profile mappings — needs its own PRD; this ADR only fixes that confirmation must happen, not how.
- Concrete per-wiki policy content (which right is enwiki's autopatrol-equivalent, which namespaces are mainspace on a given project) — that is config data, not an architectural decision.
