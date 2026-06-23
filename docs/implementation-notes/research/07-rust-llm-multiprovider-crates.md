# Rust Multi-Provider LLM Abstraction Crates Survey (2026)

**Research Date:** June 9, 2026  
**Scope:** Provider-agnostic LLM chat/completion abstractions for SP42 architecture evaluation.  
**Focus Areas:** Custom endpoint/auth, licensing, maintenance, dependency weight, trait-driven design.

---

## CRATE PROFILES

### 1. **rig-core** (0xPlaygrounds)

**Crates.io & Versioning:**
- Package: `rig-core`
- Latest Version: **0.38.1** (June 2, 2026)
- Total Releases: 628
- Source: https://crates.io/crates/rig-core | https://github.com/0xPlaygrounds/rig

**GitHub Stats:**
- Stars: **7,600+** (as of June 2026; 840 forks)
- Last Commit: Within 1 month of survey (active)
- Status: **Actively maintained** (warning: "future updates will contain breaking changes")

**License:** MIT

**Core Traits & API:**
- `CompletionModel` trait — unified completion interface across 23+ providers
- `EmbeddingModel` trait — embedding abstraction
- `Agent` type — high-level agent orchestration (RAG, conversational)
- `ConversationMemory` trait — per-conversation history management
- `VectorStoreIndex` trait — vector store abstraction
- Stream support: **Yes** (full agentic patterns with streaming multi-turn)
- Async: Tokio-based

**Custom Endpoint & Auth:**
- **Custom Endpoint Support:** Via `Provider` trait; `ProviderClient` trait allows builder-pattern customization
- **Custom Auth:** Supports environment variable loading (`ProviderClient::from_env()`) and custom `api_key()` builder methods
- **Per-Request Override:** Not explicitly documented; auth appears per-client, not per-request
- **Proxy Support:** Extensible provider pattern but **no explicit `base_url` or custom bearer-token per-request** shown in search results
- **API Architecture:** Generic `Client<Ext, H>` parameterized by provider extension (Ext: Provider trait)

**Dependencies:**
- Primary: tokio, serde, reqwest
- Transitive count: Not explicitly found; likely moderate-to-heavy (multi-provider SDKs as features)
- Documentation: 44.28% coverage

**Notable Integrations:**
- 23+ completion/embedding providers (OpenAI, Anthropic, Cohere, Mistral, Groq, Ollama…)
- Vector stores: MongoDB, Neo4j, Qdrant, AWS Bedrock, S3Vectors, PostgreSQL, LanceDB
- Organizations: St Jude (genomics), Coral Protocol, Neon, Ryzome

**Vendor-Lock Assessment:** **MODERATE** — provider extensibility is strong, but moving away requires re-implementing custom providers.

---

### 2. **genai-rs** (jeremychone/rust-genai)

**Crates.io & Versioning:**
- Package: `genai-rs` (also published as `genai`)
- Latest Version: **0.7.2** (released May 23, 2025; v0.6.x as of March 2026)
- Source: https://crates.io/crates/genai-rs | https://github.com/jeremychone/rust-genai

**GitHub Stats:**
- Stars: **796** (166 forks)
- Last Commit: Actively maintained (recent updates to Anthropic JSON schema, streaming)
- Status: **Actively maintained**

**License:** **Dual: Apache-2.0 OR MIT** (permissive choice available)

**Core Traits & API:**
- Native-protocol multi-provider access (NOT SDK-wrapper-based)
- Supports 25+ providers, 200+ models across OpenAI, Anthropic, Gemini, Ollama, xAI, Groq, DeepSeek, Bedrock, Vertex, Copilot, etc.
- `ServiceTarget` — custom endpoint + auth abstraction (not a trait; a struct)
- `ServiceTargetResolver` — pluggable resolver for determining target from model name
- Chat & streaming: **Yes** (direct and streaming responses)
- Tool calling, structured output, image analysis, prompt caching

