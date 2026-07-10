# Research: Reconcile ADR-0006 model-client boundary with wikiharness + per-call proxy authorization

**Date:** 2026-06-09  
**Task:** Reconcile SP42 ADR-0006 (endpoint modes, credential ownership, proxy role) with wikiharness/alex-cite-checker implementation patterns and the new wrinkle: sponsor-proxy per-call/per-prompt authorization without letting the proxy tilt feature judgment.

---

## 1. What ADR-0006 ALREADY Commits To — And Where It Is SILENT on Per-Call Authorization

### Committed language (exact quotes from ADR-0006, 2026-06-08):

**Decision 4 — The three endpoint modes:**
- **Local:** "a model server on the operator's own machine/network. No provider key leaves the machine; the offline / open-default / dev mode."
- **Direct:** "SP42 calls a provider directly with a key held by the deployment — the operator's own, or one supplied by a third party / sponsor (e.g. a WMF-issued HuggingFace token). Only for shells that can safely hold a secret (server, CLI, desktop); never the browser."
- **Sponsor / hosted proxy:** "SP42 calls a keyless (or proxy-token-gated) proxy that owns the provider keys, budget, rate limits, and routing — run by SP42, a sponsor (e.g. WMF), or self-hosted. The only remote mode the browser may use, the default for keyless operators, and the mode in which the funder is swapped by re-pointing one config URL."

**Decision 5 — Credential ownership:**
- "A provider key exists inside SP42 only in Direct mode, held in the shell adapter (the operator's own or a sponsor-supplied token), in memory, never in sp42-core, never persisted."
- "Local and Proxy thus keep provider keys out of SP42 entirely — which is what makes the browser shell viable."

**Decision 6 — The proxy's role (CRITICAL):**
> "The proxy is a thin transport + budget boundary: keys, budget (rate limit, model allowlist, token/size caps), routing, optional logging. It **never** runs a feature's judgment or gating logic and never sees the bytes a feature fetched. A sponsor therefore **cannot tilt a result** — every gate stays SP42's, run on SP42's side, wherever the model executed."

### WHERE ADR-0006 IS SILENT:

**ADR-0006 does NOT specify per-call/per-prompt authorization at the proxy level.** It names the proxy's _infrastructure_ (rate limits, model allowlist, token/size caps) but does NOT address:
- How the proxy inspects or gates individual prompts before forwarding to the upstream model
- Whether the proxy can reject a call based on the prompt content or metadata (claim text, source domain, etc.)
- What metadata the client sends to the proxy so it can make authorization decisions
- How the client signals whether a call is "citation-verification approved" vs. some other capability
- The contract shape for per-call authorization requests/responses

**The constraint is clear:** Decision 6 says the proxy cannot "run a feature's judgment" and cannot "tilt a result." But that does NOT forbid the proxy from **inspecting the request for policy compliance** (allowlist checks, budget enforcement, prompt safety) before forwarding, as long as the proxy does NOT **override or modify the verdict logic** that runs in SP42.

---

## 2. What Wikiharness/Alex-Cite-Checker Actually Used

### Wikiharness model wiring:

**File:** `/var/home/louie/Projects/Volunteering-Consulting/wikiharness/packages/server/src/model-config.ts`

```typescript
export function resolveModelConfig(env: Readonly<Record<string, string | undefined>>): ModelConfig {
  const apiKey = env.OPENROUTER_API_KEY;
  const baseURL = env.WIKIHARNESS_MODEL_BASE_URL ?? DEFAULT_BASE_URL;
  const model = env.WIKIHARNESS_MODEL ?? DEFAULT_MODEL;
  const modelEnabled = typeof apiKey === 'string' && apiKey.length > 0;
  return { modelEnabled, apiKey, baseURL, model, ... };
}
const DEFAULT_BASE_URL = 'https://openrouter.ai/api/v1';
const DEFAULT_MODEL = 'mistralai/mistral-small-3.2-24b-instruct';
```

