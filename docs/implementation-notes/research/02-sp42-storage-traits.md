# 02 — SP42 storage + platform-trait patterns (blueprint for ADR-0009 source-snapshot + verdict-record storage)

Research notes for the faithful Rust port. Source of truth: the existing SP42 implementation
in this repo. Everything below is **as-built**, quoted from source, with exact signatures.

Files read:
- `crates/sp42-types/src/traits.rs` — every platform trait + every test double
- `crates/sp42-types/src/lib.rs` — module layout + re-exports
- `crates/sp42-types/src/transport.rs` — `HttpRequest`/`HttpResponse`/`HttpMethod` DTOs
- `crates/sp42-types/src/errors.rs` — transport/storage error enums
- `crates/sp42-core/src/traits.rs` — compatibility re-export shim
- `crates/sp42-core/src/errors.rs` — `WikiStorageError`
- `crates/sp42-core/src/wiki_storage.rs` — the versioned-envelope build/parse split
- `crates/sp42-core/src/oauth.rs` (lines 8, 133–138) — the ONLY existing `Sha256` usage
- `Cargo.toml` (workspace) + `crates/sp42-core/Cargo.toml` — dep facts

---

## 0. Crate layout (where the new code goes)

`sp42-types` is the **dependency-free contract crate** (`#![forbid(unsafe_code)]`):
holds the platform traits (`HttpClient`, `Storage`, `Clock`, `Rng`, `EventSource`,
`WebSocket`), their test doubles, the transport DTOs, and the error enums. It depends only on
`async-trait`, `serde`, `url`, `thiserror` (+ `futures` as a dev-dep for `block_on` in tests).

`sp42-core` consumes `sp42-types` and holds the **logic** (build/parse splits, the
`wiki_storage` envelope machinery). `sp42-core/src/traits.rs` is a 6-line **compatibility
re-export** — it `pub use sp42_types::{Clock, ... Storage, ...}` so older code importing from
`sp42_core::traits` keeps compiling. **Port placement decision for ADR-0009:** the
`Snapshot`/`Verdict` envelope structs + their build/parse functions go in **`sp42-core`** (next
to `wiki_storage.rs`), reusing the injected `Storage` trait from `sp42-types`. (Mirrors ADR-0008
Decision 7 "light" path: contract types could later move to `sp42-types`.)

`sp42-core/Cargo.toml` already has `sha2.workspace = true` — content-hashing needs **no new
dep**. `sha2 = "0.10.9"` is pinned in the workspace. **There is NO `hex` crate and NO
`base16ct` crate** in the workspace — see §5 for the hex-encoding decision.

---

## 1. `HttpClient` trait (FULL)

`crates/sp42-types/src/traits.rs:18-21`. Uses `#[async_trait]`. Single method.

```rust
#[async_trait]
pub trait HttpClient: Send + Sync {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, HttpClientError>;
}
```

- `request` is taken **by value** (`HttpRequest`, owned).
- Returns `Result<HttpResponse, HttpClientError>`.
- Bound: `Send + Sync` (shared behind `&self`, so usable as `&dyn HttpClient`).
- Logic functions take it generically as `C: HttpClient + ?Sized` (see §5 / `load_wiki_storage_document`).

### Test double — `StubHttpClient` (`traits.rs:50-83`)
Queue-of-responses double; pops one canned `Result<HttpResponse, HttpClientError>` per call.

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
    { Self { responses: Mutex::new(responses.into_iter().collect()) } }
}