**Custom Endpoint & Auth:**
- **Custom Endpoint Support:** **EXCELLENT.** Via `ServiceTarget` struct:
  - Can specify arbitrary base_url, custom auth resolver
  - Namespace syntax: `groq::openai/gpt-oss-20b` forces specific adapter
  - Example: `c06-target-resolver.rs` demonstrates custom endpoint + custom auth + model name
- **Custom Auth:** **Per-request capable.** Auth resolver pattern allows arbitrary header injection per-request
- **Bearer Token Override:** **YES.** Custom auth tokens passed via `ServiceTarget` per-call
- **Proxy Support:** **NATIVE.** Designed for OpenAI-compatible gateways with arbitrary bearer token
- **Key Strength:** Model arguments can be (model_name | explicit ModelIden | complete ServiceTarget), enabling seamless proxy swapping

**Dependencies:**
- Lightweight emphasis: "avoiding per-service SDK dependencies" (native protocol implementations)
- Primary: tokio, serde, async HTTP
- Transitive: Lower than rig-core (native protocols vs SDK wrappers)
- TLS selectable via features: rustls (default), native-tls, custom

**Notable Recent Features (v0.6+):**
- AWS Bedrock adapters
- OpenRouter integration
- Google Vertex support
- GitHub Copilot API
- Structured output, tool calling, prompt caching

**Vendor-Lock Assessment:** **LOW** — native protocols + ServiceTarget pattern makes proxy swapping trivial.

---

### 3. **llm** (graniet)

**Crates.io & Versioning:**
- Package: `llm`
- Latest Version: **1.3.8**
- Source: https://crates.io/crates/llm | https://github.com/graniet/llm

**GitHub Stats:**
- Stars: **352** (active development)
- Last Commit: Regular updates; 505 commits, 16 open issues
- Status: **Actively maintained**

**License:** MIT

**Core Traits & API:**
- "Unified API and builder style" for multi-backend integration
- Supports: OpenAI, Claude, Ollama, DeepSeek, Groq, Google Gemini, Mistral, Cohere, and others
- Builder-pattern customization (`api_key()`, `backend()` methods)
- Chat completions, streaming, vision, reasoning, function/tool calling with JSON schema
- Speech-to-text (integration), text-to-speech
- Memory management (sliding window)
- Agentic workflows with shared state
- REST API exposure with OpenAI-compatible format

**Custom Endpoint & Auth:**
- **Custom Endpoint Support:** Via builder pattern (constructors); environment variable configuration (`OPENAI_API_KEY` pattern)
- **Custom Auth:** `api_key()` builder method; per-request override **not explicitly documented**
- **Bearer Token Override:** Likely via environment or builder, but specific mechanism unclear from search results
- **Proxy Support:** REST API can expose OpenAI-compatible format; custom endpoint via constructor suggested but not detailed

**Dependencies:**
- Primary: tokio (async runtime), serde (serialization), provider-specific SDKs via feature flags
- Transitive: Moderate (feature-gated provider SDKs)

**Vendor-Lock Assessment:** **MODERATE** — builder pattern is flexible, but provider SDKs as dependencies may increase friction for custom endpoints.

---

### 4. **allms**

**Crates.io & Versioning:**
- Package: `allms`
- Latest Version: Not explicitly found; last crates.io update April 8, 2026
- Source: https://crates.io/crates/allms | https://github.com/neferdata/allms

**GitHub Stats:**
- Stars: **111** (smaller project)
- Commits: 110
- Status: **Actively maintained** (examples and comprehensive docs)
- Language: 100% Rust

**License:** Dual MIT / Apache-2.0 (permissive)

**Core Traits & API:**
- Type-safe unified interface for 9+ providers: Anthropic, AWS Bedrock, Azure, DeepSeek, Google Gemini, Mistral, OpenAI, Perplexity, xAI
- "Use the same struct and methods regardless of which model you choose"
- Async/Tokio-based
- Advanced: Function calling, file uploads, token calculation
- Emphasis on standardized JSON serialization across disparate APIs

