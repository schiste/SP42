# SP42 Runtime Configuration

This document covers runtime settings shared by local development, Wikimedia
Cloud VPS deployments, and the desktop sidecar mode.

## Server Runtime Settings

The server reads these environment variables:

- `SP42_DEPLOYMENT_MODE=local|vps|desktop` controls deployment-sensitive
  defaults. It defaults to `local`.
- `SP42_BIND_ADDR` controls the server bind address. It defaults to
  `127.0.0.1:8788`.
- `SP42_PUBLIC_BASE_URL` sets the externally visible HTTP(S) base URL, for
  example `https://sp42.example.wmcloud.org`.
- `SP42_APP_DIST_DIR` points the server at the compiled browser bundle.
- `SP42_RUNTIME_DIR` points persistent runtime files at a deployable data
  directory.
- `SP42_ALLOWED_ORIGINS` is a comma-separated list of extra credentialed CORS
  origins.
- `SP42_WIKI_CONFIG_DIR` points at a directory containing top-level `*.yaml`
  wiki configs. It defaults to `configs/`, with an embedded `frwiki` fallback
  for local development and tests.
- `SP42_DEFAULT_WIKI_ID` selects the default active wiki from the loaded wiki
  registry. It defaults to the first loaded wiki config.
- `SP42_SUPERVISOR_WIKIS` optionally overrides the comma-separated wiki list
  watched by ingestion supervisors. If unset, the configured default wiki is
  watched.

In `vps` mode, local dev-token bootstrap is disabled and session cookies are
marked `Secure`. Across modes, cookie-auth state-changing routes require an
SP42 CSRF header.

## Frontend API Base URL

The browser app uses same-origin API paths by default. For split frontend/API
deployments, set one of:

- `window.__SP42_RUNTIME_CONFIG__.apiBaseUrl`
- `window.__SP42_RUNTIME_CONFIG__.defaultWikiId`
- `<meta name="sp42-api-base-url" content="...">`
- `<meta name="sp42-default-wiki-id" content="...">`
- `SP42_API_BASE_URL` at frontend build time
- `SP42_DEFAULT_WIKI_ID` at frontend build time

When the browser bundle is served by `sp42-server`, `/runtime-config.js` sets
`window.__SP42_RUNTIME_CONFIG__.defaultWikiId` before the Wasm app starts.

## Local Wikimedia Development Auth

SP42 supports a localhost-only single-user development auth path. This is for
local testing only, not for public or multi-user deployment.

Create a local file named `.env.wikimedia.local` at the repository root with:

```env
WIKIMEDIA_CLIENT_APPLICATION_KEY=...
WIKIMEDIA_CLIENT_APPLICATION_SECRET=...
WIKIMEDIA_ACCESS_TOKEN=...
WIKIMEDIA_OAUTH_CALLBACK_URL=http://localhost:4173/oauth/callback
```

Rules:

- Keep this file local only.
- Never commit it.
- Tokens must not be exposed to browser storage or client-side code.
- The browser should interact with Wikimedia only through the localhost bridge
  in development.

The file is ignored by `.gitignore`.

The local dev-auth bridge contract is recorded in
[ADR-0002](adr/0002-local-dev-auth-bridge.md).

## Wikimedia Cloud VPS Package

Build the deployable VPS package with:

```sh
./scripts/package-vps.sh
```

The generated package includes the release server binary, built browser bundle,
configs, schemas, example environment file, systemd unit, and its own deployment
README.

## Desktop Runtime

Desktop-specific sidecar and remote-backend settings live in
[docs/platform/DESKTOP_DISTRIBUTION.md](DESKTOP_DISTRIBUTION.md).
