# SP42 external-service "edge" pattern — the template for `build_citation_verify_request` / `execute_citation_verify` / `parse_citation_verify_response`

Research date: 2026-06-08. Read-only study of the LiftWing edge in `SP42-impl-citation`.

This is the blueprint for the ADR-0008 Decision 3 model-call edge. The canonical reference is
`crates/sp42-core/src/liftwing.rs` (the build/execute/parse triple) plus its supporting types in
`sp42-types` (the `HttpClient` trait, transport structs, error enums, `StubHttpClient`).

**Mirror these files when porting:**
- `crates/sp42-core/src/liftwing.rs` — the edge (build/execute/parse + private parse helpers + `#[cfg(test)]`)
- `crates/sp42-core/src/errors.rs` — domain error enum (`LiftWingError`), `thiserror` style
- `crates/sp42-types/src/transport.rs` — `HttpMethod`, `HttpRequest`, `HttpResponse`
- `crates/sp42-types/src/traits.rs` — `HttpClient` trait + `StubHttpClient` test double
- `crates/sp42-types/src/errors.rs` — `HttpClientError`
- `crates/sp42-core/src/types.rs` — `WikiConfig` (where `liftwing_url: Option<Url>` lives)
- `crates/sp42-core/src/test_fixtures.rs` — `fixture_wiki_config()`
- `crates/sp42-server/src/runtime_adapters.rs` — `BearerHttpClient` (where the bearer token is actually injected — NOT in `build_*`)

---

## 0. The shared transport contract (re-used verbatim; do NOT redefine)

From `crates/sp42-types/src/transport.rs` (re-exported through `sp42-core/src/types.rs` as
`pub use sp42_types::{HttpMethod, HttpRequest, HttpResponse, ...}`):

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub url: Url,                               // url::Url, not String
    #[serde(default)]
    pub headers: BTreeMap<String, String>,      // BTreeMap = deterministic key order
    #[serde(default)]
    pub body: Vec<u8>,                          // raw bytes; JSON is serialized into this
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub body: Vec<u8>,
}
```

Key facts for the port:
- `HttpRequest.url` is a `url::Url` (already-parsed), taken from `WikiConfig`. The build function
  never parses a URL string; it clones the configured `Url`.
- `headers` is a `BTreeMap<String, String>` — keys are lowercased convention (`"content-type"`).
- `body` is `Vec<u8>` — `serde_json::to_vec(...)` is the producer.

The `HttpClient` trait (from `crates/sp42-types/src/traits.rs`) — this is the DI seam:

```rust
#[async_trait]
pub trait HttpClient: Send + Sync {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, HttpClientError>;
}
```

`async_trait` is used (object-safe; `?Sized` works against it). Note `execute` takes `request` **by
value** (owned `HttpRequest`), returns `Result<HttpResponse, HttpClientError>`.

---

## 1. build / execute / parse — exact signatures and composition

The triple lives in `crates/sp42-core/src/liftwing.rs`. The request input is a tiny owned struct:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiftWingRequest {
    pub rev_id: u64,
}
```

### 1a. `build_*` — pure, sync, `&WikiConfig` + `&Request` → `Result<HttpRequest, DomainError>`

```rust
pub fn build_liftwing_score_request(
    config: &WikiConfig,
    request: &LiftWingRequest,
) -> Result<HttpRequest, LiftWingError> {
    if request.rev_id == 0 {
        return Err(LiftWingError::InvalidRequest {
            message: "rev_id must be non-zero".to_string(),
        });
    }

    let url = config
        .liftwing_url
        .clone()
        .ok_or_else(|| LiftWingError::InvalidRequest {
            message: "liftwing_url is not configured".to_string(),
        })?;

    let body = serde_json::to_vec(&serde_json::json!({
        "rev_id": request.rev_id
    }))
    .map_err(LiftWingError::from)?;

    Ok(HttpRequest {
        method: HttpMethod::Post,
        url,
        headers: BTreeMap::from([("content-type".to_string(), "application/json".to_string())]),
        body,
    })
}
```

What to copy exactly:
- **Pure & synchronous.** No `async`. No I/O. Input validation happens here (the `rev_id == 0`
  guard → `InvalidRequest`). This is where the citation-verify port validates its inputs (claim
  text non-empty, source text non-empty, URL present, etc.).
