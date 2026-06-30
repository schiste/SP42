//! The rmcp stdio server exposing SP42's citation verbs as MCP tools (PRD-0010, Phase 4).
//!
//! The tool handlers are thin adapters over the stub-tested verb functions (`probe_source`,
//! `verify_claim`): they deserialize typed parameters, call the verb with the server's injected
//! fetch/model/panel state, and return the result as JSON text. Output rides as a JSON string
//! rather than MCP structured content because the result types embed `sp42-core` verdict types
//! that do not implement `schemars::JsonSchema`; structured output is a later refinement.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use sp42_inference::GenaiModelClient;
use sp42_types::{ModelRef, SystemClock};

use crate::{GuardedHttpClient, Source, StatementRef, probe_source, verify_claim};

/// Parameters for the `probe_source` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProbeParams {
    /// The source URL to probe for reachability and pipeline-extractability.
    pub url: String,
}

/// Parameters for the `verify_claim` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct VerifyParams {
    /// The claim text to verify.
    pub claim: String,
    /// The source to verify against: a URL to fetch, or pre-fetched content.
    pub source: Source,
}

/// The MCP server: holds the injected fetch client, model client, and model panel.
pub struct Sp42McpServer {
    fetch: GuardedHttpClient,
    model: GenaiModelClient,
    panel: Vec<ModelRef>,
}

impl Sp42McpServer {
    /// Construct the server from the environment (`SP42_INFERENCE_*`, `SP42_FETCH_ALLOW_PRIVATE`).
    ///
    /// # Errors
    ///
    /// Returns an error string if the fetch client, model client, or panel cannot be built.
    pub fn from_env() -> Result<Self, String> {
        Ok(Self {
            fetch: GuardedHttpClient::from_env()?,
            model: sp42_inference::client_from_env()?,
            panel: sp42_inference::panel_from_env()?,
        })
    }
}

#[tool_router]
impl Sp42McpServer {
    /// Probe whether a source URL is reachable and pipeline-extractable, without any model call.
    #[tool(
        description = "Probe whether a cited source URL is reachable and whether SP42's pipeline \
            can extract usable text from it — deterministic, no model inference. Distinguishes \
            unreachable from reachable-but-unextractable (a human may still be able to read it).",
        annotations(read_only_hint = true, idempotent_hint = true)
    )]
    async fn probe_source(
        &self,
        Parameters(ProbeParams { url }): Parameters<ProbeParams>,
    ) -> Result<String, String> {
        let result = probe_source(&self.fetch, &url).await;
        serde_json::to_string(&result).map_err(|error| error.to_string())
    }

    /// Verify whether a source supports a claim, via the model panel with verbatim grounding.
    #[tool(
        description = "Verify whether a source supports a claim, via a multi-model panel with \
            verbatim quote grounding. Returns a governed verdict (Supported / Partial / \
            NotSupported / SourceUnavailable) and, for a support verdict, the quote re-located \
            verbatim in the source. The source may be a URL or pre-fetched content.",
        annotations(read_only_hint = true)
    )]
    async fn verify_claim(
        &self,
        Parameters(VerifyParams { claim, source }): Parameters<VerifyParams>,
    ) -> Result<String, String> {
        let result = verify_claim(
            &self.fetch,
            &self.model,
            &SystemClock,
            &self.panel,
            &claim,
            &source,
        )
        .await
        .map_err(|error| error.to_string())?;
        serde_json::to_string(&result).map_err(|error| error.to_string())
    }

    /// Verify a Wikidata statement against its P854 reference URL.
    #[tool(
        description = "Verify a Wikidata statement against its P854 reference URL. Renders the \
            statement (entity, property, value) into a natural-language claim, resolves English \
            labels, and verifies it with verbatim grounding. Returns the rendered claim (for \
            inspection), the reference URL, and the verdict; a statement with no reference URL \
            returns SourceUnavailable.",
        annotations(read_only_hint = true)
    )]
    async fn verify_wikidata_statement(
        &self,
        Parameters(statement): Parameters<StatementRef>,
    ) -> Result<String, String> {
        let result = crate::verify_wikidata_statement(
            &self.fetch,
            &self.model,
            &SystemClock,
            &self.panel,
            &statement,
        )
        .await?;
        serde_json::to_string(&result).map_err(|error| error.to_string())
    }
}

#[tool_handler(
    name = "sp42-citation",
    version = "0.1.0",
    instructions = "SP42 citation verification. Use probe_source to cheaply screen a source URL, \
        then verify_claim to judge whether a source supports a claim with verbatim grounding."
)]
impl ServerHandler for Sp42McpServer {}

#[cfg(test)]
mod tests {
    use super::Sp42McpServer;

    #[test]
    fn tool_router_lists_the_mvp_verbs() {
        let router = Sp42McpServer::tool_router();
        assert!(router.has_route("probe_source"), "probe_source registered");
        assert!(router.has_route("verify_claim"), "verify_claim registered");
        assert!(
            router.has_route("verify_wikidata_statement"),
            "verify_wikidata_statement registered"
        );
        assert_eq!(router.list_all().len(), 3, "the three MVP verbs");
        // Each verb advertises a tool definition the client can introspect.
        assert!(router.get("probe_source").is_some());
        assert!(router.get("verify_claim").is_some());
        assert!(router.get("verify_wikidata_statement").is_some());
    }
}
