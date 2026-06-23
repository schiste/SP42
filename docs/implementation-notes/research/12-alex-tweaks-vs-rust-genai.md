# Part A: Tweaks/Hacks in alex-cite-checker & wikiharness Model Calls

## Summary Table: ALL Provider-Specific Tweaks

| # | Tweak | File:Line | OpenAI-Standard vs Provider-Specific | Why It Exists |
|---|-------|-----------|-------------------------------------|---------------|
| 1 | **System message in `system` option (not `messages` array)** | wikiharness: `packages/edges/src/model/vercel.ts:42-57` | OpenAI-standard; Vercel AI SDK abstraction | Prevents prompt-injection warning on every SDK call; moves system into supported channel |
| 2 | **Bearer token auth header** | alex-cite-checker: `citation-checker-script/core/providers.js:21` | OpenAI-standard (generic) | Standard OAuth header; used by all OpenAI-compatible providers |
| 3 | **OpenRouter attribution headers (HTTP-Referer, X-Title)** | alex-cite-checker: `citation-checker-script/core/providers.js:87-97` | OpenRouter-specific | Analytics/attribution on OpenRouter; NOT required but recommended by provider |
| 4 | **HuggingFace router direct endpoint** | alex-cite-checker: `citation-checker-script/core/providers.js:67-80` | HuggingFace-specific | Direct routing to `router.huggingface.co/v1/chat/completions` vs proxy fallback |
| 5 | **X-HF-Bill-To header (Wikimedia)** | alex-cite-checker: `public-ai-proxy/src/index.js:332` | HuggingFace-specific | Cost attribution in HF billing; hardcoded "wikimedia" for Wikimedia Free Tier sponsorship |
| 6 | **HF_MAX_TOKENS cap + forced override** | alex-cite-checker: `public-ai-proxy/src/index.js:18, 318-320` | HuggingFace-specific | HF router has lower token ceiling than others; proxy enforces `4096` max, silently clamps user requests |
| 7 | **HF_MAX_BODY_BYTES cap** | alex-cite-checker: `public-ai-proxy/src/index.js:19, 302-304` | HuggingFace-specific | Prevents 413 Payload Too Large on HF endpoint; proxy rejects oversized requests early |
| 8 | **HF_UPSTREAM_TIMEOUT_MS explicit backoff** | alex-cite-checker: `public-ai-proxy/src/index.js:20, 325` | HuggingFace-specific | HF router slower than OpenAI/OpenRouter; explicit 60s timeout with AbortController |
| 9 | **Gemini JSON mode (`responseMimeType: 'application/json'`)** | alex-cite-checker: `citation-checker-script/core/providers.js:159` | Gemini-specific (non-OpenAI) | Gemini's native JSON constraint (not OpenAI `response_format`); prevents Gemini from wrapping output in markdown fences |
| 10 | **Gemini `systemInstruction` (not `system` in messages)** | alex-cite-checker: `citation-checker-script/core/providers.js:143-151` | Gemini-specific (native protocol) | Gemini's native system message shape; separate from user content in `contents` |
| 11 | **Gemini `maxOutputTokens` (not `max_tokens`)** | alex-cite-checker: `citation-checker-script/core/providers.js:153` | Gemini-specific | Gemini param name differs from OpenAI standard |
| 12 | **Claude `x-api-key` header (not `Authorization`)** | alex-cite-checker: `citation-checker-script/core/providers.js:100-116` | Anthropic-specific (non-OpenAI) | Claude uses proprietary header; also requires `anthropic-version` + `anthropic-dangerous-direct-browser-access` |
| 13 | **Claude `system` param (native, not in messages)** | alex-cite-checker: `citation-checker-script/core/providers.js:104` | Anthropic-specific | Claude's native system message param |
| 14 | **Claude `max_tokens` with different default (3000)** | alex-cite-checker: `citation-checker-script/core/providers.js:100-103` | Anthropic-specific | Claude has different token budgets/defaults vs OpenAI |
| 15 | **HF model allowlist + namespace (`[model]:suffix` parsing)** | alex-cite-checker: `public-ai-proxy/src/index.js:13-17, 312-315` | HuggingFace-specific | Proxy enforces only 3 whitelisted models; parses `:free`/`:nitro` variants |
| 16 | **Wayback `id_` flag URL rewrite** | alex-cite-checker: `public-ai-proxy/src/index.js:32-37` | Not provider-specific (archival/extraction) | Strips Wayback chrome (toolbar/banner) for cleaner extraction; purely content-handling |
| 17 | **Temperature defaults differ by provider** | alex-cite-checker: `core/providers.js:9-17` varies; wikiharness: `benchmark.ts:58` | OpenAI-standard param, but used inconsistently | alex sets `0.1` (deterministic), wikiharness also `0.1`; Gemini + OpenAI same; Claude omitted in provider call |
| 18 | **maxTokens param inconsistency (max_tokens vs maxOutputTokens)** | alex-cite-checker: varies by provider | Provider-specific field naming | OpenAI uses `max_tokens`, Gemini `maxOutputTokens`, Claude native |
| 19 | **Open model pinning via OpenRouter** | wikiharness: `packages/server/src/model-config.ts:23` | Provider-selection + model-pinning (not API tweak) | Defaults to open `mistralai/mistral-small-3.2-24b-instruct`; overridable via env |
| 20 | **Raw response body capture (unused in alex, present in rust-genai)** | N/A in alex/wikiharness | rust-genai feature | Debugging/inspection; not used by alex-cite-checker directly |

