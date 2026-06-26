//! The provider-agnostic model-client boundary (ADR-0006).
//!
//! Feature crates depend on the [`ModelClient`] trait and the neutral request/response
//! DTOs here — never on a specific provider's wire format — so the concrete adapter (the
//! `rust-genai` crate, adopted external + version-pinned and living in a shell, ADR-0006
//! Decision 7) can be swapped without touching feature code. This mirrors the
//! `HttpClient` / `Storage` platform-edge pattern; the trait is dependency-free and the
//! vendor dependency never enters a domain crate.
//!
//! Every invocation is fingerprinted by [`ModelInvocation`] (provider, model id, version,
//! quantization, sampling/reasoning params, and a prompt+input hash) so a result is
//! auditable and replayable (ADR-0006 Decision 8).

use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::errors::ModelClientError;

/// A chat message role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    /// System instruction.
    System,
    /// User turn.
    User,
    /// Assistant turn.
    Assistant,
}

/// One chat message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    /// The message role.
    pub role: ChatRole,
    /// The message content.
    pub content: String,
}

impl ChatMessage {
    /// A `system` message.
    #[must_use]
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::System,
            content: content.into(),
        }
    }

    /// A `user` message.
    #[must_use]
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::User,
            content: content.into(),
        }
    }
}

/// Identity of a model — provider, model id, and pinned version (ADR-0006 Decision 8).
/// Never a key or token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRef {
    /// The provider (e.g. `openrouter`, `local`).
    pub provider: String,
    /// The model id sent in the request.
    pub model: String,
    /// The pinned model version recorded for reproducibility (often equal to `model`).
    pub version: String,
}

impl ModelRef {
    /// Construct a model reference.
    #[must_use]
    pub fn new(
        provider: impl Into<String>,
        model: impl Into<String>,
        version: impl Into<String>,
    ) -> Self {
        Self {
            provider: provider.into(),
            model: model.into(),
            version: version.into(),
        }
    }
}

/// Sampling / reasoning parameters for a model request.
///
/// Carries `f64` knobs for the wire; the audit-side normalization is
/// [`SamplingParams::fingerprint`] (string map, deterministic order).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SamplingParams {
    /// Sampling temperature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Nucleus-sampling top-p.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    /// Maximum output tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Sampling seed, when the provider supports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    /// Any additional provider-specific params (e.g. reasoning effort), as strings.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, String>,
}

impl SamplingParams {
    /// Deterministic params (temperature 0) — the default for citation verification.
    #[must_use]
    pub fn deterministic() -> Self {
        Self {
            temperature: Some(0.0),
            ..Self::default()
        }
    }

    /// A normalized, deterministically-ordered string map of the set params, for the
    /// invocation fingerprint.
    #[must_use]
    pub fn fingerprint(&self) -> BTreeMap<String, String> {
        let mut map = self.extra.clone();
        if let Some(temperature) = self.temperature {
            map.insert("temperature".to_string(), format!("{temperature}"));
        }
        if let Some(top_p) = self.top_p {
            map.insert("top_p".to_string(), format!("{top_p}"));
        }
        if let Some(max_tokens) = self.max_tokens {
            map.insert("max_tokens".to_string(), max_tokens.to_string());
        }
        if let Some(seed) = self.seed {
            map.insert("seed".to_string(), seed.to_string());
        }
        map
    }
}

/// The fingerprint of a single model invocation — what makes a result auditable and
/// replayable (ADR-0006 Decision 8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelInvocation {
    /// The model identity.
    pub model: ModelRef,
    /// Quantization, when known (e.g. `Q4_K_M`); usually `None` for hosted models.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quant: Option<String>,
    /// The sampling/reasoning params, normalized to strings.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, String>,
    /// SHA-256 hex of the exact prompt+input sent to the model (for replay).
    pub prompt_hash: String,
}

/// A neutral model-completion request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelCompletionRequest {
    /// The model to invoke.
    pub model: ModelRef,
    /// The chat messages (the full input).
    pub messages: Vec<ChatMessage>,
    /// Sampling / reasoning params.
    #[serde(default)]
    pub params: SamplingParams,
}

/// A neutral model completion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCompletion {
    /// The generated text.
    pub text: String,
    /// The model id the provider reports having served, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub served_model: Option<String>,
}

