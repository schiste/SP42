export const meta = {
  name: 'model-boundary-research',
  description: 'Deep research: Rust LLM-client boundary + sponsor-proxy per-call authorization for SP42',
  phases: [{ title: 'Research', detail: '5 parallel deep-research strands, each writes a notes file' }],
}

const SP42 = '/var/home/louie/Projects/Volunteering-Consulting/SP42-impl-citation'
const PRD0001 = '/var/home/louie/Projects/Volunteering-Consulting/SP42-prd0001'
const ADRC = '/var/home/louie/Projects/Volunteering-Consulting/SP42-adr-citation'
const WH = '/var/home/louie/Projects/Volunteering-Consulting/wikiharness'
const ALEX = '/var/home/louie/Projects/Volunteering-Consulting/alex-cite-checker'
const NOTES = SP42 + '/docs/implementation-notes/research'

const GUARD = [
  '',
  'HARD RULES:',
  '- READ-ONLY on all source code + git trees. The ONLY file you WRITE is your single notes file (the path below), via the Write tool.',
  '- NEVER run any mutating git command (stash/clean/checkout/reset/restore) in any repo. Untracked deliverables must survive. Cloning external repos to a temp dir is fine.',
  '- Be concrete and current: real crate versions, last-release dates, star counts, exact LICENSE, transitive dep counts, exact trait/function signatures, exact proxy-auth mechanisms. Cite sources (URLs). Vagueness is useless — this decides an architecture.',
].join('\n')

const CONTEXT = [
  '',
  'DECISION CONTEXT (SP42, a Rust browser-native Wikipedia patrol platform):',
  '- Goal: a PROVIDER-AGNOSTIC model-client boundary so feature crates never depend on a provider wire format. A reviewer suggested vendoring graniet/llm; the lead wants the FULL landscape first, not a leap to one crate.',
  '- LOAD-BEARING constraint (the crux): the CLIENT (esp. a Wasm browser shell) must hold NO provider key. A SPONSOR (e.g. WMF) may hold the key and PAY for inference, but only wants to authorize CERTAIN calls/prompts (e.g. pay only for citation-verification prompts against an allowlisted set of open models, with caps) — NOT blindly forward everything. So a sponsor PROXY must be able to inspect/authorize per-call while the feature\'s own judgment gate (anti-fabrication, scoring) stays on SP42\'s side and the proxy can never tilt a result.',
  '- Existing decision: ADR-0006 already names three endpoint modes (Local / Direct / Sponsor-or-hosted-proxy) and says the proxy carries transport+budget but never a feature\'s judgment. The new wrinkle is per-call/per-prompt AUTHORIZATION at the proxy.',
  '- Constraints: licenses must be permissive (Apache-2.0/MIT/BSD/ISC/Zlib/Unicode/CC0/BSL/CDLA or GPL-3.0-only) for cargo-deny; >50 transitive deps needs lead approval; the workspace already has reqwest(rustls)+serde+serde_json+tokio.',
].join('\n')

phase('Research')

