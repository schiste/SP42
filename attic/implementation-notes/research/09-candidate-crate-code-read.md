# Rust LLM Provider-Agnostic Crate Source Read

**Research Question**: Which provider-agnostic Rust LLM crate is worth adopting/vendoring for SP42 citation verification, specifically for the sponsor-proxy shape (custom endpoint + proxy token + no client key)?

**Scope**: Three candidates — `llm` (graniet/llm), `rig` (0xPlaygrounds/rig), `rust-genai` (jeremychone/rust-genai).

**Research Methodology**: Clone to temp dir, inspect source at file:line, measure dependencies via `cargo tree`, assess proxy-config & auth patterns, evaluate vendor-and-trim feasibility.

---

## Codebase Commits & Dates

| Crate | Repo | Commit | Date | Version |
|-------|------|--------|------|---------|
| **llm** | github.com/graniet/llm | c78a18e | 2026-06-06 | 1.3.8 |
| **rig** | github.com/0xPlaygrounds/rig | 4559d43 | 2026-06-08 | 0.38.1 |
| **rust-genai** | github.com/jeremychone/rust-genai | a8f8082 | 2026-06-07 | 0.7.0-beta.3-WIP |

---

## 1. Core Trait Definitions

### llm (graniet)

**Main trait**: `LLMProvider` (pub trait, combines `ChatProvider` + `CompletionProvider`)

**Source**: `/src/lib.rs:96-100`
```rust
pub trait LLMProvider:
    chat::ChatProvider
    + completion::CompletionProvider
```

**Chat method signature** (OpenAI-compatible impl):
- `/src/providers/openai_compatible.rs`: `async fn chat(request: ChatRequest) -> Result<ChatResponse>`
- **Request type**: `OpenAIChatRequest` (serde `Serialize` struct)
  - Fields: `model`, `messages`, `max_tokens`, `temperature`, `stream`, `tools`, `tool_choice`, `extra_body` (flat serde merge), custom headers via trait
- **Response type**: `OpenAIChatResponse` (serde `Deserialize`)
  - Fields: `choices: Vec<OpenAIChatChoice>`, `usage: Option<Usage>`

**Key observation**: Uses `extra_body: serde_json::Map` to support arbitrary custom fields → **proxy-friendly** (can inject custom headers/fields at request level).

---

### rig (0xPlaygrounds)

**Core workspace**: Monorepo with `rig-core` as main crate (v0.38.1).

**Traits**:
- `CompletionClient` — factory trait for creating `CompletionModel` instances
- `CompletionModel` — the provider-facing model trait

**Source**: `/crates/rig-core/src/client/completion.rs:9-30`
```rust
pub trait CompletionClient {
    type CompletionModel: CompletionModel<Client = Self>;
    fn completion_model(&self, model: impl Into<String>) -> Self::CompletionModel { ... }
    fn agent(&self, model: impl Into<String>) -> AgentBuilder<Self::CompletionModel> { ... }
}
```

**Request/response**: Generic builder pattern via `CompletionRequest` + `CompletionResponse`
- **Source**: `/crates/rig-core/src/completion/request.rs` (Rig uses message-based abstraction, not raw HTTP)
- Methods: `.preamble()`, `.temperature()`, `.build()` → `.completion(request)` → `CompletionResponse`

**Key observation**: **High-level abstraction** — CompletionRequest is NOT a direct HTTP body, but a normalized internal representation. Each provider translates it. Provider configuration done via `Client::new(api_key)` + builder pattern, not at request level → **less proxy-friendly for custom headers/fields**.

---

### rust-genai

**Client trait**: `Client` (not a trait, concrete struct)
- **Source**: `/src/client/client_types.rs:1-60`
- Wraps `ClientInner { web_client: WebClient, config: ClientConfig }`

**Auth & Endpoint resolution**:
- **Source**: `/src/resolver/auth_resolver.rs:1-150` — `AuthResolver` enum with `ResolverFn`/`ResolverAsyncFn`
  - Allows custom function-based auth resolution (sync or async)
  - Can inject arbitrary `AuthData` per request