- **`&WikiConfig` is borrowed; the URL is `.clone()`d** off `config.liftwing_url` (an
  `Option<Url>`), and the `None` case is mapped to a domain `InvalidRequest` error via
  `.ok_or_else(...)`. For citation-verify the analog is the model-endpoint URL.
- **Method = `HttpMethod::Post`** for a body-bearing inference call.
- **Headers built with `BTreeMap::from([(...)])`**, lowercase header names; only
  `content-type: application/json` is set here. **No auth header is set in `build_*`** (see §5).
- **Body = `serde_json::to_vec(&serde_json::json!({...}))`**, with the serde error funneled through
  `.map_err(LiftWingError::from)` (the `#[from] serde_json::Error` variant).

### 1b. `execute_*` — async, generic over `C: HttpClient + ?Sized`

```rust
pub async fn execute_liftwing_score<C>(
    client: &C,
    config: &WikiConfig,
    request: &LiftWingRequest,
) -> Result<f32, LiftWingError>
where
    C: HttpClient + ?Sized,
{
    let http_request = build_liftwing_score_request(config, request)?;
    let response =
        client
            .execute(http_request)
            .await
            .map_err(|error| LiftWingError::InvalidResponse {
                message: error.to_string(),
            })?;

    if !(200..300).contains(&response.status) {
        return Err(LiftWingError::InvalidResponse {
            message: format!("unexpected HTTP status {}", response.status),
        });
    }

    parse_liftwing_score_response(&response.body)
}
```

What to copy exactly:
- **Generic bound `C: HttpClient + ?Sized`** — accepts both a concrete client AND a `&dyn HttpClient`
  trait object. Client passed as `&C` (borrow).
- **Argument order:** `(client, config, request)`.
- **Composition is literally three steps:** (1) `build_*(config, request)?`; (2)
  `client.execute(http_request).await` with the transport `HttpClientError` **flattened into the
  domain error** via `.map_err(|error| LiftWingError::InvalidResponse { message: error.to_string() })`
  (note: transport errors become `InvalidResponse`, NOT a dedicated transport variant); (3) the
  status gate.
- **Status gate is inline in `execute_*`, not in `parse_*`:** `if !(200..300).contains(&response.status)`
  → `InvalidResponse { message: format!("unexpected HTTP status {}", response.status) }`. So `parse_*`
  only ever sees a 2xx body.
- **Return type is the domain value** (`f32` here), not the raw response. For citation-verify this
  is the parsed verdict struct (the `CitationVerdict` / per-model vote).
- The final line is `parse_liftwing_score_response(&response.body)` — body passed as `&[u8]`.

### 1c. `parse_*` — pure, sync, `&[u8]` → `Result<DomainValue, DomainError>`

```rust
pub fn parse_liftwing_score_response(body: &[u8]) -> Result<f32, LiftWingError> {
    let parsed: Value = serde_json::from_slice(body).map_err(LiftWingError::from)?;

    if let Some(probability) = extract_probability_from_supported_shapes(&parsed) {
        return validate_probability(probability);
    }

    Err(LiftWingError::InvalidResponse {
        message: "response does not contain a supported probability field".to_string(),
    })
}
```

What to copy exactly:
- **Pure & sync; takes `&[u8]`** (the raw body). `serde_json::from_slice(body)` with the error funneled
  via `.map_err(LiftWingError::from)`.
- Parses into an untyped `serde_json::Value` first, then extracts the field-of-interest through a set
  of fallback shape-matchers (see §2). For citation-verify the model returns the agreed JSON verdict
  schema, so the port can `serde_json::from_slice::<CitationVerdictResponse>(body)` into a typed
  struct directly — BUT note SP42's defensive style of tolerating multiple response shapes; if the
  model's JSON envelope is uncertain, mirror the `Value`-first + shape-matcher fallback approach.

**Triple summary for the port (ADR-0008 Decision 3):**

```rust
// pure, sync
pub fn build_citation_verify_request(
    config: &WikiConfig,                          // or a dedicated ModelConfig
    request: &CitationVerifyRequest,             // { claim, source_text, source_url, model_ref, ... }
) -> Result<HttpRequest, CitationVerifyError>;

// async, generic over the HttpClient seam
pub async fn execute_citation_verify<C>(
    client: &C,
    config: &WikiConfig,
    request: &CitationVerifyRequest,
) -> Result<CitationVerdict, CitationVerifyError>
where
    C: HttpClient + ?Sized;

// pure, sync
pub fn parse_citation_verify_response(body: &[u8]) -> Result<CitationVerdict, CitationVerifyError>;
```

