# ADR-0002: Local dev-auth bridge contract

**Status:** Accepted
**Date:** 2026-03-24
**Author:** SP42

## Context

SP42 needs a single-user local integration path for live Wikimedia verification while keeping browser code free of raw tokens. The browser is an interface, not the credential store. The server owns the token, derives the effective identity and capabilities, and exposes only session state back to the browser.

## Decision

The local dev-auth bridge uses a canonical empty JSON bootstrap payload at `POST /dev/auth/session/bootstrap`.

- The browser may construct a compatibility `DevAuthBootstrapRequest`, but its serialized payload is always `{}`.
- The server derives username, effective scopes, and expiry from the local Wikimedia token stored in `.env.wikimedia.local`.
- The browser only receives session status and capability reports through localhost endpoints and cookies.

## Consequences

- Token material stays server-side and in memory only.
- Browser code cannot request arbitrary identity, scope, or expiry values during bootstrap.
- The bootstrap contract is explicit and stable for single-user local development.
- Live Wikimedia verification can be exercised locally without creating a separate credential store or persisting secrets in the browser.
