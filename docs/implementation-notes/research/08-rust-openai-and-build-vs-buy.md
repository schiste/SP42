# Research: Rust OpenAI-Compatible HTTP Client Options & Build-vs-Buy for SP42

## Executive Summary

For SP42's first cut (single OpenAI-compatible chat-completions endpoint behind a sponsor proxy), a **hand-rolled ~100-line adapter over reqwest is the recommended approach**. The reasoning: minimal first-cut shape (one request/response type), strong existing dependencies (reqwest+serde already in use), and clear abstraction boundary. Adopt a third-party multi-provider crate only when a real second provider or shape emerges.

---

## Research Findings

### 1. async-openai (v0.41.0)

**Status:** Active, production-ready  
**Repository:** https://github.com/64bit/async-openai  
**License:** MIT  
**Stars:** 1.9k (as of 2026-06-04)  
**Last Release:** 2026-06-04  
**Rust MSRV:** 1.75  

**Features:**
- Full OpenAI API coverage (chat, completions, embeddings, images, audio, assistants, etc.)
- Support for OpenAI, Azure OpenAI, and OpenAI-compatible providers
- Custom base URL: **YES** — `OpenAIConfig::with_api_key()` + custom endpoints via Azure-style configuration
- Custom headers: **YES** — `.header()` / `.headers()` methods on API groups; "additive - adds to existing query or headers"
- Proxy support: **YES** — via custom `reqwest::Client` passed to `Client::with_http_client()`
- Streaming support (Server-Sent Events, WebSocket realtime)
- Tower service support for advanced routing

**Dependencies (transitive count ~50+):**
- reqwest (with json, stream, multipart, query features)
- tokio (with fs, macros)
- serde / serde_json
- tower (with limit, retry, timeout, util)
- futures / tokio-util
- eventsource-stream, tokio-tungstenite (WebSocket)
- secrecy, hmac, sha2 (webhook verification)

**Config struct methods** (from docs.rs):
```rust
pub struct Client<C: Config> { /* private */ }

// Configuration:
impl OpenAIConfig {
    pub fn new() -> Self
    pub fn with_api_key(self, api_key: String) -> Self
    pub fn with_org_id(self, org_id: String) -> Self
    pub fn with_http_client(self, client: HttpClient) -> Self
}

// Usage:
let config = OpenAIConfig::new()
    .with_api_key("...")
    .with_org_id("...");
let client = Client::with_config(config);
```

**Custom HTTP Client (for proxies/custom auth):**
```rust
let http_client = reqwest::ClientBuilder::new()
    .proxy(reqwest::Proxy::https("http://proxy.example.com")?)
    .default_headers(/* custom headers */)
    .build()?;

let client = Client::new().with_http_client(http_client);
```

**Assessment:** Very capable, but brings 50+ transitive deps. If SP42 only needs one shape (chat completions), overkill. Good fallback if second provider arrives.

---

### 2. openai-api-rs (v10.0.1)

**Status:** Minimally documented, unmaintained appearance  
**Repository:** https://github.com/dongri/openai-api-rs  
**License:** MIT  
**Documentation Coverage:** 0.13% (minimal inline docs)  
**Architecture:** Builder-based with two modules: `v1` (standard API) + `realtime` (WebSocket)  

**Core Dependencies:**
- reqwest, tokio, serde/serde_json
- tokio-tungstenite (WebSocket)
- futures-util
- Custom builder macro: `impl_builder_methods`

**Assessment:** Lightweight compared to async-openai but severely undocumented. Repository activity unclear. Not recommended for production without significant documentation investment.

---

### 3. ollama-rs (v0.3.4)

**Status:** Active; specialized for Ollama (not OpenAI-compatible)  
**Repository:** https://github.com/LlamaEdge/ollama-rs  
**Purpose:** Ollama model server interaction (local LLM inference)  
**License:** Not documented in crate metadata  

**Assessment:** WRONG TOOL for OpenAI-compatible endpoint. Ollama API ≠ OpenAI API. Useful only if SP42 adds local Ollama support as a separate provider path.

---

### 4. openai_dive (v1.4.3)

**Status:** Active, well-featured  
**Repository:** Implied from docs.rs  
**License:** MIT  
**Features:**
- Chat completions, vision, audio, images, embeddings, fine-tuning, batch, web search, realtime (WebSocket)
- **Custom base URL: YES** — "By simply changing the base URL, you can use this crate with other OpenAI-compatible APIs"
- Example: DeepSeek compatibility confirmed

**Assessment:** Feature-complete but brings similar dept weight as async-openai. Better if needing multi-shape support, but overkill for MVP-1.

---

### 5. Hand-Rolled Adapter over reqwest

**Rationale:**
- SP42 already depends on reqwest (rustls), serde, serde_json
- First cut = ONE shape (OpenAI chat-completions POST)
- ~100–150 lines for: request builder → post → parse response
- Full control over headers, auth, base URL, proxy