#[async_trait]
impl HttpClient for StubHttpClient {
    async fn execute(&self, _request: HttpRequest) -> Result<HttpResponse, HttpClientError> {
        let mut responses = self.responses.lock()
            .map_err(|_| HttpClientError::StatePoisoned { resource: "stub_http_client.responses" })?;
        responses.pop_front().unwrap_or_else(|| Err(HttpClientError::InvalidResponse {
            message: "stub client has no queued response".to_string(),
        }))
    }
}
```

`HttpClientError` (`errors.rs:5-13`): `Transport { message: String }`,
`InvalidResponse { message: String }`, `StatePoisoned { resource: &'static str }`.
Derives `Debug, Error, Clone, PartialEq, Eq`. `thiserror` messages:
`"transport failed: {message}"` / `"response was invalid: {message}"` /
`"stub state is poisoned: {resource}"`.

---

## 2. `Storage` trait (FULL) — THE KEY BLUEPRINT for the snapshot store

`crates/sp42-types/src/traits.rs:29-34`. `#[async_trait]`, `Send + Sync`. **Key = `&str` / `String`;
value = `Vec<u8>` (raw bytes).** There is **NO `list`** method — the trait is intentionally
get/set/remove only (a key/value blob store, not a namespaced scan store).

```rust
#[async_trait]
pub trait Storage: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError>;
    async fn set(&self, key: String, value: Vec<u8>) -> Result<(), StorageError>;
    async fn remove(&self, key: &str) -> Result<(), StorageError>;
}
```

- `get`: borrows key `&str`; returns `Option<Vec<u8>>` (`None` == absent, NOT an error).
- `set`: takes **owned** `key: String` and **owned** `value: Vec<u8>` (consumes both).
- `remove`: idempotent — removing an absent key is `Ok(())` (see `FileStorage` NotFound arm).
- **No `list`/`scan`/prefix-iteration.** If ADR-0009 needs to enumerate verdict records
  (e.g. "all verdicts for an article"), you must maintain a **separate index blob** under a
  known key (the `wiki_storage` plan/index pattern does exactly this for documents). Do NOT
  assume a list method exists.

`StorageError` (`errors.rs:23-29`), `Debug, Error, Clone, PartialEq, Eq`:
```rust
pub enum StorageError {
    #[error("storage failed: {message}")]
    Operation { message: String },
    #[error("stub state is poisoned: {resource}")]
    StatePoisoned { resource: &'static str },
}
```

### `MemoryStorage` impl (`traits.rs:121-162`)
`#[derive(Debug, Default)]`. Backing = `Mutex<BTreeMap<String, Vec<u8>>>` (BTreeMap → sorted,
deterministic iteration if ever needed).

```rust
#[derive(Debug, Default)]
pub struct MemoryStorage { values: Mutex<BTreeMap<String, Vec<u8>>> }

#[async_trait]
impl Storage for MemoryStorage {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let values = self.values.lock()
            .map_err(|_| StorageError::StatePoisoned { resource: "memory_storage.values" })?;
        Ok(values.get(key).cloned())
    }
    async fn set(&self, key: String, value: Vec<u8>) -> Result<(), StorageError> {
        let mut values = self.values.lock()
            .map_err(|_| StorageError::StatePoisoned { resource: "memory_storage.values" })?;
        values.insert(key, value);
        Ok(())
    }
    async fn remove(&self, key: &str) -> Result<(), StorageError> {
        let mut values = self.values.lock()
            .map_err(|_| StorageError::StatePoisoned { resource: "memory_storage.values" })?;
        values.remove(key);
        Ok(())
    }
}
```
Lock-poison → `StatePoisoned { resource: "memory_storage.values" }`. Pure in-memory; the canonical
hermetic test double for the snapshot store.

### `FileStorage` impl (`traits.rs:164-235`) — durable disk store, atomic writes
`#[derive(Debug, Clone, PartialEq, Eq)]`. Field: `root: PathBuf`. Ctor `new(root: PathBuf)`,
accessor `root(&self) -> &Path`.

**Key encoding** (`key_path`, `traits.rs:182-188`): the key string's UTF-8 bytes are
**lower-hex-encoded** then suffixed `.bin` — so any key (incl. a content hash, or a key with
slashes/colons) is filesystem-safe:
```rust
fn key_path(&self, key: &str) -> PathBuf {
    let mut encoded = String::with_capacity(key.len() * 2);
    for byte in key.as_bytes() { let _ = write!(&mut encoded, "{byte:02x}"); }
    self.root.join(format!("{encoded}.bin"))
}
```
(`use std::fmt::Write as _;` brings the `write!`-into-String trait into scope.)

**`get`**: `fs::read(path)`; `ErrorKind::NotFound` → `Ok(None)`; other IO err →
`StorageError::Operation { message: error.to_string() }`.

**`set`** is **atomic via temp-file + rename** (the load-bearing durability pattern to mirror):
1. `fs::create_dir_all(&self.root)`
2. compute target path via `key_path`
3. grab a process-unique sequence: `static FILE_STORAGE_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);`
   then `.fetch_add(1, Ordering::Relaxed)`
4. temp path = `root/.<target_file_name>.<sequence>.tmp`
5. `fs::write(&temp_path, value)`
6. `fs::rename(temp_path, target)` — atomic publish
Every IO error → `StorageError::Operation { message: error.to_string() }`.

**`remove`**: `fs::remove_file`; NotFound → `Ok(())` (idempotent); other err → `Operation`.

**Round-trip test** (`traits.rs:364-385`) — note it uses `block_on` (futures) and a temp dir:
```rust
#[test]
fn file_storage_round_trips_values() {
    let root = std::env::temp_dir().join(format!(
        "sp42-file-storage-{}-{}", std::process::id(), FixedClock::new(42).now_ms()));
    let storage = FileStorage::new(root.clone());
    block_on(storage.set("key".to_string(), b"value".to_vec())).expect("set should succeed");
    let value = block_on(storage.get("key")).expect("get should succeed");
    assert_eq!(value, Some(b"value".to_vec()));
    block_on(storage.remove("key")).expect("remove should succeed");
    assert_eq!(block_on(storage.get("key")).expect("get after remove should succeed"), None);
    let _ = std::fs::remove_dir_all(root);
}
```

---

## 3. `Clock` trait + `FixedClock` (and `SystemClock`)

`Clock` is **NOT** async, no `async_trait`. Returns **`i64` milliseconds** (signed; epoch-ms).

```rust
pub trait Clock: Send + Sync {              // traits.rs:36-38
    fn now_ms(&self) -> i64;
}
```

`FixedClock` (`traits.rs:237-253`) — deterministic test double, `const` ctor:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedClock { now_ms: i64 }
impl FixedClock {
    #[must_use] pub const fn new(now_ms: i64) -> Self { Self { now_ms } }
}
impl Clock for FixedClock {
    fn now_ms(&self) -> i64 { self.now_ms }
}
```

`SystemClock` (`traits.rs:255-266`) — production impl; `SystemTime::now().duration_since(UNIX_EPOCH)`,
`i64::try_from(duration.as_millis())`, **falls back to `0`** on any error (never panics):
```rust
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;
impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        SystemTime::now().duration_since(UNIX_EPOCH).ok()
            .and_then(|d| i64::try_from(d.as_millis()).ok()).unwrap_or(0)
    }
}
```

Test: `fn fixed_clock_is_deterministic() { assert_eq!(FixedClock::new(42).now_ms(), 42); }`.

**Port note for VerdictEnvelope:** inject `&dyn Clock` (or `C: Clock`) to stamp
`recorded_at_ms: i64` on the verdict record — keeps timestamps deterministic under `FixedClock`
in tests. Do NOT call `SystemTime::now()` inside the build function.

### `Rng` trait + `SequenceRng` (bonus — same DI pattern, NOT async)
```rust
pub trait Rng: Send { fn next_u64(&mut self) -> u64; }   // traits.rs:40-42
```
`SequenceRng::new<I: IntoIterator<Item=u64>>(values)` pops seeded `u64`s, returns `0` when drained
(`traits.rs:268-289`). Mutable `&mut self` (unlike Clock/Storage). Only relevant if a verdict needs
a random id; content-addressing (§5) makes that unnecessary.

---

## 4. `HttpRequest` / `HttpResponse` / `HttpMethod` (transport DTOs)

`crates/sp42-types/src/transport.rs`. All derive `Debug, Clone, PartialEq, Eq, Serialize,
Deserialize`. Headers are **`BTreeMap<String, String>`** (sorted → deterministic serialization,
critical if you ever content-hash a request). Bodies are **`Vec<u8>`** (raw bytes, not String).

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HttpMethod { Get, Post, Put, Patch, Delete }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub url: Url,                                   // url::Url (serde feature on)
    #[serde(default)] pub headers: BTreeMap<String, String>,
    #[serde(default)] pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status: u16,                               // numeric HTTP status
    #[serde(default)] pub headers: BTreeMap<String, String>,
    #[serde(default)] pub body: Vec<u8>,
}
```
`#[serde(default)]` on headers/body → omitted/empty fields deserialize cleanly. `url::Url` carries
the `serde` feature (workspace dep `url = { version = "2.5.7", features = ["serde"] }`).
Success-status check idiom used in `wiki_storage`: `if !(200..300).contains(&response.status)`.