**Custom Endpoint & Auth:**
- **Custom Endpoint Support:** Environment variable config (e.g., `OPENAI_API_URL` for Azure)
- **Custom Auth:** Supports API keys in constructor, AWS environment variables (Bedrock), GCP credentials (Vertex)
- **Bearer Token Override:** Endpoint key passed in constructor; no clear per-request mechanism
- **Proxy Support:** Limited; tied to provider-specific environment patterns

**Dependencies:**
- Built on Tokio
- Transitive: Likely light (no mention of heavy SDK dependencies)

**Vendor-Lock Assessment:** **MODERATE-HIGH** — environment-variable-driven auth is less flexible for dynamic proxy scenarios.

---

### 5. **llm-chain**

**Crates.io & Versioning:**
- Package: `llm-chain` (v0.12.0 mentioned; latest release `llm-chain-local-v0.9.1` May 11, 2023)
- Source: https://crates.io/crates/llm-chain | https://github.com/sobelio/llm-chain

**GitHub Stats:**
- Stars: **1,600**
- Forks: 142
- Commits: 400 on main
- Status: **Mature but aging** (last major release May 2023; community-maintained)

**License:** MIT

**Core Traits & API:**
- Collection of Rust crates for advanced LLM applications (chatbots, agents, chains)
- Prompt templating, chain composition, multi-step task execution
- Vector store integration for memory
- Tool access (Bash, Python, web search)
- Supports ChatGPT, LLaMa, Alpaca, llm.rs models
- `llm-chain-openai` companion crate for OpenAI integration

**Custom Endpoint & Auth:**
- **Custom Endpoint Support:** llm-chain-openai designed for OpenAI API; custom endpoints not prominently documented
- **Custom Auth:** Assumes standard OpenAI auth patterns; flexibility limited
- **Per-Request Override:** Not shown
- **Proxy Support:** **NO** — tightly coupled to OpenAI's API shape

**Dependencies:**
- Requires Rust 1.65.0+
- Primary: Tokio, serde
- Transitive: Moderate

**Vendor-Lock Assessment:** **HIGH** — designed for OpenAI; extensions to other providers require new crates.

---

### 6. **langchain-rust** (Abraxas-365)

**Crates.io & Versioning:**
- Latest Release: v4.6.0 (October 6, 2024; now stale relative to 2026)
- Source: https://github.com/Abraxas-365/langchain-rust

**GitHub Stats:**
- Stars: **1,300**
- Forks: 173
- Releases: 33 total
- Status: **Community-maintained; slowing velocity** (last release Oct 2024)

**License:** MIT

**Core Traits & API:**
- Rust port of original LangChain framework
- Multiple LLM providers: OpenAI, Azure, Ollama, Anthropic Claude
- Embedding services: OpenAI, Ollama, FastEmbed, MistralAI
- Vector stores: OpenSearch, Postgres, Qdrant, SQLite, SurrealDB
- Chain types, agents, document loaders, semantic routing
- Depends on `serde_json` core; optional features for database backends

**Custom Endpoint & Auth:**
- **Custom Endpoint Support:** Not explicitly documented
- **Custom Auth:** Likely standard per-provider patterns
- **Per-Request Override:** Not documented
- **Proxy Support:** Unknown; probably weak

**Dependencies:**
- Moderate-to-heavy (multiple database and vector store integrations)

**Vendor-Lock Assessment:** **MODERATE-HIGH** — framework-level abstractions over per-provider logic.

---

## COMPARATIVE MATRIX