- **Pattern:** DIRECT mode only — holds `OPENROUTER_API_KEY` in the server config (env var, shell adapter, in-memory only per Art. 10.1).
- **Provider:** OpenRouter (OpenAI-compatible endpoint).
- **Credential ownership:** The server holds the key; there is NO sponsor proxy in wikiharness.
- **No per-call authorization:** The server sends the API key on every request to OpenRouter; OpenRouter's rate limit is the only gate.

**Benchmark runner wiring:**

**File:** `/var/home/louie/Projects/Volunteering-Consulting/wikiharness/scripts/benchmark.ts:42–62`

```typescript
function buildPanel(modelIds: readonly string[]): BenchmarkModel[] {
  const apiKey = process.env.OPENROUTER_API_KEY;
  if (!apiKey) throw new Error('OPENROUTER_API_KEY is required — ...');
  const openrouter = createOpenAICompatible({
    name: 'openrouter',
    baseURL: 'https://openrouter.ai/api/v1',
    apiKey,
  });
  return modelIds.map((id) => ({
    label: id,
    client: new VercelModelClient(openrouter(id), {
      modelInfo: { provider: 'openrouter', model: id },
      temperature: 0.1,
      maxOutputTokens: 1000,
    }),
  }));
}
```

- **Pattern:** Direct mode with a panel of 3 open models (mistralai/mistral-small, qwen/qwen3-32b, google/gemma-3-27b-it).
- **No sponsor proxy:** The benchmark calls OpenRouter directly with the `OPENROUTER_API_KEY`.
- **No per-call authorization:** Each model call goes straight to OpenRouter; no intermediary gate.

### Alex-cite-checker's `public-ai-proxy`:

**File:** `/var/home/louie/Projects/Volunteering-Consulting/alex-cite-checker/public-ai-proxy/src/index.js:300–365`

```javascript
const HF_ALLOWED_MODELS = new Set([
  "openai/gpt-oss-20b",
  "Qwen/Qwen3-32B",
  "deepseek-ai/DeepSeek-V3.2-Exp",
]);
const HF_MAX_TOKENS = 4096;
const HF_MAX_BODY_BYTES = 200 * 1024;

if (url.pathname === '/hf') {
  const modelId = typeof body.model === 'string' ? body.model : '';
  const baseModel = modelId.split(':')[0];
  if (!HF_ALLOWED_MODELS.has(baseModel)) {
    return jsonError(400, `Model not allowed: ${modelId || '(missing)'}`, cors);
  }
  if (typeof body.max_tokens === 'number' && body.max_tokens > HF_MAX_TOKENS) {
    body.max_tokens = HF_MAX_TOKENS;
  }
  const upstream = await fetch("https://router.huggingface.co/v1/chat/completions", {
    method: "POST",
    headers: {
      "Authorization": `Bearer ${env.HF_TOKEN}`,
      "X-HF-Bill-To": "wikimedia",  // sponsor signal
    },
    body: JSON.stringify(body),
  });
}
```

- **Proxy pattern:** Sponsor-proxy mode (WMF-issued HuggingFace token, `HF_TOKEN` in Cloudflare Workers environment).
- **Per-call model allowlisting:** The proxy inspects `body.model` and rejects any model not in `HF_ALLOWED_MODELS` **before forwarding to HuggingFace** (line 314).
- **Per-call token cap:** `max_tokens` is capped per call (line 318).
- **Rate limiting:** Per-IP rate limiter (20 requests/min, line 6–10).
- **Billing attribution:** `X-HF-Bill-To: "wikimedia"` signals the sponsor but does NOT include the prompt or claim — just a budget identifier.
- **No prompt-content inspection:** The proxy does NOT read or authorize based on the request body content (the claim text or source URL); it only checks the model name and token budget.

---

## 3. The DELTA: What ADR-0006 Must ADD to Support Sponsor Per-Call Authorization

### The new wrinkle:

A sponsor (e.g. WMF) wants to PAY for inference but retain **per-call authorization** — e.g., "spend money only on citation-verification calls against an allowlisted set of prompts, not on arbitrary LLM usage." Currently ADR-0006 is silent on:
1. How the client signals the purpose of a call (citation-verification vs. other capabilities)
2. How the proxy inspects or authorizes the call before forwarding
3. What the authorization contract shape is (request metadata, response gating)