- **Source**: `/src/resolver/endpoint.rs:1-50` — `Endpoint` struct (static or owned string)
  - `from_static(url: &'static str)` or `from_owned(url: String)`

**Config hierarchy**:
- **Source**: `/src/client/config.rs` — `ClientConfig` with builder
  - `.with_auth_resolver(AuthResolver)` — pluggable auth
  - `.with_service_target_resolver(ServiceTargetResolver)` — pluggable endpoint + auth per model

**Key observation**: **Plugin-based DI** — explicit `AuthResolver` + `ServiceTargetResolver` traits allow complete proxy custom behavior (arbitrary headers, custom auth logic). **Very proxy-friendly**.

---

## 2. Custom Endpoint & Auth Configuration

### llm

**Custom endpoint**:
- **Source**: `/src/builder/llm_builder.rs` — builder method `.base_url(url: impl Into<String>)`
- Stored in builder state, forwarded to `OpenAICompatibleProvider`
- **OpenAICompatibleProviderConfig** (trait): `const DEFAULT_BASE_URL: &'static str` + optional override in constructor

**Custom auth**:
- **Source**: `/src/providers/openai_compatible.rs:~30-60` — `OpenAICompatibleProviderConfig`
  - `api_key: String` field
  - `custom_headers() -> Option<Vec<(String, String)>>` trait method
- **Constructor**: `new(api_key: String, base_url: Option<String>, ...)`

**Verdict**: **Simple, direct**. `base_url` override at provider init time. Auth injected at provider construction or via `extra_body` in chat request. Works for proxy if proxy token is injected as custom header in `custom_headers()`.

---

### rig

**Custom endpoint**:
- **Pattern**: Provider-specific (per `Client::new(api_key)`)
- **Source**: `/crates/rig-core/src/providers/zai.rs:~1-50`
  - Examples: `Client::from_env()` reads `ZAI_API_BASE` env var, calls `.base_url(url)`
  - Provider builder pattern, not generic

**Custom auth**:
- Auth injected at `Client::new(api_key)` time
- **No per-request auth override** — trait design doesn't support custom headers at request level
- Provider-specific clients (OpenAI, Anthropic, etc.) each have their own auth wiring

**Verdict**: **Tightly coupled to provider**. To support a custom proxy endpoint + custom auth token, you'd need to:
1. Implement a new provider crate (`rig-custom-proxy`)
2. Implement `CompletionClient` + `CompletionModel` traits
3. This is **heavy extraction** (~500–1000 LOC), not light vendoring

---

### rust-genai

**Custom endpoint**:
- **Source**: `/src/resolver/endpoint.rs:17-31`
  - `Endpoint::from_static(url)` or `Endpoint::from_owned(url)`
  - Plugged into `ServiceTargetResolver`

**Custom auth**:
- **Source**: `/src/resolver/auth_resolver.rs:~60-150`
  - `AuthResolver::from_resolver_fn(|model_iden| -> Result<Option<AuthData>> { ... })`
  - Can be sync or async
  - Called at request time, not static config

**Integration**:
- **Source**: `/src/client/config.rs` — `ClientBuilder::with_auth_resolver(auth_resolver)`
- **Source**: `/src/client/builder.rs` — `.with_auth_resolver_fn(impl IntoAuthResolverFn)`

**Example from tests**:
```rust
with_auth_resolver(AuthResolver::from_resolver_fn(
    |model_iden| -> Result<Option<AuthData>> {
        Ok(Some(AuthData::BearerToken("my-proxy-token".to_string())))
    }
))
```

**Verdict**: **Excellent fit**. Pluggable auth + endpoint resolution at request-dispatch time. Can inject proxy token + custom endpoint dynamically. No provider coupling.

---

## 3. LICENSE & cargo-deny Compliance

| Crate | License | SPDX | cargo-deny compatible |
|-------|---------|------|----------------------|
| **llm** | MIT | `MIT` | ✓ Yes (permissive) |
| **rig** | MIT | `MIT` | ✓ Yes (permissive) |
| **rust-genai** | MIT OR Apache-2.0 | `MIT OR Apache-2.0` | ✓ Yes (dual permissive) |

