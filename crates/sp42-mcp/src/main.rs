//! `sp42-mcp` — the SP42 citation-verification MCP server (PRD-0010).
//!
//! Speaks the Model Context Protocol over stdio. The agent-builder runs this binary, brings
//! their own inference credentials via `SP42_INFERENCE_*`, and pays their own inference (MVP =
//! local, bring-your-own-key). Nothing is logged to stdout — stdout is the MCP transport.

use rmcp::ServiceExt;
use rmcp::transport::stdio;
use sp42_mcp::Sp42McpServer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = Sp42McpServer::from_env()?;
    let running = server.serve(stdio()).await?;
    running.waiting().await?;
    Ok(())
}
