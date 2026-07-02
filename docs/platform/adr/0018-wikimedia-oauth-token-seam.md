# ADR-0018: Wikimedia OAuth token seam for non-server shells

**Status:** Draft
**Date:** 2026-07-01
**Author:** Luis Villa (drafted with Claude)

> Spawned by PRD-0013; extends ADR-0014.

## Context

ADR-0014 established required Wikimedia OAuth login and any-project resolution, but the token
exchange and holding live entirely **server-side**: `sp42-server`'s `/auth/login` → `/auth/callback`
routes exchange the code, store the token in a session, and actions run with the session user's
token. Nothing exposes a **reusable way for a non-server shell to obtain and hold a Wikimedia OAuth
token.**

A new shell now needs one: the MCP editing surface (PRD-0013) runs as a standalone (stdio, or hosted)
process with no browser session and no cookie jar, but must make **authenticated** edits on the
operator's behalf. Two acquisition paths follow from that — a bring-your-own owner-only token from the
environment (the MVP, no browser at all) and an interactive login for shells that can open a URL — and
the seam has to serve both. The same need will recur for the CLI insertion flow (PRD-0012) and any
future non-server consumer.

This is architecturally significant — and hence an ADR — because it introduces a **new
`sp42-platform` seam**: a token-source contract that every shell depends on. Getting the seam boundary
wrong couples each shell to a specific token-acquisition mechanism, so the boundary is the decision,
not an implementation detail for a design plan.

**Why a separate ADR, not an edit to 0014.** ADR-0014 decided the *server-side* login flow (browser
session, `/auth/login` → `/auth/callback`). This ADR **extends** that decision rather than superseding
it: the server path stays valid, and this adds a platform seam the server *may* later adopt but is not
forced to. Extension-without-contradiction is a new ADR that references 0014, not a retroactive
re-scoping of an accepted record.

Two constraints from adjacent decisions shape this:
- ADR-0014 §4: an **owner-only consumer token suffices for a developer's own account** (credentials
  never committed; `.env.wikimedia.local` precedent).
- The MCP authorization spec's third-party rule: a shell must use a **separate downstream token** and
  **must never forward** an inbound (MCP client→server) token to Wikimedia (confused-deputy). This rule
  governs the interactive/hosted path where the shell is itself an authenticated MCP server; the
  BYO-env MVP has **no inbound token at all**, so it satisfies the rule vacuously.

## Decision

### 1. A platform token-source contract
Define a `sp42-platform` contract — `WikimediaTokenSource` — that decouples *how* a Wikimedia token is
obtained from *how* edits consume it. The guarded-edit pipeline (PRD-0013) and every shell depend only
on this contract:

```rust
/// Yields an authenticated (bearer) HTTP client for a resolved wiki.
/// Implementations differ only in how the token is acquired and held.
#[async_trait]
pub trait WikimediaTokenSource {
    /// A bearer-authenticated client scoped to `wiki`, plus the token's identity and scopes.
    /// Errors if no token is available or acquisition fails. Only the interactive impl may block
    /// on a browser/login; the env impl must not.
    async fn authenticated_client(
        &self,
        wiki: &WikiConfig,
    ) -> Result<AuthenticatedClient, TokenSourceError>;
}

pub struct AuthenticatedClient {
    pub http: Arc<dyn HttpClient>, // carries the bearer; downstream code never sees the raw token
    pub identity: TokenIdentity,   // resolved username / centralauth id
    pub scopes: Vec<String>,       // granted OAuth scopes, for pre-flight capability checks
}
```

**Returns a client, not a token.** The seam hands back a bearer-carrying `HttpClient`, never the raw
token string. The edit pipeline therefore *cannot* log, forward, or otherwise leak the secret across
the boundary — the seam is the only code that touches it. This is the mechanism that makes §3's
never-forwarded guarantee structural rather than a convention callers must remember.

**Resolution is upstream.** `WikiConfig` is *handed in* already resolved (SiteMatrix resolution is
ADR-0014's job, done before the token source is called). The token source consumes a resolved config;
it does not re-run project resolution.

### 2. Two implementations, one seam
- **BYO owner-only token (env).** Read a Wikimedia OAuth owner-only access token from the environment
  (naming to align with ADR-0014's `.env.wikimedia.local` convention); build a bearer client directly.
  The MVP path — mirrors bring-your-own inference keys.
- **Interactive login.** Reuse `sp42-platform/oauth.rs` PKCE builders + a shell-provided callback
  (loopback for local, hosted callback for HTTP) + refresh; cache the token across restarts.

### 3. Separate downstream token, never forwarded
The token obtained here is used **only** against Wikimedia; an inbound MCP client→server access token
(if the shell is itself an authenticated MCP server) is never forwarded to Wikimedia. This is enforced
structurally by §1's client-not-token boundary — the raw token never leaves the seam, so there is
nothing for the pipeline to forward.

### 4. Any Wikimedia project
The seam resolves per `WikiConfig` (ADR-0014 SiteMatrix), so one token model serves Wikipedia,
Wikidata, and any other project without per-project code.

## Consequences

- **The server can later adopt the seam, but isn't forced to.** ADR-0014's session path keeps working;
  when convenient, `sp42-server` can implement `WikimediaTokenSource` over its session store so all
  shells share one contract. Until then the two coexist — no migration is gated on this ADR.
- **Token lifetime and refresh live behind the seam.** Callers get a currently-valid client; refresh
  (interactive impl) or "owner-only, no refresh" (env impl) is the source's concern, invisible to the
  edit pipeline.
- **Cached-token protection is the interactive path's burden.** The env impl caches nothing (the token
  lives in the process environment). The interactive impl must persist a token across restarts — OS
  keychain when available, `0600` file fallback, never committed (mirrors `.env.wikimedia.local`). This
  is PRD-0013 Open Question #2; same decision, tracked in one place.
- **Hosted multi-user is out of scope for the MVP.** An owner-only token is one operator's account. A
  multi-tenant hosted deployment (per-user tokens, session isolation) needs more, but the seam is
  compatible with it — each user gets their own `WikimediaTokenSource` — so nothing here blocks that
  later.
- **Explicitly NOT covered:** MCP client→server authentication (the client's rmcp `auth` half), and
  inference/hosting auth (orthogonal, a different payer — moot under bring-your-own-key).

## Alternatives considered

- **Proxy the running `sp42-server` session** (reuse its OAuth wholesale instead of a platform seam):
  rejected — couples every non-server shell to a running server and its browser session, defeating the
  stdio/BYO-cred posture PRD-0013 depends on. The seam instead lets the server *optionally* back the
  contract later (Consequences) without any shell requiring it.
- **Rely on rmcp's `auth` feature:** rejected — it is the *client→server* authorization half (the shell
  authenticating an inbound caller), not a *downstream* Wikimedia-token/broker seam. Orthogonal layer;
  it does not solve "hold a Wikimedia token to make edits."
- **Hand back the raw token instead of a client** (let each shell build its own authenticated client):
  rejected — leaks the secret across the seam boundary, making §3's never-forwarded guarantee a
  convention every caller must remember rather than a structural property. Directly contradicts §1's
  client-not-token decision; recorded here so a later "simplification" to return the token string does
  not silently reopen the leakage surface.