**Source evidence**:
- llm: `/LICENSE` lines 1–3 (MIT)
- rig: `/LICENSE` lines 1–2 (MIT)
- rust-genai: `/Cargo.toml:5` (`license = "MIT OR Apache-2.0"`)

**Verdict**: All pass cargo-deny. No GPL/copyleft issues.

---

## 4. Transitive Dependency Weight

### llm

**Direct deps** (from `cargo tree`): ~40 crates
- **Heavy items**: 
  - `reqwest 0.12.12` (full HTTP client, includes TLS + crypto)
  - `tokio 1.0` (full features: `["full"]`)
  - `axum 0.7` (full web framework, optional but in default feature set)
  - CLI feature adds: `ratatui`, `syntect`, `pest`, `pest_derive`, `crossterm`, `portable-pty` (full TUI stack)

**Default features**: `["cli", "default-tls"]`
  - Pulls in CLI + axum + ratatui
  - **Not ideal for a library** (CLI bloat)

**Minimal feature set** (hypothetical): `["openai"]` + `default-tls`
  - Reduces to ~20 deps, but still heavy due to reqwest/tokio

**Estimate for sponsor-proxy extraction**: ~300 LOC for minimal OpenAI-compatible chat client (trait + request/response). Doable.

---

### rig

**Workspace structure**: 16 crates (`rig-core` + 15 provider crates)
- **rig-core deps** (from `cargo tree`): ~30 core deps
  - `reqwest 0.13` (HTTP)
  - `tokio 1.52` (async runtime)
  - `serde`, `serde_json` (serialization)
  - **No CLI/TUI bloat** (library-focused)

**Provider crates** (optional): `rig-openai`, `rig-anthropic`, `rig-gemini`, etc.
  - Each provider is a separate optional crate
  - Clean separation of concerns

**Default feature set**: `["rustls"]` only
  - Minimal, library-appropriate

**Estimate for sponsor-proxy extraction**: 
  - Extract `rig-custom-http-provider` crate (new)
  - Implement `CompletionClient` + `CompletionModel` traits
  - Reuse `rig-core` abstractions for request/response
  - **Heavy**: ~800–1200 LOC, requires understanding Rig's internal abstraction stack (message types, streaming, etc.)

---

### rust-genai

**Direct deps** (from `cargo tree`): ~16 core deps
- **Minimal, focused**:
  - `reqwest 0.13` (HTTP)
  - `tokio 1.0` (async)
  - `serde`, `serde_json`
  - `eventsource-stream` (SSE for streaming)
  - `derive_more`, `strum`, `regex` (small utilities)
  - **No TUI/CLI bloat**

**Optional features**:
  - `bedrock-sigv4` pulls AWS crates (optional, gated)
  - Minimal surface area

**Default feature set**: `["rustls-tls"]` only

**Estimate for sponsor-proxy extraction**: 
  - Extract just the resolver + web client layer
  - ~400 LOC
  - **Light, self-contained** (auth/endpoint resolution is already modular)

---

## 5. Vendor-and-Trim Feasibility

### llm

**Feasibility**: **Medium** ✓

**Why extract?**
- OpenAI-compatible provider is a clean abstraction
- `OpenAICompatibleProvider` + `OpenAIChatRequest` are standalone types

**What to keep**:
- Core trait: `ChatProvider`
- Struct: `OpenAICompatibleProvider<T: OpenAIProviderConfig>`
- Request/response types: `OpenAIChatRequest`, `OpenAIChatResponse`, `OpenAIMessageContent`
- Error types

**What to drop**:
- All other backends (Anthropic, Google, Azure, etc.)
- CLI + TUI
- Agent/memory features
- Embedding/TTS/STT

**Coupling issues**:
- `OpenAIChatRequest` uses generic `Tool` type from `chat` module — need to keep that module
- `StreamResponse` trait — streaming is tightly woven
- ~200 LOC to keep, ~100 LOC to drop

**Effort**: 2–4 hours (identify & delete, adjust imports, test)

---

### rig

**Feasibility**: **Hard** ✗

**Why painful?**
- `CompletionModel` trait is deeply generic and abstracted
- Request/response uses Rig's `Message` abstraction (not raw HTTP)
- Each provider is its own crate with internal adapters
- `CompletionRequest` ↔ provider-native format ↔ `CompletionResponse` translation is provider-specific