---

## Tweak Categories by Scope

### 1. **Authentication** (Straightforward, Provider-Specific Naming)
- **Bearer token**: Universal (OpenAI/OpenRouter/HF/Together/most OpenAI-compatible)
- **X-API-Key** (Claude/Anthropic): Non-standard header name
- **HF-specific**: `X-HF-Bill-To` header (sponsorship attribution)

### 2. **Custom Headers** (Optional Vanity/Attribution)
- **OpenRouter**: `HTTP-Referer` + `X-Title` (analytics, NOT required)
- **HuggingFace**: `X-HF-Bill-To` (cost attribution on Wikimedia Free Tier)

### 3. **System Message Routing** (SDK/Framework Abstraction)
- **Vercel AI SDK** (wikiharness): Hoists `role:'system'` to `system` option, avoiding injection warnings
- **Gemini native**: `systemInstruction` (separate from `contents`)
- **Claude native**: `system` param (separate from messages)
- **OpenAI**: `messages` array or (newer SDKs) `system` option

### 4. **Request Payload Field Naming** (Provider-Specific)
- **max_tokens**: OpenAI, OpenRouter, HF, Together (OpenAI-compatible)
- **maxOutputTokens**: Gemini
- **max_tokens** (also called): Claude (via `max_tokens` in native protocol)

### 5. **Output Format / Structured Responses**
- **Gemini `responseMimeType: 'application/json'`**: Forces JSON output, prevents markdown wrapping
- **OpenAI `response_format`**: NOT used in alex (no JSON mode invoked); schema/JSON support deferred in wikiharness

### 6. **Rate Limiting / Request Caps** (Proxy Enforced)
- **HF_MAX_TOKENS**: 4096 cap, silently overridden if user requests > 4096
- **HF_MAX_BODY_BYTES**: 200 KB cap, rejects oversized payloads early
- **HF_UPSTREAM_TIMEOUT_MS**: 60 s (longer than OpenAI/OpenRouter defaults)

### 7. **Model Allowlisting / Routing**
- **HF proxy**: Only allows 3 whitelisted models; parses `:free`/`:nitro` suffix variants
- **OpenRouter**: No allowlist (open routing across any model in catalog)

### 8. **URL Rewriting (Content, Not API)**
- **Wayback `id_` flag**: Strips archive UI chrome; not provider-specific, extraction-layer concern

---

# Part B: rust-genai Coverage vs alex/wikiharness Tweaks

## rust-genai Architecture