**OpenAI Chat Completions API Contract** (from OpenAI docs):

**Request:**
```
POST /v1/chat/completions
Authorization: Bearer {api_key}
Content-Type: application/json

{
  "model": "gpt-4o",
  "messages": [
    { "role": "system", "content": "..." },
    { "role": "user", "content": "..." }
  ],
  "temperature": 0.7,
  "top_p": 1.0,
  "max_completion_tokens": 1024,
  "user": "user-123"  // optional; for usage tracking
}
```

**Response:**
```json
{
  "id": "chatcmpl-...",
  "object": "chat.completion",
  "created": 1234567890,
  "model": "gpt-4o",
  "choices": [
    {
      "index": 0,
      "message": {
        "role": "assistant",
        "content": "..."
      },
      "finish_reason": "stop"
    }
  ],
  "usage": {
    "prompt_tokens": N,
    "completion_tokens": M,
    "total_tokens": N+M
  }
}
```

**Proxy Authorization Mechanisms:**

For a **sponsor proxy** (e.g., a Wikidata/Wikimedia foundation-operated endpoint), the request flow is:

1. **Browser shell** (no provider key) → **Sponsor Proxy** (holds real key)
2. **SP42 CLI** (custom token) → **Sponsor Proxy** (forwards real key)

**Required header forwarding / proxy-auth patterns:**

| Field/Header | Purpose | Forwarding |
|---|---|---|
| `Authorization: Bearer <token>` | Auth to sponsor proxy (NOT OpenAI) | Proxy's own token; proxy strips & re-auths to real provider |
| `model` (request body) | Model selection | Forwarded as-is or proxy-overridden |
| `user` (request body) | Optional; usage attribution | Forwarded; helps proxy track per-user quotas |
| `Custom-Header` (request) | Metadata/tags for proxy logging | Proxy can add/forward if design supports |
| `X-OpenAI-Organization` | Org routing (native OpenAI) | Forwarded if proxy supports multi-org |

**Minimal SP42 trait design:**

```rust
#[async_trait::async_trait]
pub trait LlmClient: Send + Sync {
    async fn chat_completion(
        &self,
        req: ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse, LlmError>;
}

pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub user: Option<String>,
}

pub struct Message {
    pub role: MessageRole,
    pub content: String,
}

pub enum MessageRole {
    System,
    User,
    Assistant,
}

pub struct ChatCompletionResponse {
    pub id: String,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Usage,
}

pub struct Choice {
    pub message: Message,
    pub finish_reason: String,
}

pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}
```

**Hand-rolled implementation** (~120 lines):

```rust
use async_trait::async_trait;
use reqwest::Client as ReqwestClient;
use serde_json::json;

pub struct OpenAICompatibleClient {
    http_client: ReqwestClient,
    base_url: String,
    auth_token: String,
}

impl OpenAICompatibleClient {
    pub fn new(base_url: String, auth_token: String) -> Self {
        Self {
            http_client: ReqwestClient::new(),
            base_url,
            auth_token,
        }
    }

    pub fn with_proxy(base_url: String, auth_token: String, proxy_url: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let client = ReqwestClient::builder()
            .proxy(reqwest::Proxy::https(proxy_url)?)
            .build()?;
        Ok(Self {
            http_client: client,
            base_url,
            auth_token,
        })
    }
}

#[async_trait]
impl LlmClient for OpenAICompatibleClient {
    async fn chat_completion(
        &self,
        req: ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse, LlmError> {
        let url = format!("{}/v1/chat/completions", self.base_url);
        
        let body = json!({
            "model": req.model,
            "messages": req.messages.iter().map(|m| {
                json!({
                    "role": format!("{:?}", m.role).to_lowercase(),
                    "content": m.content
                })
            }).collect::<Vec<_>>(),
            "temperature": req.temperature.unwrap_or(0.7),
            "max_tokens": req.max_tokens,
            "user": req.user,
        });

        let response = self.http_client
            .post(&url)
            .bearer_token(&self.auth_token)
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            return Err(LlmError::HttpError(response.status().as_u16()));
        }

        let data: serde_json::Value = response.json().await
            .map_err(|e| LlmError::ParseError(e.to_string()))?;

        // Parse response into ChatCompletionResponse
        // ... (brief parsing logic)
        
        Ok(ChatCompletionResponse {
            id: data["id"].as_str().unwrap_or("").to_string(),
            model: data["model"].as_str().unwrap_or("").to_string(),
            choices: vec![], // populate from data["choices"]
            usage: Usage {
                prompt_tokens: data["usage"]["prompt_tokens"].as_u64().unwrap_or(0) as u32,
                completion_tokens: data["usage"]["completion_tokens"].as_u64().unwrap_or(0) as u32,
                total_tokens: data["usage"]["total_tokens"].as_u64().unwrap_or(0) as u32,
            },
        })
    }
}
```