(Also present, lower priority: `ServerSentEvent { event_type: Option<String>, id: Option<String>,
data: String, retry_ms: Option<u64> }` and `WebSocketFrame { Text(String) | Binary(Vec<u8>) |
Close }`. Their traits `EventSource` (async, `&mut self`, doubles `ReplayEventSource`) and
`WebSocket` (async, `&mut self`, double `LoopbackWebSocket`) exist but are NOT relevant to
snapshot/verdict storage.)

---

## 5. The versioned-envelope pattern (THE blueprint for `SnapshotEnvelope` / `VerdictEnvelope`)

From `crates/sp42-core/src/wiki_storage.rs`. SP42 already has a **versioned serde envelope +
build/parse split + injected `HttpClient`** pattern. The on-wiki variant embeds the JSON in
wiki markup; for ADR-0009 you want the **same envelope discipline but persisted via the injected
`Storage` trait** and **keyed by content hash** instead of by wiki title.

### 5a. The envelope struct (`wiki_storage.rs:112-121`) — pattern to copy
```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikiStoragePayloadEnvelope {
    pub project: String,        // == PROJECT_NAME ("SP42") — provenance marker
    pub version: u32,           // schema version; starts at 1
    pub title: String,
    pub kind: String,           // string label, e.g. "personal-profile"
    pub site_wiki_id: String,
    pub realm: WikiStorageRealm,
    pub data: Value,            // serde_json::Value — the actual payload
}
```
Key envelope conventions to mirror:
- A `project: String` provenance field (= `crate::branding::PROJECT_NAME`).
- An explicit **`version: u32`**, set to `1` on build (`render_wiki_storage_document_page`
  line 348: `version: 1`). On parse it deserializes whatever is on disk → forward-compat: you
  can match on `version` to migrate.