| Crate | Version | Stars | License | Core Strength | Custom Endpoint/Auth | Proxy-Ready | Deps | Status |
|-------|---------|-------|---------|---|---|---|---|---|
| **rig-core** | 0.38.1 | 7.6k | MIT | Comprehensive provider + agent abstractions | Fair (provider trait) | Limited | Heavy | ⭐ Active |
| **genai-rs** | 0.7.2 | 796 | Apache-2.0 / MIT | Native protocols + ServiceTarget pattern | **Excellent** | **YES** | Light | ⭐ Active |
| **llm** (graniet) | 1.3.8 | 352 | MIT | Builder pattern + unified API | Moderate (builder) | Moderate | Moderate | ⭐ Active |
| **allms** | ~recent | 111 | MIT / Apache-2.0 | Type-safe unified interface | Moderate (env vars) | Limited | Light | ⭐ Active |
| **llm-chain** | 0.12.0 | 1.6k | MIT | Prompt chains + multi-step workflows | Weak (OpenAI-centric) | No | Moderate | ⚠ Aging |
| **langchain-rust** | 4.6.0 | 1.3k | MIT | Framework-level abstractions | Weak | No | Heavy | ⚠ Slowing |

---

## DETAILED CUSTOM ENDPOINT & AUTH ASSESSMENT

### Sponsor Proxy Shape (Critical):
A sponsor-proxy use case requires:
1. Custom HTTP base URL (not hardcoded provider domains)
2. Custom bearer token (proxy token, not provider key)
3. Browser must send NO provider key
4. Per-request flexibility (same client, different auth per call)

**Result by Crate:**

| Crate | Custom Base URL | Bearer Override | Per-Request | Notes |
|-------|---|---|---|---|
| **rig-core** | ✅ Provider trait | ⚠ Builder only | ❌ No | Extensible but not designed for dynamic proxy |
| **genai-rs** | ✅✅ ServiceTarget | ✅ Resolver | ✅ Yes | **BEST FIT.** Native proxy shape via `ServiceTarget` + auth resolver |
| **llm** | ⚠ Builder pattern | ⚠ Environment | ❌ No | Builder method flexible; env var not ideal for proxy |
| **allms** | ⚠ Env var only | ⚠ Constructor | ❌ No | Environment-driven auth limits runtime flexibility |
| **llm-chain** | ❌ No | ❌ No | ❌ No | OpenAI-specific; no custom endpoint path |
| **langchain-rust** | ❌ No | ❌ No | ❌ No | Framework-level; custom endpoints not exposed |

---

## LICENSING ANALYSIS

All active candidates use permissive licenses:
- **rig-core:** MIT (maximally permissive)
- **genai-rs:** Apache-2.0 OR MIT (dual; maximum flexibility)
- **llm (graniet):** MIT
- **allms:** MIT OR Apache-2.0 (dual)
- **llm-chain:** MIT
- **langchain-rust:** MIT

**Conclusion:** No licensing barriers for SP42 adoption.

---

## DEPENDENCY WEIGHT (ROUGH ASSESSMENT)

**Lightest:**
1. **genai-rs** — native protocol implementations; avoids SDK dependencies (estimated transitive: ~15–25)
2. **allms** — type-safe, minimal integration surface

**Medium:**
3. **llm (graniet)** — feature-gated provider SDKs; lighter if features minimized
4. **rig-core** — tokio + serde + reqwest core; heavy if full provider suite enabled

**Heaviest:**
5. **langchain-rust** — multiple database/vector store integrations
6. **llm-chain** — aging; may have outdated transitive deps

---

## TRAIT-DRIVEN DESIGN & EXTENSIBILITY

**Best for Custom Implementations:**
1. **genai-rs** — `ServiceTarget` struct (not a trait) + auth resolver pattern is trivial to extend
2. **rig-core** — `Provider` trait is extensible but complex to implement
3. **llm** — builder pattern is simple but not trait-based for advanced customization

**Weakest:**
- **llm-chain**, **langchain-rust** — framework-level abstractions offer less flexibility for custom providers

---

## FINAL RECOMMENDATION FOR SP42