const tasks = [
  {
    label: 'rust-multiprovider-crates',
    type: 'ed3d-research-agents:internet-researcher',
    notes: NOTES + '/07-rust-llm-multiprovider-crates.md',
    prompt: [
      'Survey the CURRENT (2025-2026) landscape of Rust crates that provide a PROVIDER-AGNOSTIC LLM chat/completion abstraction. Cover at least: rig (rig-core, 0xPlaygrounds), genai (jeremychone), llm (graniet/llm), allms, llm-chain, langchain-rust, and any other actively-maintained multi-provider crate you find.',
      'For EACH, report: crates.io name + latest version + last-release date; GitHub stars + recent activity (maintained vs abandoned); the EXACT core trait(s) and the chat method signature (request/response shape); how provider/model/sampling-params are selected; whether a CUSTOM base_url / endpoint and CUSTOM auth (arbitrary headers / bearer override per-request or per-client) are supported (CRITICAL — a sponsor proxy needs a custom endpoint + a proxy token, and the browser must send NO provider key); streaming support; the LICENSE (exact SPDX); rough transitive-dependency weight; and whether it is realistically "vendor-and-trim a thin OpenAI-compatible chat client we own" friendly.',
      'Conclude with a ranked shortlist for SP42 (provider-agnostic, permissive license, maintained, light deps, custom-endpoint+auth friendly) and a one-line take on each. Note explicitly which support pointing at an arbitrary OpenAI-compatible gateway with a custom token (the sponsor-proxy shape).',
    ].join('\n'),
  },
  {
    label: 'rust-openai-low-level',
    type: 'ed3d-research-agents:internet-researcher',
    notes: NOTES + '/08-rust-openai-and-build-vs-buy.md',
    prompt: [
      'Research the OpenAI-centric + lower-level Rust HTTP options for calling an OpenAI-compatible chat-completions endpoint, AND the build-vs-buy question for SP42.',
      'Cover: async-openai (has it; custom base_url? custom headers? license? deps?), openai-api-rs, ollama-rs, openai_dive, and "just reqwest + serde_json ourselves". For each: latest version, maintenance, license, dep weight, and crucially whether you can set a CUSTOM base_url AND inject CUSTOM auth headers / a proxy token (so SP42 can point at a sponsor proxy, and the browser shell can send NO provider key — relying on the proxy to hold the real key).',
      'Then a build-vs-buy analysis: given SP42 already has reqwest(rustls)+serde+serde_json and needs (for the first cut) ONE shape — a single OpenAI-compatible chat-completions POST behind a trait we own — is a hand-rolled ~100-line adapter over reqwest the right call, with a third-party multi-provider crate adopted only when a real second provider/shape appears? Weigh maintenance/dep/licensing risk vs. reuse. Give a concrete recommendation with reasoning.',
      'Also: what request fields/headers does an OpenAI-compatible call carry that a SPONSOR PROXY would need to forward or could authorize on (model, messages, metadata/tags, user field, headers)?',
    ].join('\n'),
  },
  {
    label: 'candidate-code-read',
    type: 'ed3d-research-agents:remote-code-researcher',
    notes: NOTES + '/09-candidate-crate-code-read.md',
    prompt: [
      'Deep-read the ACTUAL source of the top provider-agnostic Rust LLM crates so a vendoring/adoption decision is grounded in code, not READMEs. Clone (to a temp dir) and inspect: graniet/llm (the `llm` crate the reviewer suggested), 0xPlaygrounds/rig (rig-core), and jeremychone/rust-genai. (If one is clearly unsuitable, say why and move on.)',
      'For EACH, report with file:line evidence:',
      '1. The EXACT provider-agnostic trait definition(s) (e.g. ChatProvider/CompletionProvider) and the chat method signature + request/response types.',
      '2. How a CUSTOM base_url/endpoint is configured, and how auth is injected (can you supply an arbitrary bearer/header? can you run with NO key and let a proxy add it? is the OpenAI backend pointable at any OpenAI-compatible URL?).',
      '3. The LICENSE file (exact SPDX) and whether it is permissive enough for cargo-deny (Apache/MIT/BSD/ISC/Zlib/GPL-3.0-only ok).',
      '4. Transitive dependency weight (run cargo tree or read Cargo.toml + lockfile): rough crate count, and any heavy/risky deps (its own async runtime, vendored openssl, etc.).',
      '5. An HONEST "vendor-and-trim" assessment: could SP42 extract JUST a thin OpenAI-compatible chat client (custom endpoint + custom auth) cleanly, or is the abstraction entangled? How many LOC, how coupled?',
      'Conclude: which (if any) is worth adopting/vendoring vs. hand-rolling, specifically for the sponsor-proxy shape (custom endpoint + proxy token + no client key).',
    ].join('\n'),
  },
  {
    label: 'sponsor-proxy-auth-patterns',
    type: 'ed3d-research-agents:internet-researcher',
    notes: NOTES + '/10-sponsor-proxy-per-call-authorization.md',
    prompt: [
      CONTEXT,
      '',
      'Research the ARCHITECTURE + prior art for the load-bearing constraint above: a sponsor/proxy holds the provider key and pays, the client holds no key, and the proxy authorizes only CERTAIN calls/prompts (not blind forwarding), while the feature\'s judgment gate stays on the consuming app\'s side and the proxy cannot tilt results.',
      'Study concrete mechanisms in: OpenRouter (BYOK, provisioning/virtual API keys, per-key model + spend limits, request metadata, the `models` allowlist, the `user`/metadata fields); LiteLLM proxy (virtual keys, model_access_groups/team budgets, guardrails, request tagging, key-level model allowlists); Cloudflare AI Gateway; Helicone; Portkey AI gateway; Kong AI Gateway; and any "Public AI" inference utility / nonprofit AI-proxy efforts (and how WMF exposes ML today — Lift Wing, api.wikimedia.org, OAuth2 — as a likely sponsor posture).',
      'Answer precisely:',
      '1. To authorize a SPECIFIC call/prompt (vs. blanket forwarding), what does the proxy need to SEE — model id, a capability/purpose tag, prompt content/hash, size, caller identity? What is the common convention (request metadata/tags, virtual-key scoping, model allowlists, header tags)?',
      '2. How do real gateways let a sponsor say "pay only for THESE prompts/models with THESE caps"? (virtual keys scoped to models/budgets; request tagging; guardrails/policy on content).',
      '3. How do you keep the proxy as transport+budget+authorization ONLY, never running the consuming feature\'s judgment, and unable to tilt results?',
      '4. Implications for OUR design: does the model-client REQUEST DTO need a capability/purpose tag (so the proxy can authorize)? does the endpoint CONFIG need {mode: local|direct|proxy, base_url, optional proxy_token, capability_tag}? How does the browser shell send NO provider key yet reach the proxy (proxy token vs. session-scoped credential vs. same-origin)? Recommend a concrete shape.',
    ].join('\n'),
  },
  {
    label: 'reconcile-adr-and-wikiharness',
    type: 'ed3d-research-agents:codebase-investigator',
    notes: NOTES + '/11-reconcile-adr0006-and-wikiharness.md',
    prompt: [
      CONTEXT,
      '',
      'Reconcile the boundary + sponsor-proxy-authorization design with what is ALREADY decided in SP42 and ALREADY built in wikiharness. Read (read-only):',
      '- ' + PRD0001 + '/docs/adr/0006-using-llms.md  (Decisions 4 endpoint modes, 5 credential ownership, 6 proxy carries transport+budget-never-judgment, 7 one-shared-edge / ModelClient-deferred trigger, 8 ModelRef attributability).',
      '- ' + ADRC + '/docs/adr/0008-citation-verification-contract.md  (Decision 3 the per-model HttpClient edge; Alternative (f) ModelClient DEFERRED; Decision 7 crate placement).',
      '- ' + ADRC + '/docs/adr/0009-citation-source-snapshot-storage.md  (what per-model identity is persisted).',
      '- wikiharness inference wiring: ' + WH + '/packages/edges/ (the Vercel-AI-SDK ModelClient adapter), the benchmark runner that calls real open models via OpenRouter (search packages/evals + scripts/benchmark.ts), and any public-ai-proxy / keyless reference. Also check ' + ALEX + ' (alex-cite-checker) for how it reached models + any proxy/.env shape (do NOT print secrets — just the SHAPE: which env vars, which endpoint, proxy vs direct).',
      'Report:',
      '1. EXACTLY what ADR-0006 already commits to for endpoint modes + credential ownership + the proxy\'s role, quoting the relevant lines. Where does it ALREADY cover the "client holds no key / sponsor pays" case, and where is it SILENT on per-call/per-prompt AUTHORIZATION (the new wrinkle)?',
      '2. What wikiharness/alex ACTUALLY used to reach open models (OpenRouter direct? a proxy? env-var shape), and whether a sponsor-pays-but-gates pattern was used or just direct keys.',
      '3. The concrete DELTA: what ADR-0006 (and the ModelClient boundary + request DTO + endpoint config) must ADD to support sponsor per-call authorization, WITHOUT violating Decision 6 (proxy never runs feature judgment). Propose the minimal ADR-0006 wording change + the request-DTO/config shape.',
      '4. Whether reversing ADR-0008 Alt (f) (adopt ModelClient now) is consistent with ADR-0004 crate discipline, and where the trait + adapter should live (sp42-types vs sp42-core vs a new crate).',
    ].join('\n'),
  },
]

const results = await parallel(tasks.map(t => () =>
  agent(
    t.prompt +
      '\n\nWrite your full detailed notes (with source URLs / file:line evidence) to this exact path via the Write tool: ' + t.notes +
      '\n' + GUARD +
      '\n\nAfter writing the notes file, RETURN a concise (<= 450 word) summary: the key findings + your concrete recommendation for SP42, plus confirm the notes file path. Detail lives in the file.',
    { label: t.label, phase: 'Research', agentType: t.type },
  ).then(text => ({ label: t.label, notes: t.notes, text })),
))

return results.filter(Boolean)