- A typed **`kind`** discriminator (here `String`; for verdict/snapshot prefer a typed enum or a
  `&'static str` label like `document_kind_label` at `wiki_storage.rs:821-835`).
- A **`data: serde_json::Value`** open payload slot, OR (better for a typed port) a concrete
  generic/struct field. For `Snapshot`/`Verdict` use concrete typed fields, not `Value`.

### 5b. Build/parse split — pure functions, no I/O inside the (de)serializer
- **Build** = `render_wiki_storage_document_page(document, human_summary, data) ->
  Result<String, WikiStorageError>` (`wiki_storage.rs:340-385`): constructs the envelope, sets
  `version: 1`, `serde_json::to_string_pretty(&envelope)` (map err →
  `WikiStorageError::Serialize { message }`), wraps it between markers.
- **Parse** = `parse_wiki_storage_payload_envelope(body: &str) ->
  Result<WikiStoragePayloadEnvelope, WikiStorageError>` (`wiki_storage.rs:506-535`): finds the
  begin/end markers, strips wrapper, `serde_json::from_str` (err → `Serialize`).
- The **I/O lives in a separate async fn** that takes the injected client and calls build/parse:
  `load_wiki_storage_document<C: HttpClient + ?Sized>(client, config, title)` (lines 427-443) and
  `save_wiki_storage_document<C: HttpClient + ?Sized>(client, config, request)` (lines 543-586).
  **This is the split to replicate**: pure `serialize_snapshot(&Snapshot) -> Vec<u8>` /
  `parse_snapshot(&[u8]) -> Result<Snapshot, _>`, then thin async
  `store_snapshot<S: Storage + ?Sized>(storage, &snapshot)` /
  `load_snapshot<S: Storage + ?Sized>(storage, hash)`.

