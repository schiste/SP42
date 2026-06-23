# Research: Sponsor-Proxy Per-Call Authorization for Model API Access

**Research Date:** 2026-06-09  
**Question:** What mechanisms let a sponsor/proxy authorize SPECIFIC calls/prompts (vs. blind forwarding) while keeping the consuming feature's judgment gate local and the proxy unable to tilt results?

---

## Executive Summary

**The Load-Bearing Problem:**
- Client (browser) holds NO provider key → model access must go through a sponsor proxy
- Proxy holds the provider key, pays for inference, but must authorize **per-call** (not blanket forward all)
- Feature's judgment gate (anti-fabrication, scoring) stays on SP42's side; proxy is **transport + budget + authorization only**
- Proxy **cannot run the feature's logic** and therefore **cannot tilt results**

**The Solution Pattern:**
A three-layer authorization model is emerging across industry gateways:
1. **Request metadata + purpose/capability tags** identify which feature/prompt is being called
2. **Deterministic policy rules** at the gateway authorize based on model allowlist, budget, and metadata
3. **Judgment remains local** — the feature queries the proxy for inference only; policy scoring/verification runs entirely on the consuming app's side

---

## Concrete Authorization Mechanisms Found

### 1. LiteLLM Proxy (Leader in per-call granularity)

**Model Access Groups — Per-Key Allowlisting**

- **Mechanism:** Group multiple models under a logical name; grant API keys access to specific groups
- **Config (yaml):** `access_groups: ["beta-models"]` on model definitions
- **API:** Create groups dynamically with `/access_group/new` endpoint without restarting
- **Per-Key Allowlist:** When generating a key, specify `{"models": ["beta-models"]}` — key can only call models in that group
- **Request Validation:** On each request, the proxy checks if `request.model` belongs to an access group assigned to the key; returns `invalid_request_error` if denied
- **Dynamic Updates:** Add/remove models from a group and ALL existing keys get the updated permissions (no key re-issue needed)
- **Wildcard Support:** Enterprise-only patterns like `openai/*` enable prefix-based model restrictions

**Request Identification for Per-Call Tracking**

- **Headers (priority order):**
  1. `x-litellm-customer-id`
  2. `x-litellm-end-user-id`
  3. Custom header via `user_header_mappings`
  4. `user` field (standard OpenAI format)
  5. `litellm_metadata.user`
  6. `metadata.user_id`
- **Spend Tracking:** Customer ID "upserted into DB with new spend" — per-customer cost tracking automatic
- **Use Case:** Browser client sends `x-litellm-customer-id: session-uuid` (not the actual API key) with each request; proxy looks up allowlist for that customer and enforces model restrictions

**Forward Client Headers (BYOK Pattern)**

