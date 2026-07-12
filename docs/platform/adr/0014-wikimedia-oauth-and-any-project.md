# ADR-0014: Required Wikimedia OAuth login + any-Wikimedia-project resolution

**Status:** Accepted
**Date:** 2026-06-27
**Author:** Christophe Henner (drafted with Claude)
**Summary:** Login is required via Wikimedia OAuth, and any Wikimedia project resolves from a vendored SiteMatrix snapshot (`dbname → base url`) under a universal default scoring policy, so SP42 works across projects without per-wiki configuration.

## Context

For SP42 to be usable by real editors, two gaps had to close:

1. **It only worked on hand-configured wikis.** Wikis came from static
   `configs/*.yaml` (`enwiki`/`frwiki`/`testwiki`); an unknown `wiki_id` returned
   an error with no fallback. Editors could not point SP42 at the project they
   actually patrol.
2. **No per-user identity.** The full Wikimedia OAuth 2.0 PKCE flow already
   existed server-side (`/auth/login` → `/auth/callback` → token exchange →
   profile/capability probe → session cookie + CSRF; actions run with each user's
   own token), but nothing in the UI used it — only the local dev-token bootstrap
   was wired, and `/auth/*` wasn't even reachable in local dev.

## Decision

### 1. Resolve any Wikimedia project from an embedded authoritative list
Vendor a snapshot of the Wikimedia **SiteMatrix** (`dbname → base url`) into
`sp42-wiki` (`crates/sp42-wiki/data/wikimedia-sites.json`, refreshed by
`scripts/sync-wikis.sh`) and compile it in. `WikiRegistry::resolve()` returns a
hand-configured wiki when present, otherwise **derives** a `WikiConfig` from the
embedded entry: `api_url`/`parsoid_url` from the base host, the shared Wikimedia
endpoints (eventstreams/oauth/liftwing — identical for every project) as
constants, a default namespace allowlist, and a **universal language-agnostic
scoring policy**. Embedding gives speed *and* authority with no runtime network
dependency. It is SSRF-safe: hosts only ever come from the vendored data, so an
unknown/arbitrary `wiki_id` derives nothing.

### 2. Universal default scoring policy
Add `active/default-language-agnostic` to the embedded compiled-policy set; any
wiki without a tuned policy uses it. It mirrors the enwiki vandalism policy and
leans on the language-agnostic LiftWing revertrisk model, so it behaves
reasonably across languages and sister projects.

### 3. Login is required, via Wikimedia OAuth
The browser app gates the whole workspace behind a Wikimedia login: on load it
calls `GET /auth/session`; unauthenticated users get a "Log in with your
Wikimedia account" screen (→ `/auth/login`), authenticated users get the
workspace with their username + a logout control. Actions continue to run with
the logged-in user's own OAuth token. The **dev-token bootstrap is demoted** to a
secondary, local-mode-only convenience. `Trunk.toml` proxies `/auth` so login
works in local dev.

### 4. Operational prerequisite
Multi-user login requires a registered Wikimedia OAuth 2.0 **consumer**
(meta.wikimedia.org `Special:OAuthConsumerRegistration`), non-owner-only for
general use, callback `{public_base_url}/auth/callback`, with `basic` + action
grants. Credentials are read from `.env.wikimedia.local`
(`WIKIMEDIA_CLIENT_APPLICATION_KEY` / `WIKIMEDIA_CLIENT_APPLICATION_SECRET`) and
never committed. An owner-only consumer suffices for a developer's own account.

## Alternatives considered

- **Pattern-derive wiki URLs from the wiki_id** (no embedded list): simpler but
  not authoritative — odd hosts (wikidata `www.wikidata.org`, sister projects)
  and closed/renamed wikis would be wrong or missing. Rejected for the embedded
  authoritative snapshot.
- **Live SiteMatrix lookup at runtime**: authoritative but adds a network
  dependency + caching to a hot path. Rejected — embedding is both fast and
  authoritative; `sync-wikis.sh` refreshes it deliberately.
- **Optional login (read anonymously, log in to act)**: viable, but the project
  chose required login for a single, clear identity model.

## Consequences

- Any Wikimedia project resolves out of the box; configured wikis still win and
  keep their tuned policies.
- The app is unusable without a Wikimedia login (by design); a server without
  OAuth credentials shows a clear "not configured" message and (in local mode)
  the dev-token fallback.
- The embedded site list is a point-in-time snapshot; run `scripts/sync-wikis.sh`
  to refresh as Wikimedia adds/renames projects.

### Deferred (production hardening, tracked separately)
Persistent/distributed session storage (sessions are in-memory today), a user
table, refresh-token rotation, device/headless flow, multi-wiki single session,
and audit log / remote revocation. These block only multi-instance production
scale, not single-server use.