### The constraint (non-negotiable):

**Decision 6 MUST hold:** The proxy must never see the verdict-computation logic; it cannot inspect the source bytes or run grounding checks or override the gate. The proxy is a **policy enforcement point for call authorization**, not a **feature-judgment point**.

### Minimal ADR-0006 wording change needed:

Add to **Decision 6** (after "optional logging"):

> **Per-call authorization (optional, proxy implementation detail):** A sponsor proxy MAY inspect individual requests before forwarding to the upstream model in order to enforce policy (e.g., allowlist prompts by capability / claim category, cap per-session spend, audit request metadata). The proxy **never inspects or modifies the verdict** itself; the proxy's sole authority is to ACCEPT the call (forward to upstream) or REJECT it (return 401/403) at the request-evaluation stage, before the model executes. All verdict gates, anti-fabrication checks, and feature judgment remain on SP42's side after the model response returns. The proxy carries **audit metadata** (claim text, capability name, user/session id for logging) but never credentials, witness bytes, or verdict parameters. The authorization decision is deterministic, policy-driven, and transparent to SP42: a rejected call is surfaced as a `CitationVerificationError::SourceUnavailable` (Decision 4 / ADR-0007) or similar, keeping the fetch/verify surface intact.

### Request DTO shape (ADR-0006 new subsection or ADR-0008 extension):

The `build_citation_verify_request` (ADR-0008 Decision 3) must thread authorization metadata:

```rust
// In the per-model edge (ADR-0008 Decision 3)
pub struct CitationVerifyRequestBuilder {
    pub wiki_id: String,
    pub claim: String,
    pub source_url: Url,
    pub claim_category: Option<String>,  // "fact-check", "quote-attribution", etc. — for proxy audit
    pub requester: Option<String>,        // session id or user context — for proxy audit (no PII)
}

// The proxy receives (in the HTTP body forwarded to the model endpoint):
// {
//   "messages": [...],  // the actual verify prompt (model input)
//   "x-sp42-authorization": {
//     "capability": "citation-verify",
//     "claim_category": "fact-check",
//     "requester_session": "abc123",
//     "source_domain": "example.com"
//   }
// }
```

**Key principle:** The `x-sp42-authorization` metadata is **FOR POLICY GATING ONLY**. It is never fed to the model. The actual verify prompt (in `messages`) is **unchanged**.

### Proxy authorization logic (implementer responsibility, not ADR scope):

A sponsor proxy that implements per-call authorization would:
1. Parse the incoming request.
2. Extract `x-sp42-authorization.capability` and `x-sp42-authorization.claim_category`.
3. Check if the call is allowed (e.g., "citation-verify with claim_category=quote-attribution? yes").
4. Check budget (tokens this session, calls per user, etc.).
5. **Forward unmodified** if allowed; return `{ error: { message: "Authorization denied: ..." } }` (matching OpenAI-compatible error shape) if denied.
6. Log the decision (audit trail for WMF spending).

### How SP42 handles the denial (no new contract needed):

If the proxy denies the call, the HTTP response is an error (401/403/429). The `parse_citation_verify_response` (ADR-0008 Decision 3) already defaults to `SourceUnavailable` on an unrecoverable response:

> "The pure parser ends in a `validate_*` gate that defaults an unrecoverable model response to *not supported*, never to a support judgment."

So: **no change to SP42's verdict surface**. The verdict is `NotSupported` + `SourceFetched` grounding, surfaced to the operator without a model call having occurred.

---

## 4. Should We Adopt `ModelClient` Now, and Where Does It Live?

### The ADR-0008 context:

**Alternative (f) — "Deferred."**
> "The per-model edge here stays on the existing `HttpClient` trait + a config endpoint (the `liftwing.rs` shape, no new abstraction, ADR-0004). The concrete trigger to adopt a `ModelClient` trait — a **heterogeneous panel** that one transport cannot serve uniformly — is owned by **ADR-0006**; until then the `HttpClient` edge suffices."