### 5c. Content addressing with `Sha256` — exact existing idiom
The ONLY `sha2` usage in the codebase is `crates/sp42-core/src/oauth.rs`:
```rust
use sha2::{Digest, Sha256};                         // oauth.rs:8
// ...
let digest = Sha256::digest(verifier.as_bytes());   // oauth.rs:136  -> GenericArray<u8, 32>
Ok(URL_SAFE_NO_PAD.encode(digest))                  // oauth.rs:137  base64-url-no-pad of the 32 bytes
```
So the established pattern is: `Sha256::digest(bytes)` → encode the 32-byte digest to a string key.
**Encoding decision (no `hex` crate exists):** two faithful options —
1. **base64 url-safe no-pad** (matches oauth precedent exactly; dep `base64` already present,
   `base64.workspace = true` in sp42-core): `URL_SAFE_NO_PAD.encode(Sha256::digest(canonical_bytes))`.
2. **lower-hex by hand** (matches `FileStorage::key_path`'s `write!("{byte:02x}")` idiom; no extra
   dep): loop the 32 digest bytes writing `{b:02x}` into a `String`.
   Recommend **hex** for snapshot keys so the content-hash is the human-readable, canonical id
   (mirrors `FileStorage`'s own encoding and reads naturally as `"sha256:<hex>"`).

**Canonicalization caveat (load-bearing for content addressing):** hash the **canonical serialized
bytes**, not a pretty-printed string, and use deterministic ordering. SP42 already uses
`BTreeMap` for headers (sorted) — keep that discipline: serialize with `serde_json::to_vec`
(compact) over a struct whose map fields are `BTreeMap`, so the same logical snapshot always
hashes identically. Hash the SOURCE BODY BYTES for the snapshot key; hash the verdict's canonical
JSON for any dedup of verdict records (but a verdict is better keyed by
`<snapshot_hash>/<panel_run_id>` so re-runs append rather than collide).

### 5d. The envelope round-trip test (quote, `wiki_storage.rs:969-988`)
```rust
#[test]
fn parses_embedded_payload_envelope() {
    let plan = build_wiki_storage_plan(&WikiStorageConfig::default(), &sample_input());
    let page = render_wiki_storage_document_page(
        &plan.personal_documents[0],
        &["Compact theme".to_string()],
        &json!({ "theme": "compact" }),
    ).expect("document page should render");

    let envelope = parse_wiki_storage_payload_envelope(&page).expect("payload should parse");
    assert_eq!(envelope.kind, "personal-profile");
    assert_eq!(envelope.data.get("theme").and_then(serde_json::Value::as_str), Some("compact"));
}
```
And the **Storage-injected** round-trip discipline to copy is `memory_storage_round_trips_values`
(§2 / `traits.rs:354-362`) + `file_storage_round_trips_values` (§2). For ADR-0009: build a
`Snapshot`, `store_snapshot(&MemoryStorage::default(), &snap)` → get back the content-hash key,
`load_snapshot(storage, &key)` → `assert_eq!(loaded, snap)` (envelope derives
`PartialEq, Eq`, so direct equality works).

### 5e. Error enum to mirror — `WikiStorageError` (`errors.rs:145-155`)
`#[derive(Debug, Error)]` (note: NOT Clone/Eq, unlike the transport errors):
```rust
pub enum WikiStorageError {
    #[error("wiki storage input is invalid: {message}")]   InvalidInput { message: String },
    #[error("wiki storage serialization failed: {message}")] Serialize { message: String },
    #[error("wiki storage transport failed: {message}")]   Transport { message: String },
    #[error("wiki storage write conflict on `{title}`: {message}")] Conflict { title: String, message: String },
}
```
For a new `SnapshotStoreError` / `VerdictStoreError`, mirror this shape: `InvalidInput`,
`Serialize` (wrap serde_json err message), `Storage` (wrap `StorageError`, replacing the
wiki-specific `Transport`/`Conflict`). Pattern: each variant carries a `message: String`,
derived from `.to_string()` of the underlying error.

---

## 6. Conventions to honor in the port (cross-cutting)

- **`#![forbid(unsafe_code)]`** at crate root (sp42-types has it).
- **Workspace lints are strict:** `warnings = "deny"` (rust) and **`clippy::pedantic = "deny"`**.
  → annotate pure constructors `#[must_use]`; use `const fn` where possible (FixedClock pattern);
  map every error explicitly; no `unwrap` in non-test code (use `.ok_or_else`, `.map_err`).
- **Doc-comment `# Errors` sections** are mandatory on every public `Result`-returning fn
  (see wiki_storage `///# Errors`), required by pedantic.
- **DI via generics with `?Sized`:** logic fns take `C: HttpClient + ?Sized` /
  `S: Storage + ?Sized` so callers can pass `&dyn Trait` or a concrete type.
- **`async_trait`** only where the trait has async methods (HttpClient/Storage/EventSource/
  WebSocket). `Clock`/`Rng` are sync — do NOT add async_trait to a Clock-like trait.
- **Tests use `futures::executor::block_on`** (futures is the test-runner; tokio is server-only),
  and `FixedClock`/`MemoryStorage`/`StubHttpClient` for hermeticity.
- **Edition 2024, rust-version 1.92, license GPL-3.0-only.**
- **No new deps needed:** `sha2`, `base64`, `serde`, `serde_json`, `url`, `thiserror`,
  `async-trait` all already in `sp42-core`. (No `hex` crate — encode by hand or via base64.)

---

## 7. Concrete starting shape for ADR-0009 (synthesis, port-ready)

In `sp42-core` (e.g. `src/snapshot_storage.rs`):

```rust
// content-addressed snapshot of a fetched source body
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotEnvelope {
    pub project: String,          // PROJECT_NAME
    pub version: u32,             // 1
    pub source_url: Url,
    pub fetched_at_ms: i64,       // from injected Clock
    pub content_type: Option<String>,
    pub body: Vec<u8>,            // the raw fetched bytes (what the hash is over)
    // sha256-hex of `body` is the STORE KEY (not stored in the envelope, or stored for audit)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerdictEnvelope {
    pub project: String,
    pub version: u32,
    pub snapshot_hash: String,    // links to the SnapshotEnvelope key
    pub recorded_at_ms: i64,      // injected Clock
    pub panel: Vec<ModelVote>,    // per-model votes (ADR-0006/0007)
    pub agreement: PanelAgreement,// measured agreement, NOT confidence
    // per-vote ModelRef { provider, model, version } per ADR-0006 Decision 8
}

// pure: fn snapshot_key(body: &[u8]) -> String  // "sha256:" + lower-hex(Sha256::digest(body))
// pure: fn serialize_envelope<T: Serialize>(&T) -> Result<Vec<u8>, _>  // serde_json::to_vec
// pure: fn parse_envelope<T: DeserializeOwned>(&[u8]) -> Result<T, _>
// async fn store_snapshot<S: Storage + ?Sized>(s: &S, env: &SnapshotEnvelope) -> Result<String,_>
// async fn load_snapshot<S: Storage + ?Sized>(s: &S, key: &str) -> Result<Option<SnapshotEnvelope>,_>
```
Test with `MemoryStorage::default()` + `FixedClock::new(...)` + `block_on`, asserting the round-trip
and that the key == the sha256 of the body.