---

## 2. Response parsing & failure handling — the `validate_*` gate and the "default"

There is **no silent default** — every miss is an `Err(InvalidResponse {...})`. The structure:

1. `serde_json::from_slice` → on malformed JSON, `Err(LiftWingError::Json(...))` (via `#[from]`).
2. `extract_probability_from_supported_shapes(&Value) -> Option<f32>` tries shapes in order
   (returns `None` if none match):

```rust
fn extract_probability_from_supported_shapes(value: &Value) -> Option<f32> {
    let object = value.as_object()?;
    direct_probability(object)
        .or_else(|| scores_probability(object))
        .or_else(|| output_probability(object))
        .or_else(|| prediction_list_probability(object.get("outputs")))
        .or_else(|| prediction_list_probability(object.get("predictions")))
}
```

   - `direct_probability`: `object["probability"]`
   - `scores_probability`: `object["scores"]["damaging"]` then `["revertrisk"]`
   - `output_probability`: `object["output"]["probabilities"]["true"]` then `["damaging"]`
   - `prediction_list_probability(object["outputs"])` / `(object["predictions"])`: first array
     entry with `"probability"` then `"score"`
   - `value_as_f32`: accepts only `Value::Number` (string `"0.61"` is **rejected**), parses via
     `number.to_string().parse::<f32>()`, and **filters out non-finite** (`.filter(|p| p.is_finite())`).
3. If a value was extracted, it runs the **`validate_*` gate**:

```rust
fn validate_probability(probability: f32) -> Result<f32, LiftWingError> {
    if !(0.0..=1.0).contains(&probability) {
        return Err(LiftWingError::InvalidResponse {
            message: format!("probability {probability} is outside 0.0..=1.0"),
        });
    }
    Ok(probability)
}
```

4. If no shape matched, `Err(InvalidResponse { message: "response does not contain a supported
   probability field" })`.

**Port takeaways:** YES there is a dedicated `validate_*` domain gate (the range check), separate from
extraction. There is NO default value — a missing/out-of-range/wrong-type field is always an error.
For citation-verify: after deserializing the model's JSON, run a `validate_citation_verdict(...)` gate
that rejects unknown verdict enum values, missing required fields (e.g. a quote claimed as support but
absent), or any anti-fabrication invariant — mirror "extract then validate, never default."

---

## 3. `LiftWingError` — the full `thiserror` enum style (verbatim)

From `crates/sp42-core/src/errors.rs`:

```rust
#[derive(Debug, Error)]
pub enum LiftWingError {
    #[error("liftwing request is invalid: {message}")]
    InvalidRequest { message: String },
    #[error("liftwing response is invalid: {message}")]
    InvalidResponse { message: String },
    #[error("liftwing serialization failed: {0}")]
    Json(#[from] serde_json::Error),
}
```

Style conventions to copy (observed across the whole `errors.rs`):
- `#[derive(Debug, Error)]` from `thiserror` (`use thiserror::Error;` at top of file).
- **Struct variants with named fields** (`{ message: String }`) for human-message cases;
  `#[error("...: {message}")]` interpolates the named field. Message text is **lowercase,
  domain-prefixed** ("liftwing request is invalid: ...").
- **`#[from]` for source-error wrapping**: `Json(#[from] serde_json::Error)` with positional
  `{0}` in the format string. This is what makes `serde_json::to_vec(...).map_err(LiftWingError::from)`
  and `serde_json::from_slice(...).map_err(LiftWingError::from)` work.
- For variants that wrap another domain error transparently, the codebase uses
  `#[error(transparent)] Variant(#[from] OtherError)` (see `StreamRuntimeError`,
  `BacklogRuntimeError`, `ReviewWorkbenchError` in the same file). Use this if the citation-verify
  edge needs to compose sub-errors.
- **`&'static str` reasons** appear in the transport-layer errors (`HttpClientError::StatePoisoned
  { resource: &'static str }`) — use a `&'static str` field for fixed, non-formatted reason tokens.
  Domain-message fields are `String`.