Today: **Homogeneous panel** (one OpenAI-compatible endpoint, N models). The `HttpClient` edge (build `HttpRequest`, parse `HttpResponse`) is sufficient.

### Would per-call authorization trigger a `ModelClient`?

**No.** Per-call authorization is purely a **request/response transformation at the proxy layer**. The SP42 `→ HttpClient` edge is unchanged:
- `build_citation_verify_request` builds an `HttpRequest` (with optional `x-sp42-authorization` headers).
- The proxy sees and gates on the headers.
- `parse_citation_verify_response` parses the `HttpResponse`, whether the proxy forwarded it or rejected it.

A `ModelClient` becomes necessary when:
- The panel is **heterogeneous** (mixed OpenAI + Anthropic + local-model formats — different request/response shapes).
- There is **multi-turn conversation** with tool use (agentic) — richer interaction than single-request/single-response.

Per-call authorization does NOT change the transport contract; it only adds optional metadata headers.

### Recommendation: Keep `HttpClient` for v1.

**Rationale:**
1. **ADR-0004 extraction rule:** "a split is not justified when it only renames modules." A `ModelClient` over a homogeneous panel is just renaming; it adds no capability.
2. **Deferred trigger:** ADR-0006 Decision 7 and ADR-0008 Alternative (f) already defer it; no urgency.
3. **Crate placement:** If a `ModelClient` were adopted today, it would live in `sp42-core` (by ADR-0008 Decision 7 — unproven, single caller, CLI-first contract). No new crate justification exists yet.
4. **Request/response headers (metadata):** Naturally fit on the `HttpRequest`/`HttpResponse` types in `sp42-types` (`sp42-types/src/traits.rs:19–32`). No new trait needed.

---

## 5. Concrete Evidence from Wikiharness Code

### Wikiharness `ModelClient` interface (not adopted by ADR-0008):

**File:** `/var/home/louie/Projects/Volunteering-Consulting/wikiharness/packages/core/src/edges.ts:140–142`

```typescript
export interface ModelClient {
  complete(req: ModelRequest): Promise<ModelResponse>;
}
```

**Not used in SP42-equivalent code paths.** Wikiharness built `ModelClient` because it needed:
- Multi-turn agent loops (blocking pre/post hooks, durable pause-on-proposal).
- Tool-call orchestration (the agent loop drives step-by-step interaction).

SP42's citation-verification is **single-turn**: build request → send to model → parse response. The loop stays on SP42's side; the model just returns a categorical verdict.

### Wikiharness `VercelModelClient` (the real adapter):

**File:** `/var/home/louie/Projects/Volunteering-Consulting/wikiharness/packages/edges/src/model/vercel.ts:106–151`

```typescript
export class VercelModelClient implements ModelClient {
  constructor(
    private readonly model: LanguageModel,
    private readonly options: VercelModelClientOptions = {},
  ) {}

  async complete(req: ModelRequest): Promise<ModelResponse> {
    const { system, rest } = extractSystem(req.messages);
    const result = await generateText({
      model: this.model,
      system,
      messages: toAiMessages(rest),
      ...
    });
    return { text, toolCalls, finishReason, ...(modelInfo ? { model } : {}) };
  }
}
```

**Key design:** The `LanguageModel` (Vercel AI SDK type) is the provider-specific thing; the `ModelClient` is the **provider-neutral wrapper**. In SP42, this role is played by `HttpClient` + the request/response DTOs, which are already provider-neutral.

---

## 6. Summary Table: ADR-0006 Status vs. Wikiharness/Alex Pattern