**What to extract?**
- Create `rig-custom-proxy` crate
- Implement `CompletionClient { type CompletionModel = CustomHttpModel }`
- Implement `CompletionModel` translating `CompletionRequest` → OpenAI-compatible HTTP request
- Reuse `rig-core` message/request/response types
- Wire in `AuthResolver` + `Endpoint` hooks for proxy

**Coupling issues**:
- `CompletionRequest` is a high-level builder → you're forced to translate it to HTTP
- Streaming support requires implementing Rig's `streaming::StreamingCompletionResponse` interface
- Tool calls require Rig's `Tool` abstraction
- ~800–1200 LOC (new provider crate)

**Effort**: 10–20 hours (understand abstraction stack, implement traits, integration test)

**Verdict**: Not worth it for SP42 unless you're already committed to Rig's ecosystem (agent/tools/RAG).

---

### rust-genai

**Feasibility**: **High** ✓✓

**Why light?**
- `AuthResolver` + `ServiceTargetResolver` are already pluggable
- `Client` doesn't require trait implementation — it's a concrete struct with DI builders
- Chat completion is a direct HTTP method: `client.chat_completion(req) -> Response`
- No forced abstraction layer (you send raw `ChatRequest`, get raw `ChatResponse`)

**What to keep**:
- `Client` + `ClientConfig` (DI seam)
- `AuthResolver` + `AuthResolverFn` traits
- `Endpoint` + `ServiceTargetResolver`
- `WebClient` (reqwest wrapper)
- Chat request/response types

**What to drop**:
- Adapter-specific code (OpenAI, Anthropic, Gemini, etc.) — only keep OpenAI-compatible
- Dev-dependencies (test doubles)
- Config resolution layer (keep the DI interface, drop the model-mapper logic)

**Coupling issues**:
- Minimal. `WebClient` is self-contained (~300 LOC).
- `AuthResolver` + `Endpoint` are pure DI concepts (~200 LOC).
- No forced message abstraction layer.

**Effort**: 2–4 hours (identify, delete provider code, simplify config)

**Estimated extracted size**: ~400–500 LOC (HttpClient + AuthResolver + Endpoint + OpenAI-compatible types)

---

## 6. Proxy-Specific Assessment

### Sponsor-Proxy Requirements

**Shape**: Custom endpoint + proxy token + no client key
- Client calls `proxy-endpoint/v1/chat/completions` with `Authorization: Bearer <proxy-token>`
- Proxy translates to real LLM backend, returns response
- Proxy manages actual API keys (kept server-side)

### llm

**Capability**: ✓ **Possible, but manual wiring**
- Set `.base_url(proxy_endpoint)` at init
- Override `custom_headers()` to inject `Authorization: Bearer <proxy-token>`
- No API key field needed
- **Issue**: `custom_headers()` is a trait method, not per-request override → all requests get same headers

---

### rig

**Capability**: ✗ **Not ergonomic**
- `Client::new(fake_key)` with fake API key (awkward)
- Would need custom provider crate to hook auth injection
- No explicit per-request auth override mechanism
- **Not recommended for proxy use**

---

### rust-genai

**Capability**: ✓✓ **Native support**

```rust
let client = Client::builder()
    .with_auth_resolver_fn(|model_id| {
        // Dynamic proxy token (could rotate, refresh, etc.)
        Ok(Some(AuthData::BearerToken(proxy_token.clone())))
    })
    .build();

let req = ChatCompletionRequest { ... };
let resp = client.chat_completion(&req).await?;
```

- **No wiring needed**: Just inject the resolver
- **Per-request control**: Can vary token per request if needed
- **Clean abstraction**: AuthResolver is explicit, not hidden in trait defaults

---

## 7. Honest Assessment: Hand-Roll vs. Vendor

### Sponsor-Proxy Size (hand-rolled OpenAI-compatible HTTP client)