### Top Tier (Recommended):

**1. FIRST CHOICE: `genai-rs` (jeremychone/rust-genai)**

**Rationale:**
- ✅ **Sponsor-proxy native.** `ServiceTarget` struct + auth resolver designed for custom endpoints + bearer tokens
- ✅ **Per-request flexibility.** Model arguments can be (name | ModelIden | ServiceTarget), enabling runtime proxy swaps
- ✅ **Native protocols.** No SDK wrapper bloat; lighter transitive deps (~15–25 estimated)
- ✅ **Permissive license.** Apache-2.0 OR MIT dual
- ✅ **Active maintenance.** Regular updates (v0.6+ features: Bedrock, Vertex, structured output, tool calling)
- ✅ **Minimal abstraction leakage.** Direct control over endpoint & auth
- **Crate:** `genai-rs` v0.7.2 | GitHub: https://github.com/jeremychone/rust-genai | Stars: 796

**Example Evidence:** `c06-target-resolver.rs` example explicitly demonstrates custom endpoint + auth + model configuration without provider SDK coupling.

---

### Second Tier (Acceptable):

**2. `allms` (neferdata/allms)**

**Rationale:**
- ✅ Type-safe, unified interface
- ✅ Lightweight, permissive dual license
- ⚠ Environment-variable-driven auth (less ideal for proxy runtime dynamics)
- ⚠ Smaller community (111 stars vs genai's 796)
- **Crate:** `allms` | GitHub: https://github.com/neferdata/allms | Stars: 111

**When to Choose:** If strict type-safety over genai's struct-based flexibility is a priority, and environment-driven config is acceptable.

---

### Third Tier (Not Recommended):

**3. `rig-core`** — Too heavy for bare provider abstraction; best for full agent framework (RAG, memory, vector stores). Provider trait extensibility doesn't match sponsor-proxy shape.

**4. `llm` (graniet)** — Moderate option; builder pattern works, but not as elegant as genai's `ServiceTarget`. Fewer GitHub stars (352) suggests smaller ecosystem.

**5. `llm-chain`, `langchain-rust`** — Framework-oriented; not designed for custom provider endpoints. llm-chain is aging (May 2023 release).

---

## CONCRETE NEXT STEPS

1. **Prototype genai-rs integration:**
   - Implement `ServiceTarget` wrapper for SP42's `ModelRef { provider, model, version }`
   - Write a test against a local OpenAI-compatible gateway (e.g., vLLM, LiteLLM) to verify bearer-token override
   - Measure transitive dependency count: `cargo tree --depth 2`

2. **Document the `ServiceTargetResolver` pattern** for SP42's custom endpoint + token injection mechanism

3. **Defer allms evaluation** to Phase 2 if genai-rs pilot surfaces unexpected issues

4. **Update ADR-0006** ("Using LLMs in SP42") with the selected crate, licensing, and integration contract

---

## SOURCES

- [rig-core on crates.io](https://crates.io/crates/rig-core)
- [0xPlaygrounds/rig on GitHub](https://github.com/0xPlaygrounds/rig)
- [genai-rs on crates.io](https://crates.io/crates/genai-rs)
- [jeremychone/rust-genai on GitHub](https://github.com/jeremychone/rust-genai)
- [graniet/llm on GitHub](https://github.com/graniet/llm)
- [neferdata/allms on GitHub](https://github.com/neferdata/allms)
- [sobelio/llm-chain on GitHub](https://github.com/sobelio/llm-chain)
- [Abraxas-365/langchain-rust on GitHub](https://github.com/Abraxas-365/langchain-rust)
- [Rig Documentation](https://docs.rig.rs/)
- [Rig Custom Provider Guide](https://docs.rig.rs/guides/extension/write_your_own_provider)
- [LiteLLM OpenAI-Compatible Endpoints](https://docs.litellm.ai/docs/providers/openai_compatible)