| Aspect | ADR-0006 Commits | Wikiharness Reality | Alex `public-ai-proxy` | SP42 v1 Implication |
|--------|-----------------|-------------------|----------------------|-------------------|
| **Endpoint modes** | 3 (Local/Direct/Sponsor Proxy) | Direct only (OPENROUTER_API_KEY) | Sponsor Proxy (HF_TOKEN) | ✓ Committed, no change needed |
| **Credential ownership** | Key in shell, never in core | ✓ Server holds OPENROUTER_API_KEY | ✓ Cloudflare Workers holds HF_TOKEN | ✓ Committed, no change needed |
| **Proxy role** | Transport + budget, never judgment | N/A (no proxy) | ✓ Rate limit, model allowlist, budget cap | ✓ Committed for infrastructure; per-call authz DEFERRED |
| **Per-call authorization** | SILENT | N/A | ✓ Model allowlisting (`HF_ALLOWED_MODELS` check) | **ADD to Decision 6** (optional, proxy detail) |
| **Proxy sees verdict logic** | NO (Decision 6) | N/A | NO (proxy only checks model name + tokens) | ✓ Preserved by design |
| **Request metadata shape** | N/A | Simple (model id, base URL, API key) | Simple (model id in body, headers) | **ADD optional `x-sp42-authorization` headers** |
| **ModelClient trait** | Deferred (homogeneous panel) | ✓ Built (multi-turn agent loops) | N/A (simple HTTP) | **Keep deferred; `HttpClient` sufficient** |
| **Crate placement** | sp42-core (unproven) | edges/ → core/ interface | N/A | **No extraction trigger yet** |

---

## 7. Concrete Next Steps (for SP42 implementation)

### 1. **Extend Decision 6 in ADR-0006** (sentence or subsection):
   - Add the optional per-call authorization pattern to the proxy's allowed responsibilities.
   - Clarify that the proxy can gate on **request metadata only**, not verdict logic.
   - Link to the request DTO addition (step 2 or an ADR-0008 companion note).

### 2. **Thread authorization metadata into the request DTO** (ADR-0008 or new note):
   - Add optional `x-sp42-authorization` headers to the `HttpRequest` (or a parallel metadata struct).
   - Document that the headers carry capability name, claim category, requester session id — **never** credentials, witness bytes, or verdict parameters.
   - Make it an optional, proxy-implementation-specific concern: SP42 builds the headers; the proxy uses them or ignores them (transparent to SP42).

### 3. **Default error handling (already sufficient)**:
   - If the proxy rejects the call (401/403), the `parse_citation_verify_response` gate defaults to `SourceUnavailable`.
   - No change to the verdict surface; the verdict stays a Finding with no verdict value (abstention).

### 4. **Keep `HttpClient` for v1**:
   - No new `ModelClient` trait.
   - No new crate.
   - Homogeneous panel + single-turn verification fit perfectly on `HttpClient`.

---

## Cross-Reference Evidence

- **ADR-0006:** `/var/home/louie/Projects/Volunteering-Consulting/SP42-prd0001/docs/adr/0006-using-llms.md` (Decisions 4–6, Alternative (f))
- **ADR-0008:** `/var/home/louie/Projects/Volunteering-Consulting/SP42-adr-citation/docs/adr/0008-citation-verification-contract.md` (Decision 3, Decision 7, Alternative (f))
- **ADR-0009:** `/var/home/louie/Projects/Volunteering-Consulting/SP42-adr-citation/docs/adr/0009-citation-source-snapshot-storage.md` (Decision 3 — panel votes + ModelRef persisted)
- **Wikiharness ModelClient:** `/var/home/louie/Projects/Volunteering-Consulting/wikiharness/packages/core/src/edges.ts:140–142`
- **Wikiharness model wiring:** `/var/home/louie/Projects/Volunteering-Consulting/wikiharness/packages/server/src/model-config.ts`
- **Wikiharness VercelModelClient:** `/var/home/louie/Projects/Volunteering-Consulting/wikiharness/packages/edges/src/model/vercel.ts`
- **Benchmark runner:** `/var/home/louie/Projects/Volunteering-Consulting/wikiharness/scripts/benchmark.ts:42–62`
- **Alex public-ai-proxy HuggingFace endpoint:** `/var/home/louie/Projects/Volunteering-Consulting/alex-cite-checker/public-ai-proxy/src/index.js:300–365` (model allowlisting, per-call token cap, sponsor billing attribution)