**Advantages:**
- 0 new transitive deps (uses existing reqwest/serde)
- Full control over headers, base URL, auth token injection
- Trivial proxy support (reqwest handles it natively)
- Easy to extend (add new fields as needed)
- Minimal scope drift (MVP-1 = one shape only)

**Disadvantages:**
- No streaming support out-of-box (but can add with `eventsource-stream` if needed)
- Must maintain response parsing as OpenAI API evolves
- No multi-provider abstraction (but unnecessary for MVP-1)

---

## Build-vs-Buy Decision Matrix

| Criterion | Hand-Rolled | async-openai | openai_dive | openai-api-rs |
|---|---|---|---|---|
| **Transitive deps** | 0 new | ~50+ | ~40+ | ~20+ |
| **First-cut scope (1 shape)** | Perfect fit | Overkill | Overkill | Acceptable |
| **Custom base_url** | Native | Yes | Yes | Unclear |
| **Custom auth headers** | Native | Yes (custom client) | Likely | Unclear |
| **Proxy support** | Native (reqwest) | Yes (reqwest) | Yes (reqwest) | Yes (reqwest) |
| **Documentation** | Self-evident | Excellent | Good | Poor (0.13%) |
| **Maintenance** | Self | Active | Active | Minimal |
| **Streaming (nice-to-have)** | Manual add | Built-in | Built-in | Built-in |
| **License** | N/A | MIT | MIT | MIT |

---

## Recommendation: Hand-Rolled Adapter (MVP-1)

### For SP42 (first cut):

1. **Define your LlmClient trait** (as shown above) — ~30 lines
2. **Implement OpenAICompatibleClient** — ~120 lines, trait impl
3. **Test against a real proxy endpoint** — use cassettes for hermetic tests
4. **Add to `sp42-core` or new `sp42-llm` subpackage**

**Transition path:**
- **MVP-1:** Hand-rolled + one provider (sponsor proxy)
- **If a second provider arrives:** Evaluate async-openai or openai_dive; migrate internal impl; keep LlmClient trait stable
- **No upstream migration pressure:** Keep the trait, swap the impl; wikiharness model layer stays independent

### For wikiharness (ongoing):

- Keep **VercelModelClient** (AI SDK wrapper) — proven, battle-tested
- Do NOT force async-openai into wikiharness; wikiharness is model-agnostic (already supports both Vercel SDK + ScriptedModelClient)
- If wikiharness needs native OpenAI provider: add it as a new `ModelClient` impl, not by adopting async-openai library

---

## OpenAI API Request/Response Shape Summary

**Request fields to forward through proxy:**

- `model` — string; cannot be empty
- `messages` — array of `{ role, content }`; at least one required
- `temperature` — 0.0–2.0; default 1.0
- `top_p` — 0.0–1.0; default 1.0
- `max_completion_tokens` — integer; optional
- `user` — string; optional; used for abuse monitoring & per-user quotas

**Response fields to consume:**

- `choices[].message` — `{ role, content }`
- `choices[].finish_reason` — "stop", "length", "tool_calls", etc.
- `usage` — `{ prompt_tokens, completion_tokens, total_tokens }`
- `model` — echo of request model (or actual used model if proxy overrides)
- `id` — request ID (for logging/audit)

**Headers a sponsor proxy would need to forward:**

| Header | Proxy Action |
|---|---|
| `Authorization` | **REPLACE** with real provider key; enforce sponsor auth token instead |
| `Content-Type` | Pass through (should be `application/json`) |
| `User-Agent` | Pass through (add proxy ID if audit needed) |
| Custom metadata headers | Pass through if proxy design supports tagging |

**Browser shell security (if web client added later):**

- Never send provider key to browser
- Browser sends request to `POST /api/llm/chat-completion` (CSRF-protected)
- Server holds sponsor-proxy token; sends request on client's behalf
- Response streamed back to browser

---

## Conclusion

**Recommend hand-rolled ~100-line adapter for SP42 MVP-1.** It:
- Avoids 40+ transitive deps for a single shape
- Gives full control over proxy auth & base URL
- Keeps SP42 composable & lightweight
- Provides a clear exit ramp if a second provider arrives (migrate impl, keep trait)
- Aligns with SP42's foundational philosophy: minimal, owned, auditable

**Sources:**
- async-openai 0.41.0: https://docs.rs/async-openai/0.41.0/
- docs.rs async-openai config: https://docs.rs/async-openai/latest/async_openai/config/
- openai-api-rs 10.0.1: https://docs.rs/openai-api-rs/10.0.1/
- openai_dive 1.4.3: https://docs.rs/openai_dive/latest/
- reqwest 0.13.4: https://docs.rs/reqwest/latest/
- Tower Service trait: https://docs.rs/tower-service/latest/tower_service/trait.Service.html
- Rust API Guidelines: https://rust-lang.github.io/api-guidelines/
- Hyper: https://github.com/hyperium/hyper (16.1k ⭐, MIT, low-level HTTP)
- OpenAI Python client (base_url/proxy patterns): https://github.com/openai/openai-python