/// Where inference runs and how SP42 authenticates to it (ADR-0006 Decisions 4–6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EndpointMode {
    /// A model server on the operator's own machine/network; no key.
    #[default]
    Local,
    /// SP42 calls a provider directly with a key held by the deployment (server/CLI/
    /// desktop only — never the browser).
    Direct,
    /// SP42 calls a keyless (or proxy-token-gated) sponsor/hosted proxy that owns the
    /// provider keys, budget, and per-call authorization; the only remote mode the
    /// browser may use.
    SponsorProxy,
}

/// The model inference endpoint configuration the shell uses to build the adapter
/// (ADR-0006 Decisions 4–7). The browser sends a session-scoped `auth_token` (a proxy
/// token in `SponsorProxy` mode), never a provider key; `capability_tag` rides as
/// transport authorization metadata a sponsor proxy may gate on (Decision 6), never model
/// input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelEndpointConfig {
    /// Where inference runs / how it is authenticated.
    pub mode: EndpointMode,
    /// The OpenAI-compatible (or provider) base URL.
    pub base_url: String,
    /// The bearer token: a provider key in `Direct` mode, a proxy token in
    /// `SponsorProxy` mode, absent in `Local`. Held in memory only, never persisted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_token: Option<String>,
    /// An optional capability tag (e.g. `citation-verify`) carried as authorization
    /// metadata for a sponsor proxy; never added to the model input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_tag: Option<String>,
}

/// The provider-agnostic model edge (ADR-0006). Concrete adapters (OpenAI-compatible,
/// local, sponsor proxy) live in non-contract crates and never leak their wire format.
#[async_trait]
pub trait ModelClient: Send + Sync {
    /// Run one completion.
    ///
    /// # Errors
    ///
    /// Returns [`ModelClientError`] on a transport failure or an unusable response.
    async fn complete(
        &self,
        request: &ModelCompletionRequest,
    ) -> Result<ModelCompletion, ModelClientError>;
}

/// A queue-of-responses test double for [`ModelClient`] (mirrors `StubHttpClient`).
#[derive(Debug)]
pub struct StubModelClient {
    responses: Mutex<VecDeque<Result<ModelCompletion, ModelClientError>>>,
}

impl StubModelClient {
    /// Seed the stub with a FIFO queue of canned completions.
    #[must_use]
    pub fn new<I>(responses: I) -> Self
    where
        I: IntoIterator<Item = Result<ModelCompletion, ModelClientError>>,
    {
        Self {
            responses: Mutex::new(responses.into_iter().collect()),
        }
    }
}

#[async_trait]
impl ModelClient for StubModelClient {
    async fn complete(
        &self,
        _request: &ModelCompletionRequest,
    ) -> Result<ModelCompletion, ModelClientError> {
        let mut responses = self
            .responses
            .lock()
            .map_err(|_| ModelClientError::StatePoisoned {
                resource: "stub_model_client.responses",
            })?;
        responses.pop_front().unwrap_or_else(|| {
            Err(ModelClientError::InvalidResponse {
                message: "stub client has no queued response".to_string(),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ChatMessage, ChatRole, ModelClient, ModelCompletion, ModelCompletionRequest, ModelRef,
        SamplingParams, StubModelClient,
    };
    use futures::executor::block_on;

    #[test]
    fn chat_message_helpers_set_roles() {
        assert_eq!(ChatMessage::system("s").role, ChatRole::System);
        assert_eq!(ChatMessage::user("u").role, ChatRole::User);
    }

    #[test]
    fn fingerprint_normalizes_set_params_only() {
        let params = SamplingParams {
            temperature: Some(0.0),
            max_tokens: Some(256),
            ..SamplingParams::default()
        };
        let fingerprint = params.fingerprint();
        assert_eq!(
            fingerprint.get("temperature").map(String::as_str),
            Some("0")
        );
        assert_eq!(
            fingerprint.get("max_tokens").map(String::as_str),
            Some("256")
        );
        assert!(!fingerprint.contains_key("top_p"));
    }

    #[test]
    fn stub_pops_queued_completions_then_errors() {
        let client = StubModelClient::new([Ok(ModelCompletion {
            text: "hello".to_string(),
            served_model: None,
        })]);
        let request = ModelCompletionRequest {
            model: ModelRef::new("p", "m", "m"),
            messages: vec![ChatMessage::user("hi")],
            params: SamplingParams::deterministic(),
        };
        let first = block_on(client.complete(&request)).expect("first ok");
        assert_eq!(first.text, "hello");
        assert!(block_on(client.complete(&request)).is_err());
    }
}