**Key Files:**
- `src/client/config.rs`: ClientConfig, builder, resolver chain
- `src/client/headers.rs`: Headers map (arbitrary KV pairs, merged via `Headers::merge()`)
- `src/chat/chat_options.rs`: ChatOptions with `extra_body: Option<Value>` + `extra_headers: Option<Headers>`
- `src/adapter/adapters/openai/adapter_shared.rs`: OpenAI adapter merges `extra_body` via `x_merge()`
- `src/resolver/auth_resolver.rs`: AuthResolver trait + AsyncAuthResolver for custom auth
- `src/resolver/endpoint.rs`: Endpoint struct (base_url only, static or owned)
- `src/adapter/adapters/open_router/adapter_impl.rs`: OpenRouter adapter (delegates to OpenAI adapter)

**Dep Count:** ~30 direct + transitive (reqwest, serde, tokio, futures, base64, tracing, uuid, strum, derive_more, paste, value-ext, regex, mime_guess, eventsource-stream, bytes, plus AWS Bedrock optionals).  
**License:** MIT OR Apache-2.0 (permissive, dual).

---

## Coverage Table: Each Tweak vs rust-genai Capability

| # | Tweak | rust-genai Support | HOW / Evidence | GAPS |
|---|-------|-------------------|-----------------|------|
| 1 | System message in `system` option | **PARTIAL** | ChatOptions carries system msg; adapters (OpenAI-derived) separate it from messages. Vercel SDK does same abstraction. | N/A — rust-genai follows similar pattern |
| 2 | Bearer token auth header | **FULL** | `AuthResolver` trait + `AuthData::from_single(key)` inject Bearer token. OpenAI adapter applies via reqwest headers. | N/A |
| 3 | OpenRouter HTTP-Referer + X-Title | **FULL** | `ChatOptions.extra_headers: Option<Headers>`; merges into reqwest RequestBuilder. Callers set `("HTTP-Referer", "..."), ("X-Title", "...")` | N/A |
| 4 | HF direct endpoint routing | **FULL** | `ServiceTargetResolver` + `Endpoint` allow per-call base_url override. Caller configures `Endpoint::from_owned("https://router.huggingface.co/v1")` | N/A |
| 5 | X-HF-Bill-To header | **FULL** | `ChatOptions.extra_headers` (as above) | N/A |
| 6 | HF_MAX_TOKENS cap | **PARTIAL** | `ChatOptions.max_tokens` is carried to adapter; adapter respects it. But NO automatic clamping logic — adapter passes request as-is. Caller must enforce cap. | MISSING: no automatic max override (caller must pre-clamp); suggest: wrapper layer does this |
| 7 | HF_MAX_BODY_BYTES cap | **MISSING** | No per-adapter request-body-size gate in rust-genai. Caller must validate before calling. | MISSING: reqwest allows oversized payloads; no early rejection |
| 8 | HF_UPSTREAM_TIMEOUT_MS | **PARTIAL** | `WebConfig` (reqwest config) carries `timeout(Duration)`. But it's CLIENT-LEVEL default, not per-call override. | MISSING: no per-request timeout override (only client-level via `WebConfig::with_timeout()`) |
| 9 | Gemini JSON mode (responseMimeType) | **PARTIAL** | Gemini adapter exists. JSON mode via `extra_body: json!({"responseMimeType": "application/json"})` passed to adapter. Adapter handles it. | PARTIAL: no first-class `response_format` for Gemini (extra_body required; not Gemini-native ChatOptions field) |
| 10 | Gemini systemInstruction | **FULL** | Gemini adapter has native system handling; ChatRequest.system → Gemini's systemInstruction. | N/A |
| 11 | Gemini maxOutputTokens | **FULL** | Gemini adapter translates ChatOptions.max_tokens → Gemini's maxOutputTokens field. | N/A |
| 12 | Claude x-api-key header | **FULL** | Anthropic adapter configures auth via `AuthData::from_single(key)` → `x-api-key` header. AuthResolver injects it. | N/A |
| 13 | Claude system param | **FULL** | Anthropic adapter routes ChatRequest.system → Claude's native `system` param. | N/A |
| 14 | Claude max_tokens | **FULL** | Anthropic adapter handles max_tokens translation (Claude uses same param name as OpenAI). | N/A |
| 15 | HF model allowlist | **MISSING** | No adapter-level allowlist enforcement. Caller must pre-validate model names. | MISSING: no model allowlist gate; proxy-side concern in alex, not genai concern |
| 16 | Wayback id_ rewrite | **OUT-OF-SCOPE** | Not an API layer concern; extraction/HTTP layer handles URL transformation before fetch. | N/A |
| 17 | Temperature defaults | **FULL** | `ChatOptions.temperature` optional; adapter uses or omits per provider default. | N/A |
| 18 | maxTokens naming | **FULL** | Each adapter knows its field names; ChatOptions.max_tokens translated per provider. | N/A |
| 19 | Open model pinning | **FULL** | `ModelIden` + routing selects model. Caller pins via config or per-call. | N/A |
| 20 | Raw response body capture | **FULL** | `ChatOptions.capture_raw_body: Option<bool>` enables debugging. | N/A |