- **Default:** LiteLLM strips auth headers (`x-api-key`, `x-goog-api-key`) from client requests
- **BYOK Mode:** `forward_llm_provider_auth_headers: true` preserves provider auth so client can supply its own key
- **Feature Isolation:** The proxy still enforces its own model/budget gates independently of client-supplied keys
- **Source:** [LiteLLM Proxy Documentation](https://docs.litellm.ai/docs/proxy/model_access_groups), [How Model Access Works](https://docs.litellm.ai/docs/proxy/model_access_guide)

---

### 2. Cloudflare AI Gateway (Per-Request Control)

**Per-Request Metadata & Scoped Limits**

- **Custom Metadata:** Up to 5 key-value pairs per request (strings/numbers/booleans) visible in logs
- **Scoping:** Limits can be scoped by model, provider, or **custom metadata dimensions** (user, team, application)
- **Per-Request Headers:** `cf-aig-*` headers override gateway-level defaults on a per-request basis
- **Use Case:** Send metadata like `{"feature": "citation-verify", "user_tier": "basic"}` in request; gateway evaluates spent budget + model allowlist against that metadata before forwarding

**Authentication & Authorization Headers**

- **REST API:** Standard `Authorization: Bearer <cloudflare-api-token>`
- **Provider-Native Endpoints:** `cf-aig-authorization: <token>` header instead
- **Request Validation:** Requests without proper auth headers are rejected with 4xx (prevents unauthorized usage)
- **Worker Binding:** Cloudflare Workers pre-authenticate, no manual header needed
- **Source:** [Custom Metadata](https://developers.cloudflare.com/ai-gateway/observability/custom-metadata/), [Authentication](https://developers.cloudflare.com/ai-gateway/configuration/authentication/)

---

### 3. Kong AI Gateway (Policy Enforcement at Gateway Layer)

**Continuous Per-Request Authorization**

- **Access Stage:** During the "access" stage of request processing, Kong's policy plugin instantiates an access function
- **Deterministic Decision:** Plugin calls the authorization service and receives "Allow" or "Deny" (no LLM inference in the policy decision path)
- **AI-Specific Controls:** Enforce authentication (API key, JWT, OAuth 2.0) + granular authorization
- **Fine-Grained Filtering:** Allows/denies based on identity, model allowlist, tool access
- **Default-Deny Posture:** MCP-based agent authorization implements "default deny" — agents only get tools they explicitly need
- **No Code Changes:** Governance applied at gateway layer; consuming feature code stays unchanged

**Separation of Concerns**

- **Policy Enforcement (Deterministic):** Happens at gateway
- **Feature Logic (Judgment):** Runs on consuming app's side
- **Result:** Proxy cannot tilt the verdict because it has no visibility into the feature's decision logic
- **Source:** [Kong AI Gateway](https://developer.konghq.com/ai-gateway/), [MCP Tool ACLs](https://konghq.com/blog/product-releases/mcp-tool-acls-ai-gateway)

---

### 4. Portkey AI Gateway (Metadata-Driven Observability)

**Request Metadata Structure**

- **Metadata:** Arbitrary string key–value pairs on gateway requests
- **Dimensions:** Tag by `user`, `feature`, `environment`, `tenant_id`, `session_id`, `trace_id`
- **Observability Only:** Metadata used for **analytics and cost attribution**, NOT authorization
- **Special Keys:** `_user` drives per-user analytics in the dashboard
- **Warning:** "Never store API keys, passwords, or sensitive PII—only opaque identifiers"

**Enterprise Metadata Enforcement**

- **Validation:** Workspace/API key level enforcement via JSON Schema (ensures required fields present)
- **Limitation:** Metadata presence validation only; does NOT control access
- **Authorization:** Separate from metadata; enforced via scoped per-group/per-project keys with permissions + spending ceilings
- **Source:** [Request Metadata Use Cases](https://portkey.ai/docs/guides/use-cases/metadata-use-cases)

---

### 5. OpenRouter (Virtual API Keys & Model Filtering)

**Scoped API Keys with Model Allowlists**

- **Per-Key Credit Limits:** Optional credit limit during key creation
- **IP Allowlisting:** Restrict keys to IP address ranges; gateway rejects requests from outside the range even if key is valid
- **BYOK Model Filtering:** Optional model filter to restrict keys to specific models (e.g., `openai/gpt-4o` only)
- **API Key Filter (BYOK):** Restrict which OpenRouter API keys can use a BYOK key (isolates usage to specific apps/environments)
- **Usage Tracking:** Monitor usage across daily/weekly/monthly windows per key

**Mechanism:** Model scoping is enforced at request time — if key is scoped to `openai/*` models, request for `anthropic/claude` is rejected

- **Source:** [API Authentication](https://openrouter.ai/docs/api/reference/authentication), [Management API Keys](https://openrouter.ai/docs/guides/overview/auth/management-api-keys), [BYOK](https://openrouter.ai/docs/guides/overview/auth/byok)

---

### 6. Wikimedia Lift Wing API (Bearer Token + Rate-Limit Tiers)

**OAuth2 Bearer Token Authentication**

- **Mechanism:** User authenticates with MediaWiki OAuth, receives bearer token
- **Header:** Standard `Authorization: Bearer <token>` format
- **Rate Limit Encoding:** Rate limit is **encoded in the token itself**; reset token after privilege change to apply new limit
- **Tiers:** 
  - Anonymous (no auth): 50,000 req/hr per IP
  - Authenticated: 100,000 req/hr
  - Wikimedia Enterprise: 200,000 req/hr (requires token elevation)

**Model Access Control (Implicit)**

- Certain ML models/endpoints available only to authenticated users or specific tiers
- Rate-limit tier acts as a proxy for feature/model access
- **Limitation:** Not granular per-model; more of a tier-based approach

- **Source:** [Authentication](https://www.mediawiki.org/wiki/Wikimedia_APIs/Authentication), [Lift Wing API](https://api.wikimedia.org/wiki/Lift_Wing_API)

---

## Deterministic Policy vs. LLM-Based Filtering

**Key Finding from Academic Research:**

Authorization proxies should use **deterministic policy evaluation**, NOT LLM-based judgment:
- **Deterministic Policy:** Returns same decision for same inputs every time; no model inference in the evaluation path
- **LLM-Based Filter:** Reads request as natural language, produces judgment ("seems okay given context"); can be wrong
- **Anti-Tilt Guarantee:** When proxy uses deterministic rules, it cannot run the feature's judgment and therefore cannot tilt results
- **Layering:** Request arrives → deterministic gateway policy (Allow/Deny) → if approved, reaches feature logic → feature runs its own judgment (anti-fabrication, scoring, etc.)

**Source:** ["Before the Tool Call: Deterministic Pre-Action Authorization for Autonomous AI Agents" (arxiv 2603.20953)](https://arxiv.org/html/2603.20953v1), [Data443 Report](https://data443.com/blog/agent-to-agent-proxy-security/)

---

## Browser Client → Proxy → Model Architecture

**The Challenge:**
- Browser client has NO provider API key (can't store secrets in browser)
- Must reach a proxy that holds the real key
- Proxy must authenticate the client + authorize per-call
- Proxy must NOT tilt feature results

**Three Endpoint Modes (from ADR-0006):**

| Mode | Client → Proxy Auth | Proxy → Model Auth | Budget Owner | Feature Judgment |
|------|---|---|---|---|
| **Local** | None (same-origin) | Browser's own key (if provided) | Client | Client-side |
| **Direct** | Client's own token | Client's token or key | Client | Client-side |
| **Sponsor Proxy** | Session-scoped credential (proxy token) | Sponsor's provider key | Sponsor | SP42's feature (not proxy) |

**Sponsor-Proxy Concrete Pattern:**

1. **Browser client requests inference:**
   ```http
   POST https://proxy.example.com/chat/completions
   Authorization: Bearer <session-scoped-proxy-token>
   Content-Type: application/json
   
   {
     "model": "openai/gpt-4o-mini",
     "messages": [...],
     "x-litellm-customer-id": "session-uuid-12345",
     "litellm_metadata": {"feature": "citation-verify"}
   }
   ```

2. **Proxy validates:**
   - Token maps to SP42 account (session-scoped, time-limited, no provider key exposure)
   - Customer ID in request matches token's assigned customer/tenant
   - Model `openai/gpt-4o-mini` is in the allowlist for this customer's tier
   - Budget remaining for this month/day/customer
   - Feature tag `citation-verify` is allowed (allow/deny list per feature)
   - **Decision: Allow or Deny** (deterministic rules, no LLM inference)

3. **If allowed, proxy forwards:**
   ```http
   POST https://api.openai.com/v1/chat/completions
   Authorization: Bearer sk-... (sponsor's real key)
   ...request body...
   ```

4. **Response returns to client; SP42 runs feature logic:**
   - Receives model response
   - Runs anti-hallucination gate (grounding check)
   - Runs voting/scoring (if multi-model)
   - Emits verdict or proposal
   - **Proxy never sees this logic; cannot tilt results**

**Key Insight:**
- Session-scoped proxy token is the **transport credential** (low-privilege, can't directly call models)
- Customer ID + feature metadata enable the proxy to enforce allowlists
- Proxy is **stateless policy** (no logic, no models, no secrets)
- Feature judgment stays entirely in SP42 code

---

## Request DTO Shape Recommendation for SP42

**Client-to-Proxy Request (OpenAI-compatible format with metadata):**

```typescript
// Base OpenAI format (inherited)
{
  model: string,           // e.g., "openai/gpt-4o-mini"
  messages: ChatMessage[],
  temperature?: number,
  max_tokens?: number,
  // ... standard OpenAI fields
  
  // Per-Call Authorization Metadata
  user?: string,           // Customer ID (required in sponsor-proxy mode)
  litellm_metadata?: {     // LiteLLM standard
    customer_id?: string,
    feature?: string,      // e.g., "citation-verify", "bare-url-repair"
    tenant?: string,
    use_case?: string,     // Purpose/capability tag
  },
  // OR Cloudflare style
  x-custom-metadata?: Record<string, string | number | boolean>,
}
```

**Proxy Configuration (ENV + config.yaml):**

```yaml
# .env or config
SPONSOR_PROXY_TOKEN_SECRET=<secret>  # Signs session-scoped tokens
SPONSOR_PROXY_ENDPOINT=https://proxy.example.com
SPONSOR_MODEL_ALLOWLIST=               # JSON array or file path
  ["openai/gpt-4o-mini", "anthropic/claude-3.5-sonnet"]

# config.yaml (LiteLLM style)
model_info:
  access_groups:
    citation-verify: ["openai/gpt-4o-mini", "anthropic/claude-3.5-sonnet"]
    bare-url-repair: ["openai/gpt-4o-mini"]

model_access:
  customers:
    sp42-premium:
      models: citation-verify
      budget_monthly: 500_000_tokens
    sp42-free:
      models: bare-url-repair
      budget_monthly: 50_000_tokens
```

**Proxy Authorization Rules (Deterministic):**

```python
def authorize_request(token, customer_id, model, feature) -> bool:
    # All must pass (AND logic, no short-circuit — all gates visible)
    checks = {
        "token_valid": verify_session_token(token),
        "customer_exists": customer_exists(customer_id),
        "customer_match": token.customer_id == customer_id,
        "model_allowed": model in ALLOWLIST[feature][customer_tier],
        "budget_remaining": get_remaining_budget(customer_id) > min_tokens,
        "feature_enabled": is_feature_enabled(feature, customer_tier),
    }
    return all(checks.values()), checks  # Return reasons for logging/debugging
```

---

## License & Dependency Implications

**LiteLLM:** Apache-2.0; ~50 transitive deps; already used in landscape  
**Cloudflare AI Gateway:** Proprietary; managed service (no self-hosted OSS version for DIY)  
**Kong AI Gateway:** Proprietary + OSS Kong Gateway (Tyk competitor); community/commercial tiers  
**Portkey:** Proprietary gateway; vendor lock-in (use for observability, not policy)  
**OpenRouter:** Proprietary SaaS; use only as an example of scoping patterns

**For SP42:**
- **If building self-hosted proxy:** Adapt LiteLLM's `model_access_groups` + `customer_id` patterns (permissive license)
- **If using managed gateway:** Cloudflare AI Gateway or OpenRouter (but commit to deterministic rule evaluation, not delegation)
- **Key principle:** Policy evaluation is deterministic; feature logic is where judgment lives

---

## Canonical References

### Tier 1 (Official Documentation)

- [LiteLLM Model Access Groups](https://docs.litellm.ai/docs/proxy/model_access_groups)
- [LiteLLM How Model Access Works](https://docs.litellm.ai/docs/proxy/model_access_guide)
- [LiteLLM Customers & Cost Tracking](https://docs.litellm.ai/docs/proxy/customers)
- [Cloudflare AI Gateway Custom Metadata](https://developers.cloudflare.com/ai-gateway/observability/custom-metadata/)
- [Cloudflare AI Gateway Authentication](https://developers.cloudflare.com/ai-gateway/configuration/authentication/)
- [Kong AI Gateway](https://developer.konghq.com/ai-gateway/)
- [OpenRouter API Authentication](https://openrouter.ai/docs/api/reference/authentication)
- [OpenRouter Management API Keys](https://openrouter.ai/docs/guides/overview/auth/management-api-keys)
- [Wikimedia Wikimedia APIs Authentication](https://www.mediawiki.org/wiki/Wikimedia_APIs/Authentication)

### Tier 2 (Academic / Research)

- ["Before the Tool Call: Deterministic Pre-Action Authorization for Autonomous AI Agents"](https://arxiv.org/html/2603.20953v1) (arxiv 2603.20953) — establishes that deterministic policy ≠ LLM judgment
- [Data443 Report: "Why Agent-to-Agent Proxies Need Deterministic Policy, Not LLM-Based Filters"](https://data443.com/blog/agent-to-agent-proxy-security/)
- ["API Authorization Patterns: A 2026 Practitioner's Guide"](https://guptadeepak.com/ciam-compass/guides/api-authorization-patterns/) — consensus on OAuth 2.1 + scopes

### Tier 3 (Community / Blog)

- [Zuplo: "Best API Gateways for AI and LLM Workloads (2026)"](https://zuplo.com/learning-center/best-api-gateways-ai-llm-workloads-2026)
- [Medium: "AI Patterns: The Capability Filter Proxy"](https://medium.com/@toddschilling_45518/ai-patterns-the-capability-filter-proxy-524f2a2a5b1b)

---

## Summary Table: Authorization Mechanisms by Gateway

| Gateway | Per-Call Granularity | Model Allowlist | Metadata/Tags | Budget Enforcement | Policy Isolation |
|---------|---|---|---|---|---|
| **LiteLLM** | `access_groups` + customer_id headers | ✓ Dynamic groups | ✓ x-litellm-customer-id, metadata | ✓ Per-customer tracking | ✓ (deterministic) |
| **Cloudflare AI** | `cf-aig-*` headers + metadata | ✓ Scoped limits | ✓ Up to 5 key-value pairs | ✓ Alerts (no hard enforce) | ✓ (deterministic) |
| **Kong AI** | Plugin-based access stage | ✓ ACL + MCP tool filtering | ✓ Headers + request context | ✓ (with plugins) | ✓ (deterministic) |
| **Portkey** | Request metadata (observability only) | ✗ (via separate key scoping) | ✓ Feature, environment, tenant, session | ✓ (observability only) | ✓ (separate from metadata) |
| **OpenRouter** | Scoped API keys | ✓ Per-key model allowlist + BYOK | ✗ (not first-class) | ✓ Per-key credit limits | ✓ (scoping is deterministic) |
| **Wikimedia Lift Wing** | Bearer token + tier | ✓ (implicit per tier) | ✗ | ✓ (rate-limit tiers) | ✓ (tier-based) |

---

## Recommendation for SP42

**Use a three-part design:**

1. **Endpoint Configuration Shape:**
   ```rust
   pub struct ModelEndpointConfig {
       pub mode: EndpointMode, // Local | Direct | SponsorProxy
       pub base_url: String,
       pub proxy_token: Option<String>, // For sponsor-proxy mode (session-scoped, NOT provider key)
       pub capability_tag: String,      // "citation-verify", "bare-url-repair"
       pub allowed_models: Vec<String>, // Allowlist enforced at proxy or locally
   }
   
   pub enum EndpointMode {
       Local,        // No key needed; feature logic gates model choice
       Direct,       // Client token required; feature logic gates model choice
       SponsorProxy, // Proxy token required; proxy enforces model allowlist + budget
   }
   ```

2. **Request DTO (extend OpenAI-compatible):**
   - Inherit base `ChatCompletionRequest` (model, messages, etc.)
   - Add optional metadata: `customer_id`, `feature_tag`, `use_case`
   - Proxy uses metadata to enforce allowlist; feature logic never runs in proxy

3. **Authorization Rule (Deterministic):**
   ```
   For SponsorProxy mode:
     - Verify proxy_token is valid, not expired
     - Look up customer_id from token
     - Fetch allowed_models for (customer_id, capability_tag)
     - Check requested model is in allowed_models
     - Check budget remaining
     → If all pass: forward to proxy
     → If any fail: return 403 + reason list (not suppressed, logged)
   
   For feature judgment:
     - Receive response from proxy
     - Run SP42's anti-fabrication gate (grounding check)
     - Run voting / scoring (if multi-model)
     - Feature judgment is NEVER influenced by proxy's authorization decision
   ```

4. **Session-Scoped Proxy Token (not the provider key):**
   - Signed by SP42 (HMAC-SHA256 or similar)
   - Contains: customer_id, expiry, allowed_features, tier
   - Proxy validates signature before lookup
   - Rotates frequently (per-session or hourly)
   - **Never contains the actual provider API key**

**This design ensures:**
- ✓ Browser client holds NO provider key
- ✓ Proxy authorizes per-call via allowlists + budget
- ✓ Feature judgment stays in SP42 code
- ✓ Proxy cannot tilt results (no access to feature logic)
- ✓ Deterministic policy (no LLM in authorization path)
- ✓ Cost attribution to customer (proxy tracks spend)

---

**Last Updated:** 2026-06-09  
**Status:** Research complete; recommendations ready for ADR-0006 refinement