- A richer variant precedent (for retryable transport classification) is `ActionError::Execution`:
  `{ message: String, code: Option<String>, http_status: Option<u16>, retryable: bool }`. If
  citation-verify needs to surface model-API HTTP status / retryability, mirror this shape.

**Suggested `CitationVerifyError` for the port:**

```rust
#[derive(Debug, Error)]
pub enum CitationVerifyError {
    #[error("citation verify request is invalid: {message}")]
    InvalidRequest { message: String },
    #[error("citation verify response is invalid: {message}")]
    InvalidResponse { message: String },
    #[error("citation verify serialization failed: {0}")]
    Json(#[from] serde_json::Error),
}
```

(Note: the transport `HttpClientError` is deliberately NOT a `#[from]` variant here — LiftWing flattens
it into `InvalidResponse { message: error.to_string() }` inside `execute_*`. Decide whether to keep that
flattening or add a dedicated transport variant; LiftWing chose flattening.)

---

## 4. The `StubHttpClient` unit-test pattern (`#[cfg(test)]`)

`StubHttpClient` lives in `crates/sp42-types/src/traits.rs` (NOT in the edge file — it is a shared
test double exported from `sp42-types`, imported in tests via `use crate::traits::StubHttpClient;`):

```rust
#[derive(Debug)]
pub struct StubHttpClient {
    responses: Mutex<VecDeque<Result<HttpResponse, HttpClientError>>>,
}

impl StubHttpClient {
    #[must_use]
    pub fn new<I>(responses: I) -> Self
    where
        I: IntoIterator<Item = Result<HttpResponse, HttpClientError>>,
    {
        Self {
            responses: Mutex::new(responses.into_iter().collect()),
        }
    }
}

#[async_trait]
impl HttpClient for StubHttpClient {
    async fn execute(&self, _request: HttpRequest) -> Result<HttpResponse, HttpClientError> {
        let mut responses = self
            .responses
            .lock()
            .map_err(|_| HttpClientError::StatePoisoned {
                resource: "stub_http_client.responses",
            })?;

        responses.pop_front().unwrap_or_else(|| {
            Err(HttpClientError::InvalidResponse {
                message: "stub client has no queued response".to_string(),
            })
        })
    }
}
```

Seeding & behavior:
- **`StubHttpClient::new([...])`** is seeded with an iterator of
  `Result<HttpResponse, HttpClientError>` — one queued result **per `execute` call**, popped FIFO
  from a `Mutex<VecDeque<...>>`. The `_request` is **ignored** (so the stub asserts on the verdict,
  not the request — request assertions go through a separate `build_*` test, see below).
- Running out of queued responses returns `Err(InvalidResponse { "stub client has no queued response" })`.
- For a multi-model panel port, seed N responses (one per panel member) into one stub, or use one
  stub per member.

The `cfg(test)` module in `liftwing.rs` imports:

```rust
use futures::executor::block_on;            // drives async in a sync #[test]
use proptest::prelude::*;                    // property tests
use crate::test_fixtures::fixture_wiki_config;
use crate::traits::StubHttpClient;
use crate::types::HttpResponse;
```

**Full quoted test — the execute-through-the-HTTP-trait pattern** (from `liftwing.rs` tests):

```rust
#[test]
fn executes_liftwing_request_through_http_trait() {
    let config = fixture_wiki_config();
    let client = StubHttpClient::new([Ok(HttpResponse {
        status: 200,
        headers: BTreeMap::new(),
        body: br#"{"scores":{"damaging":0.67}}"#.to_vec(),
    })]);

    let probability = block_on(execute_liftwing_score(
        &client,
        &config,
        &LiftWingRequest { rev_id: 123_456 },
    ))
    .expect("liftwing execution should succeed");

    assert!((probability - 0.67).abs() < f32::EPSILON);
}
```

Patterns to copy:
- **`block_on(execute_*(...))`** — `futures::executor::block_on` runs the async edge inside a plain
  sync `#[test]` (no `#[tokio::test]` needed in `core`). The whole `execute_*(&client, &config, &req)`
  future is the argument to `block_on`.
- **Response body as a byte-string literal:** `br#"{...}"#.to_vec()` with `status: 200`,
  `headers: BTreeMap::new()`.
- **Config from `fixture_wiki_config()`** (loads the real embedded `frwiki.yaml`, so `liftwing_url` is
  populated — that is why the build test below can assert on the URL).