---

## Critical Gaps: Tweaks rust-genai Does NOT Assist

### Gap 1: **Request Body Size Cap (HF_MAX_BODY_BYTES)**
- **Tweak**: `public-ai-proxy/src/index.js:302-304` rejects payloads > 200 KB before sending
- **rust-genai**: No built-in pre-flight request-size validation
- **Implication**: SP42 must wrap/validate request size before calling genai, or let HF reject 413
- **Workaround**: Caller layer (SP42) validates serialized ChatRequest JSON size

### Gap 2: **Max Tokens Auto-Clamp (HF_MAX_TOKENS)**
- **Tweak**: `public-ai-proxy/src/index.js:318-320` silently overrides user `max_tokens` > 4096 to 4096
- **rust-genai**: ChatOptions.max_tokens passed through as-is; no per-adapter max override
- **Implication**: SP42 must pre-clamp user input or accept HF error
- **Workaround**: Wrapper enforces `min(requested, 4096)` before constructing ChatOptions

### Gap 3: **Per-Request Timeout (HF_UPSTREAM_TIMEOUT_MS)**
- **Tweak**: alex-cite-checker proxy sets 60 s timeout per HF request via AbortController
- **rust-genai**: WebConfig timeout is CLIENT-LEVEL default, not per-call
- **Implication**: SP42 either accepts client-level timeout or must hand-roll per-request logic
- **Workaround**: Set WebConfig timeout once; all HF calls get same timeout (acceptable if 60 s is reasonable default)

### Gap 4: **Gemini JSON Mode as First-Class Field**
- **Tweak**: Gemini requires `responseMimeType: 'application/json'` in generationConfig
- **rust-genai**: No `ChatOptions.gemini_response_mime_type`; requires `extra_body: {"responseMimeType": "..."}` 
- **Implication**: Callers must pass raw JSON to extra_body; less ergonomic
- **Workaround**: Wrapper adds `with_gemini_json_mode()` helper on top of extra_body

### Gap 5: **Provider-Specific Model Allowlist**
- **Tweak**: HF proxy whitelist only 3 models; OpenRouter allows any
- **rust-genai**: No adapter-level allowlist
- **Implication**: Model vetting is caller concern, not genai; acceptable
- **Workaround**: SP42 maintains its own allowlist (may differ per provider)

---

## Sponsor-Proxy Shape (Custom Base URL + Custom Token)

