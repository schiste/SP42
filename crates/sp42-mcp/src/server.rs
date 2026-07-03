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
use sp42_wiki::WikiRegistry;

use crate::{
    GuardedHttpClient, PageInput, Source, StatementRef, probe_source, verify_claim,
    verify_wikipedia_page,
};

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

/// Parameters for the `verify_wikipedia_page` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct PageParams {
    /// The canonical page title, e.g. `Cosmic latte`.
    pub title: String,
    /// The revision id to pin verification to.
    pub rev_id: u64,
    /// The wiki to resolve the Parsoid endpoint from; omit for the server's default wiki.
    #[serde(default)]
    pub wiki_id: Option<String>,
    /// An explicit Parsoid REST base URL (e.g. `https://en.wikipedia.org/w/rest.php`), overriding
    /// the resolved wiki's configured endpoint. Accepted only when `SP42_FETCH_ALLOW_PRIVATE=1`
    /// is set; otherwise page verification uses registry-configured Parsoid endpoints only.
    #[serde(default)]
    pub parsoid_url: Option<String>,
    /// When `true`, fetch and decompose the page but run no panel — return the use-site count and
    /// implied panel-call count only. The cheap way to size a job before committing inference.
    #[serde(default)]
    pub estimate_only: bool,
    /// Override the default fan-out cap on use-sites verified in one run.
    #[serde(default)]
    pub max_use_sites: Option<usize>,
}

/// The MCP server: holds the injected fetch client, model client, model panel, and wiki registry.
pub struct Sp42McpServer {
    fetch: GuardedHttpClient,
    model: GenaiModelClient,
    panel: Vec<ModelRef>,
    registry: WikiRegistry,
}

impl Sp42McpServer {
    /// Construct the server from the environment (`SP42_INFERENCE_*`, `SP42_FETCH_ALLOW_PRIVATE`).
    ///
    /// # Errors
    ///
    /// Returns an error string if the fetch client, model client, panel, or wiki registry cannot
    /// be built.
    pub fn from_env() -> Result<Self, String> {
        Ok(Self {
            fetch: GuardedHttpClient::from_env()?,
            model: sp42_inference::client_from_env()?,
            panel: sp42_inference::panel_from_env()?,
            registry: WikiRegistry::load().map_err(|error| error.to_string())?,
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

    /// Verify every URL-bearing citation on a Wikipedia revision (or estimate the job's size).
    #[tool(
        description = "Verify every URL-bearing citation on a Wikipedia revision: fetch the page \
            through Parsoid, decompose it into citation use-sites, and judge each with the model \
            panel and verbatim grounding. Returns one verdict per use-site. Set estimate_only to \
            size the job first (use-site count and implied panel calls) without running the panel; \
            a default fan-out cap (overridable via max_use_sites) bounds a single run.",
        annotations(read_only_hint = true)
    )]
    async fn verify_wikipedia_page(
        &self,
        Parameters(params): Parameters<PageParams>,
    ) -> Result<String, String> {
        let mut config = match &params.wiki_id {
            Some(wiki_id) => self
                .registry
                .config(wiki_id)
                .map_err(|error| error.to_string())?,
            None => self.registry.default_config(),
        };
        if let Some(parsoid_url) = &params.parsoid_url {
            config.parsoid_url = Some(parse_parsoid_override(
                parsoid_url,
                self.fetch.allows_private_addresses(),
            )?);
        }
        let result = verify_wikipedia_page(
            &self.fetch,
            &self.model,
            &SystemClock,
            &self.panel,
            &config,
            &PageInput {
                title: params.title,
                rev_id: params.rev_id,
            },
            params.estimate_only,
            params.max_use_sites,
        )
        .await?;
        serde_json::to_string(&result).map_err(|error| error.to_string())
    }
}

#[tool_handler(
    name = "sp42-citation",
    version = "0.1.0",
    instructions = "SP42 citation verification. Use probe_source to cheaply screen a source URL, \
        verify_claim to judge whether a source supports a single claim with verbatim grounding, \
        verify_wikidata_statement for a Wikidata statement's reference, and verify_wikipedia_page \
        to verify a whole revision's citations (with estimate_only to size the job first)."
)]
impl ServerHandler for Sp42McpServer {}

fn parse_parsoid_override(parsoid_url: &str, allow_private: bool) -> Result<url::Url, String> {
    if !allow_private {
        return Err(
            "parsoid_url overrides require SP42_FETCH_ALLOW_PRIVATE=1; use the configured wiki registry endpoint instead"
                .to_owned(),
        );
    }
    parsoid_url
        .parse()
        .map_err(|error: url::ParseError| format!("invalid parsoid_url {parsoid_url:?}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{Sp42McpServer, parse_parsoid_override};

    #[test]
    fn tool_router_lists_the_mvp_verbs() {
        let router = Sp42McpServer::tool_router();
        assert!(router.has_route("probe_source"), "probe_source registered");
        assert!(router.has_route("verify_claim"), "verify_claim registered");
        assert!(
            router.has_route("verify_wikidata_statement"),
            "verify_wikidata_statement registered"
        );
        assert!(
            router.has_route("verify_wikipedia_page"),
            "verify_wikipedia_page registered"
        );
        assert_eq!(router.list_all().len(), 4, "the four MVP verbs");
        // Each verb advertises a tool definition the client can introspect.
        assert!(router.get("probe_source").is_some());
        assert!(router.get("verify_claim").is_some());
        assert!(router.get("verify_wikidata_statement").is_some());
        assert!(router.get("verify_wikipedia_page").is_some());
    }

    #[test]
    fn parsoid_override_is_rejected_without_private_escape_hatch() {
        let error = parse_parsoid_override("http://169.254.169.254/latest/meta-data/", false)
            .expect_err("caller-controlled Parsoid URLs are disabled by default");

        assert!(
            error.contains("SP42_FETCH_ALLOW_PRIVATE=1"),
            "error should name the explicit escape hatch: {error}"
        );
    }

    #[test]
    fn parsoid_override_requires_a_valid_url_under_escape_hatch() {
        let error =
            parse_parsoid_override("not a url", true).expect_err("invalid override URL rejected");

        assert!(
            error.contains("invalid parsoid_url"),
            "unexpected parse error: {error}"
        );
    }

    #[test]
    fn parsoid_override_is_allowed_under_private_escape_hatch() {
        let url = parse_parsoid_override("http://127.0.0.1:8080/w/rest.php", true)
            .expect("dev/test escape hatch allows caller-supplied Parsoid endpoints");

        assert_eq!(url.as_str(), "http://127.0.0.1:8080/w/rest.php");
    }
}
