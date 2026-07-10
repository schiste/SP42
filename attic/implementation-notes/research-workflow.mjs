export const meta = {
  name: 'citation-impl-research',
  description: 'Map SP42 edge/storage/cli patterns + wikiharness citation algorithms for the Rust port',
  phases: [{ title: 'Research', detail: '6 parallel read-only mappers, each writes a notes file' }],
}

const SP42 = '/var/home/louie/Projects/Volunteering-Consulting/SP42-impl-citation'
const WH = '/var/home/louie/Projects/Volunteering-Consulting/wikiharness'
const NOTES = SP42 + '/docs/implementation-notes/research'

const GUARD = [
  '',
  'HARD RULES:',
  '- You are READ-ONLY on all source code. The ONLY file you may WRITE is your single notes file (path given below).',
  '- NEVER run: git stash, git clean, git checkout, git reset, git restore, or any mutating git command. Untracked deliverables in these trees MUST survive.',
  '- Do not run builds or tests. Just read source and write your notes file.',
  '- Be concrete: include exact type/function signatures, exact regex/normalization rules, exact prompt text, exact HTTP shapes. This is for a faithful Rust port; vagueness is useless.',
].join('\n')

phase('Research')

const tasks = [
  {
    label: 'sp42-edge-liftwing',
    notes: NOTES + '/01-sp42-edge-pattern.md',
    prompt: [
      'Map SP42 external-service "edge" pattern — the template I will mirror for a citation-verify model-call edge.',
      'Read these files in ' + SP42 + ':',
      '- crates/sp42-core/src/liftwing.rs  (THE template: build_*/execute_* generic over HttpClient / parse_* split + its StubHttpClient unit tests)',
      '- crates/sp42-core/src/errors.rs    (LiftWingError + the thiserror style used for domain errors)',
      '- crates/sp42-core/src/types.rs     (find WikiConfig and how liftwing_url Option<Url> is declared/used)',
      '',
      'Document, with EXACT code excerpts and signatures:',
      '1. The exact build_/execute_/parse_ function signatures and how they compose (generic bound C: HttpClient + ?Sized, &WikiConfig usage, the HttpRequest construction: method, url, headers, body).',
      '2. How the response is parsed and how a parse failure is handled (is there a validate_* gate / default?).',
      '3. The full thiserror enum style (struct variants, error attributes, from, static-str reasons) — quote LiftWingError verbatim.',
      '4. The StubHttpClient unit-test pattern in liftwing cfg(test) (how StubHttpClient::new is seeded, block_on usage, assertions). Quote one full test.',
      '5. How liftwing_url is shaped on WikiConfig (Option<Url>, default-absent) and where the endpoint URL/headers/bearer token come from.',
      'This is the blueprint for build_citation_verify_request / execute_citation_verify over HttpClient / parse_citation_verify_response (ADR-0008 Decision 3).',
    ].join('\n'),
  },
  {
    label: 'sp42-storage-traits',
    notes: NOTES + '/02-sp42-storage-traits.md',
    prompt: [
      'Map SP42 storage + platform-trait patterns I will mirror for source-snapshot + verdict-record storage (ADR-0009) and the injected edges.',
      'Read in ' + SP42 + ':',
      '- crates/sp42-types/src/traits.rs   (HttpClient, Storage, Clock, Rng, any WebSocket trait + the test doubles StubHttpClient, MemoryStorage, FileStorage, FixedClock)',
      '- crates/sp42-types/src/lib.rs      (what is exported; module layout)',
      '- crates/sp42-types/src/*.rs        (HttpRequest/HttpResponse transport DTOs, transport/storage error enums)',
      '- crates/sp42-core/src/wiki_storage.rs  (WikiStoragePayloadEnvelope versioned-envelope pattern: build/parse split + injected Storage, version u32, content addressing)',
      '',
      'Document, with EXACT signatures:',
      '1. The full HttpClient trait (method names, async_trait?, request/response types, error type).',
      '2. The full Storage trait (get/put/list signatures, key type, value type bytes?, error type) + how MemoryStorage and FileStorage implement it.',
      '3. The Clock trait (now_ms) + FixedClock double.',
      '4. HttpRequest / HttpResponse struct fields (method, url, headers, body, status).',
      '5. The WikiStoragePayloadEnvelope pattern: how a versioned serde envelope is defined, how content-hash (sha2 / Sha256) is computed + used as a key, the build/parse split, how Storage is injected. Quote the envelope struct + one round-trip test.',
      'This is the blueprint for SnapshotEnvelope / VerdictEnvelope + the Storage-backed snapshot store.',
    ].join('\n'),
  },
  {
    label: 'sp42-lib-build-test',
    notes: NOTES + '/03-sp42-lib-build-test.md',
    prompt: [
      'Map how sp42-core is organized, how to register a new module, how tests run, and the lint bar — so my new code compiles clean.',
      'Read in ' + SP42 + ':',
      '- crates/sp42-core/src/lib.rs            (module declarations + re-exports)',
      '- crates/sp42-core/src/types.rs          (WikiConfig FULL definition — every field, so I know how to add a model-panel/endpoint config field)',
      '- crates/sp42-core/src/action_contracts.rs  (SessionActionExecutionRequest, SessionActionKind — read/write separation; I will NOT modify these)',
      '- crates/sp42-core/src/routes.rs         (route path constants — the read/write lane)',
      '- crates/sp42-core/Cargo.toml            (deps available to sp42-core)',
      'Also locate + read the test runner: an xtask crate (crates/xtask or xtask/), .github/workflows/*.yml, and any Cargo feature named integration (grep the workspace).',
      '',
      'Document:',
      '1. How modules are declared/exported in lib.rs — exactly how I add e.g. a citation module with submodules.',
      '2. The FULL WikiConfig struct (all fields) + how an Option<Url> endpoint field is added + serde defaults.',
      '3. The exact command(s) to run sp42-core unit tests and any integration-feature tests (cargo test -p sp42-core? via xtask? what does CI run?).',
      '4. Deps already available to sp42-core (so I avoid adding new ones).',
      '5. Clippy pedantic gotchas visible in existing code (must_use, missing_errors_doc, etc.) — how existing code satisfies pedantic=deny.',
      '6. Confirm where ADR-0008 section 5 says CitationFinding would attach (crates/sp42-reporting/src/live_operator_view.rs) — note its shape; first cut is CLI so display may be deferred.',
    ].join('\n'),
  },
  {
    label: 'sp42-cli',
    notes: NOTES + '/04-sp42-cli.md',
    prompt: [
      'Map sp42-cli so I can add a read-only verify citation subcommand with human / JSON / verdict-only output (PRD-0001 Surface + DoD item 8).',
      'Read in ' + SP42 + ':',
      '- crates/sp42-cli/src/main.rs  (large ~92KB — map its structure)',
      '- crates/sp42-cli/Cargo.toml',
      '',
      'Document:',
      '1. Arg parsing: clap (derive or builder?) or hand-rolled? Quote the top-level command/subcommand enum + dispatch.',
      '2. How an existing subcommand works end-to-end: how it builds the concrete HttpClient adapter (BearerHttpClient? from sp42-server? its own), reads config, calls sp42-core build_/execute_/parse_, formats output.',
      '3. How output is printed (println? a formatter? any existing --json / machine-readable flag pattern?).',
      '4. Existing CLI tests (where, how — tests/ dir? assert_cmd? inline?).',
      '5. Async runtime: tokio::main? how are async core fns awaited?',
      '6. Which crates sp42-cli depends on (sp42-core, sp42-types, sp42-server, reqwest, clap, tokio...).',
      'Give me the concrete shape of how to add a verify subcommand taking one of: an article title, a rev id, a single citation (claim snippet OR report index), or an ad-hoc claim + source URL; with an output-format flag human|json|verdict.',
    ].join('\n'),
  },
  {
    label: 'wikiharness-pure-algos',
    notes: NOTES + '/05-wikiharness-pure-algorithms.md',
    prompt: [
      'Extract the EXACT proven algorithms from wikiharness (TypeScript) that I will port faithfully to pure Rust (sp42-core). These are the heart of citation verification.',
      'Read in ' + WH + ':',
      '- packages/core/src/locate-quote.ts            (locateQuoteInSource — the anti-fabrication locator)',
      '- packages/core/src/citation/body-classifier.ts (the deterministic GIGO body-usability gate)',
      '- packages/core/src/citation/voting.ts          (nClassVote / binaryVote — measured agreement + tiebreak)',
      '- packages/core/src/citation/article.ts         (the between-markers claim extraction / citation-walk over the parsed article)',
      '- packages/core/src/citation/prompts.ts         (buildVerifyPrompt — the gold two-step verification prompt; and the metadata "context only" section)',
      '- packages/core/src/citation/parsing.ts         (the verdict parser — parsing the model response into a graded verdict)',
      '- packages/core/src/concurrency.ts              (mapWithConcurrency — bounded worker pool)',
      'Read the corresponding *.test.ts for each to capture exact expected behavior / edge cases.',
      '',
      'Document, precisely enough to reimplement in Rust:',
      '1. locateQuoteInSource: EXACT normalization steps (NFC, whitespace collapse, curly->straight quotes), case-sensitivity, empty-quote handling, return value (offset?).',
      '2. body-classifier: EVERY unusable-body pattern/regex, the length floor, order of checks, return value, ReDoS notes. Quote patterns verbatim.',
      '3. voting: exact nClassVote tally + tiebreak rule + the PanelAgreement-equivalent it returns; binaryVote too.',
      '4. between-markers claim extraction: EXACT rule (run from previous marker to this marker in a block; first marker from block start; whitespace collapse; strip footnote numbers + maintenance tags; bundled-marker handling — note wikiharness DROPS empty-span bundled markers; non-prose skip; no-URL filter). Quote the core function.',
      '5. buildVerifyPrompt: the FULL prompt text (system + user template), the two-step framing, exactly how/where metadata renders as context-only-do-not-quote.',
      '6. the verdict parser: how model text maps to supported/partial/not_supported, the default-to-not_supported behavior, how the located quote is extracted.',
      '7. mapWithConcurrency: signature + bounded behavior + input-order results.',
      'Flag any place ADR-0007 deviates from wikiharness (ADR-0007 says bundled markers SHARE the span + strip maintenance tags, where wikiharness drops/differs) so I implement the ADR rule where they differ.',
    ].join('\n'),
  },
  {
    label: 'wikiharness-fetch-citoid',
    notes: NOTES + '/06-wikiharness-fetch-citoid-article.md',
    prompt: [
      'Extract the EXACT HTTP shapes + fetch/recovery/extraction logic from wikiharness to port to Rust edges (source fetch, Wayback recovery, Citoid sidecar, article fetch+parse).',
      'Read in ' + WH + ':',
      '- packages/core/src/citation/source-fetch.ts   (recoverWaybackBody — Wayback chrome recovery)',
      '- packages/core/src/citation/urls.ts            (rewriteWaybackUrl — the id_ raw-snapshot rewrite rule)',
      '- packages/core/src/citation/citoid.ts          (buildCitoid request, parseCitoid response, buildCitoidHeader — the metadata sidecar, NEVER grounded)',
      '- packages/tools/src/citation/get-article.ts    (how an article + revision is fetched: REST/Parsoid endpoint, URL, headers)',
      '- packages/tools/src/citation/resolve-citation-url.ts (how a citation source URL is resolved/walked from the parsed article)',
      '- packages/core/src/citation/article.ts         (the ParsedArticle shape: refs/citations, how claim + source URL associate)',
      'and their *.test.ts for exact expected request/response shapes.',
      '',
      'Document, precisely:',
      '1. Article fetch: EXACT endpoint URL pattern (REST v1? action=parse? Parsoid?), query params, headers (User-Agent string!), for fetching enwiki article HTML + a specific revision. How the parsed article exposes citation use-sites + their source URLs.',
      '2. resolve_citation_url: how a ref/citation external source URL is extracted (template params? bare url? archive url preference?).',
      '3. rewriteWaybackUrl: the EXACT string rule to turn a web.archive.org URL into the raw id_ form. Quote it.',
      '4. recoverWaybackBody: when/how it falls back to Wayback, the request shape.',
      '5. Citoid: EXACT endpoint URL, how the query is built, what fields are parsed (title/author/publication), how buildCitoidHeader renders the do-not-quote context block. Confirm metadata is NEVER content-hashed.',
      '6. The required Wikimedia User-Agent / etiquette (backoff, GET/HEAD only).',
      'Note: first cut per ADR-0009 = HTML pages + existing Wayback snapshots only (no PDF, no Save-Page-Now). Flag anything PDF/SPN to skip.',
    ].join('\n'),
  },
]

const results = await parallel(tasks.map(t => () =>
  agent(
    t.prompt +
      '\n\nWrite your full detailed notes to this exact path (use the Write tool, never shell): ' + t.notes +
      '\n' + GUARD +
      '\n\nAfter writing the notes file, RETURN a concise (<= 400 word) summary: the key signatures/algorithms I need to START implementing, plus confirm the notes file path. Do not return the full detail — that lives in the file.',
    { label: t.label, phase: 'Research' },
  ).then(text => ({ label: t.label, notes: t.notes, text })),
))

return results.filter(Boolean)