**Requirement**: Client holds NO provider keys; all auth lives in a sponsor proxy (e.g., Wikimedia's HF sponsor endpoint).

**rust-genai Coverage:**
- ✅ `ServiceTargetResolver`: Allows custom endpoint per call (custom base_url)
- ✅ `AuthResolver`: Custom auth (can inject sponsor-proxy token instead of provider key)
- ✅ `extra_headers`: Sponsor metadata headers (e.g., X-HF-Bill-To, X-Sponsor-ID)

**Implementation:**
```rust
// Pseudo-code
let auth_resolver = AuthResolver::from_resolver_async_fn(|model| {
    Box::pin(async { Ok(Some(AuthData::from_single(sponsor_proxy_token))) })
});
let endpoint = Endpoint::from_owned("https://sponsor-proxy.example.com/v1");
let mut config = ClientConfig::default()
    .with_auth_resolver(auth_resolver)
    .with_service_target_resolver(ServiceTargetResolver::new(|_| Ok(ServiceTarget { 
        endpoint, /* ... */ 
    })));
```

**Verdict**: ✅ **rust-genai FULLY supports sponsor-proxy shape** (custom auth + custom endpoint per request).

---

## Model Fingerprinting / Attribution

**Requirement**: Record which model version actually served the response (esp. if endpoint routes to different model than requested).

**rust-genai Coverage:**
- ✅ `ChatResponse`: Carries `ModelIden` (provider, model name)
- ✅ `Usage` + response metadata: Available for audit trails
- ⚠️ **NO** raw `x-served-model` header capture from response

**Gap**: If the sponsor proxy routes requests to a different model than requested, genai doesn't auto-extract `x-served-model` from response headers. Caller must parse response headers manually or ask proxy to include a `served-model` JSON field in the response body.

**Workaround**: Wrapper adds response-header extraction if needed.

---

# Part C: Verdict & Recommendation

## Summary of Actual Tweaks in alex-cite-checker + wikiharness

**Total tweaks found:** 20 distinct behaviors  
**Distribution:**
- **OpenAI-standard (generic)**: 5 (bearer auth, system separation, temp/max_tokens, response format, JSON capture)
- **Provider-specific (Gemini/Claude/HF/OpenRouter)**: 12 (native system param shape, maxOutputTokens naming, JSON mode, headers, allowlist, caps)
- **Proxy/extraction-layer (not API)**: 2 (Wayback rewrite, model allowlist, timeouts)
- **SDK-level abstraction (Vercel, not provider)**: 1 (system hoisting)

---

## rust-genai Coverage Verdict

### ASSISTED (genai handles or provides extension point):
1. ✅ **Bearer token + custom auth** (AuthResolver trait)
2. ✅ **Custom headers** (ChatOptions.extra_headers with Headers map + merge)
3. ✅ **Custom base URL per call** (ServiceTargetResolver + Endpoint)
4. ✅ **System message routing** (native per-adapter, like Vercel SDK)
5. ✅ **max_tokens / temperature / top_p** (ChatOptions standard fields)
6. ✅ **Gemini native systemInstruction + maxOutputTokens** (Gemini adapter handles translation)
7. ✅ **Claude x-api-key + system param** (Anthropic adapter native)
8. ✅ **OpenRouter (delegates to OpenAI adapter)** — works as-is
9. ✅ **Sponsor-proxy shape** (custom auth + endpoint)
10. ✅ **Response usage + model identity** (ChatResponse carries metadata)
11. ✅ **extra_body for non-standard fields** (e.g., Gemini JSON mode via extra_body)

### PARTIALLY ASSISTED (genai provides building blocks, caller must wrap):
1. ⚠️ **HF_MAX_TOKENS auto-clamp** — genai passes max_tokens through; caller must pre-clamp
2. ⚠️ **Per-request timeout (HF 60s)** — WebConfig timeout is client-level; caller wraps or accepts default
3. ⚠️ **Gemini JSON mode ergonomics** — works via extra_body, but not first-class ChatOptions field
4. ⚠️ **Raw response body capture** — ChatOptions.capture_raw_body exists but output is adapter-specific

### NOT ASSISTED (genai not designed for these):
1. ❌ **Request body size cap (200 KB)** — validate before calling genai; genai doesn't pre-flight size
2. ❌ **Model allowlist enforcement** — caller maintains allowlist; genai just routes
3. ❌ **x-served-model header extraction** — caller parses response headers if needed
4. ❌ **Wayback URL rewriting** — HTTP/extraction layer concern, not API layer

---

## Recommendation

### **(c) HAND-ROLLED THIN ADAPTER (over rust-genai)**

**Why (not (b) "adopt rust-genai now"):**

1. **Assisted tweaks are TRIVIAL to reimplement** (5 lines each):
   - Custom base URL: store in client config ✓
   - Custom auth: pass bearer token via header ✓
   - Custom headers: merge into reqwest RequestBuilder ✓
   - max_tokens/temperature: pass through ChatRequest ✓

2. **Partially-assisted tweaks require wrapper logic ANYWAY:**
   - Max token clamp: `max(requested, HF_MAX) → request` (1 line, callers' problem)
   - Timeout: WebConfig default or per-request wrapper (trivial)
   - Gemini JSON: `if gemini { extra_body.json_mode = true }` (local helper)

3. **NOT-assisted tweaks are caller/proxy responsibility:**
   - Request size validation: can't be in genai (upstream concern)
   - Allowlist: SP42 policy, not library
   - Response header parsing: already done for fingerprinting

4. **rust-genai Adds Significant Overhead:**
   - ~30 direct deps (reqwest via genai, plus serde, tokio, futures, strum, uuid, base64, …)
   - MIT/Apache-2.0 license OK, but dependency surface for maintenance
   - Build time + compile time for a library that handles 20 models SP42 will never use
   - Adapter model (custom per-provider handler) is overkill for SP42's "OpenAI-compatible + HF + optional sponsor proxy" scope

5. **SP42's Actual Scope is NARROW:**
   - OpenAI-compatible base (OpenRouter, OpenAI, Together, etc. all speak OpenAI chat/completion API)
   - HF router as ONE special case (also OpenAI-compatible, but with caps/timeout)
   - Sponsor proxy: custom endpoint + custom auth (hand-rollable in 20 lines)
   - NO: Gemini, Claude, Anthropic, Vertex, Ollama, Bedrock, Groq, DeepSeek, etc. (genai's main value)

6. **Risk: genai Churn:**
   - rust-genai is `0.7.0-beta.3-WIP` (pre-1.0; API surface may shift)
   - SP42 is a long-lived open-source project; genai 1.0 or major version changes = maintenance burden
   - Hand-rolled adapter is stable, auditable, testable without upstream churn

---

### **Recommendation: Hand-Rolled Thin Adapter**

**Structure:**
```rust
// sp42-citation crate or sp42-live
pub struct SP42ModelClient {
    base_url: String,
    api_key: String,
    timeout_ms: u64,
    extra_headers: HashMap<String, String>,
}

impl SP42ModelClient {
    async fn call(
        &self,
        model: &str,
        messages: Vec<ChatMessage>,
        system: Option<String>,
        max_tokens: Option<u32>,
    ) -> Result<String> {
        let mut body = json!({
            "model": model,
            "messages": messages_to_openai_compat(&messages),
            "max_tokens": max_tokens.map(|t| min(t, HF_MAX_TOKENS)),
            "temperature": 0.1,
        });
        if let Some(sys) = system {
            body["system"] = sys.into();
        }
        
        let mut headers = self.extra_headers.clone();
        headers.insert("Authorization", format!("Bearer {}", self.api_key));
        headers.insert("Content-Type", "application/json");
        
        // reqwest call
        let client = reqwest::Client::new();
        let resp = client
            .post(&format!("{}/chat/completions", self.base_url))
            .headers(headers_to_reqwest(&headers))
            .json(&body)
            .timeout(Duration::from_millis(self.timeout_ms))
            .send()
            .await?;
        
        // Parse OpenAI-compat response
        let data = resp.json::<serde_json::Value>().await?;
        Ok(data["choices"][0]["message"]["content"].as_str().unwrap_or("").to_string())
    }
}
```

**Advantages:**
- ✅ **Zero vendor lock-in:** Fully owned, auditable code
- ✅ **No churn risk:** Frozen, versioned behavior
- ✅ **Minimal deps:** Only `reqwest` (already a peer dep) + `serde_json` (standard)
- ✅ **SP42-specific:** Can hardcode HF caps, timeout, allowlist checks inline
- ✅ **Sponsor-proxy ready:** Custom endpoint + custom auth out of the box
- ✅ **Testable:** Hand-rolled mocks/stubs; no genai trait coverage needed
- ✅ **Documentable:** 50-line adapter that new contributors understand immediately

**Costs:**
- ❌ No automatic Gemini/Claude/Vertex support (but SP42 doesn't need them now)
- ❌ No genai version bump to gain new providers (but not a v1 priority)
- ❌ Responsibility for timeout/retry/backoff logic (genai abstracts some; but reqwest covers most)

---

## Final Answer to Luis

**The actual tweaks in alex-cite-checker + wikiharness are straightforward OpenAI-compatible calls with provider-specific HTTP headers and param naming.**

**rust-genai is a multi-provider abstraction that FULLY supports the tweaks SP42 needs (custom auth, custom endpoint, extra headers, HF/OpenRouter), BUT:**

1. **The tweaks genai assists with are trivial to reimplement** — 1–5 lines each
2. **The tweaks genai doesn't assist with are caller responsibility** (size validation, allowlist) — not genai's job
3. **rust-genai adds 30+ transitive deps + pre-1.0 churn risk** for a library handling 20 models SP42 will never use
4. **SP42's scope is narrow:** OpenAI-compatible OpenRouter/Together/OpenAI + HF router (both OpenAI-compatible) + sponsor proxy

**Recommendation: (c) hand-rolled thin adapter over SP42's ModelClient trait.**
- ~50 lines of code
- Zero vendor lock-in
- Sponsor-proxy ready (custom endpoint + custom auth)
- Minimal deps (reqwest + serde_json, already used elsewhere)
- Fully testable, auditable, stable
- Future upgrade path: if SP42 adds Gemini/Claude, add adapters alongside the hand-rolled OpenAI-compatible core

---

## Path Forward

1. **Implement `sp42-live::model::OpenAICompatibleClient`** (hand-rolled, 50–100 lines)
2. **Wire into the citation-finding ADR-0006 verifier** (the endpoint + key + options flow)
3. **Add HF-specific wrapper** (max_tokens clamp, timeout, headers, allowlist)
4. **Test against cassette + live OpenRouter** (ADR-0006 validation)
5. **Defer multi-model panel to ADR-0007** (voting logic); hand-rolled adapter supports it via loop-calling
6. **If/when SP42 adds Gemini/Anthropic:** add provider-specific branches in the same adapter file; no genai required

---

## References

- **alex-cite-checker**: `/var/home/louie/Projects/Volunteering-Consulting/alex-cite-checker/citation-checker-script/core/providers.js`
- **alex proxy**: `/var/home/louie/Projects/Volunteering-Consulting/alex-cite-checker/public-ai-proxy/src/index.js`
- **wikiharness vercel adapter**: `/var/home/louie/Projects/Volunteering-Consulting/wikiharness/packages/edges/src/model/vercel.ts`
- **wikiharness benchmark**: `/var/home/louie/Projects/Volunteering-Consulting/wikiharness/scripts/benchmark.ts`
- **wikiharness model config**: `/var/home/louie/Projects/Volunteering-Consulting/wikiharness/packages/server/src/model-config.ts`
- **rust-genai**: `https://github.com/jeremychone/rust-genai` (v0.7.0-beta.3-WIP, MIT/Apache-2.0, ~30 deps)
  - `src/client/config.rs`: ClientConfig, resolvers, builder
  - `src/chat/chat_options.rs`: ChatOptions with extra_body + extra_headers
  - `src/adapter/adapters/openai/adapter_shared.rs`: extra_body merge
  - `src/adapter/adapters/open_router/adapter_impl.rs`: OpenRouter delegates to OpenAI
  - `src/resolver/{auth_resolver.rs,endpoint.rs}`: Custom auth + custom endpoint