- **Float assert:** `(value - expected).abs() < f32::EPSILON`.

The companion `build_*` test (asserts on the constructed request, since the stub ignores it):

```rust
#[test]
fn builds_liftwing_request() {
    let config = fixture_wiki_config();
    let request = build_liftwing_score_request(&config, &LiftWingRequest { rev_id: 123_456 })
        .expect("request should build");
    let body = String::from_utf8(request.body).expect("body should be utf-8");

    assert!(request.url.as_str().contains("revertrisk-language-agnostic"));
    assert!(body.contains("\"rev_id\":123456"));
}
```

Plus many pure `parse_*` tests (no client, no async), e.g.:

```rust
#[test]
fn parses_direct_probability_shape() {
    let probability =
        parse_liftwing_score_response(br#"{"probability":0.91}"#).expect("should parse");
    assert!((probability - 0.91).abs() < f32::EPSILON);
}

#[test]
fn rejects_probability_outside_unit_interval() {
    let error = parse_liftwing_score_response(br#"{"probability":1.4}"#)
        .expect_err("invalid probability should fail");
    assert!(error.to_string().contains("outside 0.0..=1.0"));
}
```

And a property test driving the round-trip:

```rust
proptest! {
    #[test]
    fn property_direct_probability_shape_round_trips(probability in 0.0f32..=1.0f32) {
        let body = serde_json::json!({ "probability": probability }).to_string();
        let parsed = parse_liftwing_score_response(body.as_bytes()).expect("probability should parse");
        prop_assert!((parsed - probability).abs() < 0.000_001);
    }
}
```

**Test taxonomy to mirror for the port:** (a) one `build_*` test asserting request URL/method/body;
(b) pure `parse_*` tests per success shape + per failure (malformed JSON, missing field, out-of-range,
wrong type — assert on `error.to_string().contains("...")`); (c) one `execute_*` test through
`StubHttpClient` + `block_on`; (d) optionally a proptest round-trip.

---

## 5. `liftwing_url` on `WikiConfig` + where endpoint/headers/bearer token come from

### `WikiConfig` shape (`crates/sp42-core/src/types.rs`)

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikiConfig {
    pub wiki_id: String,
    pub display_name: String,
    pub api_url: Url,
    pub eventstreams_url: Url,
    pub oauth_authorize_url: Url,
    pub oauth_token_url: Url,
    pub liftwing_url: Option<Url>,            // <-- the external-service endpoint, OPTIONAL
    pub coordination_url: Option<Url>,
    #[serde(default)]
    pub namespace_allowlist: Vec<i32>,
    #[serde(default = "default_scoring_policy_ref")]
    pub scoring_policy_ref: String,
    #[serde(default)]
    pub scoring: ScoringConfig,
    #[serde(default)]
    pub templates: WikiTemplates,
}
```

- **`liftwing_url: Option<Url>`** — `url::Url`, wrapped in `Option`, **default-absent**. There is no
  `#[serde(default)]` and no default fn on it; absence in YAML deserializes to `None` because the
  field is an `Option` (serde treats a missing `Option` field as `None`). Required external endpoints
  (`api_url`, `eventstreams_url`, oauth urls) are bare `Url` (deserialization fails if missing);
  **optional external endpoints (`liftwing_url`, `coordination_url`) are `Option<Url>`** — the
  external-service edge is opt-in per wiki. **For the citation-verify port: add a
  `verify_endpoint_url: Option<Url>` (or a dedicated model config) field in this `Option<Url>` style** —
  default-absent, and `build_*` errors with `InvalidRequest` when it is `None`.

### Where the actual URL value comes from

`crates/sp42-core/src/test_fixtures.rs` loads it from the on-disk config:

```rust
pub(crate) fn fixture_wiki_config() -> WikiConfig {
    let compiled =
        crate::scoring_policy::load_embedded_compiled_scoring_policy("active/frwiki-vandalism")
            .expect("embedded frwiki scoring policy should compile");
    let mut config =
        serde_yaml::from_str::<WikiConfig>(include_str!("../../../configs/frwiki.yaml"))
            .expect("embedded frwiki config should deserialize");
    config.scoring = compiled.scoring_config;
    config
}
```

And `configs/frwiki.yaml` provides the literal value:

```yaml
api_url: https://fr.wikipedia.org/w/api.php
oauth_authorize_url: https://meta.wikimedia.org/w/rest.php/oauth2/authorize
oauth_token_url: https://meta.wikimedia.org/w/rest.php/oauth2/access_token
liftwing_url: https://api.wikimedia.org/service/lw/inference/v1/models/revertrisk-language-agnostic:predict
```

So: **endpoint URL = a config field deserialized from YAML**, surfaced to `build_*` as `&WikiConfig`.

### Where the bearer token / auth header comes from — **NOT in `build_*`**

`build_liftwing_score_request` sets ONLY `content-type: application/json`. The auth header is injected
by a **decorator `HttpClient`** at the composition root — `BearerHttpClient` in
`crates/sp42-server/src/runtime_adapters.rs`:

```rust
pub(crate) struct BearerHttpClient {
    client: reqwest::Client,
    access_token: String,
}

impl BearerHttpClient {
    pub(crate) fn new(client: reqwest::Client, access_token: String) -> Self { ... }
}

#[async_trait]
impl HttpClient for BearerHttpClient {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, HttpClientError> {
        let mut builder = match request.method {
            HttpMethod::Get => self.client.get(request.url),
            HttpMethod::Post => self.client.post(request.url),
            // ... Put/Patch/Delete
        }
        .bearer_auth(&self.access_token);          // <-- token injected HERE, not in build_*

        for (key, value) in request.headers {
            builder = builder.header(&key, &value);
        }
        // ... body + send + map reqwest error -> HttpClientError::Transport
    }
}
```

**Critical port pattern:** the secret (bearer token / model API key) is held by the **concrete
`HttpClient` adapter at the edge of the system (`sp42-server`)**, applied via `.bearer_auth(...)` in
`execute`. The pure `core` `build_*` function **never sees the token** — it stays I/O-free and
secret-free. For the citation-verify model edge, the model API key lives in a
`BearerHttpClient`-style adapter (or equivalent), and `build_citation_verify_request` constructs only
the URL/method/content-type/body. This keeps `core` deterministic and testable with `StubHttpClient`
(which sets no auth at all), and keeps the platform jargon/secrets out of the domain layer.

---

## 6. Port checklist (ADR-0008 Decision 3 — `build/execute/parse_citation_verify`)

1. Add `verify_endpoint_url: Option<Url>` (or a dedicated `ModelConfig`) to `WikiConfig` in the
   `Option<Url>`, default-absent style; wire it into the relevant config YAML + `fixture_wiki_config`.
2. Define `CitationVerifyError` in `errors.rs` mirroring `LiftWingError`
   (`InvalidRequest`/`InvalidResponse` struct variants + `Json(#[from] serde_json::Error)`;
   add an `Action`-style retryable variant only if the model API needs it).
3. Define the input struct `CitationVerifyRequest { ... }` (claim, source_text, source_url, model_ref,
   prompt inputs) + the output `CitationVerdict` (per ADR-0007 two-axis verdict).
4. `build_citation_verify_request(&config, &req) -> Result<HttpRequest, CitationVerifyError>` — pure,
   sync; validate inputs → `InvalidRequest`; clone endpoint `Url` (`None` → `InvalidRequest`);
   `method: Post`, `content-type: application/json`, body = `serde_json::to_vec(...)` (prompt/messages
   envelope), **no auth header**.
5. `execute_citation_verify<C: HttpClient + ?Sized>(&client, &config, &req) -> Result<CitationVerdict,
   CitationVerifyError>` — `build_*?` → `client.execute().await.map_err(... InvalidResponse ...)` →
   `(200..300)` status gate → `parse_*(&response.body)`.
6. `parse_citation_verify_response(&[u8]) -> Result<CitationVerdict, CitationVerifyError>` — pure,
   sync; `serde_json::from_slice` (typed struct, or `Value` + shape-matchers if envelope is uncertain)
   + a `validate_citation_verdict(...)` gate (reject unknown enums / missing fields / anti-fabrication
   violations; never default).
7. Inject the model API key in the concrete adapter (`BearerHttpClient`-style) at the `sp42-server`
   composition root — keep `core` secret-free.
8. Tests in `#[cfg(test)]`: one `build_*` (assert URL/method/body), pure `parse_*` per success + per
   failure (malformed JSON / missing field / bad enum / validate-gate), one `execute_*` via
   `StubHttpClient` + `block_on`, optionally a proptest.