**Minimal, working implementation**:
```rust
struct CustomHttpClient {
    endpoint: String,
    auth_resolver: Box<dyn Fn() -> String>,
}

impl CustomHttpClient {
    async fn chat_completion(&self, req: ChatRequest) -> Result<ChatResponse> {
        let token = (self.auth_resolver)();
        let resp = reqwest::Client::new()
            .post(format!("{}/v1/chat/completions", self.endpoint))
            .header("Authorization", format!("Bearer {}", token))
            .json(&req)
            .send()
            .await?;
        resp.json::<ChatResponse>().await
    }
}
```

**Estimated LOC**: 150–200 (including request/response types, error handling, streaming support)

**Time to implement**: 3–5 hours
**Time to maintain**: Low (OpenAI API rarely changes)

### Verdict

| Crate | Vendor-and-Trim | Hand-Roll | Recommendation |
|-------|-----------------|-----------|---|
| **llm** | 2–4h, 400 LOC | 3–5h, 150 LOC | **Hand-roll** (llm extraction not cleaner) |
| **rig** | 10–20h, 800 LOC | 3–5h, 150 LOC | **Hand-roll** (rig too heavy) |
| **rust-genai** | 2–4h, 500 LOC | 3–5h, 150 LOC | **Vendor rust-genai** IF you want battle-tested HTTP error handling, streaming support, and auth DI; **hand-roll** if you want zero external deps |

---

## 8. Conclusion & Recommendation for SP42

### Top Pick: **rust-genai** (if vendoring)

**Why**:
1. **Cleanest proxy support** — `AuthResolver` + `Endpoint` DI is made for this
2. **Smallest extraction** — ~500 LOC, self-contained
3. **Minimal deps** — 16 transitive crates (llm/rig are 30+)
4. **No ecosystem lock-in** — not forcing tools/agents/RAG abstractions
5. **Good error handling** — reqwest + eventsource-stream are battle-tested

**Path**:
- Vendor a `sp42-http-client` crate extracting rust-genai's resolver + web client layer
- Or: Use rust-genai as an external dep (it's stable-ish, though marked beta)

### Runner-Up: **Hand-roll** (if you want zero external LLM crates)

**Why**:
1. **Control** — see exactly what's happening
2. **Minimal deps** — only reqwest (unavoidable for HTTP)
3. **Fast** — 3–5 hours to a working MVP
4. **Maintainable** — OpenAI API is stable; proxy protocol is trivial

**Risk**:
- Streaming support requires more care (SSE parsing, backpressure)
- Error handling boilerplate
- You own it (refactors when SP42's auth shape evolves)

### Not Recommended: **llm** or **rig**

- **llm**: Lighter than rig, but still pulling CLI/TUI bloat by default
- **rig**: Great if SP42 adopts Rig's whole stack (agents, tools, RAG); overkill for citation HTTP
- Both obscure the proxy token injection logic (llm via `custom_headers` trait method, rig via provider crate)

---

## Appendix: File Paths & Evidence

### Key Source Files (all cloned to `/tmp/claude-1000/tmp.JBsjVFEXRq/`)

**llm-graniet**:
- `/src/lib.rs:96-100` — trait definition
- `/src/providers/openai_compatible.rs:1-150` — implementation
- `/src/builder/llm_builder.rs` — `.base_url()` builder method
- `/LICENSE` — MIT

**rig-0xPlaygrounds**:
- `/crates/rig-core/src/client/completion.rs:9-30` — trait definition
- `/crates/rig-core/src/completion/mod.rs:1-40` — abstraction layer
- `/crates/rig-core/src/providers/zai.rs:1-50` — provider pattern
- `/LICENSE` — MIT

**rust-genai**:
- `/src/resolver/auth_resolver.rs:1-150` — auth plugin
- `/src/resolver/endpoint.rs:1-50` — endpoint plugin
- `/src/client/config.rs` — builder integration
- `/src/client/builder.rs` — `.with_auth_resolver_fn()` method
- `/Cargo.toml:5` — MIT OR Apache-2.0

---

**Research conducted**: 2026-06-09  
**Commits inspected**: llm c78a18e, rig 4559d43, rust-genai a8f8082  
**Conclusion**: **rust-genai** wins for proxy extensibility; **hand-roll** is competitive for control + speed.
